//! Recommendations that need flow or capacity context.

use super::capacity::{fmt_x, scale_advice};
use super::signals::{attr_f64, errors, is_buffer, is_prod, is_store, latency_ms};
use super::{Capacity, Flow, Insight, Priority};
use crate::analysis::{display_name, kind_label};
use crate::{ArchGraph, EdgeKind, Layer, NodeId, NodeKind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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
            if (0.5..1.0).contains(&share) && f.sync_depth >= 2 {
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
        let logs = crate::logs::of(n);
        let common = logs.as_ref().and_then(|l| Some((l.patterns.first()?.clone(), l.total)));
        let downstream: BTreeSet<String> =
            flows.iter().flat_map(|f| f.hops.iter()).filter(|h| &h.from == origin).map(|h| name(&h.to)).collect();
        out.push(Insight {
            kind: "error_origin".into(),
            priority: Priority::High,
            title: format!("Errors start at {}", name(origin)),
            detail: format!(
                "{:.0} errors in the last 24 hours{behind}, affecting {}{}.{}",
                errors(n),
                names.iter().map(|x| format!("“{x}”")).collect::<Vec<_>>().join(" and "),
                if downstream.is_empty() {
                    String::new()
                } else {
                    format!("; it calls {}", downstream.into_iter().collect::<Vec<_>>().join(", "))
                },
                match &common {
                    Some((p, total)) if *total > 0.0 =>
                        format!(" Most common in its logs: “{}” ({:.0} of {:.0} error lines).", p.message, p.count, total),
                    _ => String::new(),
                }
            ),
            action: Some(match &common {
                Some((p, total)) if *total > 0.0 && p.count / total >= 0.5 => {
                    format!("Start with that message: it is {:.0}% of the error lines.", 100.0 * p.count / total)
                }
                Some(_) => "Its logs show several different errors; start with the most frequent.".into(),
                None => {
                    "Open its logs for the last hours and look at the most common error message (scan with error logs on to see them here)."
                        .into()
                }
            }),
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
        if n.attrs.get("dead_letter_queue").is_none_or(Value::is_null) {
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
        for (c, _, _) in g.edges().filter(|(_, t, e)| t.id == n.id && e.kind == EdgeKind::Reads) {
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
