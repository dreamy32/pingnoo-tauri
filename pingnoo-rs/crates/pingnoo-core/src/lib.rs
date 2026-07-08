//! `pingnoo-core` — pure domain types, the Rust↔webview wire contract, and a
//! render-ready statistics accumulator.
//!
//! No I/O lives here: everything is deterministic and unit-testable. The types
//! mirror the legacy Qt contracts (`PingResult.h`, `IRouteEngine.h`,
//! `PingData.cpp`) but are reshaped around a single *latest-wins* whole-trace
//! snapshot ([`TraceUpdate`]) that streams over one Tauri `ipc::Channel`.

use serde::{Deserialize, Serialize};

/// Outcome of a single probe. Ported from `PingResult::ResultCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResultCode {
    /// Echo reply from the destination — this hop *is* the target.
    Ok,
    /// No reply within the timeout window.
    NoReply,
    /// TTL exceeded from an intermediate router — an ordinary traceroute hop.
    TimeExceeded,
}

/// IP protocol family for a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum IpVersion {
    #[default]
    V4,
    V6,
}

/// A single probe result for one hop (ttl). Ported from `PingResult`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingSample {
    pub sample_number: u64,
    pub ttl: u8,
    pub code: ResultCode,
    /// Address that answered; may differ from the target on `TimeExceeded`.
    pub responder: Option<String>,
    /// Round-trip time in seconds; `None` when there was no reply.
    pub rtt_secs: Option<f64>,
}

/// Render-ready per-hop statistics. **All latencies are milliseconds** so the
/// webview does zero math. Ported from the running values in `PingData.cpp`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HopStats {
    pub sent: u64,
    pub received: u64,
    /// Packet loss percentage: `(sent - received) / sent * 100`.
    pub loss_pct: f64,
    pub last_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Mean absolute difference between consecutive reply RTTs (ms).
    pub jitter_ms: Option<f64>,
}

/// Accumulates probe outcomes for one hop into render-ready [`HopStats`].
///
/// This is the Rust port of `PingData`'s running min/max/avg, reply/timeout
/// counts, packet-loss and jitter — computed once in the backend so the UI
/// never recomputes on the render path.
#[derive(Debug, Clone, Default)]
pub struct StatsAccumulator {
    sent: u64,
    received: u64,
    sum_secs: f64,
    min_secs: Option<f64>,
    max_secs: Option<f64>,
    last_secs: Option<f64>,
    prev_secs: Option<f64>,
    jitter_sum: f64,
    jitter_count: u64,
}

impl StatsAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one probe outcome. `rtt_secs` is `Some(seconds)` on a reply,
    /// `None` on loss. Invalid RTTs (NaN/negative) are treated as losses.
    pub fn record(&mut self, code: ResultCode, rtt_secs: Option<f64>) {
        self.sent += 1;
        let good = matches!(code, ResultCode::Ok | ResultCode::TimeExceeded);
        match rtt_secs {
            Some(rtt) if good && rtt.is_finite() && rtt >= 0.0 => {
                self.received += 1;
                self.sum_secs += rtt;
                self.last_secs = Some(rtt);
                self.min_secs = Some(self.min_secs.map_or(rtt, |m| m.min(rtt)));
                self.max_secs = Some(self.max_secs.map_or(rtt, |m| m.max(rtt)));
                if let Some(prev) = self.prev_secs {
                    self.jitter_sum += (rtt - prev).abs();
                    self.jitter_count += 1;
                }
                self.prev_secs = Some(rtt);
            }
            _ => {
                // Loss: do not advance `prev_secs`; jitter across a gap is noise.
            }
        }
    }

    /// Produces a render-ready snapshot (latencies in ms).
    pub fn snapshot(&self) -> HopStats {
        let to_ms = |s: f64| s * 1000.0;
        let timeouts = self.sent.saturating_sub(self.received);
        let loss_pct = if self.sent == 0 {
            0.0
        } else {
            timeouts as f64 / self.sent as f64 * 100.0
        };
        HopStats {
            sent: self.sent,
            received: self.received,
            loss_pct,
            last_ms: self.last_secs.map(to_ms),
            avg_ms: (self.received > 0).then(|| to_ms(self.sum_secs / self.received as f64)),
            min_ms: self.min_secs.map(to_ms),
            max_ms: self.max_secs.map(to_ms),
            jitter_ms: (self.jitter_count > 0).then(|| to_ms(self.jitter_sum / self.jitter_count as f64)),
        }
    }
}

/// One hop in a whole-trace snapshot — render-ready for the webview.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub ttl: u8,
    /// Monotonic per-hop sample counter. The UI appends a new chart point only
    /// when this advances, so coalesced/dropped snapshots never double-plot.
    pub sample_number: u64,
    pub addr: Option<String>,
    pub host: Option<String>,
    pub code: ResultCode,
    /// Latest RTT in ms (`None` on loss).
    pub current_ms: Option<f64>,
    pub stats: HopStats,
}

/// A whole-trace latest-wins snapshot: the single payload on the data channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceUpdate {
    pub session_id: u64,
    /// Monotonic snapshot sequence (visibility into coalescing).
    pub seq: u64,
    pub target: String,
    pub resolved_addr: Option<String>,
    pub ip_version: IpVersion,
    pub hops: Vec<Hop>,
    pub total_hops: u16,
    pub max_hops: u16,
    /// True once the destination has replied (route fully discovered).
    pub completed: bool,
    pub interval_ms: u32,
    /// Set when the engine failed; the UI shows this and stops.
    #[serde(default)]
    pub error: Option<String>,
}

