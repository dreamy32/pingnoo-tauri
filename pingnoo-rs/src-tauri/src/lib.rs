//! Pingnoo Tauri shell — the thin control/data plane between the Svelte webview
//! and the Rust engine.
//!
//! * **Control plane**: `#[tauri::command]`s (`start_session`, `stop_session`,
//!   `attach_session`, `list_sessions`, `list_engines`).
//! * **Data plane**: one [`tauri::ipc::Channel`] per session. A bounded mpsc
//!   sits between the engine and a *forwarder* task that latest-wins coalesces
//!   whatever is queued into a single [`TraceUpdate`] before `channel.send()`.
//!
//! Sessions OUTLIVE the webview on purpose: minimizing suspends the renderer
//! (see `power.rs`) and detaches the channel, but the engine keeps tracing.
//! When the webview returns (restore — or even a full page reload if WebView2
//! reclaimed the renderer), the frontend calls `list_sessions` /
//! `attach_session` and picks the streams back up, with the latest snapshot
//! replayed immediately so the UI repopulates without waiting a round.

mod power;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pingnoo_core::{EngineInfo, IpVersion, TraceConfig, TraceUpdate};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Static description of a running session; lets a reloaded frontend rebuild
/// its tabs via `list_sessions`.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    id: u64,
    target: String,
    ip_version: IpVersion,
    interval_ms: u32,
    max_hops: u16,
}

struct SessionEntry {
    cancel: CancellationToken,
    info: SessionInfo,
    /// Where the forwarder delivers snapshots. `None` while no webview is
    /// attached (minimized/suspended or mid-reload) — the engine keeps running
    /// and nothing queues up.
    channel: Arc<Mutex<Option<Channel<TraceUpdate>>>>,
    /// Most recent snapshot, replayed immediately on (re)attach.
    last: Arc<Mutex<Option<TraceUpdate>>>,
}

/// Live sessions, keyed by id.
#[derive(Default, Clone)]
struct Sessions {
    next_id: Arc<AtomicU64>,
    active: Arc<Mutex<HashMap<u64, SessionEntry>>>,
}

impl Sessions {
    /// Detaches every webview channel. Called before suspending the renderer so
    /// no IPC accumulates while the UI cannot consume it; engines keep tracing.
    /// (Only the Windows suspend path calls this today.)
    #[cfg_attr(not(windows), allow(dead_code))]
    fn detach_all(&self) {
        for entry in self.active.lock().unwrap().values() {
            entry.channel.lock().unwrap().take();
        }
    }
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

/// Lists running trace sessions (for a freshly-loaded webview to rebuild tabs).
#[tauri::command]
fn list_sessions(sessions: State<'_, Sessions>) -> Vec<SessionInfo> {
    let mut infos: Vec<SessionInfo> = sessions
        .active
        .lock()
        .unwrap()
        .values()
        .map(|e| e.info.clone())
        .collect();
    infos.sort_by_key(|i| i.id);
    infos
}

/// (Re)attaches a webview channel to a running session and immediately replays
/// the latest snapshot so the UI repopulates without waiting for the next round.
#[tauri::command]
fn attach_session(
    id: u64,
    channel: Channel<TraceUpdate>,
    sessions: State<'_, Sessions>,
) -> Result<SessionInfo, String> {
    let active = sessions.active.lock().unwrap();
    let entry = active.get(&id).ok_or("session is no longer running")?;
    // Install first, then replay: no live update can slip through unseen
    // between the two. The replay may arrive after a newer live update — the
    // client's monotonic-seq guard discards it in that case.
    let replay = {
        let mut slot = entry.channel.lock().unwrap();
        *slot = Some(channel.clone());
        entry.last.lock().unwrap().clone()
    };
    if let Some(update) = replay {
        let _ = channel.send(update);
    }
    Ok(entry.info.clone())
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
    let slot = Arc::new(Mutex::new(Some(channel)));
    let last: Arc<Mutex<Option<TraceUpdate>>> = Arc::new(Mutex::new(None));
    sessions.active.lock().unwrap().insert(
        id,
        SessionEntry {
            cancel: cancel.clone(),
            info: SessionInfo {
                id,
                target: config.target.clone(),
                ip_version: config.ip_version,
                interval_ms: config.interval_ms,
                max_hops: config.max_hops,
            },
            channel: slot.clone(),
            last: last.clone(),
        },
    );

    // Bounded channel: engine → forwarder. Small, because the forwarder always
    // drains to the latest value (back-pressure without unbounded growth).
    let (tx, mut rx) = mpsc::channel::<TraceUpdate>(16);

    // Forwarder: latest-wins coalescing onto the webview channel. A detached or
    // dead webview NEVER stops the engine — the snapshot just lands in `last`
    // and is replayed on the next attach.
    tauri::async_runtime::spawn(async move {
        while let Some(mut latest) = rx.recv().await {
            while let Ok(next) = rx.try_recv() {
                latest = next; // coalesce queued snapshots to the newest
            }
            *last.lock().unwrap() = Some(latest.clone());
            let attached = slot.lock().unwrap().clone();
            if let Some(ch) = attached {
                if ch.send(latest).is_err() {
                    // Webview gone (suspended renderer, reload, closed window):
                    // detach and keep tracing. Identity-checked so a channel
                    // that attach_session just swapped in is never removed by
                    // a failure on the OLD channel.
                    let mut cur = slot.lock().unwrap();
                    if cur.as_ref().map(|c| c.id()) == Some(ch.id()) {
                        cur.take();
                    }
                }
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
    if let Some(entry) = sessions.active.lock().unwrap().remove(&id) {
        // Detach first so an in-flight round can't deliver one more update to
        // a tab the user just stopped.
        entry.channel.lock().unwrap().take();
        entry.cancel.cancel();
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
        .on_window_event(power::handle_window_event)
        .invoke_handler(tauri::generate_handler![
            list_engines,
            list_sessions,
            attach_session,
            start_session,
            stop_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running pingnoo");
}
