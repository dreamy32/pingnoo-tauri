//! Headless soak/verification harness for the streaming trace core.
//!
//! Runs a real ICMP trace and prints one line per hop for each round, proving
//! the non-blocking engine + snapshot streaming end-to-end without a GUI.
//!
//! Usage: `trace-demo [host] [rounds] [interval_ms]`
//!   e.g. `trace-demo 1.1.1.1 5 1000`

use std::time::Instant;

use pingnoo_core::{IpVersion, TraceConfig};
use pingnoo_engine::run_trace;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let host = args.next().unwrap_or_else(|| "1.1.1.1".to_string());
    let rounds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let interval_ms: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1000);

    let config = TraceConfig {
        target: host.clone(),
        ip_version: IpVersion::V4,
        interval_ms,
        timeout_ms: interval_ms,
        max_hops: 30,
    };

    println!(
        "engine: {}\ntracing {host} — {rounds} rounds @ {interval_ms}ms\n",
        pingnoo_engine::engine_info().description
    );

    let (tx, mut rx) = mpsc::channel(64);
    let cancel = CancellationToken::new();
    let started = Instant::now();

    let handle = {
        let cancel = cancel.clone();
        tokio::spawn(async move { run_trace(1, config, tx, cancel).await })
    };

    let mut seen = 0u64;
    while let Some(update) = rx.recv().await {
        seen += 1;
        println!(
            "── round {} @ {:.1}s  hops={}  completed={}  resolved={}",
            update.seq,
            started.elapsed().as_secs_f64(),
            update.total_hops,
            update.completed,
            update.resolved_addr.as_deref().unwrap_or("?"),
        );
        for hop in &update.hops {
            let addr = hop.addr.as_deref().unwrap_or("*");
            let last = hop
                .current_ms
                .map(|v| format!("{v:6.1}ms"))
                .unwrap_or_else(|| "     *  ".to_string());
            let s = &hop.stats;
            println!(
                "  {:>2}. {:<39} last={last} avg={:>6} loss={:>5.1}% jit={:>6}",
                hop.ttl,
                addr,
                s.avg_ms.map(|v| format!("{v:.1}")).unwrap_or_else(|| "-".into()),
                s.loss_pct,
                s.jitter_ms.map(|v| format!("{v:.1}")).unwrap_or_else(|| "-".into()),
            );
        }
        println!();

        if seen >= rounds {
            break;
        }
    }

    // Clean shutdown: signal cancellation and join the trace task.
    cancel.cancel();
    match handle.await {
        Ok(Ok(())) => println!("✓ trace stopped cleanly after {seen} rounds"),
        Ok(Err(e)) => {
            eprintln!("✗ trace error: {e:#}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("✗ join error: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}
