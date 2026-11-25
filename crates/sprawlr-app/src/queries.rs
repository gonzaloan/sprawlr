//! Questions about a snapshot, answered as plain data: what the MCP server (and any other
//! driver) hands to an agent. Everything here is read-only and computed from the snapshot,
//! so answers match what the app shows.

use serde::Serialize;
use sprawlr_domain::analysis::{display_name, env_name, kind_label, verb};
use sprawlr_domain::insights::{Flow, Priority};
use sprawlr_domain::status::AccountStatus;
use sprawlr_domain::{ArchGraph, EdgeKind, EvidenceKind, Health, Node, NodeId, Snapshot};
use std::collections::{BTreeMap, BTreeSet};

/// A question named something that is not in the snapshot, or more than one thing.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    NotFound(String),
    Ambiguous(String, Vec<String>),
    NoSuchApp(String, Vec<String>),
}

impl std::fmt::Display for Lookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lookup::NotFound(q) => {
                write!(f, "nothing called \"{q}\" in this scan. Use find_resources (or list_apps, list_flows) to see what there is.")
            }
            Lookup::Ambiguous(q, c) => write!(f, "\"{q}\" matches several: {}. Ask again with one of them.", c.join(", ")),
            Lookup::NoSuchApp(a, apps) => write!(f, "\"{a}\" is not an app in this scan. Apps: {}.", apps.join(", ")),
        }
    }
}

impl std::error::Error for Lookup {}

/// A resource by id (ARN), name or the name the app shows, case-insensitively.
pub fn resolve<'a>(g: &'a ArchGraph, q: &str) -> Result<&'a Node, Lookup> {
    if let Some(n) = g.node(&NodeId(q.to_string())) {
        return Ok(n);
    }
    let ql = q.to_ascii_lowercase();
    let exact: Vec<&Node> = g
        .nodes()
        .filter(|n| {
            n.name.eq_ignore_ascii_case(q) || display_name(n).eq_ignore_ascii_case(q) || n.aliases.iter().any(|a| a.eq_ignore_ascii_case(q))
        })
        .collect();
    match exact.as_slice() {
        [one] => return Ok(one),
        [] => {}
        many => return Err(Lookup::Ambiguous(q.into(), many.iter().map(|n| n.id.0.clone()).collect())),
    }
    let partial: Vec<&Node> = g.nodes().filter(|n| display_name(n).to_ascii_lowercase().contains(&ql)).collect();
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(Lookup::NotFound(q.into())),
        many => Err(Lookup::Ambiguous(q.into(), many.iter().take(8).map(|n| n.id.0.clone()).collect())),
    }
}

fn flow_by<'a>(s: &'a Snapshot, q: &str) -> Result<&'a Flow, Lookup> {
    let ql = q.to_ascii_lowercase();
    let all = &s.insights.flows;
    let label = |f: &Flow| format!("{} (id {})", f.name, f.id);
    if let Some(f) = all.iter().find(|f| f.id == q) {
        return Ok(f);
    }
    let exact: Vec<&Flow> = all.iter().filter(|f| f.name.eq_ignore_ascii_case(q)).collect();
    match exact.as_slice() {
        [one] => return Ok(one),
        [] => {}
        many => return Err(Lookup::Ambiguous(q.into(), many.iter().map(|f| label(f)).collect())),
    }
    let partial: Vec<&Flow> = all.iter().filter(|f| f.name.to_ascii_lowercase().contains(&ql)).collect();
    // "checkout" most likely means production: prefer the one flow outside other environments.
    let prod: Vec<&&Flow> = partial.iter().filter(|f| f.env.as_deref().is_none_or(|e| e == "prod")).collect();
    if let [one] = prod.as_slice() {
        return Ok(one);
    }
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(Lookup::NotFound(q.into())),
        many => Err(Lookup::Ambiguous(q.into(), many.iter().map(|f| label(f)).collect())),
    }
}

/// A resource as an agent should name it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ref {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub app: Option<String>,
    pub env: Option<String>,
}

fn r(n: &Node) -> Ref {
    Ref {
        id: n.id.0.clone(),
        name: display_name(n),
        kind: kind_label(&n.kind),
        app: n.app.as_ref().map(|a| a.value.clone()),
        env: n.env.as_ref().map(|e| env_name(&e.value)),
    }
}

