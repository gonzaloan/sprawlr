//! Live state on top of the structure: CloudWatch alarms (health), 24 hours of key metrics,
//! and X-Ray traces (traffic actually observed between components, including third parties).

use crate::link::infer_kind;
use crate::{evidence, external_node, Progress, ScanContext};
use sprawlr_core::{ArchGraph, EvidenceKind, Health, Node, NodeId, NodeKind};
use aws_sdk_cloudwatch::types::{Dimension, Metric, MetricDataQuery, MetricStat, StateValue};
use serde_json::{json, Value};
use std::collections::HashMap;

const HOURS: i64 = 24;

/// Apply alarms, metrics and traces for one region. Returns warnings (missing permissions, etc.).
pub async fn apply(g: &mut ArchGraph, ctx: &ScanContext, progress: &Progress) -> Vec<String> {
    let mut warnings = Vec::new();
    progress(format!("{}: reading alarms", ctx.region));
    if let Err(e) = alarms(g, ctx).await {
        warnings.push(format!("{}/alarms: {e:#}", ctx.region));
    }
    progress(format!("{}: reading metrics", ctx.region));
    if let Err(e) = metrics(g, ctx).await {
        warnings.push(format!("{}/metrics: {e:#}", ctx.region));
    }
    progress(format!("{}: reading X-Ray traces", ctx.region));
    if let Err(e) = traces(g, ctx).await {
        warnings.push(format!("{}/xray: {e:#}", ctx.region));
    }
    warnings
}

/// alias → node, restricted to one region (names repeat across regions).
fn index(g: &ArchGraph, region: &str) -> HashMap<String, NodeId> {
    let mut m = HashMap::new();
    for n in g.nodes().filter(|n| n.region.as_deref() == Some(region) || n.region.is_none()) {
        m.insert(n.id.0.clone(), n.id.clone());
        for a in &n.aliases {
            m.entry(a.clone()).or_insert_with(|| n.id.clone());
        }
    }
    m
}

// ------------------------------------------------------------------ alarms

/// Which resource a set of alarm dimensions points at.
pub fn node_for_dimensions(ix: &HashMap<String, NodeId>, dims: &[(String, String)]) -> Option<NodeId> {
    let get = |k: &str| dims.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if let (Some(c), Some(s)) = (get("ClusterName"), get("ServiceName")) {
        return ix.get(&format!("ecs:{c}/{s}")).cloned();
    }
    for key in ["NatGatewayId", "FunctionName", "LoadBalancer", "DBClusterIdentifier", "DBInstanceIdentifier", "QueueName", "TableName", "ApiId", "ApiName", "StateMachineArn", "TopicName", "DistributionId"] {
        if let Some(v) = get(key) {
            return ix.get(v).cloned();
        }
    }
    None
}

async fn alarms(g: &mut ArchGraph, ctx: &ScanContext) -> anyhow::Result<()> {
    let cw = aws_sdk_cloudwatch::Client::new(&ctx.sdk);
    let ix = index(g, &ctx.region);
    let mut per_node: HashMap<NodeId, Vec<Value>> = HashMap::new();
    let mut pages = cw.describe_alarms().into_paginator().send();
    while let Some(page) = pages.next().await {
        for a in page?.metric_alarms() {
            let dims: Vec<(String, String)> =
                a.dimensions().iter().filter_map(|d| Some((d.name()?.to_string(), d.value()?.to_string()))).collect();
            let Some(id) = node_for_dimensions(&ix, &dims) else { continue };
            per_node.entry(id).or_default().push(json!({
                "name": a.alarm_name(),
                "state": a.state_value().map(|s| s.as_str()),
                "metric": a.metric_name(),
                "reason": a.state_reason(),
            }));
        }
    }
    for (id, list) in per_node {
        let Some(n) = g.node_mut(&id) else { continue };
        let state = |s: &str| list.iter().any(|a| a["state"] == s);
        n.health = if state(StateValue::Alarm.as_str()) {
            Health::Alarm
        } else if state(StateValue::Ok.as_str()) {
            Health::Ok
        } else {
            n.health
        };
        set_attr(n, "alarms", Value::Array(list));
    }
    Ok(())
}

