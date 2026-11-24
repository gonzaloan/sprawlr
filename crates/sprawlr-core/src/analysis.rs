//! Findings (things worth a human look) and presentation helpers shared by docs, CLI and app.

use crate::{ArchGraph, EdgeKind, Environment, Health, Layer, Node, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub node: NodeId,
    pub rule: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
}

const STACK_TAG: &str = "aws:cloudformation:stack-name";

/// `BillingStack-prod-InvoiceProcessor9F3A21-XK2` → `InvoiceProcessor`. Mirrors `displayName` in the app.
pub fn display_name(n: &Node) -> String {
    let Some(stack) = n.tags.get(STACK_TAG) else { return n.name.clone() };
    let prefix = format!("{}-", stack.to_ascii_lowercase());
    if !n.name.to_ascii_lowercase().starts_with(&prefix) {
        return n.name.clone();
    }
    let rest = &n.name[prefix.len()..];
    let shown = strip_generated(rest);
    if shown.is_empty() { n.name.clone() } else { shown.to_string() }
}

/// Removes a CloudFormation-generated suffix: a hash starting with a digit, optionally `-XYZ`.
fn strip_generated(rest: &str) -> &str {
    let without_tail = rest
        .rsplit_once('-')
        .filter(|(a, b)| !a.is_empty() && !b.is_empty() && b.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|(a, _)| a);
    for core in [without_tail, Some(rest)].into_iter().flatten() {
        for (i, c) in core.char_indices().skip(1) {
            if !c.is_ascii_digit() {
                continue;
            }
            let tail = &core[i..];
            let upper = tail.chars().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase());
            let lower = tail.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase());
            if tail.len() >= 6 && (upper || lower) {
                return &core[..i];
            }
        }
    }
    rest
}

pub fn kind_label(k: &NodeKind) -> String {
    use NodeKind::*;
    match k {
        Account => "AWS account",
        Region => "Region",
        AvailabilityZone => "Availability zone",
        Vpc => "VPC",
        Subnet => "Subnet",
        SecurityGroup => "Security group",
        RouteTable => "Route table",
        InternetGateway => "Internet gateway",
        NatGateway => "NAT gateway",
        VpcEndpoint => "VPC endpoint",
        TransitGateway => "Transit gateway",
        CloudFront => "CloudFront distribution",
        ApiGateway => "API Gateway",
        LoadBalancer => "Load balancer",
        TargetGroup => "Target group",
        Route53Zone => "Route 53 zone",
        Ec2Instance => "EC2 instance",
        LambdaFunction => "Lambda function",
        EcsCluster => "ECS cluster",
        EcsService => "ECS service",
        EksCluster => "EKS cluster",
        SqsQueue => "SQS queue",
        SnsTopic => "SNS topic",
        EventBus => "EventBridge bus",
        EventRule => "EventBridge rule",
        StateMachine => "Step Functions state machine",
        KinesisStream => "Kinesis stream",
        RdsInstance => "RDS instance",
        RdsCluster => "Aurora / RDS cluster",
        DynamoTable => "DynamoDB table",
        S3Bucket => "S3 bucket",
        ElastiCache => "ElastiCache",
        IamRole => "IAM role",
        IamPolicy => "IAM policy",
        Secret => "Secret",
        SsmParameter => "SSM parameter",
        KmsKey => "KMS key",
        Internet => "Internet",
        ExternalEndpoint => "External service",
        Other(t) => return t.split("::").skip(1).collect::<Vec<_>>().join(" "),
    }
    .to_string()
}

pub fn verb(k: EdgeKind) -> &'static str {
    use EdgeKind::*;
    match k {
        Invokes => "invokes",
        Reads => "reads",
        Writes => "writes",
        Publishes => "publishes to",
        Subscribes => "subscribes to",
        RoutesTo => "routes to",
        ExposedVia => "exposed via",
        UsesSecret => "uses secret",
        EncryptedWith => "encrypted with",
        AssumesRole => "assumes",
        NetworkReachable => "can reach",
    }
}

