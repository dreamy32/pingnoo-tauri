//! Windows ICMP engine using the IP Helper API (`IcmpSendEcho`).
//!
//! Requires **no administrator privileges** (unlike raw sockets, which fail with
//! `WSAEACCES` when unelevated), mirroring the legacy `ICMPAPIPingEngine`. IPv4
//! only for now; one round sweeps TTLs `1..=max_ttl` sequentially, setting the
//! per-probe TTL via `IP_OPTION_INFORMATION` and stopping when the target
//! replies (or after a run of silent hops that indicates a blackhole).

use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use anyhow::{anyhow, Context};
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
};

use crate::RoundHop;

// IP_STATUS codes from <ipexport.h>.
const IP_SUCCESS: u32 = 0;
const IP_REQ_TIMED_OUT: u32 = 11010;
const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;

/// Stop a sweep after this many consecutive silent hops (likely a blackhole),
/// so an unreachable target doesn't make every round wait `max_ttl` timeouts.
const MAX_CONSECUTIVE_TIMEOUTS: u32 = 5;

pub(crate) fn run_one_round_win(
    target: IpAddr,
    max_ttl: u8,
    timeout: Duration,
) -> anyhow::Result<Vec<RoundHop>> {
    let v4 = match target {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => return Err(anyhow!("the Windows engine currently supports IPv4 only")),
    };
    // DestinationAddress is in network byte order; on little-endian Windows the
    // native-endian u32 of the octets already has that byte layout.
    let dest = u32::from_ne_bytes(v4.octets());
    let timeout_ms = timeout.as_millis().clamp(1, u32::MAX as u128) as u32;
    let request = [0x61u8; 32];

    // SAFETY: all pointers below outlive each `IcmpSendEcho` call, `reply` is
    // sized per the API contract, and the handle is closed on every path.
    unsafe {
        let handle = IcmpCreateFile().context("IcmpCreateFile failed")?;
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + request.len() + 16];
        let mut hops: Vec<RoundHop> = Vec::new();
        let mut consecutive_timeouts = 0u32;

        for ttl in 1..=max_ttl {
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

            let (addr, rtt_ms, reached_target, silent) = if replies == 0 {
                (None, None, false, true)
            } else {
                let echo = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
                let responder = IpAddr::V4(Ipv4Addr::from(echo.Address.to_ne_bytes()));
                let rtt = echo.RoundTripTime as f64;
                match echo.Status {
                    IP_SUCCESS => (Some(responder), Some(rtt), true, false),
                    IP_TTL_EXPIRED_TRANSIT => (Some(responder), Some(rtt), false, false),
                    IP_REQ_TIMED_OUT => (None, None, false, true),
                    _ => (None, None, false, true),
                }
            };

            hops.push(RoundHop { ttl, addr, rtt_ms });

            if reached_target {
                break;
            }
            if silent {
                consecutive_timeouts += 1;
                if consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
                    break;
                }
            } else {
                consecutive_timeouts = 0;
            }
        }

        let _ = IcmpCloseHandle(handle);
        Ok(hops)
    }
}
