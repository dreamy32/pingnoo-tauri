//! `pingnoo-net` — async traceroute/ping engine(s).
//!
//! Tonight's engine wraps [`trippy_core`], which owns the raw-socket ICMP
//! machinery, reply correlation and cross-platform privilege handling. We drive
//! it as **bounded single-round sweeps in a loop we control** so that:
//!
//! * stopping is genuinely clean — cancellation is checked between rounds and no
//!   trace thread is ever orphaned (each round self-terminates), and
//! * our own [`StatsAccumulator`] (the Rust port of `PingData`) is the single
//!   source of truth for the render-ready per-hop stats.
//!
//! Each round is a full TTL sweep (one sample per hop). Trippy's blocking
//! `run()` executes on a `spawn_blocking` thread so the tokio runtime — and the
//! UI it drives — is never blocked.

use std::collections::BTreeMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{anyhow, Context};
use pingnoo_core::{
    EngineInfo, Hop, IpVersion, ResultCode, StatsAccumulator, TraceConfig, TraceUpdate,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use trippy_core::{Builder, PrivilegeMode, Protocol};
use trippy_privilege::Privilege;

/// Stable identifier for the trippy-backed engine.
pub const TRIPPY_ENGINE_ID: &str = "trippy-icmp";

/// Describes the trippy engine and whether it can run on this host right now.
pub fn engine_info() -> EngineInfo {
    let (available, note) = match Privilege::discover() {
        Ok(p) if p.has_privileges() => (true, "raw socket (privileged)"),
        Ok(p) if !p.needs_privileges() => (true, "unprivileged ICMP datagram"),
        Ok(_) => (false, "needs raw-socket privileges (setcap/root)"),
        Err(_) => (true, "privilege state unknown"),
    };
    EngineInfo {
        id: TRIPPY_ENGINE_ID.to_string(),
        description: format!("ICMP traceroute via trippy — {note}"),
        priority: 100,
        available,
    }
}

/// Chooses the best privilege mode for the current platform.
fn pick_privilege_mode() -> PrivilegeMode {
    match Privilege::discover() {
        Ok(p) if p.has_privileges() => PrivilegeMode::Privileged,
        Ok(p) if !p.needs_privileges() => PrivilegeMode::Unprivileged,
        _ => PrivilegeMode::Privileged,
    }
}

/// Resolves a host (IP literal or DNS name) to an [`IpAddr`] of the requested
/// family. Runs on a blocking thread so the async runtime is never stalled.
pub async fn resolve_target(host: &str, ip_version: IpVersion) -> anyhow::Result<IpAddr> {
    if let Ok(ip) = IpAddr::from_str(host.trim()) {
        return Ok(ip);
    }
    let host = host.trim().to_string();
    let want_v6 = matches!(ip_version, IpVersion::V6);
    let addrs = tokio::task::spawn_blocking(move || {
        (host.as_str(), 0u16)
            .to_socket_addrs()
            .map(|it| it.map(|sa| sa.ip()).collect::<Vec<_>>())
    })
    .await
    .context("dns resolver task panicked")?
    .context("dns lookup failed")?;

    let mut fallback = None;
    for ip in addrs {
        match (want_v6, ip) {
            (false, IpAddr::V4(_)) | (true, IpAddr::V6(_)) => return Ok(ip),
            _ => {
                fallback.get_or_insert(ip);
            }
        }
    }
    fallback.ok_or_else(|| anyhow!("could not resolve host"))
}

/// Per-hop running state kept across rounds by the driver loop.
#[derive(Default)]
struct HopState {
    acc: StatsAccumulator,
    last_addr: Option<IpAddr>,
    last_code: Option<ResultCode>,
    last_ms: Option<f64>,
}

/// One hop's outcome within a single completed round (owned, no State borrow).
struct RoundHop {
    ttl: u8,
    addr: Option<IpAddr>,
    rtt_ms: Option<f64>,
}

/// Runs a continuous trace until `cancel` fires, emitting one [`TraceUpdate`]
/// snapshot per round into `tx`. Returns `Ok(())` on clean cancellation or when
/// the receiver is dropped.
pub async fn run_trace(
    session_id: u64,
    config: TraceConfig,
    tx: mpsc::Sender<TraceUpdate>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let target = resolve_target(&config.target, config.ip_version).await?;
    let privilege_mode = pick_privilege_mode();
    let interval = Duration::from_millis(config.interval_ms.max(100) as u64);
    let max_ttl = config.max_hops.clamp(1, 255) as u8;

    let mut hops: BTreeMap<u8, HopState> = BTreeMap::new();
    let mut seq: u64 = 0;

    while !cancel.is_cancelled() {
        // One bounded sweep on a blocking thread — never blocks the runtime.
        let round = tokio::task::spawn_blocking(move || {
            run_one_round(target, privilege_mode, max_ttl, interval)
        })
        .await
        .context("trace round task panicked")?;

        let round = match round {
            Ok(r) => r,
            Err(e) => {
                // Surface the failure to the UI as an empty snapshot, then stop.
                let _ = tx.send(error_update(session_id, seq, &config, target)).await;
                return Err(e);
            }
        };

        // Fold the round into our own accumulators (the PingData port).
        for rh in &round {
            let entry = hops.entry(rh.ttl).or_default();
            let is_target = rh.addr == Some(target);
            match rh.rtt_ms {
                Some(ms) => {
                    let code = if is_target {
                        ResultCode::Ok
                    } else {
                        ResultCode::TimeExceeded
                    };
                    entry.acc.record(code, Some(ms / 1000.0));
                    entry.last_addr = rh.addr;
                    entry.last_code = Some(code);
                    entry.last_ms = Some(ms);
                }
                None => {
                    entry.acc.record(ResultCode::NoReply, None);
                    entry.last_code = Some(ResultCode::NoReply);
                    entry.last_ms = None;
                }
            }
        }

        seq += 1;
        let completed = hops
            .values()
            .any(|h| h.last_addr == Some(target) && matches!(h.last_code, Some(ResultCode::Ok)));
        let update = build_update(session_id, seq, &config, target, &hops, completed);

        if tx.send(update).await.is_err() {
            break; // receiver dropped
        }
    }
    Ok(())
}

/// Builds and runs a single trippy round, returning per-hop results. Blocking.
fn run_one_round(
    target: IpAddr,
    privilege_mode: PrivilegeMode,
    max_ttl: u8,
    interval: Duration,
) -> anyhow::Result<Vec<RoundHop>> {
    let tracer = Builder::new(target)
        .protocol(Protocol::Icmp)
        .privilege_mode(privilege_mode)
        .max_ttl(max_ttl)
        .max_rounds(Some(1))
        .min_round_duration(interval)
        .max_round_duration(interval)
        .grace_duration(Duration::from_millis(100))
        .build()
        .context("failed to build tracer (check privileges: setcap cap_net_raw or run as root)")?;

    tracer.run().context("trace round failed")?;
    let state = tracer.snapshot();
    if let Some(err) = state.error() {
        return Err(anyhow!("tracer error: {err}"));
    }

    Ok(state
        .hops()
        .iter()
        .map(|hop| RoundHop {
            ttl: hop.ttl(),
            addr: hop.addrs().next().copied(),
            rtt_ms: hop.last_ms(),
        })
        .collect())
}

fn build_update(
    session_id: u64,
    seq: u64,
    config: &TraceConfig,
    target: IpAddr,
    hops: &BTreeMap<u8, HopState>,
    completed: bool,
) -> TraceUpdate {
    let out_hops: Vec<Hop> = hops
        .iter()
        .map(|(&ttl, st)| {
            let stats = st.acc.snapshot();
            Hop {
                ttl,
                sample_number: stats.sent,
                addr: st.last_addr.map(|a| a.to_string()),
                host: None, // reverse DNS deferred (Phase 1)
                code: st.last_code.unwrap_or(ResultCode::NoReply),
                current_ms: st.last_ms,
                stats,
            }
        })
        .collect();

    let total_hops = out_hops.len() as u16;
    TraceUpdate {
        session_id,
        seq,
        target: config.target.clone(),
        resolved_addr: Some(target.to_string()),
        ip_version: config.ip_version,
        hops: out_hops,
        total_hops,
        max_hops: config.max_hops,
        completed,
        interval_ms: config.interval_ms,
    }
}

fn error_update(session_id: u64, seq: u64, config: &TraceConfig, target: IpAddr) -> TraceUpdate {
    TraceUpdate {
        session_id,
        seq,
        target: config.target.clone(),
        resolved_addr: Some(target.to_string()),
        ip_version: config.ip_version,
        hops: Vec::new(),
        total_hops: 0,
        max_hops: config.max_hops,
        completed: false,
        interval_ms: config.interval_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_ip_literal_without_dns() {
        let ip = resolve_target("8.8.4.4", IpVersion::V4).await.unwrap();
        assert_eq!(ip.to_string(), "8.8.4.4");
    }

    #[test]
    fn engine_info_is_populated() {
        let info = engine_info();
        assert_eq!(info.id, TRIPPY_ENGINE_ID);
        assert!(!info.description.is_empty());
    }
}