fn verb_base(k: EdgeKind) -> &'static str {
    use EdgeKind::*;
    match k {
        Invokes => "invoke",
        Reads => "read",
        Writes => "write to",
        Publishes => "publish to",
        Subscribes => "subscribe to",
        RoutesTo => "route to",
        ExposedVia => "be exposed via",
        UsesSecret => "use the secret",
        EncryptedWith => "encrypt with",
        AssumesRole => "assume",
        NetworkReachable => "reach",
    }
}

pub fn env_name(e: &Environment) -> String {
    match e {
        Environment::Prod => "prod".into(),
        Environment::Staging => "staging".into(),
        Environment::Dev => "dev".into(),
        Environment::Test => "test".into(),
        Environment::Custom(s) => s.clone(),
    }
}

fn env_of(n: &Node) -> Option<String> {
    n.env.as_ref().map(|e| env_name(&e.value))
}

fn attr<'a>(n: &'a Node, key: &str) -> Option<&'a serde_json::Value> {
    n.attrs.get(key)
}

/// Everything the tool flags for review, most severe first.
pub fn findings(g: &ArchGraph) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut push = |n: &Node, rule: &str, severity: Severity, title: &str, detail: String| {
        out.push(Finding { node: n.id.clone(), rule: rule.into(), severity, title: title.into(), detail })
    };

    for n in g.nodes() {
        if let Some(keys) = attr(n, "env_plaintext_secret_keys").and_then(|v| v.as_array()).filter(|a| !a.is_empty()) {
            let keys: Vec<&str> = keys.iter().filter_map(|k| k.as_str()).collect();
            push(n, "plaintext_secret", Severity::High, "Secret stored as plain text",
                format!("{} is set directly in environment variables. Move it to Secrets Manager.", keys.join(", ")));
        }
        if n.health == Health::Alarm {
            push(n, "alarm", Severity::High, "Alarm firing", "A CloudWatch alarm on this resource is in ALARM state.".into());
        }
        if attr(n, "multi_az") == Some(&serde_json::Value::Bool(false)) && env_of(n).as_deref() == Some("prod") {
            push(n, "single_az", Severity::Medium, "Single availability zone", "A zone outage takes this database down.".into());
        }
        if n.kind == NodeKind::Secret && attr(n, "rotation_enabled") == Some(&serde_json::Value::Bool(false)) {
            push(n, "secret_rotation", Severity::Medium, "Secret is never rotated", "Turn on automatic rotation in Secrets Manager.".into());
        }
        if let Some(d) = attr(n, "last_accessed_days").and_then(|v| v.as_i64()).filter(|d| *d > 90) {
            push(n, "secret_unused", Severity::Low, "Secret not used in 90 days", format!("Last read {d} days ago. Delete it if nothing needs it."));
        }
        if attr(n, "publicly_accessible") == Some(&serde_json::Value::Bool(true)) {
            push(n, "public_database", Severity::High, "Database is publicly accessible", "It has a public IP. Turn off public access and reach it through the VPC.".into());
        }
        if n.kind == NodeKind::S3Bucket && attr(n, "public_access_block") == Some(&serde_json::Value::Bool(false)) && attr(n, "website") != Some(&serde_json::Value::Bool(true)) {
            push(n, "bucket_public_access", Severity::Medium, "Public access is not fully blocked", "Turn on all four S3 Block Public Access settings unless the bucket must be public.".into());
        }
        if attr(n, "encrypted") == Some(&serde_json::Value::Bool(false)) && matches!(n.kind, NodeKind::RdsCluster | NodeKind::RdsInstance) {
            push(n, "unencrypted_database", Severity::High, "Database storage is not encrypted", "Encryption can only be enabled by restoring a snapshot into an encrypted copy.".into());
        }
        // Telemetry: metric series flagged while reading CloudWatch (errors, 5xx, throttles, backlog).
        for m in attr(n, "metrics").and_then(|v| v.as_array()).into_iter().flatten() {
            if let Some(alert) = m.get("alert").and_then(|a| a.as_str()) {
                let label = m.get("label").and_then(|l| l.as_str()).unwrap_or("Metric");
                push(n, "metric_alert", Severity::Medium, &format!("{label} in the last 24 hours"), format!("{alert}."));
            }
        }
    }

    // Resources with at least one connection confirmed by configuration or traffic: only for
    // those can a permission-only connection be called "not used". For an EC2 instance known
    // only through IAM, the permissions are the best map we have.
    let confirmed: std::collections::BTreeSet<&NodeId> = g
        .edges()
        .filter(|(_, _, e)| e.kind.is_runtime_dependency() && !e.is_permission_only())
        .map(|(f, _, _)| &f.id)
        .collect();
    let mut extra: std::collections::BTreeMap<&NodeId, Vec<String>> = std::collections::BTreeMap::new();

    for (from, to, e) in g.edges() {
        if from.kind == NodeKind::Internet && !matches!(to.layer(), Layer::Network | Layer::Scope) {
            push(to, "internet_entry", Severity::Low, "Reachable from the internet", format!("Public entry point ({}).", verb(e.kind)));
        }
        if e.kind == EdgeKind::NetworkReachable || e.kind == EdgeKind::AssumesRole || !e.is_permission_only() {
            continue;
        }
        let (fe, te) = (env_of(from), env_of(to));
        let cross = matches!((&fe, &te), (Some(a), Some(b)) if a != b);
        let target = display_name(to);
        if cross {
            push(from, "cross_env_access", Severity::High, "Can access another environment",
                format!("IAM allows it to {} {target} ({} → {}), but nothing in its configuration does.", verb_base(e.kind), fe.unwrap(), te.unwrap()));
        } else if confirmed.contains(&from.id) {
            extra.entry(&from.id).or_default().push(format!("{} {target}", verb_base(e.kind)));
        }
    }
    for (id, list) in extra {
        let Some(n) = g.node(id) else { continue };
        let shown: Vec<&String> = list.iter().take(4).collect();
        let more = if list.len() > 4 { format!(" and {} more", list.len() - 4) } else { String::new() };
        push(n, "unused_permission", Severity::Low, if list.len() == 1 { "Permission not seen in use" } else { "Permissions not seen in use" },
            format!("IAM allows it to {}{more}, but nothing in its configuration or traffic does. Remove what it does not need.",
                shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }

    // Grants that cover a whole service: one finding per service and policy.
    for n in g.nodes() {
        let mut grouped: std::collections::BTreeMap<(String, String, String), Vec<String>> = std::collections::BTreeMap::new();
        for b in attr(n, "broad_permissions").and_then(|v| v.as_array()).into_iter().flatten() {
            let field = |k: &str| b.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let access = grouped.entry((field("service"), field("policy"), field("pattern"))).or_default();
            let a = field("access");
            if !access.contains(&a) {
                access.push(a);
            }
        }
        for ((service, policy, pattern), access) in grouped {
            let what = match service.as_str() {
                "s3" => "every S3 bucket",
                "dynamodb" => "every DynamoDB table",
                "sqs" => "every SQS queue",
                "sns" => "every SNS topic",
                "lambda" => "every Lambda function",
                "secretsmanager" => "every secret",
                "ssm" => "every SSM parameter",
                "kms" => "every KMS key",
                "events" => "every event bus",
                other => other,
            };
            let verbs = match access.as_slice() {
                [one] => one.clone(),
                [init @ .., last] => format!("{} and {last}", init.join(", ")),
                [] => "access".into(),
            };
            push(n, "broad_permission", Severity::Medium, "Permission covers a whole service",
                format!("Its role can {verbs} {what} (policy {policy}, resource `{pattern}`). Limit it to the resources it uses."));
        }
    }
    out.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.node.cmp(&b.node)));
    out
}

