//! Use cases: the things a person asks sprawlr to do. Each one orchestrates ports and domain
//! functions; none of them knows whether it runs against AWS, the demo or a test double.

use crate::ports::{ErrorLogSource, HealthSource, InfraSource, Notifier, Progress, SettingsStore, SnapshotStore, Warnings};
use sprawlr_domain::status::{news, Notice};
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

#[derive(Debug, Clone, Copy)]
pub struct RefreshOptions {
    pub error_logs: bool,
}

impl RefreshOptions {
    /// What a person pressing "Refresh health" expects: error logs again only if this
    /// snapshot has them.
    pub fn like(snapshot: &Snapshot) -> Self {
        RefreshOptions { error_logs: snapshot.nodes.iter().any(|n| n.attrs.get("log_errors").is_some()) }
    }
}

/// Read health again for a snapshot, without rediscovering the structure.
pub async fn refresh_health<S>(
    source: &S,
    snapshot: &Snapshot,
    opts: RefreshOptions,
    store: Option<&dyn SnapshotStore>,
    progress: &Progress,
) -> anyhow::Result<ScanResult>
where
    S: HealthSource + ErrorLogSource + Sync,
{
    let mut graph = ArchGraph::from_snapshot(snapshot);
    let mut warnings = source.read_health(&mut graph, progress).await?;
    if opts.error_logs {
        warnings.extend(source.read_errors(&mut graph, progress).await?);
    }
    finish(graph, warnings, store, progress)
}

/// One background refresh: health only (never discovery, never error logs, which bill per
/// GB), then a notification if something needs action now that did not before and was not
/// already notified today. Does nothing when no account was ever scanned.
pub async fn refresh_in_background<S>(
    source: &S,
    store: &dyn SnapshotStore,
    settings: &dyn SettingsStore,
    notifier: &dyn Notifier,
    progress: &Progress,
) -> anyhow::Result<Option<Background>>
where
    S: HealthSource + ErrorLogSource + Sync,
{
    let Some(before) = store.latest()? else { return Ok(None) };
    let result = refresh_health(source, &before, RefreshOptions { error_logs: false }, Some(store), progress).await?;
    let mut prefs = settings.load()?;
    let mut notified = None;
    if prefs.notifications {
        if let Some(notice) = news(&before.status_or_compute(), &result.snapshot.status_or_compute()) {
            let now = chrono::Utc::now();
            if !prefs.fresh_keys(&notice.keys, now).is_empty() {
                notifier.notify(&notice)?;
                prefs.remember_notified(&notice.keys, now);
                settings.save(&prefs)?;
                notified = Some(notice);
            }
        }
    }
    Ok(Some(Background { result, notified }))
}

#[derive(Debug, Clone)]
pub struct Background {
    pub result: ScanResult,
    pub notified: Option<Notice>,
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
        let again = refresh_health(&Demo, &first.snapshot, RefreshOptions::like(&first.snapshot), None, &silent()).await.unwrap();
        assert_eq!(first.snapshot.nodes.len(), again.snapshot.nodes.len());
        assert_eq!(first.snapshot.edges.len(), again.snapshot.edges.len());
    }

    /// Records what it was asked to read; reading health sets off an alarm on
    /// search-indexer, so the status gets worse.
    #[derive(Default)]
    struct Probe {
        health: std::sync::atomic::AtomicUsize,
        errors: std::sync::atomic::AtomicUsize,
    }

    impl HealthSource for Probe {
        async fn read_health(&self, g: &mut ArchGraph, _: &Progress) -> anyhow::Result<Warnings> {
            self.health.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            g.nodes_mut().filter(|n| n.name == "search-indexer").for_each(|n| n.health = sprawlr_domain::Health::Alarm);
            Ok(vec![])
        }
    }

    impl ErrorLogSource for Probe {
        async fn read_errors(&self, _: &mut ArchGraph, _: &Progress) -> anyhow::Result<Warnings> {
            self.errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![])
        }
    }

    #[derive(Default)]
    struct Prefs(Mutex<crate::settings::DesktopSettings>);
    impl SettingsStore for Prefs {
        fn load(&self) -> anyhow::Result<crate::settings::DesktopSettings> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, s: &crate::settings::DesktopSettings) -> anyhow::Result<()> {
            *self.0.lock().unwrap() = s.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct Inbox(Mutex<Vec<Notice>>);
    impl Notifier for Inbox {
        fn notify(&self, n: &Notice) -> anyhow::Result<()> {
            self.0.lock().unwrap().push(n.clone());
            Ok(())
        }
    }

    fn calls(c: &std::sync::atomic::AtomicUsize) -> usize {
        c.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[tokio::test]
    async fn background_does_nothing_before_a_first_scan() {
        let (probe, store, prefs, inbox) = (Probe::default(), Memory::default(), Prefs::default(), Inbox::default());
        assert!(refresh_in_background(&probe, &store, &prefs, &inbox, &silent()).await.unwrap().is_none());
        assert_eq!(calls(&probe.health), 0, "no AWS call without a scan");
    }

    #[tokio::test]
    async fn background_reads_health_only_and_notifies_once() {
        let store = Memory::default();
        let first = scan_account(&Demo, ScanOptions { health: true, error_logs: true }, Some(&store), &silent()).await.unwrap();
        let (probe, prefs, inbox) = (Probe::default(), Prefs::default(), Inbox::default());

        refresh_in_background(&probe, &store, &prefs, &inbox, &silent()).await.unwrap().unwrap();
        assert_eq!(calls(&probe.health), 1);
        assert_eq!(calls(&probe.errors), 0, "error logs are never read in the background");
        assert!(inbox.0.lock().unwrap().is_empty(), "notifications are off by default");

        prefs.0.lock().unwrap().notifications = true;
        store.save(&first.snapshot).unwrap();
        let r = refresh_in_background(&probe, &store, &prefs, &inbox, &silent()).await.unwrap().unwrap();
        assert_eq!(r.notified.as_ref().map(|n| n.body.as_str()), Some("Alarm firing on search-indexer"));

        store.save(&first.snapshot).unwrap();
        let again = refresh_in_background(&probe, &store, &prefs, &inbox, &silent()).await.unwrap().unwrap();
        assert!(again.notified.is_none(), "the same item is not notified twice in a day");
        assert_eq!(inbox.0.lock().unwrap().len(), 1);
    }
}