fn set_attr(n: &mut Node, key: &str, v: Value) {
    if !n.attrs.is_object() {
        n.attrs = json!({});
    }
    n.attrs[key] = v;
}

// ------------------------------------------------------------------ metrics

/// One series to fetch for a resource.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub key: &'static str,
    pub label: &'static str,
    pub namespace: &'static str,
    pub metric: &'static str,
    pub stat: &'static str,
    pub unit: &'static str,
    /// Multiply raw values (e.g. seconds → ms).
    pub scale: f64,
    /// When the 24 h total (or max) means something is wrong.
    pub alert: Option<Alert>,
    pub dims: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Alert {
    /// Any occurrence (errors, 5xx, throttles, failed executions).
    AnyAbove(f64),
    /// Peak above a threshold (queue age in seconds).
    MaxAbove(f64),
}

fn spec(key: &'static str, label: &'static str, ns: &'static str, metric: &'static str, stat: &'static str, unit: &'static str) -> Spec {
    Spec { key, label, namespace: ns, metric, stat, unit, scale: 1.0, alert: None, dims: vec![] }
}

/// The few metrics that matter per kind of resource.
pub fn specs_for(n: &Node) -> Vec<Spec> {
    let attr = |k: &str| n.attrs.get(k).and_then(|v| v.as_str()).map(String::from);
    let (mut v, dims): (Vec<Spec>, Vec<(&'static str, String)>) = match &n.kind {
        NodeKind::LambdaFunction => (
            vec![
                spec("invocations", "Invocations", "AWS/Lambda", "Invocations", "Sum", "count"),
                Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("errors", "Errors", "AWS/Lambda", "Errors", "Sum", "count") },
                Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("throttles", "Throttles", "AWS/Lambda", "Throttles", "Sum", "count") },
                spec("duration_p95", "Duration p95", "AWS/Lambda", "Duration", "p95", "ms"),
            ],
            vec![("FunctionName", n.name.clone())],
        ),
        NodeKind::LoadBalancer if attr("type").as_deref() != Some("network") => {
            let Some((_, frag)) = n.id.0.split_once(":loadbalancer/") else { return vec![] };
            (
                vec![
                    spec("requests", "Requests", "AWS/ApplicationELB", "RequestCount", "Sum", "count"),
                    Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("errors_5xx", "5xx from targets", "AWS/ApplicationELB", "HTTPCode_Target_5XX_Count", "Sum", "count") },
                    Spec { scale: 1000.0, ..spec("latency_p95", "Response time p95", "AWS/ApplicationELB", "TargetResponseTime", "p95", "ms") },
                ],
                vec![("LoadBalancer", frag.to_string())],
            )
        }
        NodeKind::RdsCluster => (
            vec![spec("cpu", "CPU", "AWS/RDS", "CPUUtilization", "Average", "%"), spec("connections", "Connections", "AWS/RDS", "DatabaseConnections", "Average", "count")],
            vec![("DBClusterIdentifier", n.name.clone())],
        ),
        NodeKind::RdsInstance => (
            vec![spec("cpu", "CPU", "AWS/RDS", "CPUUtilization", "Average", "%"), spec("connections", "Connections", "AWS/RDS", "DatabaseConnections", "Average", "count")],
            vec![("DBInstanceIdentifier", n.name.clone())],
        ),
        NodeKind::SqsQueue => (
            vec![
                spec("sent", "Messages sent", "AWS/SQS", "NumberOfMessagesSent", "Sum", "count"),
                spec("backlog", "Messages waiting", "AWS/SQS", "ApproximateNumberOfMessagesVisible", "Maximum", "count"),
                Spec { alert: Some(Alert::MaxAbove(3600.0)), ..spec("oldest_age", "Oldest message age", "AWS/SQS", "ApproximateAgeOfOldestMessage", "Maximum", "s") },
            ],
            vec![("QueueName", n.name.clone())],
        ),
        NodeKind::DynamoTable => (
            vec![
                spec("writes", "Write units", "AWS/DynamoDB", "ConsumedWriteCapacityUnits", "Sum", "count"),
                spec("reads", "Read units", "AWS/DynamoDB", "ConsumedReadCapacityUnits", "Sum", "count"),
                Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("write_throttles", "Write throttles", "AWS/DynamoDB", "WriteThrottleEvents", "Sum", "count") },
            ],
            vec![("TableName", n.name.clone())],
        ),
        NodeKind::EcsService => {
            let Some(cluster) = attr("cluster") else { return vec![] };
            (
                vec![spec("cpu", "CPU", "AWS/ECS", "CPUUtilization", "Average", "%"), spec("memory", "Memory", "AWS/ECS", "MemoryUtilization", "Average", "%")],
                vec![("ClusterName", cluster), ("ServiceName", n.name.clone())],
            )
        }
        NodeKind::ApiGateway if n.id.0.contains("/apis/") => (
            vec![
                spec("requests", "Requests", "AWS/ApiGateway", "Count", "Sum", "count"),
                Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("errors_5xx", "5xx", "AWS/ApiGateway", "5xx", "Sum", "count") },
                spec("latency_p95", "Latency p95", "AWS/ApiGateway", "Latency", "p95", "ms"),
            ],
            vec![("ApiId", n.id.0.rsplit('/').next().unwrap_or_default().to_string())],
        ),
        NodeKind::ApiGateway => (
            vec![
                spec("requests", "Requests", "AWS/ApiGateway", "Count", "Sum", "count"),
                Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("errors_5xx", "5xx", "AWS/ApiGateway", "5XXError", "Sum", "count") },
                spec("latency_p95", "Latency p95", "AWS/ApiGateway", "Latency", "p95", "ms"),
            ],
            vec![("ApiName", n.name.clone())],
        ),
        NodeKind::StateMachine => (
            vec![
                spec("started", "Executions", "AWS/States", "ExecutionsStarted", "Sum", "count"),
                Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("failed", "Failed executions", "AWS/States", "ExecutionsFailed", "Sum", "count") },
            ],
            vec![("StateMachineArn", n.id.0.clone())],
        ),
        NodeKind::NatGateway => {
            let Some(id) = attr("nat_gateway_id") else { return vec![] };
            (
                vec![spec("bytes_out", "Bytes out", "AWS/NATGateway", "BytesOutToDestination", "Sum", "bytes")],
                vec![("NatGatewayId", id)],
            )
        }
        NodeKind::SnsTopic => (
            vec![
                spec("published", "Messages published", "AWS/SNS", "NumberOfMessagesPublished", "Sum", "count"),
                Spec { alert: Some(Alert::AnyAbove(0.0)), ..spec("failed", "Failed deliveries", "AWS/SNS", "NumberOfNotificationsFailed", "Sum", "count") },
            ],
            vec![("TopicName", n.name.clone())],
        ),
        _ => return vec![],
    };
    for s in &mut v {
        s.dims = dims.clone();
    }
    v
}