/// Resources that differ between environments of the same app: missing in one, or with
/// different resilience settings. Matched by kind + name without environment words.
pub fn env_differences(g: &ArchGraph, app: &str) -> Vec<String> {
    use std::collections::BTreeMap;
    let members: Vec<&Node> = g.nodes().filter(|n| n.app.as_ref().is_some_and(|a| a.value == app)).collect();
    let envs: BTreeSet<String> = members.iter().filter_map(|n| env_of(n)).collect();
    if envs.len() < 2 {
        return Vec::new();
    }
    let key = |n: &Node| {
        let words: Vec<String> = crate::infer::tokens(&display_name(n))
            .into_iter()
            .filter(|t| crate::infer::env_from_name(t).is_none())
            .collect();
        format!("{:?}:{}", n.kind, words.join("-"))
    };
    let mut by_key: BTreeMap<String, BTreeMap<String, &Node>> = BTreeMap::new();
    for n in &members {
        if let Some(e) = env_of(n) {
            by_key.entry(key(n)).or_default().insert(e, n);
        }
    }
    let mut out = Vec::new();
    for per_env in by_key.values() {
        // Name it after the prod variant when there is one (`checkout-orders-db`, not `…-dev`).
        let any = per_env.get("prod").copied().unwrap_or_else(|| per_env.values().next().unwrap());
        let name = display_name(any);
        let missing: Vec<&String> = envs.iter().filter(|e| !per_env.contains_key(*e)).collect();
        if !missing.is_empty() {
            let present: Vec<&String> = per_env.keys().collect();
            out.push(format!(
                "{name} ({}) exists in {} but not in {}.",
                kind_label(&any.kind),
                join(&present),
                join(&missing)
            ));
            continue;
        }
        for attr_key in ["multi_az", "desired", "encrypted", "instances", "memory_mb", "images", "runtime"] {
            let shown = |v: &serde_json::Value| match v {
                // Container images: compare the tags people recognise, not registry URLs.
                serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str()).map(|x| x.rsplit('/').next().unwrap_or(x).to_string()).collect::<Vec<_>>().join(", "),
                serde_json::Value::String(x) => x.clone(),
                other => other.to_string(),
            };
            let vals: BTreeMap<&String, String> =
                per_env.iter().filter_map(|(e, n)| Some((e, shown(attr(n, attr_key)?)))).collect();
            let distinct: BTreeSet<&String> = vals.values().collect();
            if distinct.len() > 1 {
                let parts: Vec<String> = vals.iter().map(|(e, v)| format!("{v} in {e}")).collect();
                out.push(format!("{name}: `{attr_key}` is {}.", parts.join(", ")));
            }
        }
    }
    out
}