/// An enum as the snapshot spells it ("act_now", "tag"), not as Rust debug-prints it.
fn spelled<T: Serialize>(t: &T) -> String {
    serde_json::to_value(t).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

/// An app filter must name an app: an unknown one would read as "nothing wrong".
fn check_app(s: &Snapshot, app: Option<&str>) -> Result<(), Lookup> {
    let Some(a) = app else { return Ok(()) };
    let names: BTreeSet<&str> = s.nodes.iter().filter_map(|n| n.app.as_ref().map(|x| x.value.as_str())).collect();
    if names.iter().any(|n| n.eq_ignore_ascii_case(a)) {
        Ok(())
    } else {
        Err(Lookup::NoSuchApp(a.into(), names.into_iter().map(String::from).collect()))
    }
}

/// "Invocations: 124,800 (last 24 h)" from a stored metric summary.
fn metric_line(m: &serde_json::Value) -> Option<String> {
    let label = m.get("label")?.as_str()?;
    let unit = m.get("unit").and_then(|u| u.as_str()).unwrap_or("");
    let when = m.get("summary_label").and_then(|u| u.as_str()).unwrap_or("");
    let value = match m.get("summary")? {
        serde_json::Value::Number(n) => {
            let v = n.as_f64()?;
            match unit {
                "%" => format!("{v:.0}%"),
                "ms" | "s" => format!("{v:.0} {unit}"),
                "bytes" => {
                    let (mut x, mut i) = (v, 0);
                    while x >= 1000.0 && i < 4 {
                        x /= 1000.0;
                        i += 1;
                    }
                    format!("{x:.1} {}", ["B", "KB", "MB", "GB", "TB"][i])
                }
                _ if v.fract() == 0.0 || v >= 100.0 => format!("{v:.0}"),
                _ => format!("{v:.2}"),
            }
        }
        serde_json::Value::String(s) => s.clone(),
        _ => return None,
    };
    Some(if when.is_empty() { format!("{label}: {value}") } else { format!("{label}: {value} ({when})") })
}

fn evidence(kind: Option<EvidenceKind>) -> &'static str {
    match kind {
        Some(EvidenceKind::Observed) => "seen in traffic",
        Some(EvidenceKind::Declared) => "configured",
        Some(EvidenceKind::Permitted) => "allowed by IAM only",
        None => "unknown",
    }
}

// ------------------------------------------------------------------ status

#[derive(Debug, Clone, Serialize)]
pub struct StatusAnswer {
    pub taken_at: String,
    pub status: AccountStatus,
    pub apps: usize,
    pub resources: usize,
    pub flows: usize,
}

pub fn status(s: &Snapshot) -> StatusAnswer {
    let apps: BTreeSet<&str> = s.nodes.iter().filter_map(|n| n.app.as_ref().map(|a| a.value.as_str())).collect();
    StatusAnswer {
        taken_at: s.taken_at.to_rfc3339(),
        status: s.status_or_compute(),
        apps: apps.len(),
        resources: s.nodes.len(),
        flows: s.insights.flows.len(),
    }
}

// ------------------------------------------------------------------ apps

#[derive(Debug, Clone, Serialize)]
pub struct AppAnswer {
    pub name: String,
    pub envs: Vec<String>,
    pub resources: usize,
    pub findings: usize,
    pub flows: Vec<String>,
    /// How the app was recognised: tags, a CloudFormation stack, naming, connections.
    pub found_by: Vec<String>,
}

pub fn apps(s: &Snapshot) -> Vec<AppAnswer> {
    let mut by: BTreeMap<String, AppAnswer> = BTreeMap::new();
    for n in &s.nodes {
        let Some(app) = &n.app else { continue };
        let a = by.entry(app.value.clone()).or_insert_with(|| AppAnswer {
            name: app.value.clone(),
            envs: vec![],
            resources: 0,
            findings: 0,
            flows: vec![],
            found_by: vec![],
        });
        a.resources += 1;
        if let Some(e) = &n.env {
            let e = env_name(&e.value);
            if !a.envs.contains(&e) {
                a.envs.push(e);
            }
        }
        let how = spelled(&app.method);
        if !a.found_by.contains(&how) {
            a.found_by.push(how);
        }
        a.findings += s.findings.iter().filter(|f| f.node == n.id).count();
    }
    for f in &s.insights.flows {
        if let Some(a) = f.app.as_ref().and_then(|x| by.get_mut(x)) {
            a.flows.push(f.name.clone());
        }
    }
    by.into_values().collect()
}

