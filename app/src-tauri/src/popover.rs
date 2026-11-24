//! The small status window next to the tray icon.

use crate::state::Desk;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_positioner::{Position, WindowExt};

pub const LABEL: &str = "tray";

pub fn toggle(app: &AppHandle) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    if w.is_visible().unwrap_or(false) {
        hide(app);
        return;
    }
    // Clicking the icon while the popover is open blurs it first, which already hid it.
    let just_hidden =
        app.state::<Desk>().blur_hidden_at.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if !just_hidden {
        show(app);
    }
}

pub fn show(app: &AppHandle) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    // Next to the icon when its position is known; otherwise the top-right corner (Linux).
    let placed = if app.state::<Desk>().tray_seen.load(Ordering::SeqCst) {
        w.move_window_constrained(Position::TrayCenter)
    } else {
        w.move_window(Position::TopRight)
    };
    if let Err(e) = placed {
        eprintln!("popover position: {e}");
    }
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to(LABEL, "popover-shown", ());
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// The popover lost focus (a click elsewhere, Esc handled in the page, or the icon).
pub fn on_blur(app: &AppHandle) {
    if let Ok(mut t) = app.state::<Desk>().blur_hidden_at.lock() {
        *t = Some(Instant::now());
    }
    hide(app);
}
