//! Cloud Control: one uniform API (the CloudFormation resource model) for most AWS services.
//! Resources are listed, read in full with GetResource, then interpreted by `extract`.

use crate::extract::{extract, Lookup};
use crate::{Progress, ScanContext, ScanOutput};
use sprawlr_core::{Node, NodeId, NodeKind};
use aws_sdk_cloudcontrol::error::DisplayErrorContext;
use aws_sdk_cloudcontrol::Client;
use futures::{stream, StreamExt};
use serde_json::{json, Value};

/// Regional types, lookups first (target groups and DB instances explain other resources).
pub const TYPES: &[&str] = &[
    "AWS::EC2::Instance",
    "AWS::EC2::NatGateway",
    "AWS::EC2::EIP",
    "AWS::ElasticLoadBalancingV2::TargetGroup",
    "AWS::RDS::DBInstance",
    "AWS::ElasticLoadBalancingV2::LoadBalancer",
    "AWS::RDS::DBCluster",
    "AWS::SQS::Queue",
    "AWS::SNS::Topic",
    "AWS::DynamoDB::Table",
    "AWS::KMS::Key",
    "AWS::StepFunctions::StateMachine",
    "AWS::Events::EventBus",
    "AWS::Events::Rule",
    "AWS::ApiGatewayV2::Api",
    "AWS::ApiGateway::RestApi",
    "AWS::ECS::Cluster",
];

/// Account-wide types, read once from the first region.
pub const GLOBAL_TYPES: &[&str] = &["AWS::S3::Bucket"];

/// CloudFront is global and only answers in us-east-1.
pub const CLOUDFRONT: &str = "AWS::CloudFront::Distribution";

const CONCURRENCY: usize = 8;

type Item = (String, String, Value); // (type, identifier, properties)

/// Identifiers, plus whatever properties the list call already returned (often partial).
async fn list(c: &Client, t: &str, model: Option<&Value>) -> anyhow::Result<Vec<(String, Option<Value>)>> {
    let mut req = c.list_resources().type_name(t);
    if let Some(m) = model {
        req = req.resource_model(m.to_string());
    }
    let mut pages = req.into_paginator().send();
    let mut ids = Vec::new();
    while let Some(page) = pages.next().await {
        for r in page.map_err(|e| anyhow::anyhow!("{}", DisplayErrorContext(&e)))?.resource_descriptions() {
            let Some(id) = r.identifier() else { continue };
            let props = r.properties().and_then(|p| serde_json::from_str::<Value>(p).ok()).filter(|v| v.as_object().is_some_and(|o| !o.is_empty()));
            ids.push((id.to_string(), props));
        }
    }
    Ok(ids)
}

