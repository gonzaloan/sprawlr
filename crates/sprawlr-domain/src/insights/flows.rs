//! Flows: from each entry point, the path requests and events take, split into stages at
//! every asynchronous hop.

use super::signals::{errors, is_buffer, is_store, latency_ms, real, runs};
use super::{Flow, FlowError, Hop, Mode, Trigger};
use crate::analysis::display_name;
use crate::{ArchGraph, EdgeKind, EvidenceKind, Layer, Node, NodeId, NodeKind};
use std::collections::{BTreeSet, HashMap, VecDeque};

fn top_errors(g: &ArchGraph, nodes: &[NodeId], keep: usize) -> Vec<FlowError> {
    let mut all: Vec<FlowError> = nodes
        .iter()
        .filter_map(|id| Some((id, crate::logs::of(g.node(id)?)?)))
        .flat_map(|(id, l)| {
            l.patterns.into_iter().map(move |p| FlowError { node: id.clone(), message: p.message, count: p.count, last_seen: p.last_seen })
        })
        .collect();
    all.sort_by(|a, b| b.count.total_cmp(&a.count).then(a.message.cmp(&b.message)));
    all.truncate(keep);
    all
}

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
            EdgeKind::Publishes => {
                v.push((*t, e.kind, if is_buffer(&t.kind) || is_buffer(&n.kind) { Mode::Async } else { Mode::Sync }, inferred))
            }
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
            let runtime: Vec<_> = out
                .get(&n.id)
                .into_iter()
                .flatten()
                .filter(|(_, e)| e.kind.is_runtime_dependency() && e.kind != EdgeKind::UsesSecret && e.kind != EdgeKind::EncryptedWith)
                .collect();
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
        let called = inc
            .get(&n.id)
            .into_iter()
            .flatten()
            .any(|(src, e)| real(e) && e.kind.is_runtime_dependency() && src.kind != NodeKind::ExternalEndpoint);
        let consumer =
            out.get(&n.id).into_iter().flatten().any(|(t, e)| {
                matches!(e.kind, EdgeKind::Subscribes) || (e.kind == EdgeKind::Reads && (is_buffer(&t.kind) || from_stream(e)))
            });
        let does_work =
            out.get(&n.id).into_iter().flatten().any(|(_, e)| (real(e) || iam_only.contains(&n.id)) && e.kind.is_runtime_dependency());
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
            top_errors: top_errors(g, &order, 5),
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
    let slowest =
        nodes.iter().filter(|n| n.layer() == Layer::Compute).filter_map(|n| Some((*n, latency_ms(n)?))).max_by(|a, b| a.1.total_cmp(&b.1));
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
