//! What changed between two reads of the same account: resources added and removed,
//! settings that moved, connections gained and lost, findings opened and resolved, health
//! and error messages. Computed when a snapshot is taken and stored with it (ADR 0004), so
//! every view tells the same story.

use crate::analysis::{display_name, kind_label, verb};
use crate::status::Level;
use crate::{EvidenceKind, Health, Node, NodeId, Snapshot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Settings worth reporting when they move (capacity, runtime, durability, exposure), with
/// the words a person uses for them.
const WATCHED: [(&str, &str); 17] = [
    ("runtime", "Runtime"),
    ("memory_mb", "Memory (MB)"),
    ("timeout_s", "Timeout (s)"),
    ("images", "Image"),
    ("desired", "Desired tasks"),
    ("instance_type", "Instance type"),
    ("instances", "Instances"),
    ("multi_az", "Multi-AZ"),
    ("encrypted", "Encryption"),
    ("public_access_block", "Public access block"),
    ("versioning", "Versioning"),
    ("visibility_timeout_s", "Visibility timeout (s)"),
    ("dead_letter_queue", "Dead-letter queue"),
    ("reserved_concurrency", "Reserved concurrency"),
    ("state", "State"),
    ("engine_version", "Engine version"),
    ("env_keys", "Environment variables"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: NodeId,
    pub name: String,
    pub kind: String,
    pub app: Option<String>,
}

fn item(n: &Node) -> Item {
    Item { id: n.id.clone(), name: display_name(n), kind: kind_label(&n.kind), app: n.app.as_ref().map(|a| a.value.clone()) }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingChange {
    pub resource: Item,
    /// The attribute, as stored ("timeout_s").
    pub setting: String,
    /// What a person calls it ("Timeout (s)").
    pub label: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub from: String,
    pub verb: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthChange {
    pub resource: Item,
    pub before: Health,
    pub after: Health,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Changes {
    /// When the earlier read was taken.
    pub since: chrono::DateTime<chrono::Utc>,
    /// One line, as [`Changes::headline`] says it, so every view uses the same words.
    #[serde(default)]
    pub summary: String,
    pub added: Vec<Item>,
    pub removed: Vec<Item>,
    pub settings: Vec<SettingChange>,
    pub connections_added: Vec<Connection>,
    pub connections_removed: Vec<Connection>,
    /// "resource: title" of findings that appeared, and of findings that went away.
    pub findings_new: Vec<String>,
    pub findings_resolved: Vec<String>,
    /// Items of "what needs attention" (bottlenecks, error origins, queue settings…) that
    /// appeared, and that went away.
    pub attention_new: Vec<String>,
    pub attention_resolved: Vec<String>,
    /// The tray level before and after, when it moved.
    pub level: Option<(Level, Level)>,
    pub health: Vec<HealthChange>,
    /// "resource: message" for error patterns that were not there before.
    pub new_errors: Vec<String>,
    /// Probably-not-needed cost, USD a month, before and after.
    pub cleanup_monthly: (f64, f64),
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.settings.is_empty()
            && self.connections_added.is_empty()
            && self.connections_removed.is_empty()
            && self.findings_new.is_empty()
            && self.findings_resolved.is_empty()
            && self.attention_new.is_empty()
            && self.attention_resolved.is_empty()
            && self.level.is_none()
            && self.health.is_empty()
            && self.new_errors.is_empty()
    }

    /// One line: "3 new resources, 1 removed, 2 settings changed, 1 new finding".
    pub fn headline(&self) -> String {
        let count = |n: usize, one: &str, many: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            n => Some(format!("{n} {many}")),
        };
        let parts: Vec<String> = [
            count(self.added.len(), "new resource", "new resources"),
            count(self.removed.len(), "removed", "removed"),
            count(self.settings.len(), "setting changed", "settings changed"),
            count(self.connections_added.len() + self.connections_removed.len(), "connection changed", "connections changed"),
            count(self.attention_new.len(), "new thing to look at", "new things to look at"),
            count(self.attention_resolved.len(), "thing no longer needs attention", "things no longer need attention"),
            count(self.findings_new.len(), "new finding", "new findings"),
            count(self.findings_resolved.len(), "finding resolved", "findings resolved"),
            count(self.health.len(), "health change", "health changes"),
            count(self.new_errors.len(), "new error message", "new error messages"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() {
            "Nothing changed".into()
        } else {
            parts.join(", ")
        }
    }
}

fn account(s: &Snapshot) -> Option<&str> {
    s.nodes.iter().find_map(|n| n.account.as_deref())
}

fn value(v: Option<&serde_json::Value>) -> String {
    match v {
        None | Some(serde_json::Value::Null) => "not set".into(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// Runtime connections known from configuration or traffic (IAM-only reach is not a change
/// anyone made to how things talk).
fn connections(s: &Snapshot, names: &BTreeMap<&NodeId, String>) -> BTreeSet<(String, String, String)> {
    s.edges
        .iter()
        .filter(|e| e.edge.kind.is_runtime_dependency() && e.edge.strength().is_some_and(|k| k >= EvidenceKind::Declared))
        .filter_map(|e| Some((names.get(&e.from)?.clone(), verb(e.edge.kind).to_string(), names.get(&e.to)?.clone())))
        .collect()
}

fn names(s: &Snapshot) -> BTreeMap<&NodeId, String> {
    s.nodes.iter().map(|n| (&n.id, display_name(n))).collect()
}

/// What changed from `before` to `after`, or `None` when they are different accounts.
pub fn compare(before: &Snapshot, after: &Snapshot) -> Option<Changes> {
    if account(before).is_some() && account(after).is_some() && account(before) != account(after) {
        return None;
    }
    let was: BTreeMap<&NodeId, &Node> = before.nodes.iter().map(|n| (&n.id, n)).collect();
    let now: BTreeMap<&NodeId, &Node> = after.nodes.iter().map(|n| (&n.id, n)).collect();
    // Placeholders (references to things not scanned) come and go with the scan's reach.
    let real = |n: &&Node| n.attrs.get("placeholder").is_none() && n.layer() != crate::Layer::Scope;

    let added = after.nodes.iter().filter(real).filter(|n| !was.contains_key(&n.id)).map(item).collect();
    let removed = before.nodes.iter().filter(real).filter(|n| !now.contains_key(&n.id)).map(item).collect();

    let mut settings = Vec::new();
    let mut health = Vec::new();
    let mut new_errors = Vec::new();
    for n in after.nodes.iter().filter(real) {
        let Some(old) = was.get(&n.id) else { continue };
        for (key, label) in WATCHED {
            let (a, b) = (old.attrs.get(key), n.attrs.get(key));
            if a != b && !(a.is_none() && b.is_some_and(|v| v.is_null())) {
                settings.push(SettingChange {
                    resource: item(n),
                    setting: key.into(),
                    label: label.into(),
                    before: value(a),
                    after: value(b),
                });
            }
        }
        if old.health != n.health && old.health != Health::Unknown && n.health != Health::Unknown {
            health.push(HealthChange { resource: item(n), before: old.health, after: n.health });
        }
        let old_patterns: BTreeSet<String> =
            crate::logs::of(old).map(|l| l.patterns.into_iter().map(|p| p.message).collect()).unwrap_or_default();
        if let Some(l) = crate::logs::of(n) {
            new_errors.extend(
                l.patterns
                    .into_iter()
                    .filter(|p| !old_patterns.contains(&p.message))
                    .map(|p| format!("{}: {}", display_name(n), p.message)),
            );
        }
    }

    let (nb, na) = (names(before), names(after));
    let (cb, ca) = (connections(before, &nb), connections(after, &na));
    let conn = |(from, verb, to): &(String, String, String)| Connection { from: from.clone(), verb: verb.clone(), to: to.clone() };
    // Connections to resources that were added or removed are already told by those lists.
    let between_known = |(f, _, t): &&(String, String, String)| {
        nb.values().any(|x| x == f) && na.values().any(|x| x == f) && nb.values().any(|x| x == t) && na.values().any(|x| x == t)
    };

    let finding = |s: &Snapshot, names: &BTreeMap<&NodeId, String>| -> BTreeSet<String> {
        s.findings.iter().map(|f| format!("{}: {}", names.get(&f.node).cloned().unwrap_or_else(|| f.node.0.clone()), f.title)).collect()
    };
    let (fb, fa) = (finding(before, &nb), finding(after, &na));

    let (sb, sa) = (before.status_or_compute(), after.status_or_compute());
    let cost = |s: &Snapshot| s.insights.lifecycle.iter().filter_map(|l| l.monthly_cost).sum::<f64>();

    let mut c = Changes {
        since: before.taken_at,
        summary: String::new(),
        added,
        removed,
        settings,
        connections_added: ca.difference(&cb).filter(between_known).map(conn).collect(),
        connections_removed: cb.difference(&ca).filter(between_known).map(conn).collect(),
        findings_new: fa.difference(&fb).cloned().collect(),
        findings_resolved: fb.difference(&fa).cloned().collect(),
        attention_new: sa.items.iter().filter(|i| !sb.items.iter().any(|b| b.key == i.key)).map(|i| i.title.clone()).collect(),
        attention_resolved: sb.items.iter().filter(|i| !sa.items.iter().any(|a| a.key == i.key)).map(|i| i.title.clone()).collect(),
        level: (sb.level != sa.level).then_some((sb.level, sa.level)),
        health,
        new_errors,
        cleanup_monthly: (cost(before), cost(after)),
    };
    c.summary = c.headline();
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixtures::demo_graph, infer::infer, NodeKind};

    fn demo() -> Snapshot {
        let mut g = demo_graph();
        infer(&mut g);
        g.snapshot()
    }

    #[test]
    fn the_same_read_twice_changes_nothing() {
        let s = demo();
        let c = compare(&s, &s).unwrap();
        assert!(c.is_empty(), "{c:?}");
        assert_eq!(c.headline(), "Nothing changed");
    }

    #[test]
    fn reports_what_moved_and_how() {
        let before = demo();
        let mut g = crate::ArchGraph::from_snapshot(&before);
        // A new queue, a removed leftover, a resized function, an alarm, a new error.
        let mut q = Node::new("arn:aws:sqs:us-east-1:111122223333:refunds", NodeKind::SqsQueue, "refunds");
        q.account = Some("111122223333".into());
        g.upsert_node(q);
        let invoice = g.nodes().find(|n| n.name.contains("InvoiceProcessor")).unwrap().id.clone();
        let search = g.nodes().find(|n| n.name == "search-indexer").unwrap().id.clone();
        g.node_mut(&invoice).unwrap().attrs["memory_mb"] = serde_json::json!(2048);
        g.node_mut(&search).unwrap().health = Health::Alarm;
        let after_graph = {
            let mut snap = g.snapshot();
            snap.nodes.retain(|n| n.name != "legacy-export-job");
            snap.edges.retain(|e| !e.from.0.contains("legacy-export-job") && !e.to.0.contains("legacy-export-job"));
            snap
        };
        let c = compare(&before, &after_graph).unwrap();
        assert_eq!(c.added.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["refunds"]);
        assert_eq!(c.removed.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["legacy-export-job"]);
        let mem = c.settings.iter().find(|s| s.setting == "memory_mb").unwrap();
        assert_eq!((mem.resource.name.as_str(), mem.after.as_str()), ("InvoiceProcessor", "2048"));
        assert!(c.health.iter().any(|h| h.resource.name == "search-indexer" && h.after == Health::Alarm));
        assert!(c.findings_new.iter().any(|f| f.starts_with("search-indexer: Alarm firing")), "{:?}", c.findings_new);
        assert!(c.cleanup_monthly.0 >= c.cleanup_monthly.1);
        assert!(c.headline().starts_with("1 new resource, 1 removed, 1 setting changed"), "{}", c.headline());
        assert!(c.attention_new.iter().any(|t| t == "Alarm firing on search-indexer"), "{:?}", c.attention_new);
    }

    #[test]
    fn different_accounts_are_not_compared() {
        let a = demo();
        let mut b = a.clone();
        b.nodes.iter_mut().for_each(|n| n.account = Some("999".into()));
        assert!(compare(&a, &b).is_none());
    }
}
