//! Pingnoo Tauri shell — the thin control/data plane between the Svelte webview
//! and the Rust engine.
//!
//! * **Control plane**: `#[tauri::command]`s (`start_session`, `stop_session`,
//!   `list_engines`) that return `Result` for clean error surfacing.
//! * **Data plane**: one [`tauri::ipc::Channel`] per session. A bounded mpsc
//!   sits between the engine and a *forwarder* task that latest-wins coalesces
//!   whatever is queued into a single [`TraceUpdate`] before `channel.send()`,
//!   so a fast producer can never block the webview and paint is never stalled.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pingnoo_core::{EngineInfo, IpVersion, TraceConfig, TraceUpdate};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Live sessions, keyed by id, each holding its cancellation token.
#[derive(Default, Clone)]
struct Sessions {
    next_id: Arc<AtomicU64>,
    active: Arc<Mutex<HashMap<u64, CancellationToken>>>,
}

/// Parameters accepted from the frontend to start a trace.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartArgs {
    host: String,
    #[serde(default)]
    ip_version: IpVersion,
    #[serde(default = "default_interval")]
    interval_ms: u32,
    #[serde(default = "default_max_hops")]
    max_hops: u16,
}

fn default_interval() -> u32 {
    1000
}
fn default_max_hops() -> u16 {
    30
}

/// Lists the ping engines and whether each is usable on this host right now.
#[tauri::command]
fn list_engines() -> Vec<EngineInfo> {
    vec![pingnoo_engine::engine_info()]
}

/// Starts a trace session, streaming [`TraceUpdate`] snapshots over `channel`.
/// Returns the new session id.
#[tauri::command]
async fn start_session(
    args: StartArgs,
    channel: Channel<TraceUpdate>,
    sessions: State<'_, Sessions>,
) -> Result<u64, String> {
    let config = TraceConfig {
        target: args.host.trim().to_string(),
        ip_version: args.ip_version,
        interval_ms: args.interval_ms.clamp(100, 60_000),
        timeout_ms: args.interval_ms.clamp(100, 60_000),
        max_hops: args.max_hops.clamp(1, 255),
    };
    if config.target.is_empty() {
        return Err("please enter a host".into());
    }

    let id = sessions.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let cancel = CancellationToken::new();
    sessions.active.lock().unwrap().insert(id, cancel.clone());

    // Bounded channel: engine → forwarder. Small, because the forwarder always
    // drains to the latest value (back-pressure without unbounded growth).
    let (tx, mut rx) = mpsc::channel::<TraceUpdate>(16);

    // Forwarder: latest-wins coalescing onto the webview channel.
    tauri::async_runtime::spawn(async move {
        while let Some(mut latest) = rx.recv().await {
            while let Ok(next) = rx.try_recv() {
                latest = next; // coalesce queued snapshots to the newest
            }
            if channel.send(latest).is_err() {
                break; // webview/channel gone
            }
        }
    });

    // Engine task; removes itself from the registry when it ends.
    let active = sessions.active.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = pingnoo_engine::run_trace(id, config, tx, cancel).await {
            tracing::warn!(session = id, error = %format!("{e:#}"), "trace ended with error");
        }
        active.lock().unwrap().remove(&id);
    });

    Ok(id)
}

/// Stops a running session; safe to call for an unknown id.
#[tauri::command]
fn stop_session(id: u64, sessions: State<'_, Sessions>) -> Result<(), String> {
    if let Some(token) = sessions.active.lock().unwrap().remove(&id) {
        token.cancel();
    }
    Ok(())
}

/// Builds and runs the Tauri application.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .manage(Sessions::default())
        .invoke_handler(tauri::generate_handler![
            list_engines,
            start_session,
            stop_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running pingnoo");
}
