//! File-system adapters: the snapshot store, the desktop settings, and writing documentation
//! pages and exports into a folder.

use sprawlr_app::{DesktopSettings, SettingsStore, SnapshotStore};
use sprawlr_docs::DocFile;
use sprawlr_domain::Snapshot;
use sprawlr_iac::CodeFile;
use std::path::{Component, Path, PathBuf};

/// One JSON file holding the latest snapshot (the desktop app reopens on it), and a
/// `history/<account>/` folder next to it with each account's last [`KEEP_SCANS`] full scans,
/// to say what changed.
pub struct FileSnapshotStore {
    path: PathBuf,
}

/// Full scans kept in the history.
pub const KEEP_SCANS: usize = 30;
/// Nanoseconds, so a file name gives back exactly the `taken_at` it was written for.
const STAMP: &str = "%Y%m%dT%H%M%S%.9fZ";

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

impl FileSnapshotStore {
    fn history(&self, account: &str) -> PathBuf {
        // Account ids are digits; keep anything else out of the path.
        let safe: String = account.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        self.path.parent().unwrap_or(Path::new(".")).join("history").join(if safe.is_empty() { "unknown".into() } else { safe })
    }

    fn file_for(&self, account: &str, at: chrono::DateTime<chrono::Utc>) -> PathBuf {
        self.history(account).join(format!("scan-{}.json", at.format(STAMP)))
    }
}

fn account_of(s: &Snapshot) -> String {
    s.nodes.iter().find_map(|n| n.account.clone()).unwrap_or_default()
}

impl SnapshotStore for FileSnapshotStore {
    fn save(&self, snapshot: &Snapshot) -> anyhow::Result<()> {
        write_atomic(&self.path, &serde_json::to_vec(snapshot)?)
    }

    fn record_scan(&self, snapshot: &Snapshot) -> anyhow::Result<()> {
        let account = account_of(snapshot);
        write_atomic(&self.file_for(&account, snapshot.taken_at), &serde_json::to_vec(snapshot)?)?;
        let scans = self.scans(&account)?;
        for old in scans.iter().take(scans.len().saturating_sub(KEEP_SCANS)) {
            let _ = std::fs::remove_file(self.file_for(&account, *old));
        }
        Ok(())
    }

