//! Lifecycle: idle, orphaned and zombie resources, with what they cost.

use super::{LifeState, Lifecycle};
use crate::analysis::kind_label;
use crate::{ArchGraph, EdgeKind, Layer, Node, NodeId, NodeKind};
use serde_json::Value;
use std::collections::BTreeSet;

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
            .filter(|m| {
                !matches!(m.get("key").and_then(|k| k.as_str()), Some("errors" | "errors_5xx" | "throttles" | "failed" | "write_throttles"))
            })
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
            NodeKind::IamRole => {
                match (n.attrs.get("last_used_days").and_then(|v| v.as_i64()), n.attrs.get("created_days").and_then(|v| v.as_i64())) {
                    (Some(d), _) if d > 90 => Some(format!("This role has not been used in {d} days.")),
                    (None, Some(c)) if c > 90 && n.attrs.get("policies").is_some() => {
                        Some(format!("This role was created {c} days ago and has never been used."))
                    }
                    _ => None,
                }
            }
            k if !matches!(k.clone().layer(), Layer::Scope | Layer::Network | Layer::External | Layer::Identity)
                && !touched.contains(&n.id)
                && !idle =>
            {
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
