//! The tray icon (menu bar on macOS), its tooltip and its menu. Everything shown comes from
//! the stored snapshot's status (`sprawlr_domain::status`), so it matches the popover and the
//! briefing.

use crate::state::{Desk, MenuHandles, RefreshState};
use crate::{commands, popover, windows};
use sprawlr_domain::status::{is_stale, Level};
use std::sync::atomic::Ordering;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY: &str = "sprawlr";

/// The icon for a level. States differ by shape (see scripts/tray_icons.py): a template icon
/// on macOS, a colour tile elsewhere.
fn icon(level: Option<Level>) -> Image<'static> {
    let bytes: &[u8] = match (level, cfg!(target_os = "macos")) {
        (Some(Level::ActNow), true) => include_bytes!("../icons/tray/act_now-template.png"),
        (Some(Level::ActNow), false) => include_bytes!("../icons/tray/act_now.png"),
        (Some(Level::PlanForIt), true) => include_bytes!("../icons/tray/plan_for_it-template.png"),
        (Some(Level::PlanForIt), false) => include_bytes!("../icons/tray/plan_for_it.png"),
        (Some(Level::AllClear), true) => include_bytes!("../icons/tray/all_clear-template.png"),
        (Some(Level::AllClear), false) => include_bytes!("../icons/tray/all_clear.png"),
        (_, true) => include_bytes!("../icons/tray/no_status-template.png"),
        (_, false) => include_bytes!("../icons/tray/no_status.png"),
    };
    Image::from_bytes(bytes).expect("bundled tray icon is a valid PNG")
}

/// "12 min ago", "5 h ago", "3 days ago".
pub fn ago(then: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> String {
    let m = (now - then).num_minutes().max(0);
    match m {
        0 => "just now".into(),
        1..=59 => format!("{m} min ago"),
        60..=2879 => format!("{} h ago", m / 60),
        _ => format!("{} days ago", m / 1440),
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "No status yet", false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show status", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open sprawlr", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh health", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit sprawlr", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&status, &PredefinedMenuItem::separator(app)?, &show, &open, &refresh, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    TrayIconBuilder::with_id(TRAY)
        .icon(icon(None))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("sprawlr")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "show" => popover::show(app),
            "open" => windows::open_main(app, None),
            "refresh" => commands::spawn_refresh(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            tauri_plugin_positioner::on_tray_event(app, &event);
            // Tray events carry the icon's position; until one arrives (never on Linux),
            // tray-relative placement is not possible.
            if matches!(event, TrayIconEvent::Click { .. } | TrayIconEvent::Enter { .. } | TrayIconEvent::Move { .. }) {
                app.state::<Desk>().tray_seen.store(true, Ordering::SeqCst);
            }
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                popover::toggle(app);
            }
        })
        .build(app)?;

    if let Ok(mut m) = app.state::<Desk>().menu.lock() {
        *m = Some(MenuHandles { status, refresh });
    }
    update(app);
    Ok(())
}

/// Redraw icon, tooltip and menu from the current status and the refresh state. Reads no
/// file: the status is kept in `Desk` when a read succeeds.
pub fn update(app: &AppHandle) {
    let desk = app.state::<Desk>();
    let now = chrono::Utc::now();
    let current = desk.current();
    let (level, line, sentence) = match (&current.status, current.taken_at) {
        (Some(_), Some(at)) if is_stale(at, now) => {
            let l = format!("No status: last read {}", ago(at, now));
            (None, l.clone(), l)
        }
        (Some(st), _) => (Some(st.level), st.line(), st.sentence()),
        _ => (None, "No status yet. Scan an account in sprawlr".to_string(), "No status yet. Open sprawlr to scan an account".to_string()),
    };
    let state = desk.refresh.lock().map(|s| s.clone()).unwrap_or(RefreshState::Idle);
    let running = matches!(state, RefreshState::Running { .. });
    let suffix = match state {
        RefreshState::Idle => "",
        RefreshState::Running { .. } => ". Refreshing…",
        RefreshState::Failed { .. } => ". Last refresh failed",
        RefreshState::SignInNeeded { .. } => ". Sign in again to refresh",
    };

    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_icon(Some(icon(level)));
        let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
        let _ = tray.set_tooltip(Some(format!("sprawlr: {sentence}{suffix}")));
    }
    // Clone the handles and let go of the lock: menu calls wait for the main thread.
    let menu = desk.menu.lock().ok().and_then(|m| m.clone());
    if let Some(m) = menu {
        let _ = m.status.set_text(format!("{line}{suffix}"));
        let _ = m.refresh.set_text(if running { "Refreshing…" } else { "Refresh health" });
        let _ = m.refresh.set_enabled(!running && current.status.is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_has_an_icon() {
        for l in [None, Some(Level::ActNow), Some(Level::PlanForIt), Some(Level::AllClear), Some(Level::NotMeasured)] {
            assert_eq!(icon(l).width(), 64);
        }
    }

    #[test]
    fn ago_reads_like_a_person() {
        let t = chrono::Utc::now();
        let m = |n| t + chrono::Duration::minutes(n);
        assert_eq!(ago(t, m(0)), "just now");
        assert_eq!(ago(t, m(12)), "12 min ago");
        assert_eq!(ago(t, m(5 * 60)), "5 h ago");
        assert_eq!(ago(t, m(3 * 1440)), "3 days ago");
    }
}
