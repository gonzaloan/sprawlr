//! Tauri commands: thin calls into the use cases, plus the bookkeeping the tray needs
//! (refresh state, last profile, telling every window that the snapshot changed).

use crate::state::{tray_place, Desk, RefreshInfo, RefreshState};
use crate::{popover, tray, windows};
use serde::{Deserialize, Serialize};
use sprawlr_app::{
    ports::Progress, DesktopSettings, Every, RefreshOptions, ScanOptions, ScanResult, SettingsStore, SignInNeeded, SnapshotStore,
};
use sprawlr_aws::Aws;
use sprawlr_docs::DocFile;
use sprawlr_domain::status::Target;
use sprawlr_domain::Snapshot;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::{NotificationExt, PermissionState};

pub fn set_state(app: &AppHandle, state: RefreshState) {
    let desk = app.state::<Desk>();
    if let Ok(mut s) = desk.refresh.lock() {
        *s = state;
    }
    let _ = app.emit("refresh-state", desk.info());
    tray::update(app);
}

/// Progress for a person watching: the scan panel's line, and the popover's refresh line.
fn progress(app: &AppHandle, track: bool) -> Progress {
    let app = app.clone();
    std::sync::Arc::new(move |msg: String| {
        let _ = app.emit("scan-progress", msg.clone());
        if track {
            set_state(&app, RefreshState::Running { step: msg });
        }
    })
}

/// A read worked: clear failures and pauses, tell every window, redraw the tray, and let the
/// background loop reschedule from now.
pub fn succeeded(app: &AppHandle, snapshot: &Snapshot) {
    let desk = app.state::<Desk>();
    desk.failures.store(0, Ordering::SeqCst);
    desk.paused.store(false, Ordering::SeqCst);
    set_state(app, RefreshState::Idle);
    let _ = app.emit("snapshot-changed", snapshot);
    desk.wake.notify_one();
}

/// A read failed: a sign-in problem pauses background refresh until a read works.
pub fn failed(app: &AppHandle, e: &anyhow::Error) -> String {
    let desk = app.state::<Desk>();
    if let Some(s) = e.downcast_ref::<SignInNeeded>() {
        desk.paused.store(true, Ordering::SeqCst);
        set_state(app, RefreshState::SignInNeeded { profile: s.profile.clone(), command: s.command() });
        format!("{}. Run `{}`, then try again.", capitalise(&s.to_string()), s.command())
    } else {
        desk.failures.fetch_add(1, Ordering::SeqCst);
        let message = format!("{e:#}");
        set_state(app, RefreshState::Failed { message: message.clone() });
        message
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn remember_profile(desk: &Desk, profile: &Option<String>) {
    let mut prefs = desk.prefs();
    if prefs.last_profile != *profile {
        prefs.last_profile = profile.clone();
        let _ = desk.settings.save(&prefs);
    }
}

#[tauri::command]
pub fn list_profiles() -> Vec<String> {
    crate::profiles::list()
}

/// Read-only scan of one account across regions, with inference, telemetry and findings.
#[tauri::command]
pub async fn scan(
    app: AppHandle,
    desk: State<'_, Desk>,
    profile: Option<String>,
    regions: Vec<String>,
    telemetry: Option<bool>,
    logs: Option<bool>,
) -> Result<ScanResult, String> {
    let _one_at_a_time = desk.work.lock().await;
    remember_profile(&desk, &profile);
    let opts = ScanOptions { health: telemetry.unwrap_or(true), error_logs: logs.unwrap_or(false) };
    match sprawlr_app::scan_account(&Aws::new(profile, regions), opts, Some(&desk.snapshots), &progress(&app, false)).await {
        Ok(r) => {
            succeeded(&app, &r.snapshot);
            Ok(r)
        }
        Err(e) => Err(failed(&app, &e)),
    }
}

/// Alarms, metrics and traces for the snapshot on screen (and error logs if it has them).
#[tauri::command]
pub async fn refresh_health(
    app: AppHandle,
    desk: State<'_, Desk>,
    profile: Option<String>,
    snapshot: Snapshot,
) -> Result<ScanResult, String> {
    let _one_at_a_time = desk.work.lock().await;
    remember_profile(&desk, &profile);
    let opts = RefreshOptions::like(&snapshot);
    match sprawlr_app::refresh_health(&Aws::new(profile, vec![]), &snapshot, opts, Some(&desk.snapshots), &progress(&app, true)).await {
        Ok(r) => {
            succeeded(&app, &r.snapshot);
            Ok(r)
        }
        Err(e) => Err(failed(&app, &e)),
    }
}

/// "Refresh health" from the tray or the popover: the stored scan, with the last profile.
#[tauri::command]
pub async fn refresh_stored(app: AppHandle) -> Result<(), String> {
    let desk = app.state::<Desk>();
    let _one_at_a_time = desk.work.lock().await;
    let Some(snapshot) = desk.snapshots.latest().map_err(|e| e.to_string())? else {
        return Err("No account read yet. Scan an account in sprawlr first.".into());
    };
    let profile = desk.prefs().last_profile;
    set_state(&app, RefreshState::Running { step: "signing in".into() });
    let opts = RefreshOptions::like(&snapshot);
    match sprawlr_app::refresh_health(&Aws::new(profile, vec![]), &snapshot, opts, Some(&desk.snapshots), &progress(&app, true)).await {
        Ok(r) => {
            succeeded(&app, &r.snapshot);
            Ok(())
        }
        Err(e) => Err(failed(&app, &e)),
    }
}

pub fn spawn_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let _ = refresh_stored(app).await;
    });
}

