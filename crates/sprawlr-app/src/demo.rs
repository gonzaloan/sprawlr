//! In-memory adapter for the fictional demo account (`sprawlr_domain::fixtures`). It serves
//! the same ports as a real account, so the demo exercises the same use cases.

use crate::ports::{Discovered, ErrorLogSource, HealthSource, InfraSource, Progress, Warnings};
use sprawlr_domain::{fixtures::demo_graph, ArchGraph};

/// The demo account analysed exactly as a scan would be (IAM reach, inference, analysis),
/// with what changed since [`earlier`], for drivers that need a snapshot without running the
/// async use case.
pub fn snapshot() -> sprawlr_domain::Snapshot {
    with_changes(analysed(demo_graph()))
}

fn analysed(mut g: ArchGraph) -> sprawlr_domain::Snapshot {
    sprawlr_domain::iam::apply(&mut g);
    sprawlr_domain::infer::infer(&mut g);
    g.snapshot()
}

/// The demo as a scan two days earlier would have read it: the reports bucket did not exist
/// yet, InvoiceProcessor still timed out at 5 s (raising it to 60 s is what made the queue
/// able to deliver twice), checkout-api's connection pool was fine, and an old migration
/// security group was still around.
pub fn earlier() -> sprawlr_domain::Snapshot {
    let mut g = demo_graph();
    if let Some(n) = g.nodes_mut().find(|n| n.name.contains("InvoiceProcessor")) {
        n.attrs["timeout_s"] = serde_json::json!(5);
    }
    // The pool was fine: its logs were read, with no error lines (not "logs not read").
    if let Some(n) = g.nodes_mut().find(|n| n.id.0.ends_with("service/prod-cluster/checkout-api")) {
        if let Some(l) = n.attrs.get_mut("log_errors") {
            l["patterns"] = serde_json::json!([]);
            l["total"] = serde_json::json!(0.0);
        }
    }
    let mut s = analysed(g);
    s.nodes.retain(|n| n.name != "reports-output");
    s.edges.retain(|e| !e.from.0.ends_with(":::reports-output") && !e.to.0.ends_with(":::reports-output"));
    let mut g = ArchGraph::from_snapshot(&s);
    let mut old = sprawlr_domain::Node::new(
        "arn:aws:ec2:us-east-1:111122223333:security-group/sg-0batch2023",
        sprawlr_domain::NodeKind::SecurityGroup,
        "batch-2023-sg",
    );
    old.account = s.nodes.iter().find_map(|n| n.account.clone());
    g.upsert_node(old);
    let mut s = g.snapshot();
    s.taken_at -= chrono::Duration::days(2);
    s
}

/// Attach what changed since [`earlier`] to a demo snapshot.
pub fn with_changes(mut s: sprawlr_domain::Snapshot) -> sprawlr_domain::Snapshot {
    s.changes = sprawlr_domain::changes::compare(&earlier(), &s);
    s
}

/// The demo account. Its health and error logs are part of the fixture, so reading them
/// again changes nothing.
pub struct Demo;

impl InfraSource for Demo {
    async fn discover(&self, progress: &Progress) -> anyhow::Result<Discovered> {
        progress("reading the demo account".into());
        Ok(Discovered { graph: demo_graph(), warnings: vec![] })
    }
}

impl HealthSource for Demo {
    async fn read_health(&self, _: &mut ArchGraph, _: &Progress) -> anyhow::Result<Warnings> {
        Ok(vec![])
    }
}

impl ErrorLogSource for Demo {
    async fn read_errors(&self, _: &mut ArchGraph, _: &Progress) -> anyhow::Result<Warnings> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_demo_tells_a_story_of_change() {
        let c = super::snapshot().changes.expect("compared with two days earlier");
        assert_eq!(c.added.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["reports-output"]);
        assert_eq!(c.removed.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["batch-2023-sg"]);
        assert!(c
            .settings
            .iter()
            .any(|s| s.resource.name == "InvoiceProcessor" && s.setting == "timeout_s" && s.before == "5" && s.after == "60"));
        assert!(
            c.attention_new.iter().any(|t| t.contains("checkout-order-events")),
            "the longer timeout made the queue able to deliver twice: {:?}",
            c.attention_new
        );
        assert!(c.new_errors.iter().any(|e| e.starts_with("checkout-api: pg: connection pool exhausted")));
    }
}
