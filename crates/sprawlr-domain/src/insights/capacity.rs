//! Capacity: each resource's busiest hour against its limit, and the traffic multiplier at
//! which it saturates.

use super::signals::{attr_f64, peak, series, total};
use super::Capacity;
use crate::{ArchGraph, Node, NodeKind};

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
                    limit: if attr_f64(n, "reserved_concurrency").is_some() {
                        format!("{limit:.0} reserved")
                    } else {
                        format!("{limit:.0} per account and region, shared")
                    },
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

/// Headroom rounded down (the safe reading): two decimals below 2×, one above. Mirrors the app.
pub fn fmt_x(h: f64) -> String {
    if h < 2.0 {
        format!("{:.2}×", (h * 100.0).floor() / 100.0)
    } else {
        format!("{:.1}×", (h * 10.0).floor() / 10.0)
    }
}

pub(super) fn scale_advice(n: Option<&Node>) -> String {
    match n.map(|n| &n.kind) {
        Some(NodeKind::RdsCluster) => {
            "Add a reader for read traffic, move to a larger instance class, or put a cache in front of hot queries.".into()
        }
        Some(NodeKind::RdsInstance) => "Move to a larger instance class or add a read replica for read traffic.".into(),
        Some(NodeKind::EcsService) => "Raise the desired count or add target-tracking auto scaling on CPU and memory.".into(),
        Some(NodeKind::Ec2Instance) => {
            "Put it in an Auto Scaling group with target tracking on CPU, or move to a larger instance type.".into()
        }
        Some(NodeKind::LambdaFunction) => "Request a higher concurrency quota or reserve concurrency for this function.".into(),
        Some(NodeKind::DynamoTable) => "Switch to on-demand capacity or raise provisioned capacity and auto scaling limits.".into(),
        Some(NodeKind::ApiGateway) => "Request a higher API Gateway throttle quota.".into(),
        _ => "Add capacity before traffic grows.".into(),
    }
}
