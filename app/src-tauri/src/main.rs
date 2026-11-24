// Prevents an extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sprawlr_core::{docs::DocFile, infer::infer, ArchGraph, Snapshot};
use std::collections::BTreeSet;
use tauri::{Emitter, Manager};
use std::path::PathBuf;

/// Profile names from ~/.aws/config and ~/.aws/credentials (honouring AWS_CONFIG_FILE /
/// AWS_SHARED_CREDENTIALS_FILE). Only section headers are read, never keys.
#[tauri::command]
fn list_profiles() -> Vec<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let path = |var: &str, file: &str| std::env::var_os(var).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".aws").join(file)));

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

fn progress(app: &tauri::AppHandle) -> sprawlr_scan::Progress {
    let app = app.clone();
    std::sync::Arc::new(move |msg: String| {
        let _ = app.emit("scan-progress", msg);
    })
}

/// Where the last scan is kept so the app reopens on it.
fn last_snapshot_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("last-snapshot.json"))
}

fn remember(app: &tauri::AppHandle, snap: &Snapshot) {
    if let Some(p) = last_snapshot_path(app) {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, serde_json::to_vec(snap).unwrap_or_default());
    }
}

/// Read-only scan of one account across regions, with inference, telemetry and findings.
#[tauri::command]
async fn scan(app: tauri::AppHandle, profile: Option<String>, regions: Vec<String>, telemetry: Option<bool>) -> Result<ScanResult, String> {
    if regions.is_empty() {
        return Err("add at least one region".into());
    }
    let opts = sprawlr_scan::ScanOptions { telemetry: telemetry.unwrap_or(true) };
    let mut report = sprawlr_scan::scan(profile.as_deref(), &regions, &opts, progress(&app)).await.map_err(|e| format!("{e:#}"))?;
    infer(&mut report.graph);
    let snapshot = report.graph.snapshot();
    remember(&app, &snapshot);
    Ok(ScanResult { snapshot, warnings: report.warnings })
}

/// Alarms, metrics and traces only, for the snapshot on screen.
#[tauri::command]
async fn refresh_health(app: tauri::AppHandle, profile: Option<String>, snapshot: Snapshot) -> Result<ScanResult, String> {
    let mut g = ArchGraph::from_snapshot(&snapshot);
    let warnings = sprawlr_scan::refresh_telemetry(&mut g, profile.as_deref(), progress(&app)).await.map_err(|e| format!("{e:#}"))?;
    let snapshot = g.snapshot();
    remember(&app, &snapshot);
    Ok(ScanResult { snapshot, warnings })
}

/// The last scan, if there is one.
#[tauri::command]
fn last_snapshot(app: tauri::AppHandle) -> Option<Snapshot> {
    let raw = std::fs::read(last_snapshot_path(&app)?).ok()?;
    serde_json::from_slice(&raw).ok()
}

#[derive(serde::Serialize)]
struct ScanResult {
    snapshot: Snapshot,
    warnings: Vec<String>,
}

fn render(snapshot: &Snapshot) -> Vec<DocFile> {
    let g = ArchGraph::from_snapshot(snapshot);
    sprawlr_core::docs::generate(&g, &snapshot.findings, &snapshot.taken_at.format("%Y-%m-%d").to_string())
}

/// Markdown pages for the documentation tab.
#[tauri::command]
fn render_docs(snapshot: Snapshot) -> Vec<DocFile> {
    render(&snapshot)
}

/// Write README.md + apps/*.md into `dir`. Returns the number of pages written.
#[tauri::command]
fn save_docs(snapshot: Snapshot, dir: String) -> Result<usize, String> {
    let root = PathBuf::from(&dir);
    if !root.is_dir() {
        return Err(format!("{dir} is not a folder"));
    }
    let files = render(&snapshot);
    for f in &files {
        let rel = std::path::Path::new(&f.path);
        // Generated paths are slugs, but never trust a path that could leave the folder.
        if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("refusing to write {}", f.path));
        }
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap_or(&root)).map_err(|e| e.to_string())?;
        std::fs::write(&path, &f.content).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(files.len())
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

    #[test]
    fn saves_demo_docs_inside_the_folder() {
        let mut g = sprawlr_core::demo::demo_graph();
        infer(&mut g);
        let dir = std::env::temp_dir().join(format!("sprawlr-docs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let n = save_docs(g.snapshot(), dir.to_string_lossy().into()).unwrap();
        assert_eq!(n, 5);
        assert!(dir.join("apps/checkout.md").exists());
    }
}
