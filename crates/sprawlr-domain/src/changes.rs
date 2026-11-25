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
/// the words a person uses for them. Counts that autoscaling moves on its own (desired tasks)
/// are left out.
const WATCHED: [(&str, &str); 15] = [
    ("runtime", "Runtime"),
    ("memory_mb", "Memory (MB)"),
    ("timeout_s", "Timeout (s)"),
    ("images", "Image"),
    ("instance_type", "Instance type"),
    ("instances", "Instances"),
    ("multi_az", "Multi-AZ"),
    ("encrypted", "Encryption"),
    ("public_access_block", "Public access block"),
    ("versioning", "Versioning"),
    ("visibility_timeout_s", "Visibility timeout (s)"),
    ("dead_letter_queue", "Dead-letter queue"),
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

/// One line of the change list, in the words every view uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// attention, error, health, new, removed, changed, connection, resolved
    pub tag: String,
    pub label: String,
    pub text: String,
    /// A resource to open, when the row is about one that still exists.
    pub id: Option<NodeId>,
}

/// Insight kinds and finding rules that depend on telemetry: when only one of the two reads
/// measured health, their coming and going says nothing about the account.
const MEASURED: [&str; 5] = ["alarm", "metric_alert", "error_origin", "bottleneck", "slow_hop"];

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
    /// Everything above as one list, most telling first (see [`Changes::rows`]).
    #[serde(default)]
    pub rows: Vec<Row>,
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

    /// Every change as one list, most telling first: what needs attention now, new errors,
    /// health, then structure, then what got better.
    pub fn rows(&self) -> Vec<Row> {
        let row =
            |tag: &str, label: &str, text: String, id: Option<&NodeId>| Row { tag: tag.into(), label: label.into(), text, id: id.cloned() };
        let health = |h: Health| match h {
            Health::Ok => "OK",
            Health::Warning => "warning",
            Health::Alarm => "alarm firing",
            Health::Unknown => "unknown",
        };
        let mut out = Vec::new();
        out.extend(self.attention_new.iter().map(|t| row("attention", "Needs attention", t.clone(), None)));
        out.extend(self.new_errors.iter().map(|t| row("error", "New error", t.clone(), None)));
        out.extend(self.findings_new.iter().map(|t| row("attention", "New finding", t.clone(), None)));
        out.extend(self.health.iter().map(|h| {
            row("health", "Health", format!("{}: {} → {}", h.resource.name, health(h.before), health(h.after)), Some(&h.resource.id))
        }));
        out.extend(self.added.iter().map(|i| row("new", "New", format!("{} ({})", i.name, i.kind), Some(&i.id))));
        out.extend(self.removed.iter().map(|i| row("removed", "Removed", format!("{} ({})", i.name, i.kind), None)));
        out.extend(self.settings.iter().map(|x| {
            row("changed", "Changed", format!("{}: {} {} → {}", x.resource.name, x.label, x.before, x.after), Some(&x.resource.id))
        }));
        out.extend(
            self.connections_added.iter().map(|x| row("connection", "New connection", format!("{} {} {}", x.from, x.verb, x.to), None)),
        );
        out.extend(self.connections_removed.iter().map(|x| row("connection", "Gone", format!("{} {} {}", x.from, x.verb, x.to), None)));
        out.extend(self.attention_resolved.iter().chain(&self.findings_resolved).map(|t| row("resolved", "Resolved", t.clone(), None)));
        out
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

fn unset(v: Option<&serde_json::Value>) -> bool {
    v.is_none_or(|v| v.is_null())
}

/// Some node has metrics or a known health: telemetry was read.
fn measured(s: &Snapshot) -> bool {
    s.nodes
        .iter()
        .any(|n| n.health != Health::Unknown || n.attrs.get("metrics").is_some_and(|m| m.as_array().is_some_and(|a| !a.is_empty())))
}

fn regions(s: &Snapshot) -> BTreeSet<&str> {
    s.nodes.iter().filter_map(|n| n.region.as_deref()).collect()
}

/// Connections someone configured (not traffic that comes and goes, not IAM reach), by id.
fn connections(s: &Snapshot, keep: &dyn Fn(&NodeId) -> bool) -> BTreeSet<(NodeId, crate::EdgeKind, NodeId)> {
    s.edges
        .iter()
        .filter(|e| e.edge.kind.is_runtime_dependency() && e.edge.evidence.iter().any(|v| v.kind == EvidenceKind::Declared))
        .filter(|e| keep(&e.from) && keep(&e.to))
        .map(|e| (e.from.clone(), e.edge.kind, e.to.clone()))
        .collect()
}

/// What changed from `before` to `after`, or `None` when they are different accounts.
///
/// Only what both reads could see is compared: resources in regions both scanned, health
/// and telemetry-based items only when both read telemetry, error messages only for
/// resources whose logs were read both times, connections only as configured.
pub fn compare(before: &Snapshot, after: &Snapshot) -> Option<Changes> {
    if account(before).is_some() && account(after).is_some() && account(before) != account(after) {
        return None;
    }
    let (rb, ra) = (regions(before), regions(after));
    let both_regions: BTreeSet<&str> = rb.intersection(&ra).copied().collect();
    // Placeholders (references to things not scanned) and third parties seen only in traces
    // come and go with the scan's reach and with traffic.
    let comparable = |n: &&Node| {
        n.attrs.get("placeholder").is_none()
            && n.layer() != crate::Layer::Scope
            && n.kind != crate::NodeKind::ExternalEndpoint
            && n.region.as_deref().is_none_or(|r| both_regions.contains(r))
    };
    let was: BTreeMap<&NodeId, &Node> = before.nodes.iter().filter(comparable).map(|n| (&n.id, n)).collect();
    let now: BTreeMap<&NodeId, &Node> = after.nodes.iter().filter(comparable).map(|n| (&n.id, n)).collect();
    let telemetry_both = measured(before) && measured(after);

    let added = now.values().filter(|n| !was.contains_key(&n.id)).map(|n| item(n)).collect();
    let removed = was.values().filter(|n| !now.contains_key(&n.id)).map(|n| item(n)).collect();

    let mut settings = Vec::new();
    let mut health = Vec::new();
    let mut new_errors = Vec::new();
    for n in now.values() {
        let Some(old) = was.get(&n.id) else { continue };
        for (key, label) in WATCHED {
            let (a, b) = (old.attrs.get(key), n.attrs.get(key));
            if a != b && !(unset(a) && unset(b)) {
                settings.push(SettingChange {
                    resource: item(n),
                    setting: key.into(),
                    label: label.into(),
                    before: value(a),
                    after: value(b),
                });
            }
        }
        if telemetry_both && old.health != n.health && old.health != Health::Unknown && n.health != Health::Unknown {
            health.push(HealthChange { resource: item(n), before: old.health, after: n.health });
        }
        // Only where logs were read both times: a first read is not a burst of new errors.
        if let (Some(o), Some(l)) = (crate::logs::of(old), crate::logs::of(n)) {
            let seen: BTreeSet<String> = o.patterns.into_iter().map(|p| p.message).collect();
            new_errors.extend(
                l.patterns.into_iter().filter(|p| !seen.contains(&p.message)).map(|p| format!("{}: {}", display_name(n), p.message)),
            );
        }
    }

    let known = |id: &NodeId| was.contains_key(id) && now.contains_key(id);
    let (cb, ca) = (connections(before, &known), connections(after, &known));
    let name = |id: &NodeId| now.get(id).or_else(|| was.get(id)).map(|n| display_name(n)).unwrap_or_else(|| id.0.clone());
    let conn =
        |(from, kind, to): &(NodeId, crate::EdgeKind, NodeId)| Connection { from: name(from), verb: verb(*kind).into(), to: name(to) };

    // Findings by (resource, rule); telemetry-based ones only when both reads measured.
    let counts = |rule: &str| telemetry_both || !MEASURED.contains(&rule);
    let findings = |s: &Snapshot| -> BTreeMap<(NodeId, String), String> {
        s.findings
            .iter()
            .filter(|f| counts(&f.rule) && (known(&f.node) || !was.contains_key(&f.node) && !now.contains_key(&f.node)))
            .map(|f| ((f.node.clone(), f.rule.clone()), format!("{}: {}", name(&f.node), f.title)))
            .collect()
    };
    let (fb, fa) = (findings(before), findings(after));

    let (sb, sa) = (before.status_or_compute(), after.status_or_compute());
    let kind_of = |key: &str| key.split(':').next().unwrap_or("").to_string();
    let items = |s: &crate::status::AccountStatus| -> BTreeMap<String, String> {
        s.items.iter().filter(|i| counts(&kind_of(&i.key))).map(|i| (i.key.clone(), i.title.clone())).collect()
    };
    let (ib, ia) = (items(&sb), items(&sa));
    let cost = |s: &Snapshot| s.insights.lifecycle.iter().filter_map(|l| l.monthly_cost).sum::<f64>();

    let mut c = Changes {
        since: before.taken_at,
        summary: String::new(),
        added,
        removed,
        settings,
        connections_added: ca.difference(&cb).map(conn).collect(),
        connections_removed: cb.difference(&ca).map(conn).collect(),
        findings_new: fa.iter().filter(|(k, _)| !fb.contains_key(*k)).map(|(_, t)| t.clone()).collect(),
        findings_resolved: fb.iter().filter(|(k, _)| !fa.contains_key(*k)).map(|(_, t)| t.clone()).collect(),
        attention_new: ia.iter().filter(|(k, _)| !ib.contains_key(*k)).map(|(_, t)| t.clone()).collect(),
        attention_resolved: ib.iter().filter(|(k, _)| !ia.contains_key(*k)).map(|(_, t)| t.clone()).collect(),
        level: (telemetry_both && sb.level != sa.level).then_some((sb.level, sa.level)),
        health,
        new_errors,
        cleanup_monthly: (cost(before), cost(after)),
        rows: vec![],
    };
    c.summary = c.headline();
    c.rows = c.rows();
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
        assert_eq!(c.rows[0].tag, "attention", "most telling first");
        assert!(c.attention_new.iter().any(|t| t == "Alarm firing on search-indexer"), "{:?}", c.attention_new);
    }

    #[test]
    fn what_only_one_read_could_see_is_not_a_change() {
        let full = demo();
        // Telemetry off: no alarms resolved, no health changes, no level change.
        let mut quiet = full.clone();
        for n in quiet.nodes.iter_mut() {
            n.health = Health::Unknown;
            if let Some(a) = n.attrs.as_object_mut() {
                a.remove("metrics");
                a.remove("log_errors");
            }
        }
        let mut g = crate::ArchGraph::from_snapshot(&quiet);
        crate::infer::infer(&mut g);
        let quiet = g.snapshot();
        let c = compare(&full, &quiet).unwrap();
        assert!(c.findings_resolved.iter().all(|f| !f.contains("Alarm firing")), "{:?}", c.findings_resolved);
        assert!(c.health.is_empty() && c.level.is_none());
        // Logs read for the first time: not a burst of new errors.
        let c = compare(&quiet, &full).unwrap();
        assert!(c.new_errors.is_empty(), "{:?}", c.new_errors);
        // Another region scanned too: its resources are not "new".
        let mut wider = full.clone();
        let mut extra = Node::new("arn:aws:sqs:eu-west-1:111122223333:eu-orders", NodeKind::SqsQueue, "eu-orders");
        extra.region = Some("eu-west-1".into());
        extra.account = Some("111122223333".into());
        wider.nodes.push(extra);
        assert!(compare(&full, &wider).unwrap().added.is_empty());
    }

    #[test]
    fn copies_with_the_same_name_in_two_environments_stay_apart() {
        let before = demo();
        let prod_api = before.nodes.iter().find(|n| n.id.0.ends_with("service/prod-cluster/checkout-api")).unwrap().id.clone();
        let mut after = before.clone();
        after.nodes.retain(|n| n.id != prod_api);
        after.edges.retain(|e| e.from != prod_api && e.to != prod_api);
        let c = compare(&before, &after).unwrap();
        assert_eq!(c.removed.len(), 1);
        assert!(c.connections_removed.is_empty(), "its connections went with it: {:?}", c.connections_removed);
    }

    #[test]
    fn a_placeholder_that_gets_scanned_is_new_not_changed() {
        let before = demo();
        let mut after = before.clone();
        let mut p = Node::new("arn:aws:lambda:us-east-1:111122223333:function:late", NodeKind::LambdaFunction, "late");
        p.attrs = serde_json::json!({"placeholder": true});
        let mut b2 = before.clone();
        b2.nodes.push(p.clone());
        p.attrs = serde_json::json!({"runtime": "python3.13"});
        after.nodes.push(p);
        let c = compare(&b2, &after).unwrap();
        assert_eq!(c.added.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["late"]);
        assert!(c.settings.is_empty(), "{:?}", c.settings);
    }

    #[test]
    fn different_accounts_are_not_compared() {
        let a = demo();
        let mut b = a.clone();
        b.nodes.iter_mut().for_each(|n| n.account = Some("999".into()));
        assert!(compare(&a, &b).is_none());
    }
}
