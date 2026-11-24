//! Use cases: the things a person asks sprawlr to do. Each one orchestrates ports and domain
//! functions; none of them knows whether it runs against AWS, the demo or a test double.

use crate::ports::{ErrorLogSource, HealthSource, InfraSource, Progress, SnapshotStore, Warnings};
use sprawlr_domain::{iam, infer::infer, ArchGraph, Snapshot};

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    /// Alarms, metrics and traces (billed by CloudWatch, a few cents per scan).
    pub health: bool,
    /// Error lines from logs (Logs Insights reads log content and bills per GB scanned).
    pub error_logs: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions { health: true, error_logs: false }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanResult {
    pub snapshot: Snapshot,
    pub warnings: Warnings,
}

/// Map an account: discover its structure, add what IAM allows, read health and error logs
/// if asked, infer apps and environments, analyse, and keep the result.
pub async fn scan_account<S>(
    source: &S,
    opts: ScanOptions,
    store: Option<&dyn SnapshotStore>,
    progress: &Progress,
) -> anyhow::Result<ScanResult>
where
    S: InfraSource + HealthSource + ErrorLogSource + Sync,
{
    let discovered = source.discover(progress).await?;
    let mut graph = discovered.graph;
    let mut warnings = discovered.warnings;

    iam::apply(&mut graph);
    if opts.health {
        warnings.extend(source.read_health(&mut graph, progress).await?);
    }
    if opts.error_logs {
        warnings.extend(source.read_errors(&mut graph, progress).await?);
    }
    finish(graph, warnings, store, progress)
}

/// Read health again for the snapshot on screen, without rediscovering the structure. Error
/// logs are read again if the snapshot has them.
pub async fn refresh_health<S>(
    source: &S,
    snapshot: &Snapshot,
    store: Option<&dyn SnapshotStore>,
    progress: &Progress,
) -> anyhow::Result<ScanResult>
where
    S: HealthSource + ErrorLogSource + Sync,
{
    let had_logs = snapshot.nodes.iter().any(|n| n.attrs.get("log_errors").is_some());
    let mut graph = ArchGraph::from_snapshot(snapshot);
    let mut warnings = source.read_health(&mut graph, progress).await?;
    if had_logs {
        warnings.extend(source.read_errors(&mut graph, progress).await?);
    }
    finish(graph, warnings, store, progress)
}

fn finish(mut graph: ArchGraph, warnings: Warnings, store: Option<&dyn SnapshotStore>, progress: &Progress) -> anyhow::Result<ScanResult> {
    progress("working out apps, flows and findings".into());
    infer(&mut graph);
    let snapshot = graph.snapshot();
    if let Some(s) = store {
        s.save(&snapshot)?;
    }
    Ok(ScanResult { snapshot, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::Demo;
    use crate::ports::silent;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<Option<Snapshot>>);

    impl SnapshotStore for Memory {
        fn save(&self, s: &Snapshot) -> anyhow::Result<()> {
            *self.0.lock().unwrap() = Some(s.clone());
            Ok(())
        }
        fn latest(&self) -> anyhow::Result<Option<Snapshot>> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    #[tokio::test]
    async fn scan_infers_analyses_and_keeps_the_snapshot() {
        let store = Memory::default();
        let r = scan_account(&Demo, ScanOptions::default(), Some(&store), &silent()).await.unwrap();
        assert!(r.snapshot.nodes.iter().any(|n| n.app.is_some()), "apps inferred");
        assert!(!r.snapshot.insights.flows.is_empty(), "flows analysed");
        assert!(!r.snapshot.findings.is_empty(), "findings computed");
        assert_eq!(store.latest().unwrap().unwrap().nodes.len(), r.snapshot.nodes.len());
    }

    #[tokio::test]
    async fn refresh_keeps_structure() {
        let first = scan_account(&Demo, ScanOptions::default(), None, &silent()).await.unwrap();
        let again = refresh_health(&Demo, &first.snapshot, None, &silent()).await.unwrap();
        assert_eq!(first.snapshot.nodes.len(), again.snapshot.nodes.len());
        assert_eq!(first.snapshot.edges.len(), again.snapshot.edges.len());
    }
}
