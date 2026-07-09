//! Power management: suspend the WebView2 renderer while the window is
//! minimized, dropping GPU/CPU for a backgrounded app to ~0.
//!
//! Windows-only (WebView2 `TrySuspend`); a no-op elsewhere. Notes vs the naive
//! approach: Tauri has no `WindowEvent::Minimized` — minimize arrives as a
//! `Resized` event and must be disambiguated with `Window::is_minimized()`.
//! `TrySuspend` also requires the controller to be hidden first and takes a
//! completion handler; suspension is best-effort by design (WebView2 refuses
//! while e.g. devtools are open, and any queued IPC resumes cleanly later).

#[cfg(windows)]
pub fn handle_window_event<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    event: &tauri::WindowEvent,
) {
    match event {
        tauri::WindowEvent::Resized(_) => {
            let minimized = window.is_minimized().unwrap_or(false);
            set_suspended(window, minimized);
        }
        tauri::WindowEvent::Focused(true) => set_suspended(window, false),
        _ => {}
    }
}

#[cfg(windows)]
fn set_suspended<R: tauri::Runtime>(window: &tauri::Window<R>, suspend: bool) {
    for webview in window.webviews() {
        let _ = webview.with_webview(move |platform| {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
            use webview2_com::TrySuspendCompletedHandler;
            use windows_core::Interface;

            let controller = platform.controller();
            unsafe {
                let Ok(core) = controller.CoreWebView2() else {
                    return;
                };
                // TrySuspend/Resume live on ICoreWebView2_3 (WebView2 SDK 1.0.865+).
                let Ok(core3) = core.cast::<ICoreWebView2_3>() else {
                    return;
                };
                if suspend {
                    // A visible webview refuses to suspend: hide it first.
                    let _ = controller.SetIsVisible(false);
                    let handler =
                        TrySuspendCompletedHandler::create(Box::new(|_error_code, _suspended| {
                            Ok(())
                        }));
                    let _ = core3.TrySuspend(&handler);
                } else {
                    let _ = core3.Resume();
                    let _ = controller.SetIsVisible(true);
                }
            }
        });
    }
}

/// No-op on non-Windows platforms (webkit2gtk/WKWebView already throttle
/// hidden windows themselves; the frontend additionally pauses chart redraws
/// via the Page Visibility API).
#[cfg(not(windows))]
pub fn handle_window_event<R: tauri::Runtime>(
    _window: &tauri::Window<R>,
    _event: &tauri::WindowEvent,
) {
}