fn join(items: &[&String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        _ => format!("{} and {}", items[..items.len() - 1].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), items[items.len() - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_graph;
    use crate::infer::infer;

    fn named(name: &str) -> Node {
        let mut n = Node::new("x", NodeKind::LambdaFunction, name);
        n.tags.insert(STACK_TAG.into(), "BillingStack-prod".into());
        n
    }

    #[test]
    fn cloudformation_names_are_cleaned() {
        assert_eq!(display_name(&named("BillingStack-prod-InvoiceProcessor9F3A21-XK2")), "InvoiceProcessor");
        assert_eq!(display_name(&named("BillingStack-prod-InvoicesTable7D1E0A-1ABC")), "InvoicesTable");
        assert_eq!(display_name(&named("billingstack-prod-invoicepdfs3c2b1a-9zz")), "invoicepdfs");
        assert_eq!(display_name(&named("BillingStack-prod-MonthlyClose")), "MonthlyClose");
        assert_eq!(display_name(&named("unrelated-name")), "unrelated-name");
    }

    #[test]
    fn demo_findings_cover_every_rule() {
        let mut g = demo_graph();
        infer(&mut g);
        let rules: BTreeSet<String> = findings(&g).into_iter().map(|f| f.rule).collect();
        for r in ["plaintext_secret", "alarm", "internet_entry", "cross_env_access", "unused_permission"] {
            assert!(rules.contains(r), "missing {r}: {rules:?}");
        }
    }

    #[test]
    fn env_diff_spots_resilience_gaps() {
        let mut g = demo_graph();
        infer(&mut g);
        let d = env_differences(&g, "checkout");
        assert!(d.iter().any(|l| l.contains("multi_az") && l.contains("false in dev")), "{d:?}");
        assert!(d.iter().any(|l| l.contains("checkout-cdn") && l.contains("not in dev")), "{d:?}");
    }
}