/// Configuration for a trace session. Ported from `IPingEngine`
/// (`setInterval`/`setTimeout`) plus the target/IP-version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceConfig {
    pub target: String,
    pub ip_version: IpVersion,
    pub interval_ms: u32,
    pub timeout_ms: u32,
    pub max_hops: u16,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            target: "1.1.1.1".to_string(),
            ip_version: IpVersion::V4,
            interval_ms: 1000,
            timeout_ms: 1000,
            max_hops: 64,
        }
    }
}

/// Static description of an engine implementation. Ported from
/// `IPingEngineFactory` (`description`/`priority`/`available`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub id: String,
    pub description: String,
    /// Higher = preferred when several engines are available.
    pub priority: i32,
    pub available: bool,
}

/// Sink for trace snapshots. Keeps the engine agnostic of Tauri/tokio: the
/// engine builds [`TraceUpdate`]s and hands them here; the transport (a bounded
/// mpsc, a Tauri `Channel`, or a test collector) decides what to do with them.
/// Object-safe and I/O-free — the extension seam without any DLL machinery.
pub trait TraceSink: Send {
    fn emit(&mut self, update: TraceUpdate);
}

impl<F: FnMut(TraceUpdate) + Send> TraceSink for F {
    fn emit(&mut self, update: TraceUpdate) {
        self(update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Option<f64>, b: f64) {
        let a = a.expect("expected Some");
        assert!((a - b).abs() < 1e-9, "expected ~{b}, got {a}");
    }

    #[test]
    fn empty_accumulator_is_clean() {
        let s = StatsAccumulator::new().snapshot();
        assert_eq!(s.sent, 0);
        assert_eq!(s.received, 0);
        assert_eq!(s.loss_pct, 0.0);
        assert!(s.avg_ms.is_none());
        assert!(s.jitter_ms.is_none());
    }

    #[test]
    fn avg_min_max_last_in_ms() {
        let mut a = StatsAccumulator::new();
        a.record(ResultCode::Ok, Some(0.010)); // 10ms
        a.record(ResultCode::Ok, Some(0.030)); // 30ms
        a.record(ResultCode::Ok, Some(0.020)); // 20ms
        let s = a.snapshot();
        assert_eq!(s.sent, 3);
        assert_eq!(s.received, 3);
        assert_eq!(s.loss_pct, 0.0);
        assert_eq!(s.last_ms, Some(20.0));
        assert_eq!(s.min_ms, Some(10.0));
        assert_eq!(s.max_ms, Some(30.0));
        assert_eq!(s.avg_ms, Some(20.0));
    }

    #[test]
    fn packet_loss_matches_legacy_formula() {
        let mut a = StatsAccumulator::new();
        a.record(ResultCode::Ok, Some(0.010));
        a.record(ResultCode::NoReply, None);
        a.record(ResultCode::NoReply, None);
        a.record(ResultCode::Ok, Some(0.010));
        let s = a.snapshot();
        // timeouts / sent * 100 = 2/4*100
        assert_eq!(s.sent, 4);
        assert_eq!(s.received, 2);
        assert_eq!(s.loss_pct, 50.0);
    }

    #[test]
    fn jitter_is_mean_abs_delta_of_consecutive_replies() {
        let mut a = StatsAccumulator::new();
        a.record(ResultCode::Ok, Some(0.010)); // no prev
        a.record(ResultCode::Ok, Some(0.020)); // |20-10| = 10
        a.record(ResultCode::Ok, Some(0.015)); // |15-20| = 5
        let s = a.snapshot();
        // mean(10, 5) = 7.5 ms
        approx(s.jitter_ms, 7.5);
    }

    #[test]
    fn loss_does_not_contribute_jitter_across_gaps() {
        let mut a = StatsAccumulator::new();
        a.record(ResultCode::Ok, Some(0.010));
        a.record(ResultCode::NoReply, None);
        a.record(ResultCode::Ok, Some(0.050)); // prev was 10ms (gap ignored)
        let s = a.snapshot();
        // Only one consecutive pair (10 -> 50) => 40ms
        approx(s.jitter_ms, 40.0);
    }

    #[test]
    fn invalid_rtt_is_treated_as_loss() {
        let mut a = StatsAccumulator::new();
        a.record(ResultCode::Ok, Some(f64::NAN));
        a.record(ResultCode::Ok, Some(-1.0));
        let s = a.snapshot();
        assert_eq!(s.sent, 2);
        assert_eq!(s.received, 0);
        assert_eq!(s.loss_pct, 100.0);
    }

    #[test]
    fn closure_is_a_trace_sink() {
        let mut seen = 0u64;
        {
            let mut sink = |u: TraceUpdate| seen = u.seq;
            sink.emit(TraceUpdate {
                session_id: 1,
                seq: 42,
                target: "x".into(),
                resolved_addr: None,
                ip_version: IpVersion::V4,
                hops: vec![],
                total_hops: 0,
                max_hops: 64,
                completed: false,
                interval_ms: 1000,
                error: None,
            });
        }
        assert_eq!(seen, 42);
    }
}
