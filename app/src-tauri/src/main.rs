// Prevents an extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Desktop driver: Tauri commands that call the use cases with the AWS and file adapters.

use sprawlr_app::{ports::Progress, ScanOptions, ScanResult, SnapshotStore};
use sprawlr_aws::Aws;
use sprawlr_docs::DocFile;
use sprawlr_domain::Snapshot;
use sprawlr_fs::FileSnapshotStore;
use std::collections::BTreeSet;
use std::path::PathBuf;
use tauri::{Emitter, Manager};

/// Profile names from ~/.aws/config and ~/.aws/credentials (honouring AWS_CONFIG_FILE /
/// AWS_SHARED_CREDENTIALS_FILE). Only section headers are read, never keys.
#[tauri::command]
fn list_profiles() -> Vec<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let path =
        |var: &str, file: &str| std::env::var_os(var).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".aws").join(file)));

    let mut out = BTreeSet::new();
    if let Some(p) = path("AWS_CONFIG_FILE", "config") {
        out.extend(sections(&p).into_iter().filter_map(|s| match s.as_str() {
            "default" => Some(s),
            _ => s.strip_prefix("profile ").map(|n| n.trim().to_string()),
        }));
    }
    if let Some(p) = path("AWS_SHARED_CREDENTIALS_FILE", "credentials") {
        out.extend(sections(&p));
    }
    out.into_iter().collect()
}

fn sections(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().strip_prefix('[')?.strip_suffix(']').map(|s| s.trim().to_string()))
        .filter(|s| !s.starts_with("sso-session") && !s.starts_with("services"))
        .collect()
}

fn progress(app: &tauri::AppHandle) -> Progress {
    let app = app.clone();
    std::sync::Arc::new(move |msg: String| {
        let _ = app.emit("scan-progress", msg);
    })
}

/// Where the last scan is kept so the app reopens on it.
fn store(app: &tauri::AppHandle) -> Result<FileSnapshotStore, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(FileSnapshotStore::new(dir.join("last-snapshot.json")))
}

/// Read-only scan of one account across regions, with inference, telemetry and findings.
#[tauri::command]
async fn scan(
    app: tauri::AppHandle,
    profile: Option<String>,
    regions: Vec<String>,
    telemetry: Option<bool>,
    logs: Option<bool>,
) -> Result<ScanResult, String> {
    let opts = ScanOptions { health: telemetry.unwrap_or(true), error_logs: logs.unwrap_or(false) };
    let store = store(&app)?;
    sprawlr_app::scan_account(&Aws::new(profile, regions), opts, Some(&store), &progress(&app)).await.map_err(|e| format!("{e:#}"))
}

/// Alarms, metrics and traces only, for the snapshot on screen. Error logs are read again if
/// the snapshot has them.
#[tauri::command]
async fn refresh_health(app: tauri::AppHandle, profile: Option<String>, snapshot: Snapshot) -> Result<ScanResult, String> {
    let store = store(&app)?;
    sprawlr_app::refresh_health(&Aws::new(profile, vec![]), &snapshot, Some(&store), &progress(&app)).await.map_err(|e| format!("{e:#}"))
}

/// The last scan, if there is one.
#[tauri::command]
fn last_snapshot(app: tauri::AppHandle) -> Option<Snapshot> {
    store(&app).ok()?.latest().ok().flatten()
}

/// Markdown pages for the documentation tab.
#[tauri::command]
fn render_docs(snapshot: Snapshot) -> Vec<DocFile> {
    sprawlr_docs::render(&snapshot)
}

/// Write README.md + apps/*.md into `dir`. Returns the number of pages written.
#[tauri::command]
fn save_docs(snapshot: Snapshot, dir: String) -> Result<usize, String> {
    sprawlr_fs::write_docs(std::path::Path::new(&dir), &sprawlr_docs::render(&snapshot)).map_err(|e| format!("{e:#}"))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![list_profiles, scan, refresh_health, last_snapshot, render_docs, save_docs])
        .run(tauri::generate_context!())
        .expect("error while running sprawlr");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_profile_names_only() {
        let dir = std::env::temp_dir().join(format!("sprawlr-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config");
        std::fs::write(&cfg, "[default]\nregion=us-east-1\n[profile prod-admin]\nsso_session=x\n[sso-session x]\n").unwrap();
        let names: Vec<String> = sections(&cfg)
            .into_iter()
            .filter_map(|s| if s == "default" { Some(s) } else { s.strip_prefix("profile ").map(str::to_string) })
            .collect();
        assert_eq!(names, vec!["default", "prod-admin"]);
    }
}
