//! Where the server reads its snapshot: the desktop app's last scan, a file, or the demo.
//! A file is read again when it changes, so a refresh in the app shows up in the next answer.

use sprawlr_domain::Snapshot;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub enum Source {
    File { path: PathBuf, loaded: Option<Box<(SystemTime, Snapshot)>> },
    Fixed(Box<Snapshot>),
}

impl Source {
    pub fn file(path: PathBuf) -> Self {
        Source::File { path, loaded: None }
    }

    /// The fictional demo account, analysed as a scan would be.
    pub fn demo() -> Self {
        Source::Fixed(Box::new(sprawlr_app::demo::snapshot()))
    }

    pub fn describe(&self) -> String {
        match self {
            Source::File { path, .. } => path.display().to_string(),
            Source::Fixed(_) => "the demo account".into(),
        }
    }

    pub fn snapshot(&mut self) -> anyhow::Result<&Snapshot> {
        match self {
            Source::Fixed(s) => Ok(s),
            Source::File { path, loaded } => {
                let modified = std::fs::metadata(&*path).and_then(|m| m.modified()).map_err(|e| {
                    anyhow::anyhow!("no scan at {}: {e}. Scan an account in the sprawlr app or with `sprawlr scan`.", path.display())
                })?;
                if loaded.as_ref().is_none_or(|l| l.0 != modified) {
                    *loaded = Some(Box::new((modified, sprawlr_fs::read_snapshot(path)?)));
                }
                Ok(&loaded.as_ref().expect("just loaded").1)
            }
        }
    }
}

/// Where the desktop app keeps its last scan (Tauri's app data dir for `dev.sprawlr.desktop`).
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share")))
    };
    base.map(|b| b.join("dev.sprawlr.desktop").join("last-snapshot.json"))
}

pub fn exists(p: &Path) -> bool {
    p.is_file()
}