/// Summarise a 24-slot hourly series (oldest first, `None` = no datapoint).
pub fn summarize(s: &Spec, values: &[Option<f64>]) -> Value {
    let present: Vec<f64> = values.iter().flatten().copied().collect();
    let total: f64 = present.iter().sum();
    let max = present.iter().copied().fold(f64::NAN, f64::max);
    let latest = values.iter().rev().flatten().next().copied();
    let is_sum = s.stat == "Sum";
    let fmt = |x: f64| if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x:.1}") };
    let alert = match s.alert {
        Some(Alert::AnyAbove(t)) if total > t => Some(format!("{} {} in the last 24 hours", fmt(total), s.label.to_lowercase())),
        Some(Alert::MaxAbove(t)) if max > t => Some(format!("{} reached {} {}", s.label, fmt(max), s.unit)),
        _ => None,
    };
    json!({
        "key": s.key,
        "label": s.label,
        "unit": s.unit,
        "stat": s.stat,
        "values": values,
        "summary": if is_sum { total } else { latest.unwrap_or(f64::NAN) },
        "summary_label": if is_sum { "last 24 h" } else { "latest" },
        "alert": alert,
    })
}

async fn metrics(g: &mut ArchGraph, ctx: &ScanContext) -> anyhow::Result<()> {
    let cw = aws_sdk_cloudwatch::Client::new(&ctx.sdk);
    let end = (chrono::Utc::now().timestamp() / 3600) * 3600;
    let start = end - HOURS * 3600;

    let mut wanted: Vec<(NodeId, Spec)> = Vec::new();
    for n in g.nodes().filter(|n| n.region.as_deref() == Some(ctx.region.as_str())) {
        for s in specs_for(n) {
            wanted.push((n.id.clone(), s));
        }
    }
    let mut series: HashMap<usize, Vec<Option<f64>>> = HashMap::new();
    for (batch_no, batch) in wanted.chunks(400).enumerate() {
        let queries: Vec<MetricDataQuery> = batch
            .iter()
            .enumerate()
            .map(|(i, (_, s))| {
                let dims: Vec<Dimension> = s.dims.iter().map(|(k, v)| Dimension::builder().name(*k).value(v).build()).collect();
                let metric = Metric::builder().namespace(s.namespace).metric_name(s.metric).set_dimensions(Some(dims)).build();
                let stat = MetricStat::builder().metric(metric).period(3600).stat(s.stat).build();
                MetricDataQuery::builder().id(format!("m{}", batch_no * 400 + i)).metric_stat(stat).return_data(true).build()
            })
            .collect();
        let mut pages = cw
            .get_metric_data()
            .set_metric_data_queries(Some(queries))
            .start_time(aws_sdk_cloudwatch::primitives::DateTime::from_secs(start))
            .end_time(aws_sdk_cloudwatch::primitives::DateTime::from_secs(end))
            .into_paginator()
            .send();
        while let Some(page) = pages.next().await {
            for r in page?.metric_data_results() {
                let Some(idx) = r.id().and_then(|id| id.trim_start_matches('m').parse::<usize>().ok()) else { continue };
                let slots = series.entry(idx).or_insert_with(|| vec![None; HOURS as usize]);
                for (t, v) in r.timestamps().iter().zip(r.values()) {
                    let slot = (t.secs() - start) / 3600;
                    if (0..HOURS).contains(&slot) {
                        slots[slot as usize] = Some(*v * wanted[idx].1.scale);
                    }
                }
            }
        }
    }
    let mut per_node: HashMap<NodeId, Vec<Value>> = HashMap::new();
    for (idx, (id, s)) in wanted.iter().enumerate() {
        let mut values = series.remove(&idx).unwrap_or_else(|| vec![None; HOURS as usize]);
        if s.stat == "Sum" {
            // CloudWatch publishes nothing for a counter that did not move (a Lambda nobody
            // invoked): no datapoint means zero, which is exactly what idle detection needs.
            values.iter_mut().for_each(|v| *v = Some(v.unwrap_or(0.0)));
        } else if values.iter().all(Option::is_none) {
            continue; // gauge with no data: nothing to show
        }
        per_node.entry(id.clone()).or_default().push(summarize(s, &values));
    }
    for (id, list) in per_node {
        if let Some(n) = g.node_mut(&id) {
            set_attr(n, "metrics", Value::Array(list));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ X-Ray

/// How an X-Ray service name maps to our aliases.
pub fn xray_alias(service_type: Option<&str>, name: &str) -> Option<String> {
    Some(match service_type? {
        "AWS::DynamoDB::Table" => format!("dynamodb:{name}"),
        "AWS::S3::Bucket" => format!("s3:{name}"),
        "AWS::ApiGateway::Stage" => name.split('/').next()?.to_string(),
        "client" => return None,
        _ => name.to_string(),
    })
}

async fn traces(g: &mut ArchGraph, ctx: &ScanContext) -> anyhow::Result<()> {
    let xr = aws_sdk_xray::Client::new(&ctx.sdk);
    let end = chrono::Utc::now().timestamp();
    let mut pages = xr
        .get_service_graph()
        .start_time(aws_sdk_xray::primitives::DateTime::from_secs(end - 6 * 3600))
        .end_time(aws_sdk_xray::primitives::DateTime::from_secs(end))
        .into_paginator()
        .send();
    let mut services = Vec::new();
    while let Some(page) = pages.next().await {
        services.extend(page?.services().to_vec());
    }
    let ix = index(g, &ctx.region);
    let mut by_ref: HashMap<i32, NodeId> = HashMap::new();
    for s in &services {
        let (Some(r), Some(name)) = (s.reference_id(), s.name()) else { continue };
        let ty = s.r#type();
        let Some(alias) = xray_alias(ty, name) else { continue };
        let id = match ix.get(&alias).or_else(|| ix.get(&alias.to_ascii_lowercase())) {
            Some(id) => id.clone(),
            // A host we don't own: a third-party dependency seen in traces.
            None if ty == Some("remote") => {
                let ext = external_node(name);
                let id = ext.id.clone();
                if !g.contains(&id) {
                    g.upsert_node(ext);
                }
                id
            }
            None => continue,
        };
        by_ref.insert(r, id);
    }
    for s in &services {
        let Some(from) = s.reference_id().and_then(|r| by_ref.get(&r)) else { continue };
        for e in s.edges() {
            let Some(to) = e.reference_id().and_then(|r| by_ref.get(&r)) else { continue };
            if from == to {
                continue;
            }
            let kind = infer_kind(&g.node(to).expect("indexed").kind);
            let calls = e.summary_statistics().and_then(|st| st.total_count()).unwrap_or(0);
            g.add_evidence(from, to, kind, evidence(EvidenceKind::Observed, "xray", Some(format!("{calls} calls in 6 h"))));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarm_dimensions_resolve_to_nodes() {
        let ix: HashMap<String, NodeId> = [
            ("ecs:prod/api", "svc"),
            ("app/web/abc", "alb"),
            ("orders", "fn"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), NodeId(b.into())))
        .collect();
        let d = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(node_for_dimensions(&ix, &d(&[("ClusterName", "prod"), ("ServiceName", "api")])).unwrap().0, "svc");
        assert_eq!(node_for_dimensions(&ix, &d(&[("LoadBalancer", "app/web/abc")])).unwrap().0, "alb");
        assert_eq!(node_for_dimensions(&ix, &d(&[("FunctionName", "orders"), ("Resource", "orders:live")])).unwrap().0, "fn");
        assert!(node_for_dimensions(&ix, &d(&[("InstanceId", "i-1")])).is_none());
    }

    #[test]
    fn specs_cover_core_services() {
        let mut f = Node::new("arn:aws:lambda:us-east-1:1:function:f", NodeKind::LambdaFunction, "f");
        f.region = Some("us-east-1".into());
        assert_eq!(specs_for(&f).len(), 4);
        let lb = Node::new("arn:aws:elasticloadbalancing:us-east-1:1:loadbalancer/app/web/abc", NodeKind::LoadBalancer, "web");
        assert_eq!(specs_for(&lb)[0].dims, vec![("LoadBalancer", "app/web/abc".to_string())]);
        let mut svc = Node::new("s", NodeKind::EcsService, "api");
        svc.attrs = json!({"cluster": "prod"});
        assert_eq!(specs_for(&svc)[0].dims.len(), 2);
    }

    #[test]
    fn summaries_raise_alerts() {
        let s = specs_for(&Node::new("x", NodeKind::LambdaFunction, "f")).into_iter().find(|s| s.key == "errors").unwrap();
        let mut v = vec![None; 24];
        v[20] = Some(3.0);
        v[23] = Some(4.0);
        let out = summarize(&s, &v);
        assert_eq!(out["summary"], 7.0);
        assert_eq!(out["alert"], "7 errors in the last 24 hours");
        let q = specs_for(&Node::new("q", NodeKind::SqsQueue, "q")).into_iter().find(|s| s.key == "oldest_age").unwrap();
        assert!(summarize(&q, &[Some(10.0), Some(7200.0)])["alert"].as_str().unwrap().contains("7200"));
    }

    #[test]
    fn xray_names_map_to_aliases() {
        assert_eq!(xray_alias(Some("AWS::DynamoDB::Table"), "orders").unwrap(), "dynamodb:orders");
        assert_eq!(xray_alias(Some("AWS::ApiGateway::Stage"), "shop-api/prod").unwrap(), "shop-api");
        assert!(xray_alias(Some("client"), "Client").is_none());
    }
}