    fn scans(&self, account: &str) -> anyhow::Result<Vec<chrono::DateTime<chrono::Utc>>> {
        let Ok(dir) = std::fs::read_dir(self.history(account)) else { return Ok(vec![]) };
        let mut out: Vec<chrono::DateTime<chrono::Utc>> = dir
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|n| n.strip_prefix("scan-")?.strip_suffix(".json").map(String::from))
            .filter_map(|stamp| chrono::NaiveDateTime::parse_from_str(&stamp, STAMP).ok())
            .map(|t| t.and_utc())
            .collect();
        out.sort();
        Ok(out)
    }

    fn scan(&self, account: &str, taken_at: chrono::DateTime<chrono::Utc>) -> anyhow::Result<Option<Snapshot>> {
        match std::fs::read(self.file_for(account, taken_at)) {
            Ok(raw) => Ok(Some(serde_json::from_slice(&raw)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
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
    write_all(root, files.iter().map(|f| (f.path.as_str(), f.content.as_str())).collect())
}

/// What the last export wrote into a folder, one path per line, so the next export replaces
/// exactly that and nothing the person added (their `terraform.tfvars`, Terraform's state).
pub const EXPORT_MANIFEST: &str = ".sprawlr-export";

/// Write an export into `root`, created if missing: its code, and the documentation under
/// `docs/`. `root` must be empty or hold an earlier export; files that export wrote and this
/// one doesn't are removed. Returns the number of files written.
pub fn write_export(root: &Path, code: &[CodeFile], docs: &[DocFile]) -> anyhow::Result<usize> {
    std::fs::create_dir_all(root).map_err(|e| anyhow::anyhow!("{}: {e}", root.display()))?;
    let previous = match std::fs::read_to_string(root.join(EXPORT_MANIFEST)) {
        Ok(text) => Some(text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect::<Vec<_>>()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let empty = std::fs::read_dir(root)?.next().is_none();
    anyhow::ensure!(
        empty || previous.is_some(),
        "{} already has files sprawlr did not write. Pick an empty folder, or the folder of an earlier export",
        root.display()
    );
    let docs: Vec<(String, &str)> = docs.iter().map(|d| (format!("docs/{}", d.path), d.content.as_str())).collect();
    let mut files: Vec<(&str, &str)> = code.iter().map(|f| (f.path.as_str(), f.content.as_str())).collect();
    files.extend(docs.iter().map(|(p, c)| (p.as_str(), *c)));
    let n = write_all(root, files.clone())?;

    let written: std::collections::BTreeSet<&str> = files.iter().map(|(p, _)| *p).collect();
    for old in previous.iter().flatten().filter(|p| !written.contains(p.as_str()) && inside(p)) {
        let path = root.join(old);
        let _ = std::fs::remove_file(&path);
        // A folder the export no longer writes into goes too, if nothing else is in it.
        if let Some(dir) = path.parent().filter(|d| *d != root) {
            let _ = std::fs::remove_dir(dir);
        }
    }
    let manifest = std::iter::once(
        "# Files sprawlr wrote in its last export here. The next export replaces them and touches nothing else.".to_string(),
    )
    .chain(written.iter().map(|p| p.to_string()))
    .collect::<Vec<_>>()
    .join("\n");
    std::fs::write(root.join(EXPORT_MANIFEST), manifest + "\n")?;
    Ok(n)
}

/// A relative path that stays inside the folder.
fn inside(path: &str) -> bool {
    let rel = Path::new(path);
    !rel.is_absolute() && rel.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Every path is checked before the first file is written, so a bad one leaves nothing behind.
fn write_all(root: &Path, files: Vec<(&str, &str)>) -> anyhow::Result<usize> {
    for (path, _) in &files {
        anyhow::ensure!(inside(path), "refusing to write {path}");
    }
    for (rel, content) in &files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap_or(root))?;
        std::fs::write(&path, content).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
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
    fn history_keeps_each_accounts_last_scans_exactly() {
        let dir = temp("history");
        let store = FileSnapshotStore::new(dir.join("last.json"));
        let mut g = demo_graph();
        infer(&mut g);
        let base = g.snapshot();
        let acct = base.nodes.iter().find_map(|n| n.account.clone()).unwrap();
        assert!(store.scans(&acct).unwrap().is_empty());

        store.record_scan(&base).unwrap();
        assert_eq!(store.scans(&acct).unwrap(), vec![base.taken_at], "the file name gives back the exact time");

        for i in 1..(KEEP_SCANS + 2) {
            let mut s = base.clone();
            s.taken_at = base.taken_at + chrono::Duration::minutes(i as i64);
            store.record_scan(&s).unwrap();
        }
        let mut other = base.clone();
        other.nodes.iter_mut().for_each(|n| n.account = Some("999".into()));
        store.record_scan(&other).unwrap();

        let scans = store.scans(&acct).unwrap();
        assert_eq!(scans.len(), KEEP_SCANS, "older scans are pruned");
        assert!(scans.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(store.scans("999").unwrap().len(), 1, "another account keeps its own history");
        assert_eq!(store.scan(&acct, *scans.last().unwrap()).unwrap().unwrap().nodes.len(), base.nodes.len());
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

    #[test]
    fn an_export_writes_its_code_and_the_docs_and_refuses_a_path_that_leaves_the_folder() {
        let dir = temp("export").join("new-folder");
        let code = vec![CodeFile { path: "terraform/checkout.tf".into(), content: "# x\n".into() }];
        let docs = vec![DocFile { path: "README.md".into(), title: "x".into(), content: "# docs\n".into() }];
        assert_eq!(write_export(&dir, &code, &docs).unwrap(), 2);
        assert!(dir.join("terraform/checkout.tf").exists() && dir.join("docs/README.md").exists());

        let evil =
            vec![CodeFile { path: "ok.tf".into(), content: "x".into() }, CodeFile { path: "../escape.tf".into(), content: "x".into() }];
        assert!(write_export(&dir, &evil, &[]).is_err());
        assert!(!dir.join("ok.tf").exists(), "nothing is written when one path is refused");
    }

    #[test]
    fn an_export_never_writes_over_a_folder_it_did_not_write() {
        let dir = temp("export-foreign");
        std::fs::write(dir.join("README.md"), "the person's own").unwrap();
        let code = vec![CodeFile { path: "README.md".into(), content: "# Code\n".into() }];
        assert!(write_export(&dir, &code, &[]).unwrap_err().to_string().contains("Pick an empty folder"));
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "the person's own");
    }

    #[test]
    fn exporting_again_replaces_what_the_last_export_wrote_and_nothing_else() {
        let dir = temp("export-again").join("out");
        let file = |p: &str| CodeFile { path: p.into(), content: "x\n".into() };
        write_export(&dir, &[file("terraform/shared.tf"), file("terraform/checkout.tf"), file("cloudformation/shared.json")], &[]).unwrap();
        std::fs::write(dir.join("terraform/terraform.tfvars"), "secret = \"kept\"").unwrap();

        write_export(&dir, &[file("terraform/checkout.tf")], &[]).unwrap();
        assert!(dir.join("terraform/checkout.tf").exists());
        assert!(!dir.join("terraform/shared.tf").exists(), "a file only the last export had is removed");
        assert!(!dir.join("cloudformation").exists(), "and its folder, once empty");
        assert!(dir.join("terraform/terraform.tfvars").exists(), "what the person added stays");
    }
}