/// Full resource models. If a read fails, the properties from the list call are used instead,
/// and the warning carries AWS's own error message so it can be acted on.
async fn get_all(c: &Client, t: &str, ids: Vec<(String, Option<Value>)>, warnings: &mut Vec<String>) -> Vec<Item> {
    let results: Vec<(String, Option<Value>, Result<Value, String>)> = stream::iter(ids)
        .map(|(id, listed)| async move {
            let r = c.get_resource().type_name(t).identifier(&id).send().await;
            let props = r
                .map_err(|e| DisplayErrorContext(&e).to_string())
                .map(|o| o.resource_description().and_then(|d| d.properties()).and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default());
            (id, listed, props)
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    let failures: Vec<(&String, &String, bool)> = results
        .iter()
        .filter_map(|(id, listed, r)| r.as_ref().err().map(|e| (id, e, listed.is_some())))
        .collect();
    if let Some((id, err, _)) = failures.first() {
        let partial = failures.iter().filter(|(_, _, p)| *p).count();
        let note = if partial == failures.len() { ", kept with the data from the listing" } else { "" };
        warnings.push(format!("{}: {} could not be read in full{note}. First error ({id}): {err}", short(t), plural(failures.len(), "resource")));
    }
    results
        .into_iter()
        .map(|(id, listed, r)| (t.to_string(), id, r.ok().or(listed).unwrap_or_default()))
        .collect()
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

async fn collect(c: &Client, t: &str, model: Option<&Value>, items: &mut Vec<Item>, warnings: &mut Vec<String>, region: &str) {
    match list(c, t, model).await {
        Ok(ids) => items.extend(get_all(c, t, ids, warnings).await),
        Err(e) => warnings.push(format!("{region}/{}: {e}", short(t))),
    }
}

fn of<'a>(items: &'a [Item], t: &'a str) -> impl Iterator<Item = &'a Item> {
    items.iter().filter(move |(it, _, _)| it == t)
}

fn short(t: &str) -> String {
    t.trim_start_matches("AWS::").replace("::", " ")
}

pub async fn scan(ctx: &ScanContext, include_global: bool, progress: &Progress) -> anyhow::Result<ScanOutput> {
    let c = Client::new(&ctx.sdk);
    let r = &ctx.region;
    let mut items: Vec<Item> = Vec::new();
    let mut warnings = Vec::new();

    let globals: &[&str] = if include_global { GLOBAL_TYPES } else { &[] };
    for t in TYPES.iter().chain(globals) {
        progress(format!("{r}: reading {}", short(t)));
        collect(&c, t, None, &mut items, &mut warnings, r).await;
    }

    // Children that Cloud Control only lists under a parent.
    let clusters: Vec<String> = of(&items, "AWS::ECS::Cluster").map(|(_, id, p)| p.get("Arn").and_then(|v| v.as_str()).unwrap_or(id).to_string()).collect();
    let buses: Vec<String> = of(&items, "AWS::Events::EventBus").filter_map(|(_, _, p)| p.get("Name")?.as_str().map(String::from)).filter(|n| n != "default").collect();

    if !clusters.is_empty() {
        progress(format!("{r}: reading ECS services"));
    }
    for cl in clusters {
        collect(&c, "AWS::ECS::Service", Some(&json!({ "Cluster": cl })), &mut items, &mut warnings, r).await;
    }
    let task_defs: Vec<String> = {
        let mut v: Vec<String> = of(&items, "AWS::ECS::Service").filter_map(|(_, _, p)| p.get("TaskDefinition")?.as_str().map(String::from)).collect();
        v.sort();
        v.dedup();
        v
    };
    items.extend(get_all(&c, "AWS::ECS::TaskDefinition", task_defs.into_iter().map(|t| (t, None)).collect(), &mut warnings).await);
    // HTTP API integrations are read natively (apigw::scan_http): Cloud Control's handler
    // fails on some integration types.
    for bus in buses {
        collect(&c, "AWS::Events::Rule", Some(&json!({ "EventBusName": bus })), &mut items, &mut warnings, r).await;
    }

    Ok(interpret(ctx, &items, warnings))
}

/// CloudFront distributions (call with a us-east-1 context).
pub async fn scan_cloudfront(ctx: &ScanContext, progress: &Progress) -> anyhow::Result<ScanOutput> {
    progress("reading CloudFront distributions".into());
    let c = Client::new(&ctx.sdk);
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    collect(&c, CLOUDFRONT, None, &mut items, &mut warnings, "global").await;
    Ok(interpret(ctx, &items, warnings))
}

fn interpret(ctx: &ScanContext, items: &[Item], warnings: Vec<String>) -> ScanOutput {
    let mut lk = Lookup::default();
    for (t, _, p) in items {
        lk.learn(t, p);
    }
    let mut out = ScanOutput { authoritative: true, warnings, ..Default::default() };
    for (t, id, p) in items {
        let e = extract(ctx, t, id, p, &lk);
        out.nodes.extend(e.node);
        out.links.extend(e.links);
    }
    out
}

pub fn kind_for(type_name: &str) -> NodeKind {
    match type_name {
        "AWS::SQS::Queue" => NodeKind::SqsQueue,
        "AWS::SNS::Topic" => NodeKind::SnsTopic,
        "AWS::DynamoDB::Table" => NodeKind::DynamoTable,
        "AWS::S3::Bucket" => NodeKind::S3Bucket,
        "AWS::RDS::DBCluster" => NodeKind::RdsCluster,
        "AWS::RDS::DBInstance" => NodeKind::RdsInstance,
        "AWS::ECS::Cluster" => NodeKind::EcsCluster,
        "AWS::ECS::Service" => NodeKind::EcsService,
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => NodeKind::LoadBalancer,
        "AWS::ApiGateway::RestApi" | "AWS::ApiGatewayV2::Api" => NodeKind::ApiGateway,
        "AWS::SecretsManager::Secret" => NodeKind::Secret,
        "AWS::SSM::Parameter" => NodeKind::SsmParameter,
        "AWS::KMS::Key" => NodeKind::KmsKey,
        "AWS::StepFunctions::StateMachine" => NodeKind::StateMachine,
        "AWS::Events::EventBus" => NodeKind::EventBus,
        "AWS::IAM::Role" => NodeKind::IamRole,
        other => NodeKind::Other(other.to_string()),
    }
}

pub fn map_resource(ctx: &ScanContext, type_name: &str, ident: &str, props: Option<&str>) -> Node {
    let props: serde_json::Value = props.and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default();
    let kind = kind_for(type_name);
    let arn = ["Arn", "TopicArn", "QueueArn", "DBClusterArn", "DBInstanceArn", "LoadBalancerArn", "KeyArn", "Id"]
        .iter()
        .find_map(|k| props.get(*k).and_then(|v| v.as_str()).filter(|s| s.starts_with("arn:")))
        .map(str::to_string)
        .or_else(|| derive_arn(ctx, &kind, ident))
        .unwrap_or_else(|| format!("cc:{type_name}:{ident}"));

    let mut n = Node::new(NodeId(arn), kind.clone(), short_name(ident));
    n.account = Some(ctx.account.clone());
    n.region = Some(ctx.region.clone());
    n.parent = Some(ctx.region_id());
    n.aliases.push(ident.to_string());
    match kind {
        NodeKind::DynamoTable => n.aliases.push(format!("dynamodb:{ident}")),
        NodeKind::S3Bucket => n.aliases.push(format!("s3:{ident}")),
        _ => {}
    }
    // Endpoint hosts let env-var references (DB_HOST=...) resolve to this node.
    for path in [&["Endpoint", "Address"][..], &["DNSName"][..]] {
        if let Some(h) = path.iter().try_fold(&props, |v, k| v.get(*k)).and_then(|v| v.as_str()) {
            n.aliases.push(h.to_ascii_lowercase());
        }
    }
    n.attrs = props;
    n
}

/// Some types return only a name/URL as identifier and no ARN in properties.
fn derive_arn(ctx: &ScanContext, kind: &NodeKind, ident: &str) -> Option<String> {
    let (r, a) = (&ctx.region, &ctx.account);
    match kind {
        NodeKind::S3Bucket => Some(format!("arn:aws:s3:::{ident}")),
        NodeKind::DynamoTable => Some(format!("arn:aws:dynamodb:{r}:{a}:table/{ident}")),
        NodeKind::SqsQueue => Some(format!("arn:aws:sqs:{r}:{a}:{}", short_name(ident))),
        _ => None,
    }
}

fn short_name(ident: &str) -> String {
    ident.rsplit(['/', ':']).next().unwrap_or(ident).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ScanContext {
        ScanContext { sdk: aws_config::SdkConfig::builder().build(), account: "123".into(), region: "us-east-1".into() }
    }

    #[test]
    fn sqs_queue_url_identifier_gets_arn_and_alias() {
        let url = "https://sqs.us-east-1.amazonaws.com/123/orders";
        let n = map_resource(&ctx(), "AWS::SQS::Queue", url, Some(r#"{"QueueUrl":"x"}"#));
        assert_eq!(n.id.0, "arn:aws:sqs:us-east-1:123:orders");
        assert!(n.aliases.contains(&url.to_string()));
        assert_eq!(n.name, "orders");
    }

    #[test]
    fn rds_endpoint_becomes_alias() {
        let p = r#"{"DBClusterArn":"arn:aws:rds:us-east-1:123:cluster:orders","Endpoint":{"Address":"Orders.cluster-x.us-east-1.rds.amazonaws.com"}}"#;
        let n = map_resource(&ctx(), "AWS::RDS::DBCluster", "orders", Some(p));
        assert_eq!(n.kind, NodeKind::RdsCluster);
        assert!(n.aliases.contains(&"orders.cluster-x.us-east-1.rds.amazonaws.com".to_string()));
    }

    #[test]
    fn unknown_types_are_kept() {
        let n = map_resource(&ctx(), "AWS::Glue::Job", "etl", None);
        assert_eq!(n.kind, NodeKind::Other("AWS::Glue::Job".into()));
    }
}
