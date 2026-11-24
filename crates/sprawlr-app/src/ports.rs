//! Ports: what the use cases need from the outside world, stated in domain terms.
//!
//! Adapters implement them: `sprawlr-aws` for a real account, `sprawlr-fs` for storage,
//! [`crate::demo`] for the fictional demo account. Use cases never name an adapter.

use sprawlr_domain::{ArchGraph, Snapshot};
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

/// Where the latest snapshot is kept between runs.
pub trait SnapshotStore: Send + Sync {
    fn save(&self, snapshot: &Snapshot) -> anyhow::Result<()>;
    fn latest(&self) -> anyhow::Result<Option<Snapshot>>;
}
