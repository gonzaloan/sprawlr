//! Insights: what the graph means for the people running it.
//!
//! - **Flows**: paths requests and events take, from an entry point to where they end,
//!   split into stages at every asynchronous hop.
//! - **Capacity**: utilisation of each resource at its busiest hour, and the traffic
//!   multiplier at which it saturates (what-if).
//! - **Risks and recommendations** that need flow context: shared critical dependencies,
//!   long synchronous chains, where errors start, missing dead-letter queues, timeouts
//!   that do not fit together.
//! - **Lifecycle**: idle, orphaned and zombie resources, with what they cost.
//!
//! Everything is deterministic and derived from the graph; each item says what it rests on.

use crate::analysis::{display_name, kind_label};
use crate::{ArchGraph, EdgeKind, EvidenceKind, Layer, Node, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

// ------------------------------------------------------------------ types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Reached from the internet (CloudFront, public API, internet-facing load balancer).
    Internet,
    /// A data store emits events: S3 notifications, DynamoDB streams.
    Event,
    /// Starts on its own: a schedule, a manual run, or a caller sprawlr cannot see.
    SelfStarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Sync,
    Async,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hop {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub mode: Mode,
    /// Known only from IAM permissions (the resource shows nothing else).
    #[serde(default)]
    pub inferred: bool,
    /// 0 for the synchronous part that answers the caller; +1 after every async hop.
    pub stage: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    pub name: String,
    pub app: Option<String>,
    pub env: Option<String>,
    pub trigger: Trigger,
    pub entry: NodeId,
    /// Nodes in the order a request reaches them.
    pub nodes: Vec<NodeId>,
    pub hops: Vec<Hop>,
    pub stages: usize,
    /// Executions in the last 24 h, from the entry point's traffic metric.
    pub runs_24h: Option<f64>,
    /// Errors in the last 24 h across the flow's resources.
    pub errors_24h: f64,
    /// Longest synchronous chain from the entry point (number of hops).
    pub sync_depth: usize,
    /// Sum of p95 latencies along that chain.
    pub sync_latency_ms: Option<f64>,
    /// Node adding the most latency to the synchronous part.
    pub slowest: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capacity {
    pub node: NodeId,
    /// Fraction of the limit used at the busiest hour (1.0 = saturated).
    pub utilization: f64,
    /// Traffic multiplier at which the resource saturates (1 / utilization).
    pub headroom: f64,
    /// What was measured, in words: "CPU peaked at 80%".
    pub basis: String,
    /// What the limit is.
    pub limit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub kind: String,
    pub priority: Priority,
    pub title: String,
    pub detail: String,
    /// What to do about it.
    pub action: Option<String>,
    pub nodes: Vec<NodeId>,
    pub flow: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeState {
    Idle,
    Orphaned,
    Zombie,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lifecycle {
    pub node: NodeId,
    pub state: LifeState,
    pub reason: String,
    /// List-price estimate in USD for resources with a fixed monthly cost.
    pub monthly_cost: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Insights {
    pub flows: Vec<Flow>,
    pub capacity: Vec<Capacity>,
    pub insights: Vec<Insight>,
    pub lifecycle: Vec<Lifecycle>,
}

pub fn analyze(g: &ArchGraph) -> Insights {
    let flows = flows(g);
    let capacity = capacity(g);
    let lifecycle = lifecycle(g);
    let insights = insights(g, &flows, &capacity);
    Insights { flows, capacity, insights, lifecycle }
}

// ------------------------------------------------------------------ metric helpers

fn metric<'a>(n: &'a Node, key: &str) -> Option<&'a Value> {
    n.attrs.get("metrics")?.as_array()?.iter().find(|m| m.get("key").and_then(|k| k.as_str()) == Some(key))
}

fn series(n: &Node, key: &str) -> Vec<Option<f64>> {
    metric(n, key)
        .and_then(|m| m.get("values")?.as_array().cloned())
        .map(|v| v.iter().map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

fn total(n: &Node, key: &str) -> Option<f64> {
    let s = series(n, key);
    (!s.is_empty()).then(|| s.iter().flatten().sum())
}

fn peak(n: &Node, key: &str) -> Option<f64> {
    series(n, key).into_iter().flatten().reduce(f64::max)
}

/// The metric that best counts executions of whatever starts at this node.
fn runs(n: &Node) -> Option<f64> {
    ["requests", "invocations", "started", "sent", "published"].iter().find_map(|k| total(n, k))
}

fn errors(n: &Node) -> f64 {
    ["errors", "errors_5xx", "failed"].iter().filter_map(|k| total(n, k)).fold(0.0, |a, b| a + b)
}

fn latency_ms(n: &Node) -> Option<f64> {
    ["latency_p95", "duration_p95"].iter().find_map(|k| series(n, k).into_iter().flatten().last())
}

fn attr_f64(n: &Node, k: &str) -> Option<f64> {
    n.attrs.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
}

fn is_buffer(k: &NodeKind) -> bool {
    matches!(k, NodeKind::SqsQueue | NodeKind::SnsTopic | NodeKind::EventBus | NodeKind::KinesisStream)
}

fn is_store(k: &NodeKind) -> bool {
    matches!(k.clone().layer(), Layer::Data)
}

fn real(e: &crate::Edge) -> bool {
    e.strength().is_some_and(|s| s >= EvidenceKind::Declared)
}

// ------------------------------------------------------------------ flows

type Adj<'a> = HashMap<&'a NodeId, Vec<(&'a Node, &'a crate::Edge)>>;

fn from_stream(e: &crate::Edge) -> bool {
    e.evidence.iter().any(|v| v.source == "lambda:event_source_mapping")
}

/// Next hops from `n` in the direction requests and events travel, with whether the hop is
/// only known from IAM. `iam_only` holds compute resources for which IAM is all the evidence
/// there is (an EC2 worker whose code we cannot see): their permitted edges are followed.
fn next_hops<'a>(out: &Adj<'a>, inc: &Adj<'a>, iam_only: &BTreeSet<&NodeId>, n: &'a Node) -> Vec<(&'a Node, EdgeKind, Mode, bool)> {
    let mut v = Vec::new();
    if n.kind == NodeKind::ExternalEndpoint {
        return v;
    }
    let usable = |e: &crate::Edge, who: &NodeId| real(e) || (iam_only.contains(who) && e.has(EvidenceKind::Permitted));
    if is_store(&n.kind) {
        // Stores end flows, except for the events they emit.
        for (t, e) in out.get(&n.id).into_iter().flatten() {
            if real(e) && matches!(e.kind, EdgeKind::Invokes | EdgeKind::Publishes) {
                v.push((*t, e.kind, Mode::Async, false));
            }
        }
        for (c, e) in inc.get(&n.id).into_iter().flatten() {
            if e.kind == EdgeKind::Reads && from_stream(e) {
                v.push((*c, EdgeKind::Reads, Mode::Async, false));
            }
        }
        return v;
    }
    for (t, e) in out.get(&n.id).into_iter().flatten() {
        if !usable(e, &n.id) {
            continue;
        }
        let inferred = !real(e);
        if e.evidence.iter().any(|v| v.detail.as_deref() == Some("dead-letter queue")) {
            continue; // the failure path, not the flow
        }
        match e.kind {
            // A consumer reading a queue is fed by it, not calling it; same for a stream consumer.
            EdgeKind::Reads if is_buffer(&t.kind) => {}
            EdgeKind::Reads if from_stream(e) => {}
            EdgeKind::RoutesTo | EdgeKind::Invokes | EdgeKind::Reads | EdgeKind::Writes => v.push((*t, e.kind, Mode::Sync, inferred)),
            EdgeKind::Publishes => v.push((*t, e.kind, if is_buffer(&t.kind) || is_buffer(&n.kind) { Mode::Async } else { Mode::Sync }, inferred)),
            _ => {}
        }
    }
    if is_buffer(&n.kind) {
        // Consumers of a queue, topic or bus.
        for (c, e) in inc.get(&n.id).into_iter().flatten() {
            if usable(e, &c.id) && matches!(e.kind, EdgeKind::Reads | EdgeKind::Subscribes) {
                v.push((*c, e.kind, Mode::Async, !real(e)));
            }
        }
    }
    v
}

pub fn flows(g: &ArchGraph) -> Vec<Flow> {
    let mut out: Adj = HashMap::new();
    let mut inc: Adj = HashMap::new();
    for (f, t, e) in g.edges() {
        out.entry(&f.id).or_default().push((t, e));
        inc.entry(&t.id).or_default().push((f, e));
    }
    // Compute resources whose every runtime connection comes from IAM alone.
    let iam_only: BTreeSet<&NodeId> = g
        .nodes()
        .filter(|n| matches!(n.layer(), Layer::Compute))
        .filter(|n| {
            let runtime: Vec<_> = out.get(&n.id).into_iter().flatten().filter(|(_, e)| e.kind.is_runtime_dependency() && e.kind != EdgeKind::UsesSecret && e.kind != EdgeKind::EncryptedWith).collect();
            !runtime.is_empty() && runtime.iter().all(|(_, e)| !real(e))
        })
        .map(|n| &n.id)
        .collect();

    // Entry points.
    let mut entries: Vec<(&Node, Trigger)> = Vec::new();
    for (f, t, e) in g.edges() {
        if f.kind == NodeKind::Internet && e.kind == EdgeKind::RoutesTo && real(e) {
            entries.push((t, Trigger::Internet));
        }
    }
    for n in g.nodes() {
        let starts_work = matches!(n.layer(), Layer::Compute) || n.kind == NodeKind::StateMachine;
        if !starts_work || entries.iter().any(|(e, _)| e.id == n.id) {
            continue;
        }
        let called = inc.get(&n.id).into_iter().flatten().any(|(src, e)| real(e) && e.kind.is_runtime_dependency() && src.kind != NodeKind::ExternalEndpoint);
        let consumer = out
            .get(&n.id)
            .into_iter()
            .flatten()
            .any(|(t, e)| matches!(e.kind, EdgeKind::Subscribes) || (e.kind == EdgeKind::Reads && (is_buffer(&t.kind) || from_stream(e))));
        let does_work = out.get(&n.id).into_iter().flatten().any(|(_, e)| (real(e) || iam_only.contains(&n.id)) && e.kind.is_runtime_dependency());
        if !called && !consumer && does_work {
            entries.push((n, Trigger::SelfStarted));
        }
    }
    // Stores that emit events and are not written by anything we can see: uploads, imports.
    for n in g.nodes().filter(|n| is_store(&n.kind)) {
        let emits = out.get(&n.id).into_iter().flatten().any(|(_, e)| real(e) && matches!(e.kind, EdgeKind::Invokes | EdgeKind::Publishes))
            || inc.get(&n.id).into_iter().flatten().any(|(_, e)| e.kind == EdgeKind::Reads && from_stream(e));
        let written = inc.get(&n.id).into_iter().flatten().any(|(_, e)| real(e) && e.kind == EdgeKind::Writes);
        if emits && !written {
            entries.push((n, Trigger::Event));
        }
    }

    let mut flows = Vec::new();
    for (entry, trigger) in entries {
        // BFS; stage grows at async hops.
        let mut order: Vec<NodeId> = vec![entry.id.clone()];
        let mut stage_of: HashMap<NodeId, usize> = HashMap::from([(entry.id.clone(), 0)]);
        let mut hops = Vec::new();
        let mut seen_hops: BTreeSet<(NodeId, NodeId)> = BTreeSet::new();
        let mut q = VecDeque::from([entry]);
        while let Some(n) = q.pop_front() {
            let stage = stage_of[&n.id];
            let mut next = next_hops(&out, &inc, &iam_only, n);
            // Reads before writes: the usual order of work inside one step.
            next.sort_by_key(|(_, k, m, _)| (*m == Mode::Async, !matches!(k, EdgeKind::Reads)));
            for (t, kind, mode, inferred) in next {
                // Back to the entry only to read the file that started it all.
                let back = t.id == entry.id && !(trigger == Trigger::Event && kind == EdgeKind::Reads);
                if back || !seen_hops.insert((n.id.clone(), t.id.clone())) {
                    continue;
                }
                let s = stage + usize::from(mode == Mode::Async);
                hops.push(Hop { from: n.id.clone(), to: t.id.clone(), kind, mode, stage: s, inferred });
                if !stage_of.contains_key(&t.id) && order.len() < 60 {
                    stage_of.insert(t.id.clone(), s);
                    order.push(t.id.clone());
                    q.push_back(t);
                }
            }
        }
        if hops.is_empty() {
            continue;
        }

        // Longest synchronous chain from the entry, and its latency.
        let (sync_depth, sync_latency, slowest) = sync_chain(g, entry, &hops);
        let app = entry.app.as_ref().map(|a| a.value.clone());
        let env = entry.env.as_ref().map(|e| crate::analysis::env_name(&e.value));
        let mut name = flow_name(entry, trigger, app.as_deref());
        if let Some(e) = env.as_deref().filter(|e| *e != "prod") {
            name.push_str(&format!(" ({e})"));
        }
        let errors_24h = order.iter().filter_map(|id| g.node(id)).map(errors).fold(0.0, |a, b| a + b);
        // Executions: the first resource of the synchronous part that counts them
        // (CloudFront has no metric here; the load balancer behind it does).
        // Measured at the start; an upload is counted by the queue or function it feeds.
        let runs_at = |st: usize| order.iter().filter(|id| stage_of.get(*id) == Some(&st)).filter_map(|id| g.node(id)).find_map(runs);
        let runs_24h = runs_at(0).or_else(|| if trigger == Trigger::Event { runs_at(1) } else { None });
        flows.push(Flow {
            id: format!("flow:{}", entry.id.0),
            name,
            app,
            env,
            trigger,
            entry: entry.id.clone(),
            stages: hops.iter().map(|h| h.stage).max().unwrap_or(0) + 1,
            runs_24h,
            errors_24h,
            sync_depth,
            sync_latency_ms: sync_latency,
            slowest,
            nodes: order,
            hops,
        });
    }
    flows.sort_by(|a, b| b.runs_24h.unwrap_or(0.0).total_cmp(&a.runs_24h.unwrap_or(0.0)).then(a.name.cmp(&b.name)));
    flows
}

/// Depth of the synchronous part, end-to-end latency the caller sees, and the compute
/// resource that spends most of it. Load balancers and APIs measure the whole chain behind
/// them, so they give the end-to-end number but are never "the slow hop" themselves.
fn sync_chain(g: &ArchGraph, entry: &Node, hops: &[Hop]) -> (usize, Option<f64>, Option<NodeId>) {
    let sync: Vec<&Hop> = hops.iter().filter(|h| h.mode == Mode::Sync && h.stage == 0).collect();
    let mut depth: HashMap<&NodeId, usize> = HashMap::from([(&entry.id, 0)]);
    let mut order = vec![&entry.id];
    let mut i = 0;
    while i < order.len() {
        let n = order[i];
        for h in sync.iter().filter(|h| &h.from == n) {
            if !depth.contains_key(&h.to) {
                depth.insert(&h.to, depth[n] + 1);
                order.push(&h.to);
            }
        }
        i += 1;
    }
    let max_depth = depth.values().copied().max().unwrap_or(0);
    let nodes: Vec<&Node> = order.iter().filter_map(|id| g.node(id)).collect();
    let end_to_end = nodes.iter().find(|n| n.layer() == Layer::Edge).and_then(|n| latency_ms(n));
    let slowest = nodes
        .iter()
        .filter(|n| n.layer() == Layer::Compute)
        .filter_map(|n| Some((*n, latency_ms(n)?)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    let latency = end_to_end.or(slowest.map(|(_, l)| l));
    (max_depth, latency, slowest.map(|(n, _)| n.id.clone()))
}

fn flow_name(entry: &Node, trigger: Trigger, app: Option<&str>) -> String {
    let who = app.map(String::from).unwrap_or_else(|| display_name(entry));
    match (trigger, &entry.kind) {
        (Trigger::Internet, NodeKind::ApiGateway) => format!("API calls to {who}"),
        (Trigger::Internet, _) => format!("Web requests to {who}"),
        // Long-running services with no visible caller: something outside the scan calls them.
        (Trigger::SelfStarted, NodeKind::EcsService | NodeKind::Ec2Instance) => format!("Work done by {}", display_name(entry)),
        (Trigger::Event, NodeKind::S3Bucket) => format!("Files uploaded to {}", display_name(entry)),
        (Trigger::Event, _) => format!("Changes to {}", display_name(entry)),
        (Trigger::SelfStarted, _) => format!("{} runs", display_name(entry)),
    }
}

fn is_prod(n: &Node) -> bool {
    n.env.as_ref().map_or(true, |e| crate::analysis::env_name(&e.value) == "prod")
}

// ------------------------------------------------------------------ capacity

/// Default regional account limits used when the resource has no limit of its own.
const LAMBDA_ACCOUNT_CONCURRENCY: f64 = 1000.0;
const APIGW_ACCOUNT_RPS: f64 = 10_000.0;

pub fn capacity(g: &ArchGraph) -> Vec<Capacity> {
    let mut out = Vec::new();
    for n in g.nodes() {
        let pct = |label: &str, v: f64| Capacity {
            node: n.id.clone(),
            utilization: v / 100.0,
            headroom: if v > 0.0 { 100.0 / v } else { f64::INFINITY },
            basis: format!("{label} peaked at {v:.0}%"),
            limit: "100%".into(),
        };
        let c = match n.kind {
            NodeKind::RdsCluster | NodeKind::RdsInstance => peak(n, "cpu").map(|v| pct("CPU", v)),
            NodeKind::Ec2Instance => peak(n, "cpu").map(|v| pct("CPU", v)),
            NodeKind::EcsService => match (peak(n, "cpu"), peak(n, "memory")) {
                (Some(c), Some(m)) if m > c => Some(pct("Memory", m)),
                (Some(c), _) => Some(pct("CPU", c)),
                (None, Some(m)) => Some(pct("Memory", m)),
                _ => None,
            },
            NodeKind::LambdaFunction => {
                // Concurrency ≈ invocations per second × duration. p95 duration overestimates: conservative.
                let inv = series(n, "invocations");
                let dur = series(n, "duration_p95");
                let conc = inv.iter().zip(dur.iter()).filter_map(|(i, d)| Some(i.as_ref()? * d.as_ref()? / 3_600_000.0)).reduce(f64::max);
                let limit = attr_f64(n, "reserved_concurrency").unwrap_or(LAMBDA_ACCOUNT_CONCURRENCY);
                conc.filter(|c| *c > 0.0).map(|c| Capacity {
                    node: n.id.clone(),
                    utilization: c / limit,
                    headroom: limit / c,
                    basis: format!("about {c:.1} concurrent executions at the busiest hour"),
                    limit: if attr_f64(n, "reserved_concurrency").is_some() { format!("{limit:.0} reserved") } else { format!("{limit:.0} per account and region, shared") },
                })
            }
            NodeKind::ApiGateway => peak(n, "requests").filter(|r| *r > 0.0).map(|r| {
                let rps = r / 3600.0;
                Capacity {
                    node: n.id.clone(),
                    utilization: rps / APIGW_ACCOUNT_RPS,
                    headroom: APIGW_ACCOUNT_RPS / rps,
                    basis: format!("{rps:.1} requests per second at the busiest hour"),
                    limit: "10,000 rps account throttle, shared".into(),
                }
            }),
            NodeKind::DynamoTable if total(n, "write_throttles").unwrap_or(0.0) > 0.0 => Some(Capacity {
                node: n.id.clone(),
                utilization: 1.0,
                headroom: 1.0,
                basis: "writes are being throttled".into(),
                limit: "table capacity".into(),
            }),
            _ => None,
        };
        out.extend(c);
    }
    out.sort_by(|a, b| a.headroom.total_cmp(&b.headroom));
    out
}

// ------------------------------------------------------------------ insights

pub fn insights(g: &ArchGraph, flows: &[Flow], capacity: &[Capacity]) -> Vec<Insight> {
    let mut out = Vec::new();
    let name = |id: &NodeId| g.node(id).map(display_name).unwrap_or_else(|| id.0.clone());

    // Bottleneck: what saturates first if traffic grows.
    if let Some(c) = capacity.iter().find(|c| c.headroom.is_finite() && g.node(&c.node).is_some_and(is_prod)) {
        let priority = match c.headroom {
            h if h < 1.5 => Priority::High,
            h if h < 3.0 => Priority::Medium,
            _ => Priority::Low,
        };
        out.push(Insight {
            kind: "bottleneck".into(),
            priority,
            title: format!("{} saturates first", name(&c.node)),
            detail: format!(
                "{}; it reaches its limit at {} today's traffic. Everything that depends on it slows down or fails with it.",
                c.basis,
                fmt_x(c.headroom)
            ),
            action: Some(scale_advice(g.node(&c.node))),
            nodes: vec![c.node.clone()],
            flow: None,
        });
    }

    // Shared dependency: one resource many flows rely on.
    let mut used_by: BTreeMap<&NodeId, Vec<&Flow>> = BTreeMap::new();
    for f in flows {
        for id in &f.nodes {
            if id != &f.entry {
                used_by.entry(id).or_default().push(f);
            }
        }
    }
    for (id, fs) in &used_by {
        let Some(n) = g.node(id) else { continue };
        if fs.len() >= 2 && (is_store(&n.kind) || is_buffer(&n.kind)) {
            let single_az = n.attrs.get("multi_az") == Some(&Value::Bool(false));
            out.push(Insight {
                kind: "shared_dependency".into(),
                priority: match (is_prod(n), single_az) {
                    (true, true) => Priority::High,
                    (true, false) => Priority::Medium,
                    _ => Priority::Low,
                },
                title: format!("{} is on {} flows", name(id), fs.len()),
                detail: format!(
                    "{} sits on the path of {}. A slowdown or outage there is felt by all of them{}.",
                    kind_label(&n.kind),
                    fs.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", "),
                    if single_az { ", and it runs in a single availability zone" } else { "" }
                ),
                action: Some("Give it alarms on latency and errors, and check the flows degrade gracefully when it is slow.".into()),
                nodes: vec![(*id).clone()],
                flow: None,
            });
        }
    }

    let mut error_origins: BTreeMap<NodeId, Vec<String>> = BTreeMap::new();
    for f in flows {
        // Where errors start: compute and data resources reporting errors. Load balancer and
        // API 5xx are symptoms of what sits behind them, so they only count if nothing else errs.
        let erring: Vec<&NodeId> = f.nodes.iter().filter(|id| g.node(id).is_some_and(|n| errors(n) > 0.0)).collect();
        let origin = erring
            .iter()
            .filter(|id| g.node(id).is_some_and(|n| n.layer() != Layer::Edge))
            .max_by(|a, b| errors(g.node(a).unwrap()).total_cmp(&errors(g.node(b).unwrap())))
            .or(erring.first());
        if let Some(origin) = origin {
            error_origins.entry((*origin).clone()).or_default().push(f.name.clone());
        }
        // Long synchronous chain behind a timeout.
        if f.sync_depth >= 4 {
            out.push(Insight {
                kind: "long_sync_chain".into(),
                priority: Priority::Medium,
                title: format!("“{}” waits on {} synchronous hops", f.name, f.sync_depth),
                detail: "Every hop adds latency and a way to fail before the caller gets an answer.".into(),
                action: Some("Move work that the caller does not need to wait for behind a queue.".into()),
                nodes: vec![f.entry.clone()],
                flow: Some(f.id.clone()),
            });
        }
        if let (Some(lat), Some(slow)) = (f.sync_latency_ms, &f.slowest) {
            let share = g.node(slow).and_then(latency_ms).unwrap_or(0.0) / lat;
            if share >= 0.5 && share < 1.0 && f.sync_depth >= 2 {
                out.push(Insight {
                    kind: "slow_hop".into(),
                    priority: Priority::Low,
                    title: format!("{} is most of the wait in “{}”", name(slow), f.name),
                    detail: format!("{:.0}% of the {:.0} ms p95 the caller waits.", share * 100.0, lat),
                    action: None,
                    nodes: vec![slow.clone()],
                    flow: Some(f.id.clone()),
                });
            }
        }
    }

    for (origin, names) in &error_origins {
        let n = g.node(origin).unwrap();
        let behind = if n.layer() == Layer::Edge { " (returned by what it routes to)" } else { "" };
        let downstream: BTreeSet<String> = flows.iter().flat_map(|f| f.hops.iter()).filter(|h| &h.from == origin).map(|h| name(&h.to)).collect();
        out.push(Insight {
            kind: "error_origin".into(),
            priority: Priority::High,
            title: format!("Errors start at {}", name(origin)),
            detail: format!(
                "{:.0} errors in the last 24 hours{behind}, affecting {}{}.",
                errors(n),
                names.iter().map(|x| format!("“{x}”")).collect::<Vec<_>>().join(" and "),
                if downstream.is_empty() { String::new() } else { format!("; it calls {}", downstream.into_iter().collect::<Vec<_>>().join(", ")) }
            ),
            action: Some("Open its logs for the last hours and look at the most common error message.".into()),
            nodes: vec![origin.clone()],
            flow: flows.iter().find(|f| f.name == names[0]).map(|f| f.id.clone()),
        });
    }

    // Queue rules (only for queues that carry a flow).
    let in_flows: BTreeSet<&NodeId> = flows.iter().flat_map(|f| f.nodes.iter()).collect();
    for n in g.nodes().filter(|n| n.kind == NodeKind::SqsQueue && in_flows.contains(&n.id)) {
        let is_dlq = g.edges().any(|(_, t, e)| t.id == n.id && e.evidence.iter().any(|v| v.detail.as_deref() == Some("dead-letter queue")));
        if is_dlq {
            continue;
        }
        if n.attrs.get("dead_letter_queue").map_or(true, Value::is_null) {
            out.push(Insight {
                kind: "missing_dlq".into(),
                priority: Priority::Medium,
                title: format!("{} has no dead-letter queue", name(&n.id)),
                detail: "Messages that keep failing are retried until they expire and then disappear without a trace.".into(),
                action: Some("Add a redrive policy to a dead-letter queue and alarm on its depth.".into()),
                nodes: vec![n.id.clone()],
                flow: None,
            });
        }
        // AWS guidance: visibility timeout ≥ 6 × the consumer function's timeout.
        let vis = attr_f64(n, "visibility_timeout_s");
        for (c, _, e) in g.edges().filter(|(_, t, e)| t.id == n.id && e.kind == EdgeKind::Reads).map(|(c, t, e)| (c, t, e)) {
            let _ = e;
            if let (Some(v), Some(t)) = (vis, attr_f64(c, "timeout_s")) {
                if c.kind == NodeKind::LambdaFunction && v < 6.0 * t {
                    out.push(Insight {
                        kind: "visibility_timeout".into(),
                        priority: Priority::Medium,
                        title: format!("{} can be processed twice", name(&n.id)),
                        detail: format!(
                            "Its visibility timeout is {v:.0} s but {} may run for {t:.0} s. AWS recommends at least six times the function timeout ({:.0} s).",
                            name(&c.id),
                            6.0 * t
                        ),
                        action: Some(format!("Raise the visibility timeout to {:.0} s or more.", 6.0 * t)),
                        nodes: vec![n.id.clone(), c.id.clone()],
                        flow: None,
                    });
                }
            }
        }
    }

    out.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.title.cmp(&b.title)));
    out
}

/// Headroom rounded down (the safe reading): two decimals below 2×, one above. Mirrors the app.
pub fn fmt_x(h: f64) -> String {
    if h < 2.0 {
        format!("{:.2}×", (h * 100.0).floor() / 100.0)
    } else {
        format!("{:.1}×", (h * 10.0).floor() / 10.0)
    }
}

fn scale_advice(n: Option<&Node>) -> String {
    match n.map(|n| &n.kind) {
        Some(NodeKind::RdsCluster) => "Add a reader for read traffic, move to a larger instance class, or put a cache in front of hot queries.".into(),
        Some(NodeKind::RdsInstance) => "Move to a larger instance class or add a read replica for read traffic.".into(),
        Some(NodeKind::EcsService) => "Raise the desired count or add target-tracking auto scaling on CPU and memory.".into(),
        Some(NodeKind::Ec2Instance) => "Put it in an Auto Scaling group with target tracking on CPU, or move to a larger instance type.".into(),
        Some(NodeKind::LambdaFunction) => "Request a higher concurrency quota or reserve concurrency for this function.".into(),
        Some(NodeKind::DynamoTable) => "Switch to on-demand capacity or raise provisioned capacity and auto scaling limits.".into(),
        Some(NodeKind::ApiGateway) => "Request a higher API Gateway throttle quota.".into(),
        _ => "Add capacity before traffic grows.".into(),
    }
}

// ------------------------------------------------------------------ lifecycle

/// List price per month in us-east-1 for resources that cost money just by existing.
fn fixed_monthly_cost(n: &Node) -> Option<f64> {
    match &n.kind {
        NodeKind::NatGateway => Some(32.85),
        NodeKind::LoadBalancer => Some(16.43),
        NodeKind::Secret => Some(0.40),
        NodeKind::KmsKey => Some(1.00),
        NodeKind::Other(t) if t == "AWS::EC2::EIP" => Some(3.65),
        _ => None,
    }
}

pub fn lifecycle(g: &ArchGraph) -> Vec<Lifecycle> {
    let mut touched: BTreeSet<&NodeId> = BTreeSet::new();
    for (f, t, e) in g.edges() {
        if e.kind != EdgeKind::NetworkReachable {
            touched.insert(&f.id);
            touched.insert(&t.id);
        }
    }
    let referenced_sgs: BTreeSet<String> = g
        .nodes()
        .flat_map(|n| n.attrs.get("security_groups").and_then(|v| v.as_array()).cloned().unwrap_or_default())
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let mut out = Vec::new();
    for n in g.nodes() {
        let cost = fixed_monthly_cost(n);
        let metrics = n.attrs.get("metrics").and_then(|m| m.as_array());
        // Idle: telemetry exists and every activity counter is zero.
        let activity: Vec<f64> = metrics
            .into_iter()
            .flatten()
            .filter(|m| m.get("stat").and_then(|s| s.as_str()) == Some("Sum"))
            .filter(|m| !matches!(m.get("key").and_then(|k| k.as_str()), Some("errors" | "errors_5xx" | "throttles" | "failed" | "write_throttles")))
            .map(|m| m.get("values").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_f64()).sum()).unwrap_or(0.0))
            .collect();
        // A longer window, when the scanner read one, decides: a weekly job is quiet most days.
        let long = n.attrs.get("activity").and_then(|a| Some((a.get("days")?.as_i64()?, a.get("total")?.as_f64()?)));
        let idle = match long {
            Some((_, total)) => total == 0.0,
            None => !activity.is_empty() && activity.iter().all(|v| *v == 0.0),
        };
        let window = match long {
            Some((d, _)) => format!("{d} days"),
            None => "24 hours".to_string(),
        };

        let orphan_reason = match &n.kind {
            NodeKind::SecurityGroup if n.name != "default" && !referenced_sgs.contains(&n.id.0) && !touched.contains(&n.id) => {
                Some("No resource uses this security group.".to_string())
            }
            NodeKind::Other(t) if t == "AWS::EC2::EIP" && n.attrs.get("associated") == Some(&Value::Bool(false)) => {
                Some("This Elastic IP is not attached to anything.".to_string())
            }
            NodeKind::NatGateway => None, // judged by traffic below
            NodeKind::IamRole => match (n.attrs.get("last_used_days").and_then(|v| v.as_i64()), n.attrs.get("created_days").and_then(|v| v.as_i64())) {
                (Some(d), _) if d > 90 => Some(format!("This role has not been used in {d} days.")),
                (None, Some(c)) if c > 90 && n.attrs.get("policies").is_some() => Some(format!("This role was created {c} days ago and has never been used.")),
                _ => None,
            },
            k if !matches!(k.clone().layer(), Layer::Scope | Layer::Network | Layer::External | Layer::Identity) && !touched.contains(&n.id) && !idle => {
                Some(format!("Nothing connects to this {} and it connects to nothing.", kind_label(k).to_lowercase()))
            }
            _ => None,
        };

        let (state, reason) = if idle {
            let state = if cost.is_some() { LifeState::Zombie } else { LifeState::Idle };
            let hint = if long.is_some() { "" } else { " Check a longer window before removing it." };
            (state, format!("No activity in the last {window}.{hint}"))
        } else if let Some(r) = orphan_reason {
            (if cost.is_some() { LifeState::Zombie } else { LifeState::Orphaned }, r)
        } else {
            continue;
        };
        out.push(Lifecycle { node: n.id.clone(), state, reason, monthly_cost: cost });
    }
    out.sort_by(|a, b| b.monthly_cost.unwrap_or(0.0).total_cmp(&a.monthly_cost.unwrap_or(0.0)).then(a.node.cmp(&b.node)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_graph;
    use crate::infer::infer;

    fn demo() -> (ArchGraph, Insights) {
        let mut g = demo_graph();
        infer(&mut g);
        let i = analyze(&g);
        (g, i)
    }

    fn named(g: &ArchGraph, id: &NodeId) -> String {
        g.node(id).map(display_name).unwrap()
    }

    #[test]
    fn checkout_flow_crosses_async_stages_to_stripe() {
        let (g, i) = demo();
        let f = i.flows.iter().find(|f| f.name == "Web requests to checkout").expect("checkout flow");
        let names: Vec<String> = f.nodes.iter().map(|n| named(&g, n)).collect();
        assert_eq!(names[0], "checkout-cdn");
        for expected in ["checkout-api", "checkout-orders-db", "checkout-order-events", "InvoiceProcessor", "api.stripe.com", "search-indexer"] {
            assert!(names.contains(&expected.to_string()), "{expected} missing from {names:?}");
        }
        assert!(f.stages >= 3, "api → queue → consumer → bus → indexer: {}", f.stages);
        assert!(f.hops.iter().any(|h| h.mode == Mode::Async));
        assert!(!names.contains(&"checkout/prod/db-credentials".to_string()), "secrets are not steps of a flow");
    }

    #[test]
    fn scheduled_job_is_its_own_flow_and_consumers_are_not_entries() {
        let (g, i) = demo();
        let entries: Vec<String> = i.flows.iter().map(|f| named(&g, &f.entry)).collect();
        assert!(entries.contains(&"MonthlyClose".to_string()), "{entries:?}");
        assert!(!entries.contains(&"InvoiceProcessor".to_string()), "a queue consumer is not an entry point");
        assert!(!entries.contains(&"search-indexer".to_string()), "a bus subscriber is not an entry point");
    }

    #[test]
    fn orders_db_is_the_first_bottleneck() {
        let (g, i) = demo();
        let first = &i.capacity[0];
        assert_eq!(named(&g, &first.node), "checkout-orders-db");
        assert!(first.headroom < 1.5, "{}", first.headroom);
        assert!(i.insights.iter().any(|x| x.kind == "bottleneck" && x.priority == Priority::High));
    }

    #[test]
    fn flow_rules_fire_on_demo() {
        let (_, i) = demo();
        let kinds: BTreeSet<&str> = i.insights.iter().map(|x| x.kind.as_str()).collect();
        for k in ["bottleneck", "shared_dependency", "error_origin", "missing_dlq", "visibility_timeout"] {
            assert!(kinds.contains(k), "missing {k}: {kinds:?}");
        }
    }

    #[test]
    fn lifecycle_finds_idle_orphaned_and_zombie() {
        let (g, i) = demo();
        let states: BTreeMap<String, LifeState> = i.lifecycle.iter().map(|l| (named(&g, &l.node), l.state)).collect();
        assert_eq!(states.get("nat-legacy-dev"), Some(&LifeState::Zombie), "{states:?}");
        assert_eq!(states.get("legacy-export-job"), Some(&LifeState::Idle), "{states:?}");
        assert_eq!(states.get("migration-2024-sg"), Some(&LifeState::Orphaned), "{states:?}");
        assert!(!states.contains_key("checkout-api"), "active resources are not listed");
        assert_eq!(states.get("ci-deploy-2023"), Some(&LifeState::Orphaned), "unused role: {states:?}");
        assert!(!states.contains_key("reports-worker-role"), "a role in use is not listed");
        let savings: f64 = i.lifecycle.iter().filter_map(|l| l.monthly_cost).sum();
        assert!(savings > 30.0);
    }

    #[test]
    fn upload_flow_reaches_an_ec2_worker_known_only_through_iam() {
        let (g, i) = demo();
        let f = i.flows.iter().find(|f| f.name == "Files uploaded to reports-uploads").expect("upload flow");
        assert_eq!(f.trigger, Trigger::Event);
        let hops: Vec<(String, String, bool)> = f.hops.iter().map(|h| (named(&g, &h.from), named(&g, &h.to), h.inferred)).collect();
        assert!(hops.contains(&("reports-jobs".into(), "reports-worker".into(), true)), "{hops:?}");
        assert!(hops.contains(&("reports-worker".into(), "reports-output".into(), true)), "{hops:?}");
        assert!(hops.contains(&("reports-worker".into(), "reports-uploads".into(), true)), "the worker reads the upload: {hops:?}");
        assert!(!hops.iter().any(|(_, t, _)| t == "reports-jobs-dlq"), "the DLQ is the failure path: {hops:?}");
        assert!(f.runs_24h.is_some(), "counted by the queue it feeds");
        // IAM edges are not followed for resources whose configuration we can read.
        let checkout = i.flows.iter().find(|f| f.name == "Web requests to checkout").unwrap();
        assert!(!checkout.hops.iter().any(|h| h.inferred), "checkout is mapped from configuration");
    }
}
