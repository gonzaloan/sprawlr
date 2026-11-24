//! File-system adapters: the snapshot store, and writing documentation pages into a folder.

use sprawlr_app::SnapshotStore;
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

impl SnapshotStore for FileSnapshotStore {
    fn save(&self, snapshot: &Snapshot) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write then rename: a crash mid-write never leaves a half snapshot behind.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(snapshot)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn latest(&self) -> anyhow::Result<Option<Snapshot>> {
        match std::fs::read(&self.path) {
            Ok(raw) => Ok(Some(serde_json::from_slice(&raw)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
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