// ------------------------------------------------------------------ describe

#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub verb: String,
    pub other: Ref,
    /// The strongest evidence: "seen in traffic", "configured" or "allowed by IAM only".
    pub evidence: String,
    /// Where each piece of evidence comes from ("env:ORDERS_QUEUE_URL", "xray", "iam:role/x").
    pub sources: Vec<String>,
}

/// Error messages from a resource's logs, with when they were read: background refreshes
/// skip logs, so they can be older than the scan.
#[derive(Debug, Clone, Serialize)]
pub struct LogAnswer {
    pub hours: u32,
    pub read_at: Option<String>,
    pub total: f64,
    pub patterns: Vec<String>,
}

fn logs_of(n: &Node) -> Option<LogAnswer> {
    sprawlr_domain::logs::of(n).map(|l| LogAnswer {
        hours: l.hours,
        read_at: l.read_at.map(|t| t.to_rfc3339()),
        total: l.total,
        patterns: l.patterns.iter().map(|p| format!("{} × {}", p.count, p.message)).collect(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceAnswer {
    pub resource: Ref,
    pub region: Option<String>,
    pub health: String,
    pub app_found_by: Option<String>,
    pub settings: serde_json::Value,
    pub metrics: Vec<String>,
    pub errors_in_logs: Option<LogAnswer>,
    pub depends_on: Vec<Connection>,
    pub used_by: Vec<Connection>,
    pub findings: Vec<String>,
    pub flows: Vec<String>,
    pub probably_not_needed: Option<String>,
}

/// Settings worth reading; metrics, alarms, policies and log patterns have their own fields.
/// Attributes never hold secret values or data (ADR 0003): extractors store keys and
/// settings, and resource types without an extractor keep only safe scalars.
fn settings(n: &Node) -> serde_json::Value {
    let mut v = n.attrs.clone();
    if let Some(o) = v.as_object_mut() {
        for k in ["metrics", "alarms", "policies", "log_errors", "broad_permissions", "placeholder", "activity"] {
            o.remove(k);
        }
    }
    v
}

pub fn describe(s: &Snapshot, q: &str) -> Result<ResourceAnswer, Lookup> {
    let g = ArchGraph::from_snapshot(s);
    let n = resolve(&g, q)?;
    let conn = |verb_of: &dyn Fn(EdgeKind) -> String, other: &Node, e: &sprawlr_domain::Edge| Connection {
        verb: verb_of(e.kind),
        other: r(other),
        evidence: evidence(e.strength()).into(),
        sources: e.evidence.iter().map(|v| v.source.clone()).collect(),
    };
    let depends_on = g
        .edges()
        .filter(|(f, _, e)| f.id == n.id && e.kind != EdgeKind::NetworkReachable)
        .map(|(_, t, e)| conn(&|k| verb(k).to_string(), t, e))
        .collect();
    let used_by = g
        .edges()
        .filter(|(_, t, e)| t.id == n.id && e.kind != EdgeKind::NetworkReachable)
        .map(|(f, _, e)| conn(&|k| format!("{} it", verb(k)), f, e))
        .collect();
    let metrics = n.attrs.get("metrics").and_then(|m| m.as_array()).into_iter().flatten().filter_map(metric_line).collect();
    let errors_in_logs = logs_of(n);
    Ok(ResourceAnswer {
        resource: r(n),
        region: n.region.clone(),
        health: spelled(&n.health),
        app_found_by: n.app.as_ref().map(|a| spelled(&a.method)),
        settings: settings(n),
        metrics,
        errors_in_logs,
        depends_on,
        used_by,
        findings: s.findings.iter().filter(|f| f.node == n.id).map(|f| format!("{}: {}", f.title, f.detail)).collect(),
        flows: s.insights.flows.iter().filter(|f| f.nodes.contains(&n.id)).map(|f| f.name.clone()).collect(),
        probably_not_needed: s.insights.lifecycle.iter().find(|l| l.node == n.id).map(|l| l.reason.clone()),
    })
}

// ------------------------------------------------------------------ flows

#[derive(Debug, Clone, Serialize)]
pub struct FlowSummary {
    pub id: String,
    pub name: String,
    pub app: Option<String>,
    pub env: Option<String>,
    pub runs_24h: Option<f64>,
    pub errors_24h: f64,
}

pub fn flows(s: &Snapshot) -> Vec<FlowSummary> {
    s.insights
        .flows
        .iter()
        .map(|f| FlowSummary {
            id: f.id.clone(),
            name: f.name.clone(),
            app: f.app.clone(),
            env: f.env.clone(),
            runs_24h: f.runs_24h,
            errors_24h: f.errors_24h,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub stage: usize,
    pub from: String,
    pub verb: String,
    pub to: String,
    /// "waits for the answer" or "in the background".
    pub mode: String,
    pub allowed_by_iam_only: bool,
    /// How this step is known: "seen in traffic", "configured" or "allowed by IAM only".
    pub evidence: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowAnswer {
    pub id: String,
    pub name: String,
    pub starts: String,
    pub runs_24h: Option<f64>,
    pub errors_24h: f64,
    pub caller_waits_ms: Option<f64>,
    pub slowest: Option<String>,
    pub steps: Vec<Step>,
    pub most_frequent_errors: Vec<String>,
    pub recommendations: Vec<String>,
}

pub fn flow(s: &Snapshot, q: &str) -> Result<FlowAnswer, Lookup> {
    let f = flow_by(s, q)?;
    let g = ArchGraph::from_snapshot(s);
    let name = |id: &NodeId| g.node(id).map(display_name).unwrap_or_else(|| id.0.clone());
    let steps = f
        .hops
        .iter()
        .map(|h| {
            let async_hop = matches!(h.mode, sprawlr_domain::insights::Mode::Async);
            let consumer = async_hop && matches!(h.kind, EdgeKind::Reads | EdgeKind::Subscribes);
            // A consumer hop runs against the edge (the consumer reads the queue).
            let edge = g
                .edges()
                .find(|(f, t, e)| e.kind == h.kind && ((f.id == h.from && t.id == h.to) || (f.id == h.to && t.id == h.from)))
                .map(|(_, _, e)| e.clone());
            Step {
                stage: h.stage,
                from: name(&h.from),
                verb: if consumer { "delivers to".into() } else { verb(h.kind).into() },
                to: name(&h.to),
                mode: if async_hop { "in the background".into() } else { "waits for the answer".into() },
                allowed_by_iam_only: h.inferred,
                evidence: evidence(edge.as_ref().and_then(|e| e.strength())).into(),
                sources: edge.map(|e| e.evidence.into_iter().map(|v| v.source).collect()).unwrap_or_default(),
            }
        })
        .collect();
    Ok(FlowAnswer {
        id: f.id.clone(),
        name: f.name.clone(),
        starts: match f.trigger {
            sprawlr_domain::insights::Trigger::Internet => "from the internet",
            sprawlr_domain::insights::Trigger::Event => "when data arrives (an upload or a change)",
            sprawlr_domain::insights::Trigger::SelfStarted => "on its own (a schedule, or a caller outside the scan)",
        }
        .into(),
        runs_24h: f.runs_24h,
        errors_24h: f.errors_24h,
        caller_waits_ms: f.sync_latency_ms,
        slowest: f.slowest.as_ref().map(&name),
        steps,
        most_frequent_errors: f.top_errors.iter().map(|e| format!("{} × {} in {}", e.count, e.message, name(&e.node))).collect(),
        recommendations: s
            .insights
            .insights
            .iter()
            .filter(|i| i.flow.as_deref() == Some(f.id.as_str()) || i.nodes.iter().any(|n| f.nodes.contains(n)))
            .map(|i| format!("{}: {}{}", i.title, i.detail, i.action.as_ref().map(|a| format!(" {a}")).unwrap_or_default()))
            .collect(),
    })
}

// ------------------------------------------------------------------ blast radius

#[derive(Debug, Clone, Serialize)]
pub struct BlastAnswer {
    pub resource: Ref,
    /// What stops working, through configured or observed connections.
    pub breaks: Vec<Ref>,
    /// What IAM would let depend on it, but nothing shows doing so.
    pub might_break: Vec<Ref>,
    pub flows: Vec<String>,
}

/// Everything that depends on `q`, directly or not.
pub fn blast_radius(s: &Snapshot, q: &str) -> Result<BlastAnswer, Lookup> {
    let g = ArchGraph::from_snapshot(s);
    let n = resolve(&g, q)?;
    let (sure, maybe) = g.dependents(&n.id);
    let refs = |ids: &BTreeSet<NodeId>| {
        ids.iter().filter_map(|id| g.node(id)).filter(|x| x.layer() != sprawlr_domain::Layer::External).map(r).collect()
    };
    Ok(BlastAnswer {
        resource: r(n),
        breaks: refs(&sure),
        might_break: refs(&maybe),
        flows: s.insights.flows.iter().filter(|f| f.nodes.contains(&n.id)).map(|f| f.name.clone()).collect(),
    })
}

// ------------------------------------------------------------------ changes

#[derive(Debug, Clone, Serialize)]
pub struct ChangesAnswer {
    /// False when this scan has nothing to compare with (the first scan, or a file).
    pub known: bool,
    pub headline: String,
    pub changes: Option<sprawlr_domain::changes::Changes>,
}

/// What changed since the previous scan of the account.
pub fn changes(s: &Snapshot) -> ChangesAnswer {
    match &s.changes {
        Some(c) => ChangesAnswer {
            known: true,
            headline: format!("Since {}: {}", c.since.format("%Y-%m-%d %H:%M UTC"), c.headline()),
            changes: Some(c.clone()),
        },
        None => ChangesAnswer {
            known: false,
            headline: "No earlier scan of this account to compare with. Scan again later to see what changes.".into(),
            changes: None,
        },
    }
}

// ------------------------------------------------------------------ failures

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub resource: Ref,
    pub alarm_firing: bool,
    pub errors_in_logs: Option<LogAnswer>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureAnswer {
    /// "Errors start at X" and other act-now items, with their evidence.
    pub origins: Vec<String>,
    pub failing: Vec<Failure>,
    pub flows_with_errors: Vec<FlowSummary>,
}

/// Where errors start and what they say, for the account or one app.
pub fn why_failing(s: &Snapshot, app: Option<&str>) -> Result<FailureAnswer, Lookup> {
    check_app(s, app)?;
    let g = ArchGraph::from_snapshot(s);
    let in_app = |n: &Node| app.is_none_or(|a| n.app.as_ref().is_some_and(|x| x.value.eq_ignore_ascii_case(a)));
    let failing = g
        .nodes()
        .filter(|n| in_app(n))
        .filter_map(|n| {
            let logs = logs_of(n).filter(|l| l.total > 0.0);
            let alarm = n.health == Health::Alarm;
            if logs.is_none() && !alarm {
                return None;
            }
            Some(Failure { resource: r(n), alarm_firing: alarm, errors_in_logs: logs })
        })
        .collect();
    let origins = s
        .insights
        .insights
        .iter()
        .filter(|i| i.kind == "error_origin")
        .filter(|i| i.nodes.iter().filter_map(|id| g.node(id)).any(in_app))
        .map(|i| format!("{}: {}{}", i.title, i.detail, i.action.as_ref().map(|a| format!(" {a}")).unwrap_or_default()))
        .collect();
    let flows_with_errors = flows(s)
        .into_iter()
        .filter(|f| f.errors_24h > 0.0 && app.is_none_or(|a| f.app.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(a))))
        .collect();
    Ok(FailureAnswer { origins, failing, flows_with_errors })
}

// ------------------------------------------------------------------ findings, cleanup, find

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub priority: String,
    pub title: String,
    pub detail: String,
    pub action: Option<String>,
    pub resources: Vec<String>,
}

/// What needs attention, most important first: flow-level insights and per-resource
/// findings on one scale (act now, plan for it, good to know).
pub fn recommendations(s: &Snapshot, app: Option<&str>) -> Result<Vec<Recommendation>, Lookup> {
    check_app(s, app)?;
    let g = ArchGraph::from_snapshot(s);
    let in_app = |id: &NodeId| app.is_none_or(|a| g.node(id).and_then(|n| n.app.as_ref()).is_some_and(|x| x.value.eq_ignore_ascii_case(a)));
    let name = |id: &NodeId| g.node(id).map(display_name).unwrap_or_else(|| id.0.clone());
    let label = |rank: u8| ["act now", "plan for it", "good to know"][rank as usize].to_string();
    let rank_of = |p: Priority| match p {
        Priority::High => 0u8,
        Priority::Medium => 1,
        Priority::Low => 2,
    };
    let mut ranked: Vec<(u8, Recommendation)> = s
        .insights
        .insights
        .iter()
        .filter(|i| app.is_none() || i.nodes.iter().any(&in_app))
        .map(|i| {
            let rank = rank_of(i.priority);
            (
                rank,
                Recommendation {
                    priority: label(rank),
                    title: i.title.clone(),
                    detail: i.detail.clone(),
                    action: i.action.clone(),
                    resources: i.nodes.iter().map(&name).collect(),
                },
            )
        })
        .collect();
    ranked.extend(s.findings.iter().filter(|f| in_app(&f.node)).map(|f| {
        let rank = match spelled(&f.severity).as_str() {
            "high" => 0,
            "medium" => 1,
            _ => 2,
        };
        (
            rank,
            Recommendation {
                priority: label(rank),
                title: format!("{}: {}", name(&f.node), f.title),
                detail: f.detail.clone(),
                action: None,
                resources: vec![name(&f.node)],
            },
        )
    }));
    ranked.sort_by_key(|(rank, _)| *rank); // stable: briefing order within a rank
    Ok(ranked.into_iter().map(|(_, r)| r).collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupItem {
    pub resource: Ref,
    pub state: String,
    pub reason: String,
    pub monthly_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupAnswer {
    pub items: Vec<CleanupItem>,
    pub monthly_cost_usd: f64,
    pub note: &'static str,
}

pub fn cleanup(s: &Snapshot) -> CleanupAnswer {
    let g = ArchGraph::from_snapshot(s);
    let items: Vec<CleanupItem> = s
        .insights
        .lifecycle
        .iter()
        .filter_map(|l| {
            Some(CleanupItem {
                resource: r(g.node(&l.node)?),
                state: spelled(&l.state),
                reason: l.reason.clone(),
                monthly_cost_usd: l.monthly_cost,
            })
        })
        .collect();
    CleanupAnswer {
        monthly_cost_usd: items.iter().filter_map(|i| i.monthly_cost_usd).sum(),
        items,
        note: "List prices for resources with a fixed monthly cost. Check with the owners before removing anything.",
    }
}

/// Resources whose name, id or kind contains the text, best matches first.
pub fn find(s: &Snapshot, text: &str, limit: usize) -> Vec<Ref> {
    let t = text.to_ascii_lowercase();
    let g = ArchGraph::from_snapshot(s);
    let mut hits: Vec<(u8, Ref)> = g
        .nodes()
        .filter_map(|n| {
            let name = display_name(n).to_ascii_lowercase();
            let rank = if name == t {
                0
            } else if name.starts_with(&t) {
                1
            } else if name.contains(&t) {
                2
            } else if n.id.0.to_ascii_lowercase().contains(&t) || kind_label(&n.kind).to_ascii_lowercase().contains(&t) {
                3
            } else {
                return None;
            };
            Some((rank, r(n)))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    hits.into_iter().take(limit).map(|(_, x)| x).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprawlr_domain::{fixtures::demo_graph, infer::infer};

    fn demo() -> Snapshot {
        let mut g = demo_graph();
        infer(&mut g);
        g.snapshot()
    }

    #[test]
    fn resolves_names_ids_and_partials() {
        let s = demo();
        let g = ArchGraph::from_snapshot(&s);
        assert_eq!(display_name(resolve(&g, "InvoiceProcessor").unwrap()), "InvoiceProcessor");
        assert_eq!(display_name(resolve(&g, "invoiceprocessor").unwrap()), "InvoiceProcessor", "case-insensitive");
        let id = resolve(&g, "checkout-orders-db").unwrap().id.0.clone();
        assert_eq!(resolve(&g, &id).unwrap().id.0, id, "by id");
        assert!(matches!(resolve(&g, "no-such-thing"), Err(Lookup::NotFound(_))));
        assert!(matches!(resolve(&g, "checkout"), Err(Lookup::Ambiguous(..))), "many checkout-* resources");
    }

    #[test]
    fn describe_says_how_it_knows() {
        let a = describe(&demo(), "InvoiceProcessor").unwrap();
        assert_eq!(a.resource.app.as_deref(), Some("billing"));
        assert!(
            a.depends_on.iter().any(|c| c.other.name == "api.stripe.com" && c.evidence == "seen in traffic" && !c.sources.is_empty()),
            "{:?}",
            a.depends_on
        );
        assert!(a.errors_in_logs.as_ref().unwrap().patterns.iter().any(|e| e.contains("Timeout calling https://api.stripe.com")));
        assert!(
            a.metrics.iter().any(|m| m.starts_with("Invocations: ") && m.chars().any(|c| c.is_ascii_digit())),
            "metrics carry values: {:?}",
            a.metrics
        );
        assert_eq!(a.health, "alarm");
        assert!(a.settings.get("metrics").is_none(), "metrics are summarised, not dumped");
        assert!(!a.flows.is_empty());
    }

    #[test]
    fn flow_reads_as_steps() {
        let f = flow(&demo(), "upload").unwrap();
        assert_eq!(f.name, "Files uploaded to reports-uploads");
        assert!(
            f.steps
                .iter()
                .any(|s| s.from == "reports-jobs" && s.verb == "delivers to" && s.to == "reports-worker" && s.allowed_by_iam_only),
            "{:?}",
            f.steps
        );
        assert!(matches!(flow(&demo(), "zzz"), Err(Lookup::NotFound(_))));
        assert_eq!(flow(&demo(), "checkout").unwrap().name, "Web requests to checkout", "production first");
    }

    #[test]
    fn blast_radius_separates_sure_from_possible() {
        let b = blast_radius(&demo(), "checkout-orders-db").unwrap();
        assert!(b.breaks.iter().any(|x| x.name == "checkout-api"), "{:?}", b.breaks);
        assert!(b.flows.contains(&"Web requests to checkout".to_string()));
        assert!(b.breaks.iter().all(|x| x.name != "checkout-orders-db"), "not itself");
        assert!(b.might_break.iter().any(|x| x.name == "report-generator"), "IAM-only dependents are separate: {:?}", b.might_break);
    }

    #[test]
    fn failures_name_the_origin_and_the_message() {
        let a = why_failing(&demo(), None).unwrap();
        assert!(a.origins[0].starts_with("Errors start at InvoiceProcessor"));
        assert!(a
            .failing
            .iter()
            .any(|f| f.resource.name == "checkout-api"
                && f.errors_in_logs.as_ref().unwrap().patterns[0].contains("connection pool exhausted")));
        let billing = why_failing(&demo(), Some("billing")).unwrap();
        assert!(billing.failing.iter().all(|f| f.resource.app.as_deref() == Some("billing")));
        let wrong = why_failing(&demo(), Some("payments")).unwrap_err().to_string();
        assert!(wrong.contains("not an app") && wrong.contains("checkout"), "an unknown app is an error, not \"nothing failing\": {wrong}");
    }

    #[test]
    fn changes_say_whether_they_are_known() {
        let plain = demo();
        assert!(!changes(&plain).known);
        let with = crate::demo::snapshot();
        let c = changes(&with);
        assert!(c.known && c.headline.contains("1 new resource"), "{}", c.headline);
    }

    #[test]
    fn cleanup_find_apps_and_recommendations() {
        let s = demo();
        let c = cleanup(&s);
        assert_eq!(c.items.len(), 5);
        assert!((c.monthly_cost_usd - 36.5).abs() < 0.01);
        assert_eq!(find(&s, "invoice", 5)[0].name, "InvoiceProcessor");
        let names: Vec<String> = apps(&s).into_iter().map(|a| a.name).collect();
        assert!(names.contains(&"checkout".to_string()) && names.contains(&"billing".to_string()));
        let recs = recommendations(&s, None).unwrap();
        assert_eq!(recs[0].priority, "act now");
        let ranks: Vec<&str> = recs.iter().map(|r| r.priority.as_str()).collect();
        let order = |p: &str| ["act now", "plan for it", "good to know"].iter().position(|x| *x == p).unwrap();
        assert!(ranks.windows(2).all(|w| order(w[0]) <= order(w[1])), "most important first: {ranks:?}");
        assert!(recommendations(&s, Some("nope")).is_err());
        assert_eq!(status(&s).status.level, sprawlr_domain::status::Level::ActNow);
    }
}
