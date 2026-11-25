//! The main window's life: hide to the tray on close, come back on demand, open on an item.

use crate::popover;
use crate::state::{tray_place, Desk};
use sprawlr_app::SettingsStore;
use sprawlr_domain::status::Target;
use tauri::{AppHandle, Emitter, Manager, Window};
use tauri_plugin_dialog::DialogExt;

pub const MAIN: &str = "main";

/// Show the main window, in front, optionally on one item (a flow or a resource).
pub fn open_main(app: &AppHandle, target: Option<Target>) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if let Some(t) = target {
        let _ = app.emit_to(MAIN, "open-target", t);
    }
    popover::hide(app);
}

/// Closing the main window keeps sprawlr in the tray when that is on; otherwise it quits
/// (the hidden popover window would keep the process alive).
pub fn on_close_requested(window: &Window, api: &tauri::CloseRequestApi) {
    let app = window.app_handle();
    let desk = app.state::<Desk>();
    if !desk.close_to_tray() {
        app.exit(0);
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let mut first_time = false;
    let _ = desk.settings.update(&mut |s| {
        first_time = !s.close_hint_shown;
        s.close_hint_shown = true;
    });
    if first_time {
        let place = tray_place();
        let extra = if cfg!(target_os = "windows") { " If you can't see it, look under the ^ arrow on the taskbar." } else { "" };
        app.dialog()
            .message(format!(
                "sprawlr is still running in the {place}. Click its icon to see the account status, or choose Quit sprawlr from its menu to exit.{extra}"
            ))
            .title("sprawlr is still running")
            .show(|_| {});
    }
}
