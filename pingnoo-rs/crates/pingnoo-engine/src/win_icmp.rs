//! Windows ICMP engine using the IP Helper API (`IcmpSendEcho`).
//!
//! Requires **no administrator privileges** (unlike raw sockets, which fail with
//! `WSAEACCES` when unelevated), mirroring the legacy `ICMPAPIPingEngine`. IPv4
//! only for now; one round sweeps TTLs `1..=max_ttl` sequentially, setting the
//! per-probe TTL via `IP_OPTION_INFORMATION` and stopping when the target
//! replies.
//!
//! Real routes often contain long runs of routers that silently drop
//! TTL-expired traffic and then a perfectly reachable destination, so a naive
//! "give up after N silent hops" rule permanently truncates such routes. Each
//! round therefore starts with one full-TTL probe of the destination: if the
//! destination answers, the sweep pushes through silent runs all the way to it;
//! the early cutoff only applies when the destination itself is unreachable
//! (where sweeping to max_ttl would just burn `max_ttl × timeout`).

use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use anyhow::{anyhow, Context};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
};

use crate::RoundHop;

// IP_STATUS codes from <ipexport.h>.
const IP_SUCCESS: u32 = 0;
const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;

/// When the destination is unreachable, stop a sweep after this many
/// consecutive silent hops so each round stays bounded.
const MAX_CONSECUTIVE_TIMEOUTS: u32 = 5;

/// Outcome of one `IcmpSendEcho` probe.
struct Probe {
    addr: Option<IpAddr>,
    rtt_ms: Option<f64>,
    reached_target: bool,
}

/// Runs one traceroute round. `known_target_ttl` (learned in a previous round)
/// bounds the sweep; the returned `Option<u8>` is the (re)learned target TTL.
pub(crate) fn run_one_round_win(
    target: IpAddr,
    max_ttl: u8,
    timeout: Duration,
    known_target_ttl: Option<u8>,
) -> anyhow::Result<(Vec<RoundHop>, Option<u8>)> {
    let v4 = match target {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => {
            return Err(anyhow!(
                "the Windows engine currently supports IPv4 only — switch the session to IPv4"
            ))
        }
    };
    // DestinationAddress is in network byte order; on little-endian Windows the
    // native-endian u32 of the octets already has that byte layout.
    let dest = u32::from_ne_bytes(v4.octets());
    let timeout_ms = timeout.as_millis().clamp(1, u32::MAX as u128) as u32;
    let request = [0x61u8; 32];

    // SAFETY: the handle is closed on every path; `reply` outlives each call
    // and is sized per the API contract (reply struct + payload + slack).
    unsafe {
        let handle = IcmpCreateFile().context("IcmpCreateFile failed")?;
        let result = (|| {
            let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + request.len() + 16];

            // Destination reachability probe at full TTL: decides whether the
            // sweep pushes through silent runs or applies the early cutoff.
            let sweep_max = known_target_ttl.unwrap_or(max_ttl);
            let dest_probe = send_probe(handle, dest, &request, &mut reply, sweep_max, timeout_ms, v4);
            let dest_reachable = dest_probe.reached_target;

            let mut hops: Vec<RoundHop> = Vec::new();
            let mut learned_ttl: Option<u8> = None;
            let mut consecutive_timeouts = 0u32;

            for ttl in 1..=sweep_max {
                let p = send_probe(handle, dest, &request, &mut reply, ttl, timeout_ms, v4);
                let silent = p.addr.is_none();
                let reached = p.reached_target;
                hops.push(RoundHop {
                    ttl,
                    addr: p.addr,
                    rtt_ms: p.rtt_ms,
                });

                if reached {
                    learned_ttl = Some(ttl);
                    break;
                }
                if silent {
                    consecutive_timeouts += 1;
                    // Only truncate when the destination itself did not answer;
                    // otherwise push through silent segments to reach it.
                    if !dest_reachable && consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
                        break;
                    }
                } else {
                    consecutive_timeouts = 0;
                }
            }

            // The destination answered the reachability probe but dropped the
            // in-sweep one: credit the round with the direct measurement so the
            // final hop doesn't read as loss.
            if learned_ttl.is_none() && dest_reachable {
                hops.push(RoundHop {
                    ttl: sweep_max,
                    addr: dest_probe.addr,
                    rtt_ms: dest_probe.rtt_ms,
                });
                learned_ttl = Some(sweep_max);
            }

            Ok((hops, learned_ttl.or(known_target_ttl)))
        })();
        let _ = IcmpCloseHandle(handle);
        result
    }
}

/// Sends one echo with the given TTL and classifies the reply.
///
/// Note: unreachable-class replies (e.g. `IP_DEST_HOST_UNREACHABLE`) are
/// currently treated as silence; a future refinement could surface the
/// reporting router's address.
unsafe fn send_probe(
    handle: HANDLE,
    dest: u32,
    request: &[u8],
    reply: &mut [u8],
    ttl: u8,
    timeout_ms: u32,
    target_v4: Ipv4Addr,
) -> Probe {
    let opts = IP_OPTION_INFORMATION {
        Ttl: ttl,
        Tos: 0,
        Flags: 0,
        OptionsSize: 0,
        OptionsData: std::ptr::null_mut(),
    };
    let replies = IcmpSendEcho(
        handle,
        dest,
        request.as_ptr() as *const c_void,
        request.len() as u16,
        Some(&opts),
        reply.as_mut_ptr() as *mut c_void,
        reply.len() as u32,
        timeout_ms,
    );

    if replies == 0 {
        return Probe {
            addr: None,
            rtt_ms: None,
            reached_target: false,
        };
    }
    // The reply lands in a byte buffer with no alignment guarantee — copy it
    // out unaligned rather than dereferencing a potentially misaligned pointer.
    let echo: ICMP_ECHO_REPLY = std::ptr::read_unaligned(reply.as_ptr() as *const ICMP_ECHO_REPLY);
    let responder = Ipv4Addr::from(echo.Address.to_ne_bytes());
    let rtt = echo.RoundTripTime as f64;
    match echo.Status {
        IP_SUCCESS => Probe {
            addr: Some(IpAddr::V4(responder)),
            rtt_ms: Some(rtt),
            reached_target: responder == target_v4,
        },
        IP_TTL_EXPIRED_TRANSIT => Probe {
            addr: Some(IpAddr::V4(responder)),
            rtt_ms: Some(rtt),
            reached_target: false,
        },
        _ => Probe {
            addr: None,
            rtt_ms: None,
            reached_target: false,
        },
    }
}
