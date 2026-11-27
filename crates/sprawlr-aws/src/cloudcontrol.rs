//! Cloud Control: one uniform API (the CloudFormation resource model) for most AWS services.
//! Resources are listed, read in full with GetResource, then interpreted by `extract`.

use crate::extract::{extract, Lookup};
use crate::kms;
use crate::{Progress, ScanContext, ScanOutput};
use aws_sdk_cloudcontrol::error::ProvideErrorMetadata;
use aws_sdk_cloudcontrol::Client;
use futures::{stream, StreamExt};
use serde_json::{json, Value};
use sprawlr_domain::{Node, NodeId, NodeKind};

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

/// Attempts per read. Backoff after a throttle doubles from 1 s, with jitter, so a read can
/// wait up to about 30 s (15 s on average) before it gives up.
const MAX_ATTEMPTS: u32 = 6;

/// GetResource reads each resource through the service behind it (KMS, RDS...), whose own
/// rate limits throttle a scan that reads many in parallel. Adaptive retry slows every read
/// down after a throttle, instead of only retrying the one that failed. The SDK keeps that
/// pacing per region for the whole process, so it carries over to the next scan in the app.
pub(crate) fn client(sdk: &aws_config::SdkConfig) -> Client {
    let retry = aws_config::retry::RetryConfig::adaptive().with_max_attempts(MAX_ATTEMPTS);
    Client::from_conf(aws_sdk_cloudcontrol::config::Builder::from(sdk).retry_config(retry).build())
}

type Item = (String, String, Value); // (type, identifier, properties)

/// Identifiers, plus whatever properties the list call already returned (often partial).
async fn list(c: &Client, t: &str, model: Option<&Value>) -> Result<Vec<(String, Option<Value>)>, Failure> {
    let mut req = c.list_resources().type_name(t);
    if let Some(m) = model {
        req = req.resource_model(m.to_string());
    }
    let mut pages = req.into_paginator().send();
    let mut ids = Vec::new();
    while let Some(page) = pages.next().await {
        for r in page.map_err(|e| failure(&e))?.resource_descriptions() {
            let Some(id) = r.identifier() else { continue };
            let props =
                r.properties().and_then(|p| serde_json::from_str::<Value>(p).ok()).filter(|v| v.as_object().is_some_and(|o| !o.is_empty()));
            ids.push((id.to_string(), props));
        }
    }
    Ok(ids)
}

