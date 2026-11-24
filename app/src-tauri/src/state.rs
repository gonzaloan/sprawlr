//! What the desktop driver keeps while it runs: the stores, the refresh state shown in the
//! tray and the popover, and a few flags for window behaviour.

use serde::Serialize;
use sprawlr_app::{DesktopSettings, SettingsStore};
use sprawlr_fs::{FileSettingsStore, FileSnapshotStore};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::menu::MenuItem;
use tauri::Wry;

/// What the last or current health refresh is doing, for the popover and the menu.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RefreshState {
    Idle,
    Running { step: String },
    Failed { message: String },
    SignInNeeded { profile: Option<String>, command: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshInfo {
    #[serde(flatten)]
    pub state: RefreshState,
    /// When the next background refresh is due (RFC 3339), if one is scheduled.
    pub next_at: Option<String>,
    /// Background refresh waits for a refresh that works after a sign-in failure.
    pub paused: bool,
}

/// Menu entries whose text or state changes.
pub struct MenuHandles {
    pub status: MenuItem<Wry>,
    pub refresh: MenuItem<Wry>,
}

pub struct Desk {
    pub snapshots: FileSnapshotStore,
    pub settings: FileSettingsStore,
    pub refresh: Mutex<RefreshState>,
    pub next_at: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// Set by a sign-in failure, cleared by any refresh or scan that works.
    pub paused: AtomicBool,
    pub failures: AtomicU32,
    /// One AWS read at a time: a scan, a manual refresh or a background refresh.
    pub work: tokio::sync::Mutex<()>,
    /// Wakes the background loop when settings change.
    pub wake: tokio::sync::Notify,
    /// The tray has reported where it is (never on Linux): tray-relative positions are safe.
    pub tray_seen: AtomicBool,
    /// When the popover last hid because it lost focus. A click on the tray icon blurs the
    /// popover first; that click must not reopen it.
    pub blur_hidden_at: Mutex<Option<Instant>>,
    /// "Quit sprawlr" was chosen: let the app exit.
    pub quitting: AtomicBool,
    pub menu: Mutex<Option<MenuHandles>>,
}

impl Desk {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Desk {
            snapshots: FileSnapshotStore::new(dir.join("last-snapshot.json")),
            settings: FileSettingsStore::new(dir.join("settings.json")),
            refresh: Mutex::new(RefreshState::Idle),
            next_at: Mutex::new(None),
            paused: AtomicBool::new(false),
            failures: AtomicU32::new(0),
            work: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
            tray_seen: AtomicBool::new(false),
            blur_hidden_at: Mutex::new(None),
            quitting: AtomicBool::new(false),
            menu: Mutex::new(None),
        }
    }

    pub fn prefs(&self) -> DesktopSettings {
        self.settings.load().unwrap_or_default()
    }

    /// Close-to-tray, with the platform default when the person never chose: on for macOS
    /// and Windows, off for Linux (Tauri cannot tell whether the desktop shows tray icons).
    pub fn close_to_tray(&self) -> bool {
        self.prefs().close_to_tray.unwrap_or(!cfg!(target_os = "linux"))
    }

    pub fn info(&self) -> RefreshInfo {
        RefreshInfo {
            state: self.refresh.lock().map(|s| s.clone()).unwrap_or(RefreshState::Idle),
            next_at: self.next_at.lock().ok().and_then(|n| n.map(|t| t.to_rfc3339())),
            paused: self.paused.load(Ordering::SeqCst),
        }
    }
}

/// "menu bar" on macOS, "system tray" elsewhere: the words people use for the place.
pub fn tray_place() -> &'static str {
    if cfg!(target_os = "macos") {
        "menu bar"
    } else {
        "system tray"
    }
}
