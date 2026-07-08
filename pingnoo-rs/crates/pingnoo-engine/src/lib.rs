//! `pingnoo-engine` — async traceroute/ping engine(s).
//!
//! Engine per platform, behind one [`run_trace`] driver:
//!
//! * **Unix/macOS** wrap [`trippy_core`] (raw-socket ICMP + reply correlation +
//!   privilege handling), driven as bounded single-round sweeps in a loop we
//!   control so cancellation is clean and no trace thread is ever orphaned.
//! * **Windows** use the IP Helper API (`IcmpCreateFile` + `IcmpSendEcho` with a
//!   per-TTL TTL option) — **no Administrator required**, mirroring the legacy
//!   `ICMPAPIPingEngine`.
//!
//! Either way our own [`StatsAccumulator`] (the port of `PingData`) is the
//! single source of truth for the render-ready per-hop stats, and the blocking
//! probe work runs on `spawn_blocking` so the tokio runtime — and the UI it
//! drives — is never blocked.

use std::collections::BTreeMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use pingnoo_core::{
    EngineInfo, Hop, IpVersion, ResultCode, StatsAccumulator, TraceConfig, TraceUpdate,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(not(windows))]
use trippy_core::{Builder, PrivilegeMode, Protocol};
#[cfg(not(windows))]
use trippy_privilege::Privilege;

#[cfg(windows)]
mod win_icmp;

/// Stable identifier for the active engine.
#[cfg(not(windows))]
pub const ENGINE_ID: &str = "trippy-icmp";
#[cfg(windows)]
pub const ENGINE_ID: &str = "win-icmp";

/// Describes the engine and whether it can run on this host right now.
#[cfg(not(windows))]
pub fn engine_info() -> EngineInfo {
    let (available, note) = match Privilege::discover() {
        Ok(p) if p.has_privileges() => (true, "raw socket (privileged)"),
        Ok(p) if !p.needs_privileges() => (true, "unprivileged ICMP datagram"),
        Ok(_) => (false, "needs raw-socket privileges (setcap/root)"),
        Err(_) => (true, "privilege state unknown"),
    };
    EngineInfo {
        id: ENGINE_ID.to_string(),
        description: format!("ICMP traceroute via trippy — {note}"),
        priority: 100,
        available,
    }
}

/// Windows engine — never needs elevation.
#[cfg(windows)]
pub fn engine_info() -> EngineInfo {
    EngineInfo {
        id: ENGINE_ID.to_string(),
        description: "ICMP echo via Windows IP Helper (IcmpSendEcho) — no administrator required"
            .to_string(),
        priority: 100,
        available: true,
    }
}

#[cfg(not(windows))]
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

/// One hop's outcome within a single completed round (owned, no engine borrow).
pub(crate) struct RoundHop {
    pub(crate) ttl: u8,
    pub(crate) addr: Option<IpAddr>,
    pub(crate) rtt_ms: Option<f64>,
}

/// Whether the platform's round fn already blocks for ~the interval (trippy) or
/// returns as fast as it can and needs the driver to pace it (Windows sweep).
#[cfg(not(windows))]
const ROUND_SELF_PACED: bool = true;
#[cfg(windows)]
const ROUND_SELF_PACED: bool = false;

/// Runs a continuous trace until `cancel` fires, emitting one [`TraceUpdate`]
/// snapshot per round into `tx`. On any failure it emits a final snapshot whose
/// `error` is set (so the UI can show a banner) and returns `Err`.
pub async fn run_trace(
    session_id: u64,
    config: TraceConfig,
    tx: mpsc::Sender<TraceUpdate>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let target = match resolve_target(&config.target, config.ip_version).await {
        Ok(t) => t,
        Err(e) => {
            let _ = tx
                .send(error_update(session_id, 0, &config, None, format!("{e:#}")))
                .await;
            return Err(e);
        }
    };

    #[cfg(not(windows))]
    let privilege_mode = pick_privilege_mode();
    let interval = Duration::from_millis(config.interval_ms.max(100) as u64);
    #[cfg(windows)]
    let timeout = Duration::from_millis(config.timeout_ms.clamp(100, 60_000) as u64);
    let max_ttl = config.max_hops.clamp(1, 255) as u8;

    let mut hops: BTreeMap<u8, HopState> = BTreeMap::new();
    let mut seq: u64 = 0;

    while !cancel.is_cancelled() {
        let round_start = Instant::now();

        // One bounded sweep on a blocking thread — never blocks the runtime.
        let join = {
            #[cfg(windows)]
            {
                let (t, to) = (target, timeout);
                tokio::task::spawn_blocking(move || win_icmp::run_one_round_win(t, max_ttl, to))
                    .await
            }
            #[cfg(not(windows))]
            {
                let (t, pm, iv) = (target, privilege_mode, interval);
                tokio::task::spawn_blocking(move || run_one_round(t, pm, max_ttl, iv)).await
            }
        };

        let round = match join.context("trace round task panicked")? {
            Ok(r) => r,
            Err(e) => {
                let _ = tx
                    .send(error_update(session_id, seq, &config, Some(target), format!("{e:#}")))
                    .await;
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

        // Pace to the interval when the round fn does not do so itself.
        if !ROUND_SELF_PACED {
            let elapsed = round_start.elapsed();
            if elapsed < interval {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(interval - elapsed) => {}
                }
            }
        }
    }
    Ok(())
}

/// Builds and runs a single trippy round, returning per-hop results. Blocking.
#[cfg(not(windows))]
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
        error: None,
    }
}

fn error_update(
    session_id: u64,
    seq: u64,
    config: &TraceConfig,
    resolved: Option<IpAddr>,
    msg: String,
) -> TraceUpdate {
    TraceUpdate {
        session_id,
        seq,
        target: config.target.clone(),
        resolved_addr: resolved.map(|a| a.to_string()),
        ip_version: config.ip_version,
        hops: Vec::new(),
        total_hops: 0,
        max_hops: config.max_hops,
        completed: false,
        interval_ms: config.interval_ms,
        error: Some(msg),
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
        assert_eq!(info.id, ENGINE_ID);
        assert!(!info.description.is_empty());
    }
}
