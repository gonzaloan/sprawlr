//! File-system adapters: the snapshot store, the desktop settings, and writing documentation
//! pages into a folder.

use sprawlr_app::{DesktopSettings, SettingsStore, SnapshotStore};
use sprawlr_docs::DocFile;
use sprawlr_domain::Snapshot;
use std::path::{Component, Path, PathBuf};

/// One JSON file holding the latest snapshot (the desktop app reopens on it).
pub struct FileSnapshotStore {
    path: PathBuf,
}

impl FileSnapshotStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileSnapshotStore { path: path.into() }
    }
}

/// Write then rename: a crash mid-write never leaves half a file behind.
fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

impl SnapshotStore for FileSnapshotStore {
    fn save(&self, snapshot: &Snapshot) -> anyhow::Result<()> {
        write_atomic(&self.path, &serde_json::to_vec(snapshot)?)
    }

    fn latest(&self) -> anyhow::Result<Option<Snapshot>> {
        match std::fs::read(&self.path) {
            Ok(raw) => Ok(Some(serde_json::from_slice(&raw)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// `settings.json`: the desktop app's choices. A missing or unreadable file is the defaults,
/// so a bad edit never stops the app from starting. Changes go through one lock, so writers
/// in different threads never undo each other.
pub struct FileSettingsStore {
    path: PathBuf,
    lock: std::sync::Mutex<()>,
}

impl FileSettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileSettingsStore { path: path.into(), lock: std::sync::Mutex::new(()) }
    }

    fn read(&self) -> anyhow::Result<DesktopSettings> {
        match std::fs::read(&self.path) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DesktopSettings::default()),
            Err(e) => Err(e.into()),
        }
    }
}

impl SettingsStore for FileSettingsStore {
    fn load(&self) -> anyhow::Result<DesktopSettings> {
        let _one_writer = self.lock.lock().map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
        self.read()
    }

    fn update(&self, change: &mut dyn FnMut(&mut DesktopSettings)) -> anyhow::Result<DesktopSettings> {
        let _one_writer = self.lock.lock().map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
        let mut s = self.read()?;
        change(&mut s);
        write_atomic(&self.path, &serde_json::to_vec_pretty(&s)?)?;
        Ok(s)
    }
}

/// Read a snapshot file written by `sprawlr scan` or the app.
pub fn read_snapshot(path: &Path) -> anyhow::Result<Snapshot> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

/// Write pages under `root`, which must exist. Paths are generated slugs, but a path that
/// could leave the folder is refused anyway. Returns the number of pages written.
pub fn write_docs(root: &Path, files: &[DocFile]) -> anyhow::Result<usize> {
    anyhow::ensure!(root.is_dir(), "{} is not a folder", root.display());
    for f in files {
        let rel = Path::new(&f.path);
        anyhow::ensure!(!rel.is_absolute() && rel.components().all(|c| matches!(c, Component::Normal(_))), "refusing to write {}", f.path);
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap_or(root))?;
        std::fs::write(&path, &f.content).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprawlr_domain::{fixtures::demo_graph, infer::infer};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sprawlr-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn store_round_trips_and_starts_empty() {
        let dir = temp("store");
        let store = FileSnapshotStore::new(dir.join("nested/last.json"));
        assert!(store.latest().unwrap().is_none());
        let mut g = demo_graph();
        infer(&mut g);
        store.save(&g.snapshot()).unwrap();
        assert_eq!(store.latest().unwrap().unwrap().nodes.len(), g.nodes().count());
    }

    #[test]
    fn settings_default_when_missing_or_broken() {
        let dir = temp("settings");
        let store = FileSettingsStore::new(dir.join("settings.json"));
        assert_eq!(store.load().unwrap(), DesktopSettings::default());
        let s = store
            .update(&mut |s| {
                s.notifications = true;
                s.background = Some(sprawlr_app::Every::Hour);
            })
            .unwrap();
        assert_eq!(store.load().unwrap(), s);
        assert!(s.notifications);
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
        assert_eq!(store.load().unwrap(), DesktopSettings::default());
    }

    #[test]
    fn docs_stay_inside_the_folder() {
        let dir = temp("docs");
        let mut g = demo_graph();
        infer(&mut g);
        let n = write_docs(&dir, &sprawlr_docs::render(&g.snapshot())).unwrap();
        assert_eq!(n, 5);
        assert!(dir.join("apps/checkout.md").exists());
        let evil = vec![DocFile { path: "../escape.md".into(), title: "x".into(), content: "x".into() }];
        assert!(write_docs(&dir, &evil).is_err());
    }
}
