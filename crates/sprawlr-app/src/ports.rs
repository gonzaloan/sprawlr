//! Ports: what the use cases need from the outside world, stated in domain terms.
//!
//! Adapters implement them: `sprawlr-aws` for a real account, `sprawlr-fs` for storage,
//! [`crate::demo`] for the fictional demo account. Use cases never name an adapter.

use crate::settings::DesktopSettings;
use sprawlr_domain::status::Notice;
use sprawlr_domain::{ArchGraph, Snapshot};
use sprawlr_iac::{ConfigRead, Wanted};
use std::future::Future;
use std::sync::Arc;

/// Progress messages for a person watching ("us-east-1: reading functions").
pub type Progress = Arc<dyn Fn(String) + Send + Sync>;

/// A progress sink that drops everything.
pub fn silent() -> Progress {
    Arc::new(|_| {})
}

/// Things that went partly wrong without stopping the work ("no permission for X").
pub type Warnings = Vec<String>;

/// The structure of an account: resources, containment and declared connections.
pub struct Discovered {
    pub graph: ArchGraph,
    pub warnings: Warnings,
}

/// Where the infrastructure comes from.
pub trait InfraSource {
    fn discover(&self, progress: &Progress) -> impl Future<Output = anyhow::Result<Discovered>> + Send;
}

/// Alarms, metrics and traces for the resources already in the graph.
pub trait HealthSource {
    fn read_health(&self, graph: &mut ArchGraph, progress: &Progress) -> impl Future<Output = anyhow::Result<Warnings>> + Send;
}

/// Error lines from logs, stored on each resource as masked patterns
/// (`sprawlr_domain::logs`), never as raw lines.
pub trait ErrorLogSource {
    fn read_errors(&self, graph: &mut ArchGraph, progress: &Progress) -> impl Future<Output = anyhow::Result<Warnings>> + Send;
}

/// Each resource's whole configuration, read again when a person exports code (ADR 0006). It
/// goes into the files they asked for and nowhere else: never into a snapshot.
pub trait ConfigSource {
    fn read_configs(&self, wanted: &[Wanted], progress: &Progress) -> impl Future<Output = anyhow::Result<ConfigRead>> + Send;
}

/// Where the latest snapshot is kept between runs, and (optionally) earlier full scans to
/// say what changed.
pub trait SnapshotStore: Send + Sync {
    fn save(&self, snapshot: &Snapshot) -> anyhow::Result<()>;
    fn latest(&self) -> anyhow::Result<Option<Snapshot>>;

    /// Keep a full scan in the history of its account. Stores without history ignore it.
    fn record_scan(&self, _snapshot: &Snapshot) -> anyhow::Result<()> {
        Ok(())
    }

    /// When the kept full scans of an account were taken, oldest first.
    fn scans(&self, _account: &str) -> anyhow::Result<Vec<chrono::DateTime<chrono::Utc>>> {
        Ok(vec![])
    }

    fn scan(&self, _account: &str, _taken_at: chrono::DateTime<chrono::Utc>) -> anyhow::Result<Option<Snapshot>> {
        Ok(None)
    }
}

/// Tells a person something changed while they were not looking (desktop notifications).
pub trait Notifier: Send + Sync {
    fn notify(&self, notice: &Notice) -> anyhow::Result<()>;
}

/// The desktop app's choices between runs.
pub trait SettingsStore: Send + Sync {
    fn load(&self) -> anyhow::Result<DesktopSettings>;
    /// Load, change and save as one step, so two writers never undo each other (a background
    /// refresh recording a notification must not bring back a setting just turned off).
    fn update(&self, change: &mut dyn FnMut(&mut DesktopSettings)) -> anyhow::Result<DesktopSettings>;
}

/// Credentials are missing, expired or rejected: the person must fix them before anything can
/// be read. Adapters return it inside `anyhow::Error` with a hint in their own terms (for AWS,
/// `aws sso login --profile prod-admin`); drivers find it with `downcast_ref`.
#[derive(Debug, Clone)]
pub struct SignInNeeded {
    pub profile: Option<String>,
    /// What to run or check, for the person.
    pub hint: String,
    /// The provider's own message, for the log.
    pub reason: String,
}

impl std::fmt::Display for SignInNeeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.profile {
            Some(p) => write!(f, "can't sign in with {p}"),
            None => write!(f, "can't sign in with the default credentials"),
        }
    }
}

impl std::error::Error for SignInNeeded {}
