//! What the desktop driver keeps while it runs: the stores, the current status (so the tray
//! never re-reads the snapshot file to redraw), the refresh state and schedule, and a few
//! window flags.

use serde::Serialize;
use sprawlr_app::{DesktopSettings, Schedule, SettingsStore, SnapshotStore};
use sprawlr_domain::status::AccountStatus;
use sprawlr_domain::Snapshot;
use sprawlr_fs::{FileSettingsStore, FileSnapshotStore};
use std::sync::atomic::{AtomicBool, Ordering};
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
    SignInNeeded { profile: Option<String>, hint: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshInfo {
    #[serde(flatten)]
    pub state: RefreshState,
    /// When the next background refresh is due (RFC 3339), if one is scheduled.
    pub next_at: Option<String>,
    /// Background refresh is on.
    pub background: bool,
    /// Background refresh waits for a read that works, after a sign-in failure.
    pub paused: bool,
    /// A refresh of the stored scan reads error logs again (Logs Insights, billed per GB).
    pub reads_error_logs: bool,
    /// Parts of the last read that could not be read (no permission, an API error).
    pub warnings: usize,
}

/// The stored scan, as the tray needs it.
#[derive(Debug, Clone, Default)]
pub struct Current {
    pub status: Option<AccountStatus>,
    pub taken_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reads_error_logs: bool,
    pub warnings: usize,
}

impl Current {
    pub fn of(snapshot: &Snapshot, warnings: usize) -> Self {
        Current {
            status: Some(snapshot.status_or_compute()),
            taken_at: Some(snapshot.taken_at),
            reads_error_logs: sprawlr_app::RefreshOptions::like(snapshot).error_logs,
            warnings,
        }
    }
}

/// Menu entries whose text or state changes.
#[derive(Clone)]
pub struct MenuHandles {
    pub status: MenuItem<Wry>,
    pub refresh: MenuItem<Wry>,
}

pub struct Desk {
    pub snapshots: FileSnapshotStore,
    pub settings: FileSettingsStore,
    pub current: Mutex<Current>,
    pub refresh: Mutex<RefreshState>,
    pub schedule: Mutex<Schedule>,
    pub next_at: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// One AWS read at a time: a scan, a manual refresh or a background refresh.
    pub work: tokio::sync::Mutex<()>,
    /// Wakes the background loop when settings or the schedule change.
    pub wake: tokio::sync::Notify,
    /// A tray icon exists. Without one (a Linux desktop with no AppIndicator), sprawlr never
    /// hides its window: nothing could bring it back.
    pub has_tray: AtomicBool,
    /// The tray has reported where it is (never on Linux): tray-relative positions are safe.
    pub tray_seen: AtomicBool,
    /// When the popover last hid because it lost focus. A click on the tray icon blurs the
    /// popover first; that click must not reopen it.
    pub blur_hidden_at: Mutex<Option<Instant>>,
    pub menu: Mutex<Option<MenuHandles>>,
}

impl Desk {
    pub fn new(dir: std::path::PathBuf) -> Self {
        let snapshots = FileSnapshotStore::new(dir.join("last-snapshot.json"));
        let current = snapshots.latest().ok().flatten().map(|s| Current::of(&s, 0)).unwrap_or_default();
        Desk {
            snapshots,
            settings: FileSettingsStore::new(dir.join("settings.json")),
            current: Mutex::new(current),
            refresh: Mutex::new(RefreshState::Idle),
            schedule: Mutex::new(Schedule::default()),
            next_at: Mutex::new(None),
            work: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
            has_tray: AtomicBool::new(false),
            tray_seen: AtomicBool::new(false),
            blur_hidden_at: Mutex::new(None),
            menu: Mutex::new(None),
        }
    }

    pub fn prefs(&self) -> DesktopSettings {
        self.settings.load().unwrap_or_default()
    }

    /// Close-to-tray, with the platform default when the person never chose: on for macOS
    /// and Windows, off for Linux (Tauri cannot tell whether the desktop shows tray icons).
    /// Always off without a tray.
    pub fn close_to_tray(&self) -> bool {
        self.has_tray.load(Ordering::SeqCst) && self.prefs().close_to_tray.unwrap_or(!cfg!(target_os = "linux"))
    }

    pub fn current(&self) -> Current {
        self.current.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn info(&self) -> RefreshInfo {
        let current = self.current();
        RefreshInfo {
            state: self.refresh.lock().map(|s| s.clone()).unwrap_or(RefreshState::Idle),
            next_at: self.next_at.lock().ok().and_then(|n| n.map(|t| t.to_rfc3339())),
            background: self.prefs().background.is_some(),
            paused: self.schedule.lock().map(|s| s.paused).unwrap_or(false),
            reads_error_logs: current.reads_error_logs,
            warnings: current.warnings,
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
