// Prevents an extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Desktop driver: Tauri commands that call the use cases with the AWS and file adapters,
//! and the tray (menu bar) status with its popover. See docs/specs/0001-tray-status.md.

mod background;
mod commands;
mod notify;
mod popover;
mod profiles;
mod state;
mod tray;
mod windows;

use state::Desk;
use tauri::{Manager, RunEvent, WindowEvent};

/// Launch at login passes this: start in the tray, without the main window.
const IN_TRAY: &str = "--tray";

fn main() {
    let app = tauri::Builder::default()
        // First: a second launch focuses the running app instead of starting another.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| windows::open_main(app, None)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![IN_TRAY])))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            app.manage(Desk::new(dir));
            // Some Linux desktops have no tray: the AppIndicator library may be missing, and
            // its loader panics rather than failing. Run without a tray then, window shown.
            let handle = app.handle().clone();
            let has_tray = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tray::build(&handle))) {
                Ok(Ok(())) => true,
                Ok(Err(e)) => {
                    eprintln!("sprawlr: no tray icon on this desktop: {e}");
                    false
                }
                Err(_) => {
                    eprintln!("sprawlr: no tray icon on this desktop (AppIndicator not available)");
                    false
                }
            };
            app.state::<Desk>().has_tray.store(has_tray, std::sync::atomic::Ordering::SeqCst);
            background::spawn(app.handle().clone());
            if has_tray && std::env::args().any(|a| a == IN_TRAY) {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            } else if let Some(w) = app.get_webview_window(windows::MAIN) {
                w.show()?;
            }
            Ok(())
        })
        .on_window_event(|window, event| match (window.label(), event) {
            (popover::LABEL, WindowEvent::Focused(false)) => popover::on_blur(window.app_handle()),
            (windows::MAIN, WindowEvent::CloseRequested { api, .. }) => windows::on_close_requested(window, api),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::scan,
            commands::refresh_health,
            commands::refresh_stored,
            commands::refresh_state,
            commands::last_snapshot,
            commands::open_main,
            commands::hide_popover,
            commands::render_docs,
            commands::save_docs,
            commands::get_settings,
            commands::save_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building sprawlr");

    app.run(|app, event| {
        // Closing windows never ends the app while it lives in the tray; "Quit sprawlr" does.
        // `app.exit` ("Quit sprawlr") carries an exit code and is never held back.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            if app.state::<Desk>().close_to_tray() {
                api.prevent_exit();
            }
        }
    });
}