/// Full resource models. If a read fails, the properties from the list call are used instead,
/// and the warning carries AWS's own error message so it can be acted on.
async fn get_all(c: &Client, t: &str, ids: Vec<(String, Option<Value>)>, warnings: &mut Vec<String>) -> Vec<Item> {
    let results: Vec<(String, Option<Value>, Result<Value, Failure>)> = stream::iter(ids)
        .map(|(id, listed)| async move {
            let r = c.get_resource().type_name(t).identifier(&id).send().await;
            let props = r.map_err(|e| failure(&e)).map(|o| {
                o.resource_description().and_then(|d| d.properties()).and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default()
            });
            (id, listed, props)
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    let failures: Vec<(&String, &Failure, bool)> =
        results.iter().filter_map(|(id, listed, r)| r.as_ref().err().map(|e| (id, e, listed.is_some()))).collect();
    if let Some((id, first, _)) = failures.first() {
        let partial = failures.iter().filter(|(_, _, p)| *p).count();
        let note = if partial == failures.len() { ", kept with the data from the listing" } else { "" };
        let hint = if failures.iter().all(|(_, f, _)| f.throttled) { format!(" {SLOWED_DOWN}") } else { String::new() };
        warnings.push(format!(
            "{}: {} could not be read in full{note}.{hint} First error ({id}): {}",
            short(t),
            plural(failures.len(), "resource"),
            first.message
        ));
    }
    results.into_iter().map(|(id, listed, r)| (t.to_string(), id, r.ok().or(listed).unwrap_or_default())).collect()
}

const SLOWED_DOWN: &str = "AWS slowed these reads down: scan again in a few minutes.";

/// Why a call failed, in AWS's own words (code and message) rather than the SDK's debug dump,
/// which can carry the raw response and with it a resource's properties.
pub(crate) struct Failure {
    pub message: String,
    throttled: bool,
}

pub(crate) fn failure<E, R>(e: &aws_sdk_cloudcontrol::error::SdkError<E, R>) -> Failure
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
    R: std::fmt::Debug + 'static,
{
    match e.as_service_error() {
        Some(s) => {
            let code = s.code().unwrap_or("Error");
            let message = s.message().map(|m| format!("{code}: {m}")).unwrap_or_else(|| code.to_string());
            // The only throttling error Cloud Control models.
            Failure { message, throttled: code == "ThrottlingException" }
        }
        None => Failure { message: causes(e), throttled: false },
    }
}

/// An error and its causes, each by its Display text: "dispatch failure: io error: connection refused".
fn causes(e: &(dyn std::error::Error + 'static)) -> String {
    std::iter::successors(Some(e), |e| e.source()).map(|e| e.to_string()).collect::<Vec<_>>().join(": ")
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

async fn collect(c: &Client, t: &str, model: Option<&Value>, items: &mut Vec<Item>, warnings: &mut Vec<String>, region: &str) {
    match list(c, t, model).await {
        Ok(ids) => items.extend(get_all(c, t, ids, warnings).await),
        Err(f) if f.throttled => warnings.push(format!("{region}/{}: {}. {SLOWED_DOWN}", short(t), f.message)),
        Err(f) => warnings.push(format!("{region}/{}: {}", short(t), f.message)),
    }
}

fn of<'a>(items: &'a [Item], t: &'a str) -> impl Iterator<Item = &'a Item> {
    items.iter().filter(move |(it, _, _)| it == t)
}

fn short(t: &str) -> String {
    t.trim_start_matches("AWS::").replace("::", " ")
}

pub async fn scan(ctx: &ScanContext, include_global: bool, progress: &Progress) -> anyhow::Result<ScanOutput> {
    let c = client(&ctx.sdk);
    let r = &ctx.region;
    let mut items: Vec<Item> = Vec::new();
    let mut warnings = Vec::new();

    let globals: &[&str] = if include_global { GLOBAL_TYPES } else { &[] };
    for t in TYPES.iter().chain(globals) {
        progress(format!("{r}: reading {}", short(t)));
        collect(&c, t, None, &mut items, &mut warnings, r).await;
    }

    // Children that Cloud Control only lists under a parent.
    let clusters: Vec<String> =
        of(&items, "AWS::ECS::Cluster").map(|(_, id, p)| p.get("Arn").and_then(|v| v.as_str()).unwrap_or(id).to_string()).collect();
    let buses: Vec<String> = of(&items, "AWS::Events::EventBus")
        .filter_map(|(_, _, p)| p.get("Name")?.as_str().map(String::from))
        .filter(|n| n != "default")
        .collect();

    if !clusters.is_empty() {
        progress(format!("{r}: reading ECS services"));
    }
    for cl in clusters {
        collect(&c, "AWS::ECS::Service", Some(&json!({ "Cluster": cl })), &mut items, &mut warnings, r).await;
    }
    let task_defs: Vec<String> = {
        let mut v: Vec<String> =
            of(&items, "AWS::ECS::Service").filter_map(|(_, _, p)| p.get("TaskDefinition")?.as_str().map(String::from)).collect();
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

    let aliases = if of(&items, "AWS::KMS::Key").next().is_some() {
        progress(format!("{r}: reading KMS aliases"));
        kms::aliases(ctx).await.unwrap_or_else(|e| {
            warnings.push(format!(
                "{r}/KMS aliases: {e}. Keys show their description or id instead. If access was denied, \
                 add kms:ListAliases from iam-policy.json to the role sprawlr uses."
            ));
            Vec::new()
        })
    } else {
        Vec::new()
    };
    Ok(interpret(ctx, &items, warnings, &aliases))
}

/// CloudFront distributions (call with a us-east-1 context).
pub async fn scan_cloudfront(ctx: &ScanContext, progress: &Progress) -> anyhow::Result<ScanOutput> {
    progress("reading CloudFront distributions".into());
    let c = client(&ctx.sdk);
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    collect(&c, CLOUDFRONT, None, &mut items, &mut warnings, "global").await;
    Ok(interpret(ctx, &items, warnings, &[]))
}

/// `kms_aliases`: (alias, key id) pairs, to name keys.
fn interpret(ctx: &ScanContext, items: &[Item], warnings: Vec<String>, kms_aliases: &[(String, String)]) -> ScanOutput {
    let mut lk = Lookup::default();
    for (t, _, p) in items {
        lk.learn(t, p);
    }
    for (alias, key_id) in kms_aliases {
        lk.learn_kms_alias(alias, key_id);
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

/// Properties of a resource type without its own extractor, kept only where they cannot hold
/// data or secrets (ADR 0003): top-level scalars, never under names that suggest content,
/// credentials or code, and never long text.
pub fn safe_props(props: &serde_json::Value) -> serde_json::Value {
    const SENSITIVE: [&str; 16] = [
        "password",
        "secret",
        "token",
        "key",
        "credential",
        "environment",
        "variable",
        "userdata",
        "content",
        "body",
        "code",
        "policy",
        "definition",
        "certificate",
        "private",
        "connectionstring",
    ];
    let mut out = serde_json::Map::new();
    for (k, v) in props.as_object().into_iter().flatten() {
        let lk = k.to_ascii_lowercase();
        if SENSITIVE.iter().any(|s| lk.contains(s)) && !lk.ends_with("arn") && !lk.ends_with("id") {
            continue;
        }
        let keep = match v {
            serde_json::Value::String(s) => s.len() <= 200,
            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => true,
            _ => false,
        };
        if keep {
            out.insert(k.clone(), v.clone());
        }
    }
    serde_json::Value::Object(out)
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
    n.attrs = safe_props(&props);
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
    #[test]
    fn unknown_types_keep_only_safe_scalars() {
        let p = serde_json::json!({
            "Name": "orders-search", "Arn": "arn:aws:es:us-east-1:1:domain/x", "InstanceCount": 3, "Encrypted": true,
            "MasterUserPassword": "hunter2", "AuthToken": "t0k", "KmsKeyId": "abc", "Environment": {"A": "b"},
            "AccessPolicies": {"Statement": []}, "UserData": "IyEvYmluL3No", "Endpoints": ["a", "b"], "Description": "x".repeat(300)
        });
        let kept = super::safe_props(&p);
        let keys: Vec<&String> = kept.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["Arn", "Encrypted", "InstanceCount", "KmsKeyId", "Name"], "{kept}");
    }

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

    use crate::testing::sdk_answering;

    const THROTTLED: (u16, &str) = (400, r#"{"__type":"ThrottlingException","Message":"Rate exceeded"}"#);
    const KEY: (u16, &str) =
        (200, r#"{"TypeName":"AWS::KMS::Key","ResourceDescription":{"Identifier":"k1","Properties":"{\"KeyId\":\"k1\"}"}}"#);
    /// Status 0: the request never reaches AWS (the connection fails).
    const UNREACHABLE: (u16, &str) = (0, "connection refused");

    #[tokio::test]
    async fn a_resource_aws_throttles_a_few_times_is_still_read_in_full() {
        let sdk = sdk_answering("us-east-1", |call| if call < 4 { THROTTLED } else { KEY });
        let mut warnings = Vec::new();
        let items = get_all(&client(&sdk), "AWS::KMS::Key", vec![("k1".into(), None)], &mut warnings).await;
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(items[0].2["KeyId"], "k1");
    }

    #[tokio::test]
    async fn a_resource_aws_keeps_throttling_is_reported_plainly_with_what_to_do() {
        let sdk = sdk_answering("us-east-2", |_| THROTTLED);
        let mut warnings = Vec::new();
        let listed = Some(json!({ "KeyId": "k1" }));
        let items = get_all(&client(&sdk), "AWS::KMS::Key", vec![("k1".into(), listed)], &mut warnings).await;
        assert_eq!(
            warnings,
            ["KMS Key: 1 resource could not be read in full, kept with the data from the listing. \
              AWS slowed these reads down: scan again in a few minutes. \
              First error (k1): ThrottlingException: Rate exceeded"]
        );
        assert_eq!(items[0].2["KeyId"], "k1");
    }

    #[tokio::test]
    async fn a_listing_aws_keeps_throttling_says_what_to_do() {
        let sdk = sdk_answering("us-west-1", |_| THROTTLED);
        let (mut items, mut warnings) = (Vec::new(), Vec::new());
        collect(&client(&sdk), "AWS::KMS::Key", None, &mut items, &mut warnings, "us-west-1").await;
        assert_eq!(
            warnings,
            ["us-west-1/KMS Key: ThrottlingException: Rate exceeded. AWS slowed these reads down: scan again in a few minutes."]
        );
    }

    #[tokio::test]
    async fn a_read_that_never_reaches_aws_is_reported_without_the_sdk_internals() {
        let sdk = sdk_answering("us-west-2", |_| UNREACHABLE);
        let mut warnings = Vec::new();
        get_all(&client(&sdk), "AWS::KMS::Key", vec![("k1".into(), None)], &mut warnings).await;
        assert_eq!(
            warnings,
            ["KMS Key: 1 resource could not be read in full. First error (k1): dispatch failure: io error: connection refused"]
        );
    }

    #[tokio::test]
    async fn kms_aliases_are_read_and_a_denial_is_reported_in_aws_s_words() {
        let at = |region: &str, answer: (u16, &'static str)| ScanContext {
            sdk: sdk_answering(region, move |_| answer),
            account: "123".into(),
            region: region.into(),
        };
        let listed =
            r#"{"Aliases":[{"AliasName":"alias/orders-data","TargetKeyId":"k1"},{"AliasName":"alias/aws/unused"}],"Truncated":false}"#;
        assert_eq!(kms::aliases(&at("eu-central-1", (200, listed))).await.unwrap(), [("alias/orders-data".to_string(), "k1".to_string())]);
        let denied = r#"{"__type":"AccessDeniedException","message":"not authorized to perform: kms:ListAliases"}"#;
        assert_eq!(
            kms::aliases(&at("eu-west-3", (400, denied))).await.unwrap_err(),
            "AccessDeniedException: not authorized to perform: kms:ListAliases"
        );
    }

    #[test]
    fn unknown_types_are_kept() {
        let n = map_resource(&ctx(), "AWS::Glue::Job", "etl", None);
        assert_eq!(n.kind, NodeKind::Other("AWS::Glue::Job".into()));
    }
}
