//! IAM policy analysis: what each compute resource is *allowed* to reach.
//!
//! The scanner stores each role's statements on the role node (`attrs.policies`). For every
//! compute resource that assumes a role (Lambda, ECS task, EC2 instance profile, Step
//! Functions), statements become **Permitted** edges with a direction:
//!
//! | actions | edge |
//! |---|---|
//! | `s3:GetObject`, `dynamodb:Query`, `sqs:ReceiveMessage`, `kinesis:GetRecords` | reads |
//! | `s3:PutObject`, `dynamodb:PutItem` | writes |
//! | `sqs:SendMessage`, `sns:Publish`, `events:PutEvents`, `kinesis:PutRecord` | publishes |
//! | `lambda:InvokeFunction`, `states:StartExecution`, `execute-api:Invoke` | invokes |
//! | `secretsmanager:GetSecretValue`, `ssm:GetParameter` | uses secret |
//! | `kms:Decrypt`, `kms:GenerateDataKey` | encrypted with |
//!
//! A grant that covers a whole service (`s3:*` on `*`, or more than a handful of resources)
//! never becomes edges to everything: it is recorded as a broad permission and reported.

use crate::{ArchGraph, EdgeKind, Evidence, EvidenceKind, Node, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Above this many matching resources a grant is treated as broad.
const MAX_SPECIFIC: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Statement {
    pub allow: bool,
    pub actions: Vec<String>,
    /// `NotAction`: everything except these.
    #[serde(default)]
    pub not_actions: Vec<String>,
    pub resources: Vec<String>,
    #[serde(default)]
    pub not_resources: Vec<String>,
    /// Conditions are recorded, not evaluated.
    #[serde(default)]
    pub conditional: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Policy {
    pub name: String,
    pub statements: Vec<Statement>,
}

/// Parse a policy document (`{"Statement": [...]}`; Action/Resource may be string or list).
pub fn parse_document(name: &str, doc: &Value) -> Policy {
    let list = |v: Option<&Value>| -> Vec<String> {
        match v {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => vec![],
        }
    };
    let stmts = match doc.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(v @ Value::Object(_)) => vec![v.clone()],
        _ => vec![],
    };
    Policy {
        name: name.to_string(),
        statements: stmts
            .iter()
            .map(|s| Statement {
                allow: s.get("Effect").and_then(|e| e.as_str()) != Some("Deny"),
                actions: list(s.get("Action")),
                not_actions: list(s.get("NotAction")),
                resources: list(s.get("Resource")),
                not_resources: list(s.get("NotResource")),
                conditional: s.get("Condition").is_some(),
            })
            .collect(),
    }
}

/// Case-insensitive glob with `*` and `?` (IAM semantics).
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let t: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// (canonical action, edge kind, kinds of node it applies to)
fn catalogue() -> Vec<(&'static str, EdgeKind, fn(&NodeKind) -> bool)> {
    use EdgeKind::*;
    fn s3(k: &NodeKind) -> bool { *k == NodeKind::S3Bucket }
    fn sqs(k: &NodeKind) -> bool { *k == NodeKind::SqsQueue }
    fn sns(k: &NodeKind) -> bool { *k == NodeKind::SnsTopic }
    fn ddb(k: &NodeKind) -> bool { *k == NodeKind::DynamoTable }
    fn lam(k: &NodeKind) -> bool { *k == NodeKind::LambdaFunction }
    fn sfn(k: &NodeKind) -> bool { *k == NodeKind::StateMachine }
    fn bus(k: &NodeKind) -> bool { *k == NodeKind::EventBus }
    fn kin(k: &NodeKind) -> bool { *k == NodeKind::KinesisStream }
    fn sec(k: &NodeKind) -> bool { *k == NodeKind::Secret }
    fn ssm(k: &NodeKind) -> bool { *k == NodeKind::SsmParameter }
    fn kms(k: &NodeKind) -> bool { *k == NodeKind::KmsKey }
    fn api(k: &NodeKind) -> bool { *k == NodeKind::ApiGateway }
    fn os(k: &NodeKind) -> bool { matches!(k, NodeKind::Other(t) if t.starts_with("AWS::OpenSearchService") || t.starts_with("AWS::Elasticsearch")) }
    vec![
        ("s3:GetObject", Reads, s3),
        ("s3:PutObject", Writes, s3),
        ("s3:DeleteObject", Writes, s3),
        ("sqs:ReceiveMessage", Reads, sqs),
        ("sqs:SendMessage", Publishes, sqs),
        ("sns:Publish", Publishes, sns),
        ("dynamodb:GetItem", Reads, ddb),
        ("dynamodb:Query", Reads, ddb),
        ("dynamodb:PutItem", Writes, ddb),
        ("dynamodb:UpdateItem", Writes, ddb),
        ("dynamodb:BatchWriteItem", Writes, ddb),
        ("lambda:InvokeFunction", Invokes, lam),
        ("states:StartExecution", Invokes, sfn),
        ("events:PutEvents", Publishes, bus),
        ("kinesis:PutRecord", Publishes, kin),
        ("kinesis:PutRecords", Publishes, kin),
        ("kinesis:GetRecords", Reads, kin),
        ("secretsmanager:GetSecretValue", UsesSecret, sec),
        ("ssm:GetParameter", UsesSecret, ssm),
        ("ssm:GetParameters", UsesSecret, ssm),
        ("kms:Decrypt", EncryptedWith, kms),
        ("kms:GenerateDataKey", EncryptedWith, kms),
        ("execute-api:Invoke", Invokes, api),
        ("es:ESHttpGet", Reads, os),
        ("es:ESHttpPost", Writes, os),
        ("es:ESHttpPut", Writes, os),
    ]
}

/// The ARN a resource pattern is compared with, for a node.
/// S3 object ARNs (`bucket/key`) and DynamoDB index/stream ARNs refer to their bucket or table.
fn resource_forms(n: &Node) -> Vec<String> {
    let id = n.id.0.clone();
    match n.kind {
        NodeKind::S3Bucket => vec![id.clone(), format!("{id}/x")],
        NodeKind::DynamoTable => vec![id.clone(), format!("{id}/index/x"), format!("{id}/stream/x")],
        NodeKind::LambdaFunction => vec![id.clone(), format!("{id}:x")],
        NodeKind::ApiGateway => {
            // arn:aws:apigateway:<r>::/apis/<id> → arn:aws:execute-api:<r>:<acct>:<id>/*
            let api_id = id.rsplit('/').next().unwrap_or_default().to_string();
            let region = n.region.clone().unwrap_or_default();
            let acct = n.account.clone().unwrap_or_default();
            vec![format!("arn:aws:execute-api:{region}:{acct}:{api_id}/x/GET/x")]
        }
        NodeKind::Other(_) => vec![id.clone(), format!("{id}/x")],
        _ => vec![id],
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub to: NodeId,
    pub kind: EdgeKind,
    pub policy: String,
    pub actions: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Broad {
    /// "read", "write", "publish", "invoke", "use secrets", "use keys"
    pub access: String,
    pub service: String,
    pub pattern: String,
    pub policy: String,
}

fn access_word(k: EdgeKind) -> &'static str {
    match k {
        EdgeKind::Reads => "read",
        EdgeKind::Writes => "write",
        EdgeKind::Publishes => "publish",
        EdgeKind::Invokes => "invoke",
        EdgeKind::UsesSecret => "read secrets",
        EdgeKind::EncryptedWith => "use keys",
        _ => "access",
    }
}

/// What a set of policies allows, against the resources in the graph.
pub fn evaluate(g: &ArchGraph, policies: &[Policy]) -> (Vec<Grant>, Vec<Broad>) {
    let cat = catalogue();
    let mut grants: BTreeMap<(NodeId, EdgeKind), Grant> = BTreeMap::new();
    let mut broad: Vec<Broad> = Vec::new();
    let denies: Vec<&Statement> = policies.iter().flat_map(|p| &p.statements).filter(|s| !s.allow).collect();

    let action_hits = |s: &Statement, canonical: &str| -> bool {
        if !s.not_actions.is_empty() {
            return !s.not_actions.iter().any(|a| glob(a, canonical));
        }
        s.actions.iter().any(|a| glob(a, canonical))
    };
    let resource_hits = |s: &Statement, n: &Node| -> bool {
        let forms = resource_forms(n);
        if !s.not_resources.is_empty() {
            return !s.not_resources.iter().any(|r| forms.iter().any(|f| glob(r, f)));
        }
        s.resources.iter().any(|r| forms.iter().any(|f| glob(r, f)))
    };

    for p in policies {
        for s in p.statements.iter().filter(|s| s.allow) {
            for (canonical, kind, applies) in &cat {
                if !action_hits(s, canonical) {
                    continue;
                }
                let targets: Vec<&Node> = g
                    .nodes()
                    .filter(|n| applies(&n.kind))
                    .filter(|n| resource_hits(s, n))
                    .filter(|n| !denies.iter().any(|d| action_hits(d, canonical) && resource_hits(d, n)))
                    .collect();
                let service = canonical.split(':').next().unwrap_or_default();
                let wildcard = s.resources.iter().any(|r| r == "*" || r.ends_with(":*") || r == &format!("arn:aws:{service}:::*")) || !s.not_resources.is_empty();
                if wildcard || targets.len() > MAX_SPECIFIC {
                    let b = Broad {
                        access: access_word(*kind).into(),
                        service: service.into(),
                        pattern: s.resources.first().cloned().unwrap_or_else(|| "*".into()),
                        policy: p.name.clone(),
                    };
                    if !broad.contains(&b) {
                        broad.push(b);
                    }
                    continue;
                }
                for t in targets {
                    let e = grants.entry((t.id.clone(), *kind)).or_insert_with(|| Grant { to: t.id.clone(), kind: *kind, policy: p.name.clone(), actions: BTreeSet::new() });
                    e.actions.insert((*canonical).to_string());
                }
            }
        }
    }
    (grants.into_values().collect(), broad)
}

/// Policies stored on a role node by the scanner.
pub fn policies_of(role: &Node) -> Vec<Policy> {
    role.attrs.get("policies").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// Add Permitted edges from every compute resource to what its role allows, and record broad
/// grants on the resource (`attrs.broad_permissions`). Returns how many edges were added.
pub fn apply(g: &mut ArchGraph) -> usize {
    // principal (compute) → role
    let assumes: Vec<(NodeId, NodeId)> = g
        .edges()
        .filter(|(_, t, e)| e.kind == EdgeKind::AssumesRole && t.kind == NodeKind::IamRole)
        .map(|(f, t, _)| (f.id.clone(), t.id.clone()))
        .collect();
    let mut added = 0;
    let mut broad_by_node: BTreeMap<NodeId, Vec<Broad>> = BTreeMap::new();
    for (principal, role_id) in assumes {
        let Some(role) = g.node(&role_id) else { continue };
        let policies = policies_of(role);
        if policies.is_empty() {
            continue;
        }
        let role_name = role.name.clone();
        let (grants, broad) = evaluate(g, &policies);
        for gr in grants {
            if gr.to == principal {
                continue;
            }
            let ev = Evidence {
                kind: EvidenceKind::Permitted,
                source: format!("iam:role/{role_name}"),
                detail: Some(format!("{}: {}", gr.policy, gr.actions.iter().cloned().collect::<Vec<_>>().join(", "))),
                seen_at: chrono::Utc::now(),
            };
            if g.add_evidence(&principal, &gr.to, gr.kind, ev) {
                added += 1;
            }
        }
        if !broad.is_empty() {
            broad_by_node.entry(principal).or_default().extend(broad);
        }
    }
    for (id, list) in broad_by_node {
        if let Some(n) = g.node_mut(&id) {
            if !n.attrs.is_object() {
                n.attrs = serde_json::json!({});
            }
            n.attrs["broad_permissions"] = serde_json::to_value(list).unwrap_or_default();
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: &str, kind: NodeKind) -> Node {
        let mut n = Node::new(id, kind, id.rsplit([':', '/']).next().unwrap());
        n.region = Some("us-east-1".into());
        n.account = Some("1".into());
        n
    }

    fn graph() -> ArchGraph {
        let mut g = ArchGraph::new();
        for (id, k) in [
            ("arn:aws:s3:::reports", NodeKind::S3Bucket),
            ("arn:aws:s3:::reports-output", NodeKind::S3Bucket),
            ("arn:aws:s3:::logs", NodeKind::S3Bucket),
            ("arn:aws:sqs:us-east-1:1:report-jobs", NodeKind::SqsQueue),
            ("arn:aws:dynamodb:us-east-1:1:table/orders", NodeKind::DynamoTable),
            ("arn:aws:secretsmanager:us-east-1:1:secret:db-AbC123", NodeKind::Secret),
            ("arn:aws:ec2:us-east-1:1:instance/i-1", NodeKind::Ec2Instance),
        ] {
            g.upsert_node(node(id, k));
        }
        let mut role = node("arn:aws:iam::1:role/worker", NodeKind::IamRole);
        role.name = "worker".into();
        role.attrs = json!({ "policies": [parse_document("worker-inline", &json!({
            "Statement": [
                {"Effect": "Allow", "Action": ["s3:GetObject", "s3:ListBucket"], "Resource": ["arn:aws:s3:::reports", "arn:aws:s3:::reports/*"]},
                {"Effect": "Allow", "Action": "s3:PutObject", "Resource": "arn:aws:s3:::reports-output/*"},
                {"Effect": "Allow", "Action": ["sqs:ReceiveMessage", "sqs:DeleteMessage"], "Resource": "arn:aws:sqs:us-east-1:1:report-jobs"},
                {"Effect": "Allow", "Action": "dynamodb:*", "Resource": "*"},
                {"Effect": "Allow", "Action": "secretsmanager:GetSecretValue", "Resource": "arn:aws:secretsmanager:us-east-1:1:secret:db-*"}
            ]
        }))] });
        g.upsert_node(role);
        let ev = Evidence { kind: EvidenceKind::Declared, source: "ec2:instance_profile".into(), detail: None, seen_at: chrono::Utc::now() };
        g.add_evidence(&"arn:aws:ec2:us-east-1:1:instance/i-1".into(), &"arn:aws:iam::1:role/worker".into(), EdgeKind::AssumesRole, ev);
        g
    }

    #[test]
    fn glob_matches_iam_patterns() {
        assert!(glob("s3:Get*", "s3:GetObject"));
        assert!(glob("arn:aws:s3:::reports/*", "arn:aws:s3:::reports/x"));
        assert!(!glob("arn:aws:s3:::reports/*", "arn:aws:s3:::reports-output/x"));
        assert!(glob("arn:aws:secretsmanager:*:1:secret:db-??????", "arn:aws:secretsmanager:us-east-1:1:secret:db-AbC123"));
        assert!(glob("*", "anything"));
    }

    #[test]
    fn ec2_worker_reads_bucket_and_queue_writes_output() {
        let mut g = graph();
        let added = apply(&mut g);
        let edges: BTreeSet<(String, EdgeKind)> = g.edges().filter(|(f, _, _)| f.kind == NodeKind::Ec2Instance && f.id.0.ends_with("i-1")).map(|(_, t, e)| (t.name.clone(), e.kind)).collect();
        assert!(edges.contains(&("reports".into(), EdgeKind::Reads)), "{edges:?}");
        assert!(edges.contains(&("reports-output".into(), EdgeKind::Writes)), "{edges:?}");
        assert!(edges.contains(&("report-jobs".into(), EdgeKind::Reads)), "consumer of the queue: {edges:?}");
        assert!(edges.contains(&("db-AbC123".into(), EdgeKind::UsesSecret)), "{edges:?}");
        assert!(!edges.iter().any(|(n, _)| n == "logs"), "no grant on logs");
        assert!(!edges.iter().any(|(n, _)| n == "orders"), "dynamodb:* on * is broad, not an edge: {edges:?}");
        assert_eq!(added, 4);
        let broad = &g.node(&"arn:aws:ec2:us-east-1:1:instance/i-1".into()).unwrap().attrs["broad_permissions"];
        assert_eq!(broad[0]["service"], "dynamodb");
    }

    #[test]
    fn deny_and_not_action_are_respected() {
        let mut g = graph();
        let role = g.node_mut(&"arn:aws:iam::1:role/worker".into()).unwrap();
        role.attrs = json!({ "policies": [parse_document("p", &json!({"Statement": [
            {"Effect": "Allow", "Action": "s3:*", "Resource": ["arn:aws:s3:::reports", "arn:aws:s3:::reports/*", "arn:aws:s3:::reports-output/*"]},
            {"Effect": "Deny", "Action": ["s3:PutObject", "s3:DeleteObject"], "Resource": "arn:aws:s3:::reports/*"}
        ]}))] });
        apply(&mut g);
        let kinds: BTreeSet<(String, EdgeKind)> = g.edges().filter(|(_, t, _)| t.kind == NodeKind::S3Bucket).map(|(_, t, e)| (t.name.clone(), e.kind)).collect();
        assert!(kinds.contains(&("reports".into(), EdgeKind::Reads)));
        assert!(!kinds.contains(&("reports".into(), EdgeKind::Writes)), "denied: {kinds:?}");
        assert!(kinds.contains(&("reports-output".into(), EdgeKind::Writes)));
    }

    #[test]
    fn parses_single_statement_objects() {
        let p = parse_document("x", &json!({"Statement": {"Effect": "Allow", "Action": "sns:Publish", "Resource": "*", "Condition": {"StringEquals": {}}}}));
        assert_eq!(p.statements.len(), 1);
        assert!(p.statements[0].conditional);
    }
}
