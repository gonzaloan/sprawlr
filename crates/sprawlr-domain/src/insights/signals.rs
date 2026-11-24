//! Reading nodes: metrics, error counts, kinds that buffer or store, how sure an edge is.

use crate::{EvidenceKind, Layer, Node, NodeKind};
use serde_json::Value;

pub(super) fn metric<'a>(n: &'a Node, key: &str) -> Option<&'a Value> {
    n.attrs.get("metrics")?.as_array()?.iter().find(|m| m.get("key").and_then(|k| k.as_str()) == Some(key))
}

pub(super) fn series(n: &Node, key: &str) -> Vec<Option<f64>> {
    metric(n, key).and_then(|m| m.get("values")?.as_array().cloned()).map(|v| v.iter().map(|x| x.as_f64()).collect()).unwrap_or_default()
}

pub(super) fn total(n: &Node, key: &str) -> Option<f64> {
    let s = series(n, key);
    (!s.is_empty()).then(|| s.iter().flatten().sum())
}

pub(super) fn peak(n: &Node, key: &str) -> Option<f64> {
    series(n, key).into_iter().flatten().reduce(f64::max)
}

/// The metric that best counts executions of whatever starts at this node.
pub(super) fn runs(n: &Node) -> Option<f64> {
    ["requests", "invocations", "started", "sent", "published"].iter().find_map(|k| total(n, k))
}

/// Errors in 24 h. The error metric is authoritative (a failed invocation, a 5xx); error lines
/// in logs count only where no such metric exists (ECS services, EC2 instances).
pub(super) fn errors(n: &Node) -> f64 {
    let metric: Vec<f64> = ["errors", "errors_5xx", "failed"].iter().filter_map(|k| total(n, k)).collect();
    if metric.is_empty() {
        crate::logs::of(n).map_or(0.0, |l| l.total)
    } else {
        metric.iter().sum()
    }
}

pub(super) fn latency_ms(n: &Node) -> Option<f64> {
    ["latency_p95", "duration_p95"].iter().find_map(|k| series(n, k).into_iter().flatten().last())
}

pub(super) fn attr_f64(n: &Node, k: &str) -> Option<f64> {
    n.attrs.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
}

pub(super) fn is_buffer(k: &NodeKind) -> bool {
    matches!(k, NodeKind::SqsQueue | NodeKind::SnsTopic | NodeKind::EventBus | NodeKind::KinesisStream)
}

pub(super) fn is_store(k: &NodeKind) -> bool {
    matches!(k.clone().layer(), Layer::Data)
}

pub(super) fn real(e: &crate::Edge) -> bool {
    e.strength().is_some_and(|s| s >= EvidenceKind::Declared)
}

pub(super) fn is_prod(n: &Node) -> bool {
    n.env.as_ref().is_none_or(|e| crate::analysis::env_name(&e.value) == "prod")
}
