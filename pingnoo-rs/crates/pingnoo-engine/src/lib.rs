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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, ToSocketAddrs};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
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

/// trippy rejects TTLs above 254; keep every path inside that bound.
const MAX_SUPPORTED_TTL: u16 = 254;

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

/// Windows engine — never needs elevation. IPv4 only for now.
#[cfg(windows)]
pub fn engine_info() -> EngineInfo {
    EngineInfo {
        id: ENGINE_ID.to_string(),
        description: "ICMP echo via Windows IP Helper (IcmpSendEcho) — no administrator required, IPv4 only"
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
/// family. Strict: if the host has no address in that family this errors
/// instead of silently tracing the other family (which would make the UI's
/// IPv4/IPv6 toggle lie). Runs on a blocking thread.
pub async fn resolve_target(host: &str, ip_version: IpVersion) -> anyhow::Result<IpAddr> {
    let want_v6 = matches!(ip_version, IpVersion::V6);
    if let Ok(ip) = IpAddr::from_str(host.trim()) {
        return match (want_v6, ip) {
            (false, IpAddr::V4(_)) | (true, IpAddr::V6(_)) => Ok(ip),
            (false, IpAddr::V6(_)) => Err(anyhow!("that is an IPv6 address — switch to IPv6")),
            (true, IpAddr::V4(_)) => Err(anyhow!("that is an IPv4 address — switch to IPv4")),
        };
    }
    let host = host.trim().to_string();
    let addrs = tokio::task::spawn_blocking(move || {
        (host.as_str(), 0u16)
            .to_socket_addrs()
            .map(|it| it.map(|sa| sa.ip()).collect::<Vec<_>>())
    })
    .await
    .context("dns resolver task panicked")?
    .context("dns lookup failed")?;

    let family = if want_v6 { "IPv6" } else { "IPv4" };
    addrs
        .into_iter()
        .find(|ip| matches!((want_v6, ip), (false, IpAddr::V4(_)) | (true, IpAddr::V6(_))))
        .ok_or_else(|| anyhow!("host has no {family} address"))
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
                .send(error_update(session_id, 1, &config, None, format!("{e:#}")))
                .await;
            return Err(e);
        }
    };

    #[cfg(not(windows))]
    let privilege_mode = pick_privilege_mode();
    let interval = Duration::from_millis(config.interval_ms.max(100) as u64);
    #[cfg(windows)]
    let timeout = Duration::from_millis(config.timeout_ms.clamp(100, 60_000) as u64);
    let max_ttl = config.max_hops.clamp(1, MAX_SUPPORTED_TTL) as u8;

    let mut hops: BTreeMap<u8, HopState> = BTreeMap::new();
    let mut seq: u64 = 0;
    // Latched once the destination has ever replied (the wire contract
    // documents `completed` as "route fully discovered", not "target answered
    // this round"), plus the TTL it answered at so later rounds can prune
    // phantom hops probed past the destination.
    let mut completed = false;
    let mut target_ttl: Option<u8> = None;
    // Routes can lengthen: if the destination stops answering at the latched
    // TTL for a few rounds, unlatch so the sweep widens back to max_ttl.
    let mut target_miss_rounds = 0u32;

    // Reverse-DNS runs in the background; names appear a round or two after a
    // hop's address is first seen and never block the sample loop. `attempted`
    // is the negative cache: one lookup per address, ever.
    let dns_cache: Arc<Mutex<HashMap<IpAddr, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut dns_attempted: HashSet<IpAddr> = HashSet::new();

    while !cancel.is_cancelled() {
        let round_start = Instant::now();

        // One bounded sweep on a blocking thread — never blocks the runtime.
        // Cancellation is observed while the round is in flight: we stop
        // waiting immediately and let the (bounded, ≤ round duration) blocking
        // task finish in the background rather than emit a post-stop update.
        let join = {
            #[cfg(windows)]
            {
                let (t, to, kt) = (target, timeout, target_ttl);
                tokio::task::spawn_blocking(move || {
                    win_icmp::run_one_round_win(t, max_ttl, to, kt).map(|(hops, _)| hops)
                })
            }
            #[cfg(not(windows))]
            {
                let (t, pm, iv) = (target, privilege_mode, interval);
                tokio::task::spawn_blocking(move || run_one_round(t, pm, max_ttl, iv, session_id))
            }
        };
        let joined = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            j = join => j,
        };

        let round = match joined.context("trace round task panicked")? {
            Ok(r) => r,
            Err(e) => {
                // seq+1 so the client's monotonic-seq guard never drops the
                // error snapshot (a duplicate seq would be ignored).
                let _ = tx
                    .send(error_update(session_id, seq + 1, &config, Some(target), format!("{e:#}")))
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
                    if is_target {
                        completed = true;
                        target_ttl = Some(rh.ttl);
                    }
                }
                None => {
                    entry.acc.record(ResultCode::NoReply, None);
                    entry.last_code = Some(ResultCode::NoReply);
                    entry.last_ms = None;
                }
            }
        }

        // Route-lengthening recovery: unlatch the target TTL if the
        // destination has gone quiet at it for several consecutive rounds.
        let target_replied = round
            .iter()
            .any(|rh| rh.addr == Some(target) && rh.rtt_ms.is_some());
        if target_replied {
            target_miss_rounds = 0;
        } else if target_ttl.is_some() {
            target_miss_rounds += 1;
            if target_miss_rounds >= 3 {
                target_ttl = None;
                target_miss_rounds = 0;
            }
        }

        // Drop phantom hops beyond the destination: when the target drops one
        // probe, the sweep runs past it and materialises trailing all-timeout
        // hops that would otherwise persist (and paint loss) forever.
        if let Some(t) = target_ttl {
            hops.retain(|&ttl, _| ttl <= t);
        }

        // Kick off reverse-DNS for newly-seen addresses (non-blocking, once per
        // address — failures are negatively cached by `dns_attempted`).
        for ip in hops.values().filter_map(|h| h.last_addr) {
            if dns_attempted.insert(ip) {
                let cache = dns_cache.clone();
                tokio::task::spawn_blocking(move || {
                    if let Ok(name) = dns_lookup::lookup_addr(&ip) {
                        if name != ip.to_string() {
                            cache.lock().unwrap().insert(ip, name);
                        }
                    }
                });
            }
        }

        seq += 1;
        let names = dns_cache.lock().unwrap().clone();
        let update = build_update(session_id, seq, &config, target, &hops, completed, &names);
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
    session_id: u64,
) -> anyhow::Result<Vec<RoundHop>> {
    // trippy matches replies by (ICMP identifier, sequence window) and treats
    // identifier 0 as a wildcard, so concurrent sessions MUST each use a
    // distinct non-zero identifier — otherwise, in raw-socket mode (which sees
    // ALL inbound ICMP), two tabs cross-attribute each other's replies and both
    // hop tables silently corrupt. XOR with the pid adds cross-process
    // separation; the 0 remap keeps it non-zero. A disjoint per-session
    // sequence window is defence in depth against identifier-0 traffic.
    let trace_id = match (std::process::id() as u16) ^ (session_id as u16) {
        0 => 0x8000,
        id => id,
    };
    let initial_sequence = 33434 + ((session_id as u16 % 32) * 512);

    let tracer = Builder::new(target)
        .protocol(Protocol::Icmp)
        .privilege_mode(privilege_mode)
        .trace_identifier(trace_id)
        .initial_sequence(initial_sequence)
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

#[allow(clippy::too_many_arguments)]
fn build_update(
    session_id: u64,
    seq: u64,
    config: &TraceConfig,
    target: IpAddr,
    hops: &BTreeMap<u8, HopState>,
    completed: bool,
    names: &HashMap<IpAddr, String>,
) -> TraceUpdate {
    let out_hops: Vec<Hop> = hops
        .iter()
        .map(|(&ttl, st)| {
            let stats = st.acc.snapshot();
            Hop {
                ttl,
                sample_number: stats.sent,
                addr: st.last_addr.map(|a| a.to_string()),
                host: st.last_addr.and_then(|a| names.get(&a).cloned()),
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

    #[tokio::test]
    async fn rejects_wrong_family_ip_literal() {
        assert!(resolve_target("8.8.4.4", IpVersion::V6).await.is_err());
        assert!(resolve_target("2001:4860:4860::8888", IpVersion::V4).await.is_err());
    }

    #[test]
    fn engine_info_is_populated() {
        let info = engine_info();
        assert_eq!(info.id, ENGINE_ID);
        assert!(!info.description.is_empty());
    }
}
