//! Background health refresh: off unless chosen, health only, one read at a time, slower
//! after failures, paused after a sign-in failure (`sprawlr_app::Schedule`). Also keeps the
//! tray's "last read" honest by redrawing it every few minutes.

use crate::commands::{failed, set_state, succeeded};
use crate::notify::TauriNotifier;
use crate::state::{Desk, RefreshState};
use crate::tray;
use sprawlr_aws::Aws;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{sleep, sleep_until, Instant};

/// How often the tray is redrawn without reading AWS (staleness, "last read").
const REDRAW: Duration = Duration::from_secs(10 * 60);

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        'schedule: loop {
            let desk = app.state::<Desk>();
            let every = desk.prefs().background;
            let wait = desk.schedule.lock().ok().and_then(|s| s.wait(every));
            let due = wait.map(|w| Instant::now() + w);
            if let Ok(mut n) = desk.next_at.lock() {
                *n = wait.and_then(|w| chrono::Duration::from_std(w).ok()).map(|w| chrono::Utc::now() + w);
            }
            let _ = app.emit("refresh-state", desk.info());

            loop {
                match due {
                    Some(at) => tokio::select! {
                        _ = sleep_until(at) => break,
                        _ = sleep(REDRAW) => tray::update(&app),
                        _ = desk.wake.notified() => continue 'schedule,
                    },
                    None => tokio::select! {
                        _ = sleep(REDRAW) => tray::update(&app),
                        _ = desk.wake.notified() => continue 'schedule,
                    },
                }
            }
            run(&app).await;
        }
    });
}

async fn run(app: &AppHandle) {
    let desk = app.state::<Desk>();
    // A scan or a manual refresh is running: it reads health anyway.
    let Ok(_one_at_a_time) = desk.work.try_lock() else { return };
    let profile = desk.prefs().last_profile;
    set_state(app, RefreshState::Running { step: "reading health".into() });
    let progress_app = app.clone();
    let progress: sprawlr_app::Progress = std::sync::Arc::new(move |step: String| set_state(&progress_app, RefreshState::Running { step }));
    let notifier = TauriNotifier(app.clone());
    let now = chrono::Utc::now();
    match sprawlr_app::refresh_in_background(&Aws::new(profile, vec![]), &desk.snapshots, &desk.settings, &notifier, now, &progress).await {
        Ok(Some(done)) => succeeded(app, &done.result.snapshot, done.result.warnings.len()),
        Ok(None) => set_state(app, RefreshState::Idle),
        Err(e) => {
            failed(app, &e, false);
        }
    }
}