#[tauri::command]
pub fn refresh_state(desk: State<'_, Desk>) -> RefreshInfo {
    desk.info()
}

/// The last scan, if there is one, with its status (computed for files written before
/// status existed).
#[tauri::command]
pub fn last_snapshot(desk: State<'_, Desk>) -> Option<Snapshot> {
    let mut s = desk.snapshots.latest().ok().flatten()?;
    s.status = Some(s.status_or_compute());
    Some(s)
}

#[tauri::command]
pub fn open_main(app: AppHandle, target: Option<Target>) {
    windows::open_main(&app, target);
}

#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    popover::hide(&app);
}

/// Markdown pages for the documentation tab.
#[tauri::command]
pub fn render_docs(snapshot: Snapshot) -> Vec<DocFile> {
    sprawlr_docs::render(&snapshot)
}

/// Write README.md + apps/*.md into `dir`. Returns the number of pages written.
#[tauri::command]
pub fn save_docs(snapshot: Snapshot, dir: String) -> Result<usize, String> {
    sprawlr_fs::write_docs(std::path::Path::new(&dir), &sprawlr_docs::render(&snapshot)).map_err(|e| format!("{e:#}"))
}

/// Settings as the popover shows them.
#[derive(Debug, Serialize)]
pub struct SettingsView {
    pub background: Option<Every>,
    pub notifications: bool,
    pub launch_at_login: bool,
    pub close_to_tray: bool,
    /// The profile of the last scan or refresh (`None`: default credentials).
    pub profile: Option<String>,
    /// "menu bar" or "system tray".
    pub place: &'static str,
    /// Something the person should know about the last change (a permission the OS refused).
    pub notice: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SettingsInput {
    pub background: Option<Every>,
    pub notifications: bool,
    pub launch_at_login: bool,
    pub close_to_tray: bool,
}

fn view(app: &AppHandle, prefs: &DesktopSettings, notice: Option<String>) -> SettingsView {
    let desk = app.state::<Desk>();
    SettingsView {
        background: prefs.background,
        notifications: prefs.notifications,
        // The OS is the source of truth: a login item removed in system settings shows as off.
        launch_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        close_to_tray: desk.close_to_tray(),
        profile: prefs.last_profile.clone(),
        place: tray_place(),
        notice,
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    let prefs = app.state::<Desk>().prefs();
    view(&app, &prefs, None)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, input: SettingsInput) -> Result<SettingsView, String> {
    let desk = app.state::<Desk>();
    let mut prefs = desk.prefs();
    let mut notice = None;

    let mut notifications = input.notifications;
    if notifications && !prefs.notifications {
        let n = app.notification();
        let granted = match n.permission_state() {
            Ok(PermissionState::Granted) => true,
            _ => matches!(n.request_permission(), Ok(PermissionState::Granted)),
        };
        if !granted {
            notifications = false;
            notice = Some("Notifications are turned off for sprawlr in your system settings.".into());
        }
    }

    let autostart = app.autolaunch();
    if input.launch_at_login != autostart.is_enabled().unwrap_or(false) {
        let changed = if input.launch_at_login { autostart.enable() } else { autostart.disable() };
        if let Err(e) = changed {
            notice = Some(format!("Could not change login items: {e}"));
        }
    }

    prefs.background = input.background;
    prefs.notifications = notifications;
    prefs.close_to_tray = Some(input.close_to_tray);
    desk.settings.save(&prefs).map_err(|e| e.to_string())?;
    desk.wake.notify_one();
    Ok(view(&app, &prefs, notice))
}
