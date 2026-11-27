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

/// A policy field that may be one string or a list of them.
fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        _ => vec![],
    }
}

/// A document's statements: IAM accepts one statement object as well as a list.
fn statement_list(doc: &Value) -> Vec<&Value> {
    match doc.get("Statement") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v @ Value::Object(_)) => vec![v],
        _ => vec![],
    }
}

/// Parse a policy document (`{"Statement": [...]}`; Action/Resource may be string or list).
pub fn parse_document(name: &str, doc: &Value) -> Policy {
    Policy {
        name: name.to_string(),
        statements: statement_list(doc)
            .into_iter()
            .map(|s| Statement {
                allow: s.get("Effect").and_then(|e| e.as_str()) != Some("Deny"),
                actions: strings(s.get("Action")),
                not_actions: strings(s.get("NotAction")),
                resources: strings(s.get("Resource")),
                not_resources: strings(s.get("NotResource")),
                conditional: s.get("Condition").is_some(),
            })
            .collect(),
    }
}

/// Who a trust policy lets take a role on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// An AWS service, such as `lambda.amazonaws.com`.
    Service,
    /// A whole account: whichever of its roles and users its own policies allow.
    Account,
    Role,
    /// A user of an account. Only the account is kept: user names can name people (ADR 0003).
    User,
    /// An identity provider: OIDC (GitHub Actions, EKS), SAML or Cognito.
    Federated,
    /// A role or user deleted since the policy was written: IAM shows its unique id instead.
    Deleted,
    Anyone,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Principal {
    pub kind: PrincipalKind,
    /// The service, account, role ARN or provider, as the policy names it.
    pub id: String,
    /// The condition keys the statement checks, never their values: an `sts:ExternalId` is a
    /// shared secret, and an OIDC subject names a repository or a person.
    #[serde(default)]
    pub conditions: Vec<String>,
}

/// Who a role's trust policy (`AssumeRolePolicyDocument`) lets take it on. Deny statements are
/// left out: trust policies rarely have them, and they narrow, never widen, who may.
pub fn trusted_by(doc: &Value) -> Vec<Principal> {
    let mut out: Vec<Principal> = Vec::new();
    for s in statement_list(doc) {
        if s.get("Effect").and_then(|e| e.as_str()) == Some("Deny") {
            continue;
        }
        let mut conditions: Vec<String> = s
            .get("Condition")
            .and_then(|c| c.as_object())
            .into_iter()
            .flat_map(|operators| operators.values())
            .filter_map(|keys| keys.as_object())
            .flat_map(|keys| keys.keys().cloned())
            .collect();
        conditions.sort();
        conditions.dedup();
        for (kind, id) in principals(s.get("Principal")) {
            let p = Principal { kind, id, conditions: conditions.clone() };
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn principals(v: Option<&Value>) -> Vec<(PrincipalKind, String)> {
    match v {
        Some(Value::String(s)) if s == "*" => vec![(PrincipalKind::Anyone, s.clone())],
        Some(Value::Object(by_type)) => {
            by_type.iter().flat_map(|(t, ids)| strings(Some(ids)).into_iter().filter_map(move |id| principal(t, id))).collect()
        }
        _ => vec![],
    }
}

fn principal(principal_type: &str, id: String) -> Option<(PrincipalKind, String)> {
    match principal_type {
        "Service" => Some((PrincipalKind::Service, id)),
        "Federated" => Some((PrincipalKind::Federated, id)),
        "AWS" => Some(aws_principal(id)),
        _ => None, // CanonicalUser only appears in S3 bucket policies.
    }
}

fn aws_principal(id: String) -> (PrincipalKind, String) {
    if id == "*" {
        return (PrincipalKind::Anyone, id);
    }
    if id.len() == 12 && id.bytes().all(|b| b.is_ascii_digit()) {
        return (PrincipalKind::Account, id);
    }
    let kind = match id.splitn(6, ':').collect::<Vec<_>>()[..] {
        ["arn", _, _, _, account, "root"] => return (PrincipalKind::Account, account.to_string()),
        ["arn", _, _, _, account, resource] if resource.starts_with("user/") || resource.starts_with("federated-user/") => {
            return (PrincipalKind::User, account.to_string())
        }
        // A session's name is often the person's: keep the role it belongs to.
        ["arn", partition, "sts", _, account, resource] if resource.starts_with("assumed-role/") => {
            let role = resource.split('/').nth(1).unwrap_or_default();
            return (PrincipalKind::Role, format!("arn:{partition}:iam::{account}:role/{role}"));
        }
        ["arn", ..] => PrincipalKind::Role,
        _ => PrincipalKind::Deleted,
    };
    (kind, id)
}

/// How much a grant lets its holder do, after IAM's access levels (List and Read are one). Read
/// from the action's name, not from AWS's service reference, so it is a close reading, not IAM's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Read,
    Write,
    /// Changes what something may do: policies, grants, passing a role.
    Permissions,
    /// Every action of the service.
    Full,
}

const READ_VERBS: [&str; 17] = [
    "get",
    "list",
    "describe",
    "query",
    "scan",
    "batchget",
    "head",
    "select",
    "lookup",
    "search",
    "receive",
    "read",
    "view",
    "filter",
    "download",
    "eshttpget",
    "eshttphead",
];
/// Actions that change who may do what: policies, ACLs, grants, passing a role.
const PERMISSION_WORDS: [&str; 5] = ["policy", "acl", "passrole", "permission", "grant"];
/// Names that look like permissions but change settings: scaling, lifecycle and retention
/// policies, network and web ACLs.
const NOT_PERMISSIONS: [&str; 5] = ["scalingpolicy", "lifecyclepolicy", "retentionpolicy", "networkacl", "webacl"];

/// The access levels an action pattern covers. A leading wildcard (`s3:*Object`) covers both
/// reading and writing.
fn levels_of(action: &str) -> Vec<Level> {
    let name = action.split_once(':').map_or(action, |(_, n)| n).to_ascii_lowercase();
    if name == "*" {
        return vec![Level::Full];
    }
    if name.starts_with('*') {
        return vec![Level::Read, Level::Write];
    }
    let reads = READ_VERBS.iter().any(|v| name.starts_with(v));
    if reads {
        vec![Level::Read]
    } else if PERMISSION_WORDS.iter().any(|w| name.contains(w)) && !NOT_PERMISSIONS.iter().any(|w| name.contains(w)) {
        vec![Level::Permissions]
    } else {
        vec![Level::Write]
    }
}

/// What a role may do on one service, from statements that share effect, conditions and
/// resources. Statements on different resources stay apart, so a row never claims a write
/// that only applies elsewhere.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceAccess {
    /// IAM's service prefix (`dynamodb`), or `*` for every service.
    pub service: String,
    pub allow: bool,
    pub levels: Vec<Level>,
    /// The actions as the policies write them.
    pub actions: Vec<String>,
    /// `NotAction`: every action except these.
    #[serde(default)]
    pub except_actions: Vec<String>,
    /// Resources of this scan the grant covers.
    #[serde(default)]
    pub targets: Vec<NodeId>,
    /// Covers every resource of the service (`*`, `arn:aws:s3:::*`, or `NotResource`).
    #[serde(default)]
    pub everything: bool,
    #[serde(default)]
    pub except_resources: Vec<String>,
    /// Resource patterns that match nothing in this scan, as written.
    #[serde(default)]
    pub elsewhere: Vec<String>,
    /// Conditions are reported, not evaluated.
    #[serde(default)]
    pub conditional: bool,
    pub policies: Vec<String>,
}

/// AWS managed policies whose effect their name states: (name, service, level, actions left out).
/// sprawlr does not read AWS managed documents (thousands of them); these few are named so a
/// reader sees who may reach what.
const KNOWN_MANAGED: [(&str, &str, Level, &[&str]); 9] = [
    ("AdministratorAccess", "*", Level::Full, &[]),
    ("PowerUserAccess", "*", Level::Full, &["account:*", "iam:*", "organizations:*"]),
    ("ReadOnlyAccess", "*", Level::Read, &[]),
    ("AmazonS3FullAccess", "s3", Level::Full, &[]),
    ("AmazonS3ReadOnlyAccess", "s3", Level::Read, &[]),
    ("AmazonDynamoDBFullAccess", "dynamodb", Level::Full, &[]),
    ("AmazonDynamoDBReadOnlyAccess", "dynamodb", Level::Read, &[]),
    ("AmazonSQSFullAccess", "sqs", Level::Full, &[]),
    ("AmazonSNSFullAccess", "sns", Level::Full, &[]),
];

/// The IAM service prefixes a node's actions use (`dynamodb` for a table).
fn services_of(n: &Node) -> Vec<&str> {
    let Some(service) = arn_service(&n.id.0) else { return vec![] };
    if n.kind == NodeKind::ApiGateway {
        return vec![service, "execute-api"];
    }
    vec![service]
}

/// The service an ARN names (`s3` in `arn:aws:s3:::reports/*`); none for `*` or anything else.
fn arn_service(arn: &str) -> Option<&str> {
    arn.strip_prefix("arn:").and_then(|r| r.split(':').nth(1))
}

/// Whether a resource pattern can name resources of a service: in a statement on S3 and KMS,
/// the bucket is not a key.
fn names_service(pattern: &str, service: &str) -> bool {
    service == "*" || arn_service(pattern).is_none_or(|s| glob(s, service))
}

/// Every resource a row may reach: `*`, or, for one service, an ARN whose every part after the
/// service is a wildcard (`arn:aws:s3:::*`). For every service at once, only `*`.
fn whole(pattern: &str, service: &str) -> bool {
    pattern == "*"
        || (service != "*" && {
            let parts: Vec<&str> = pattern.split(':').collect();
            parts.len() > 3 && parts[0] == "arn" && parts[3..].iter().all(|p| p.is_empty() || *p == "*") && parts[3..].contains(&"*")
        })
}

/// Nodes of a row's service, without the services its `NotAction` leaves out whole (`iam:*`).
fn in_reach<'a>(g: &'a ArchGraph, service: &'a str, except_actions: &'a [String]) -> impl Iterator<Item = &'a Node> + 'a {
    let gone: Vec<&str> = except_actions.iter().filter_map(|a| a.strip_suffix(":*")).collect();
    g.nodes().filter(move |n| {
        let own = services_of(n);
        n.layer() != crate::Layer::Scope
            && !own.is_empty()
            && (service == "*" || own.contains(&service))
            && !own.iter().any(|s| gone.contains(s))
    })
}

fn matches(pattern: &str, n: &Node) -> bool {
    resource_forms(n).iter().any(|f| glob(pattern, f))
}

/// One statement's rows, one per service its actions name. A service none of the statement's
/// resources can name gets no row: it is granted on nothing.
fn rows_of(g: &ArchGraph, policy: &str, s: &Statement) -> Vec<ServiceAccess> {
    let mut by_service: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if !s.not_actions.is_empty() {
        by_service.entry("*".into()).or_default();
    }
    for a in &s.actions {
        let service = if a == "*" { "*" } else { a.split_once(':').map_or(a.as_str(), |(svc, _)| svc) };
        by_service.entry(service.to_ascii_lowercase()).or_default().push(a.clone());
    }
    by_service
        .into_iter()
        .filter_map(|(service, actions)| {
            let patterns: Vec<&String> = s.resources.iter().filter(|r| names_service(r, &service)).collect();
            let except_resources: Vec<String> = s.not_resources.iter().filter(|r| names_service(r, &service)).cloned().collect();
            if patterns.is_empty() && s.not_resources.is_empty() {
                return None;
            }
            let except_actions = if service == "*" { s.not_actions.clone() } else { vec![] };
            let everything = !s.not_resources.is_empty() || patterns.iter().any(|r| whole(r, &service));
            let reach = || in_reach(g, &service, &except_actions);
            let mut targets: Vec<NodeId> = if s.not_resources.is_empty() {
                reach().filter(|n| patterns.iter().any(|r| matches(r, n))).map(|n| n.id.clone()).collect()
            } else {
                reach().filter(|n| !except_resources.iter().any(|r| matches(r, n))).map(|n| n.id.clone()).collect()
            };
            targets.sort();
            let elsewhere =
                patterns.iter().filter(|r| !whole(r, &service) && !reach().any(|n| matches(r, n))).map(|r| r.to_string()).collect();
            let mut levels: Vec<Level> =
                if actions.is_empty() { vec![Level::Full] } else { actions.iter().flat_map(|a| levels_of(a)).collect() };
            levels.sort();
            levels.dedup();
            Some(ServiceAccess {
                service,
                allow: s.allow,
                levels,
                actions,
                except_actions,
                targets,
                everything,
                except_resources,
                elsewhere,
                conditional: s.conditional,
                policies: vec![policy.to_string()],
            })
        })
        .collect()
}

/// Add a row, or fold it into one that covers the same service, effect, conditions and resources.
fn merge(rows: &mut Vec<ServiceAccess>, row: ServiceAccess) {
    let same = |r: &&mut ServiceAccess| {
        (&r.service, r.allow, r.conditional, r.everything, &r.targets, &r.elsewhere, &r.except_resources, &r.except_actions)
            == (
                &row.service,
                row.allow,
                row.conditional,
                row.everything,
                &row.targets,
                &row.elsewhere,
                &row.except_resources,
                &row.except_actions,
            )
    };
    let Some(r) = rows.iter_mut().find(|r| same(r)) else {
        rows.push(row);
        return;
    };
    for (list, more) in [(&mut r.actions, row.actions), (&mut r.policies, row.policies)] {
        list.extend(more);
        list.sort();
        list.dedup();
    }
    r.levels.extend(row.levels);
    r.levels.sort();
    r.levels.dedup();
}

/// A deny of every action of a service on a resource takes it out of what the allows reach. A
/// partial deny (`s3:DeleteObject`) leaves the resource reachable, and stays listed as a deny.
fn strip_denied(rows: &mut [ServiceAccess]) {
    let denied: Vec<(String, NodeId)> = rows
        .iter()
        .filter(|r| !r.allow && !r.conditional && r.except_actions.is_empty() && r.levels.contains(&Level::Full))
        .flat_map(|r| r.targets.iter().map(|t| (r.service.clone(), t.clone())))
        .collect();
    for r in rows.iter_mut().filter(|r| r.allow) {
        let service = r.service.clone();
        r.targets.retain(|t| !denied.iter().any(|(s, d)| d == t && (s == "*" || *s == service)));
    }
}

/// What a role may do, by service, against the resources of the graph: its own and customer
/// managed policies, plus the AWS managed policies whose name states their effect. Every row
/// names the resources of the graph it reaches, service-wide grants included, so "who may reach
/// this" is a lookup.
pub fn access_of(g: &ArchGraph, policies: &[Policy], aws_managed: &[String]) -> Vec<ServiceAccess> {
    let mut rows = Vec::new();
    for p in policies {
        for s in &p.statements {
            for row in rows_of(g, &p.name, s) {
                merge(&mut rows, row);
            }
        }
    }
    for name in aws_managed {
        let Some((_, service, level, except)) = KNOWN_MANAGED.iter().find(|(n, ..)| n == name) else { continue };
        let except_actions: Vec<String> = except.iter().map(|a| a.to_string()).collect();
        let mut targets: Vec<NodeId> = in_reach(g, service, &except_actions).map(|n| n.id.clone()).collect();
        targets.sort();
        merge(
            &mut rows,
            ServiceAccess {
                service: service.to_string(),
                allow: true,
                levels: vec![*level],
                except_actions,
                targets,
                everything: true,
                policies: vec![name.clone()],
                ..Default::default()
            },
        );
    }
    strip_denied(&mut rows);
    rows
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

/// An action as IAM names it, the connection it grants, and the kinds of node it applies to.
type Grantable = (&'static str, EdgeKind, fn(&NodeKind) -> bool);

fn catalogue() -> Vec<Grantable> {
    use EdgeKind::*;
    fn s3(k: &NodeKind) -> bool {
        *k == NodeKind::S3Bucket
    }
    fn sqs(k: &NodeKind) -> bool {
        *k == NodeKind::SqsQueue
    }
    fn sns(k: &NodeKind) -> bool {
        *k == NodeKind::SnsTopic
    }
    fn ddb(k: &NodeKind) -> bool {
        *k == NodeKind::DynamoTable
    }
    fn lam(k: &NodeKind) -> bool {
        *k == NodeKind::LambdaFunction
    }
    fn sfn(k: &NodeKind) -> bool {
        *k == NodeKind::StateMachine
    }
    fn bus(k: &NodeKind) -> bool {
        *k == NodeKind::EventBus
    }
    fn kin(k: &NodeKind) -> bool {
        *k == NodeKind::KinesisStream
    }
    fn sec(k: &NodeKind) -> bool {
        *k == NodeKind::Secret
    }
    fn ssm(k: &NodeKind) -> bool {
        *k == NodeKind::SsmParameter
    }
    fn kms(k: &NodeKind) -> bool {
        *k == NodeKind::KmsKey
    }
    fn api(k: &NodeKind) -> bool {
        *k == NodeKind::ApiGateway
    }
    fn os(k: &NodeKind) -> bool {
        matches!(k, NodeKind::Other(t) if t.starts_with("AWS::OpenSearchService") || t.starts_with("AWS::Elasticsearch"))
    }
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
            vec![format!("arn:aws:execute-api:{region}:{acct}:{api_id}/x/GET/x"), id.clone(), format!("{id}/x")]
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
                let wildcard = s.resources.iter().any(|r| r == "*" || r.ends_with(":*") || r == &format!("arn:aws:{service}:::*"))
                    || !s.not_resources.is_empty();
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
                    let e = grants.entry((t.id.clone(), *kind)).or_insert_with(|| Grant {
                        to: t.id.clone(),
                        kind: *kind,
                        policy: p.name.clone(),
                        actions: BTreeSet::new(),
                    });
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

/// AWS managed policy names stored on a role node by the scanner.
fn aws_managed_of(role: &Node) -> Vec<String> {
    role.attrs.get("aws_managed_policies").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// Keep on every role whose policies were read what it may do (`attrs.access`), for the
/// role's own panel and for "who IAM lets in" on the resources it reaches.
fn describe_roles(g: &mut ArchGraph) {
    let read = |n: &&Node| n.kind == NodeKind::IamRole && n.attrs.get("policies").is_some();
    let access: Vec<(NodeId, Vec<ServiceAccess>)> =
        g.nodes().filter(read).map(|r| (r.id.clone(), access_of(g, &policies_of(r), &aws_managed_of(r)))).collect();
    for (id, rows) in access {
        if let Some(n) = g.node_mut(&id) {
            n.attrs["access"] = serde_json::to_value(rows).unwrap_or_default();
        }
    }
}

/// Add Permitted edges from every compute resource to what its role allows, and record broad
/// grants on the resource (`attrs.broad_permissions`). Each role also keeps what it may do,
/// by service (`attrs.access`). Returns how many edges were added.
pub fn apply(g: &mut ArchGraph) -> usize {
    describe_roles(g);
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
        let ev =
            Evidence { kind: EvidenceKind::Declared, source: "ec2:instance_profile".into(), detail: None, seen_at: chrono::Utc::now() };
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
        let edges: BTreeSet<(String, EdgeKind)> = g
            .edges()
            .filter(|(f, _, _)| f.kind == NodeKind::Ec2Instance && f.id.0.ends_with("i-1"))
            .map(|(_, t, e)| (t.name.clone(), e.kind))
            .collect();
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
        let kinds: BTreeSet<(String, EdgeKind)> =
            g.edges().filter(|(_, t, _)| t.kind == NodeKind::S3Bucket).map(|(_, t, e)| (t.name.clone(), e.kind)).collect();
        assert!(kinds.contains(&("reports".into(), EdgeKind::Reads)));
        assert!(!kinds.contains(&("reports".into(), EdgeKind::Writes)), "denied: {kinds:?}");
        assert!(kinds.contains(&("reports-output".into(), EdgeKind::Writes)));
    }

    #[test]
    fn a_trust_policy_names_services_accounts_and_providers_with_condition_keys_only() {
        let doc = json!({"Statement": [
            {"Effect": "Allow", "Principal": {"Service": ["lambda.amazonaws.com", "edgelambda.amazonaws.com"]}, "Action": "sts:AssumeRole"},
            {"Effect": "Allow", "Principal": {"AWS": "arn:aws:iam::444455556666:root"}, "Action": "sts:AssumeRole",
             "Condition": {"StringEquals": {"sts:ExternalId": "s3cr3t-value"}}},
            {"Effect": "Allow", "Principal": {"Federated": "arn:aws:iam::1:oidc-provider/token.actions.githubusercontent.com"},
             "Action": "sts:AssumeRoleWithWebIdentity",
             "Condition": {"StringLike": {"token.actions.githubusercontent.com:sub": "repo:acme/shop:*"},
                           "StringEquals": {"token.actions.githubusercontent.com:aud": "sts.amazonaws.com"}}}
        ]});
        let who = trusted_by(&doc);
        let kinds: Vec<(PrincipalKind, &str)> = who.iter().map(|p| (p.kind, p.id.as_str())).collect();
        assert_eq!(
            kinds,
            vec![
                (PrincipalKind::Service, "edgelambda.amazonaws.com"),
                (PrincipalKind::Service, "lambda.amazonaws.com"),
                (PrincipalKind::Account, "444455556666"),
                (PrincipalKind::Federated, "arn:aws:iam::1:oidc-provider/token.actions.githubusercontent.com"),
            ]
        );
        assert_eq!(who[2].conditions, vec!["sts:ExternalId"]);
        assert_eq!(who[3].conditions, vec!["token.actions.githubusercontent.com:aud", "token.actions.githubusercontent.com:sub"]);
        let kept = serde_json::to_string(&who).unwrap();
        assert!(!kept.contains("s3cr3t") && !kept.contains("acme/shop"), "condition values are never kept: {kept}");
    }

    #[test]
    fn trusted_users_keep_only_their_account_and_deleted_principals_say_so() {
        let doc = json!({"Statement": {"Effect": "Allow", "Action": "sts:AssumeRole", "Principal": {"AWS": [
            "arn:aws:iam::444455556666:role/deployer",
            "arn:aws:iam::444455556666:user/jane.doe",
            "555566667777",
            "AROAEXAMPLEUNIQUEID",
            "*"
        ]}}});
        let who: Vec<(PrincipalKind, String)> = trusted_by(&doc).into_iter().map(|p| (p.kind, p.id)).collect();
        assert!(who.contains(&(PrincipalKind::Role, "arn:aws:iam::444455556666:role/deployer".into())));
        assert!(who.contains(&(PrincipalKind::User, "444455556666".into())), "{who:?}");
        assert!(who.contains(&(PrincipalKind::Account, "555566667777".into())));
        assert!(who.contains(&(PrincipalKind::Deleted, "AROAEXAMPLEUNIQUEID".into())));
        assert!(who.contains(&(PrincipalKind::Anyone, "*".into())));
        assert!(!format!("{who:?}").contains("jane"), "user names can name people");
    }

    #[test]
    fn sessions_and_federated_users_keep_only_their_role_or_account() {
        let doc = json!({"Statement": {"Effect": "Allow", "Action": "sts:AssumeRole", "Principal": {"AWS": [
            "arn:aws:sts::444455556666:assumed-role/AWSReservedSSO_Admin_0123/jane.doe@example.com",
            "arn:aws:sts::444455556666:federated-user/jane.doe"
        ]}}});
        let who: Vec<(PrincipalKind, String)> = trusted_by(&doc).into_iter().map(|p| (p.kind, p.id)).collect();
        assert!(who.contains(&(PrincipalKind::Role, "arn:aws:iam::444455556666:role/AWSReservedSSO_Admin_0123".into())), "{who:?}");
        assert!(who.contains(&(PrincipalKind::User, "444455556666".into())), "{who:?}");
        assert!(!format!("{who:?}").contains("jane"), "session and federated user names can name people: {who:?}");
    }

    fn with(g: &mut ArchGraph, id: &str, kind: NodeKind) {
        g.upsert_node(node(id, kind));
    }

    #[test]
    fn a_statement_on_several_services_gives_each_service_only_its_own_resources() {
        let mut g = graph();
        with(&mut g, "arn:aws:kms:us-east-1:1:key/abc", NodeKind::KmsKey);
        let access = |doc: Value| access_of(&g, &[parse_document("p", &doc)], &[]);

        let named = access(json!({"Statement": {"Effect": "Allow", "Action": ["s3:GetObject", "kms:Decrypt"],
            "Resource": ["arn:aws:s3:::reports/*", "arn:aws:kms:us-east-1:1:key/abc"]}}));
        let (s3, kms) = (row(&named, "s3", true), row(&named, "kms", true));
        assert_eq!((s3.targets.clone(), s3.elsewhere.clone()), (vec![NodeId::from("arn:aws:s3:::reports")], vec![]));
        assert_eq!((kms.targets.clone(), kms.elsewhere.clone()), (vec![NodeId::from("arn:aws:kms:us-east-1:1:key/abc")], vec![]));

        let every_bucket = access(json!({"Statement": {"Effect": "Allow", "Action": ["s3:GetObject", "kms:Decrypt"],
            "Resource": ["arn:aws:s3:::*", "arn:aws:kms:us-east-1:1:key/abc"]}}));
        assert!(row(&every_bucket, "s3", true).everything);
        assert!(!row(&every_bucket, "kms", true).everything, "every bucket is not every key");

        let all_on_buckets = access(json!({"Statement": {"Effect": "Allow", "Action": "*", "Resource": "arn:aws:s3:::*"}}));
        let all = row(&all_on_buckets, "*", true);
        assert!(!all.everything, "every action on buckets is not every resource");
        assert_eq!(all.targets.len(), 3, "the three buckets, nothing else: {:?}", all.targets);

        let tables = access(json!({"Statement": {"Effect": "Allow", "Action": ["dynamodb:GetItem", "dynamodb:PutItem"],
            "Resource": "arn:aws:dynamodb:us-east-1:1:table/orders"}}));
        assert_eq!(row(&tables, "dynamodb", true).levels, vec![Level::Read, Level::Write]);
    }

    #[test]
    fn full_denies_and_exceptions_take_resources_out_of_what_a_role_reaches() {
        let mut g = graph();
        with(&mut g, "arn:aws:kms:us-east-1:1:key/abc", NodeKind::KmsKey);
        with(&mut g, "arn:aws:kms:us-east-1:1:key/def", NodeKind::KmsKey);
        let access = |doc: Value| access_of(&g, &[parse_document("p", &doc)], &[]);
        let names = |a: &ServiceAccess| a.targets.iter().map(|t| t.0.rsplit([':', '/']).next().unwrap().to_string()).collect::<Vec<_>>();

        let keys = access(json!({"Statement": [
            {"Effect": "Allow", "Action": "kms:*", "Resource": "*"},
            {"Effect": "Deny", "Action": "kms:*", "Resource": "arn:aws:kms:us-east-1:1:key/abc"}
        ]}));
        assert_eq!(names(row(&keys, "kms", true)), vec!["def"]);

        let but_one = access(json!({"Statement": {"Effect": "Allow", "Action": "*", "NotResource": "arn:aws:s3:::reports"}}));
        let reach = names(row(&but_one, "*", true));
        assert!(!reach.contains(&"reports".to_string()) && reach.contains(&"reports-output".to_string()), "{reach:?}");

        let but_tables = access(json!({"Statement": {"Effect": "Allow", "NotAction": "dynamodb:*", "Resource": "*"}}));
        let reach = names(row(&but_tables, "*", true));
        assert!(!reach.contains(&"orders".to_string()) && reach.contains(&"reports".to_string()), "{reach:?}");

        let admin = access(json!({"Statement": {"Effect": "Allow", "Action": "*", "Resource": "*"}}));
        assert!(names(row(&admin, "*", true)).contains(&"orders".to_string()), "every resource of the scan is named");
    }

    #[test]
    fn api_gateway_management_actions_reach_the_api_itself() {
        let mut g = graph();
        with(&mut g, "arn:aws:apigateway:us-east-1::/apis/a1b2", NodeKind::ApiGateway);
        let access = access_of(
            &g,
            &[parse_document(
                "p",
                &json!({"Statement": [
                    {"Effect": "Allow", "Action": "apigateway:GET", "Resource": "arn:aws:apigateway:us-east-1::/apis/a1b2"},
                    {"Effect": "Allow", "Action": "execute-api:Invoke", "Resource": "arn:aws:execute-api:us-east-1:1:a1b2/*"}
                ]}),
            )],
            &[],
        );
        for service in ["apigateway", "execute-api"] {
            let r = row(&access, service, true);
            assert_eq!((r.targets.len(), r.elsewhere.len()), (1, 0), "{service}: {r:?}");
        }
    }

    #[test]
    fn access_levels_follow_what_the_action_changes() {
        for (action, level) in [
            ("dynamodb:GetItem", Level::Read),
            ("es:ESHttpGet", Level::Read),
            ("logs:PutRetentionPolicy", Level::Write),
            ("application-autoscaling:PutScalingPolicy", Level::Write),
            ("ec2:ReplaceNetworkAclEntry", Level::Write),
            ("s3:PutBucketPolicy", Level::Permissions),
            ("s3:PutObjectAcl", Level::Permissions),
            ("iam:PassRole", Level::Permissions),
            ("kms:CreateGrant", Level::Permissions),
            ("lambda:AddPermission", Level::Permissions),
        ] {
            assert_eq!(levels_of(action), vec![level], "{action}");
        }
    }

    fn row<'a>(access: &'a [ServiceAccess], service: &str, allow: bool) -> &'a ServiceAccess {
        access.iter().find(|a| a.service == service && a.allow == allow).unwrap_or_else(|| panic!("no {service} row in {access:?}"))
    }

    #[test]
    fn a_role_s_access_is_grouped_by_service_and_names_the_resources_it_reaches() {
        let g = graph();
        let role = g.node(&"arn:aws:iam::1:role/worker".into()).unwrap();
        let access = access_of(&g, &policies_of(role), &[]);
        let targets = |a: &ServiceAccess| a.targets.iter().map(|t| t.0.rsplit([':', '/']).next().unwrap().to_string()).collect::<Vec<_>>();

        let reads = access.iter().find(|a| a.service == "s3" && a.actions.contains(&"s3:GetObject".to_string())).unwrap();
        assert_eq!(reads.levels, vec![Level::Read]);
        assert_eq!(targets(reads), vec!["reports"], "not reports-output, not logs");
        let writes = access.iter().find(|a| a.service == "s3" && a.actions == vec!["s3:PutObject"]).unwrap();
        assert_eq!(writes.levels, vec![Level::Write]);
        assert_eq!(targets(writes), vec!["reports-output"]);

        let queue = row(&access, "sqs", true);
        assert_eq!(queue.levels, vec![Level::Read, Level::Write], "ReceiveMessage reads, DeleteMessage writes");
        assert_eq!(targets(queue), vec!["report-jobs"]);

        let tables = row(&access, "dynamodb", true);
        assert_eq!(tables.levels, vec![Level::Full]);
        assert!(tables.everything, "dynamodb:* on *");
        assert_eq!(targets(tables), vec!["orders"], "every table of the scan is named, for who-is-allowed-in");

        let secrets = row(&access, "secretsmanager", true);
        assert_eq!(targets(secrets), vec!["db-AbC123"]);
        assert_eq!(secrets.policies, vec!["worker-inline"]);
    }

    #[test]
    fn denies_everything_except_and_unknown_resources_are_kept_apart() {
        let g = graph();
        let policies = vec![parse_document(
            "p",
            &json!({"Statement": [
                {"Effect": "Allow", "NotAction": "iam:*", "Resource": "*"},
                {"Effect": "Deny", "Action": "s3:DeleteObject", "Resource": "arn:aws:s3:::reports/*"},
                {"Effect": "Allow", "Action": "iam:PassRole", "Resource": "arn:aws:iam::1:role/worker"},
                {"Effect": "Allow", "Action": "sqs:SendMessage", "Resource": "arn:aws:sqs:us-east-1:1:not-in-this-scan", "Condition": {"Bool": {}}}
            ]}),
        )];
        let access = access_of(&g, &policies, &[]);
        let all = row(&access, "*", true);
        assert_eq!((all.levels.clone(), all.everything, all.except_actions.clone()), (vec![Level::Full], true, vec!["iam:*".to_string()]));
        let deny = row(&access, "s3", false);
        assert_eq!(deny.targets, vec![NodeId::from("arn:aws:s3:::reports")]);
        assert_eq!(row(&access, "iam", true).levels, vec![Level::Permissions], "passing a role changes what something may do");
        let elsewhere = row(&access, "sqs", true);
        assert!(elsewhere.targets.is_empty() && elsewhere.conditional);
        assert_eq!(elsewhere.elsewhere, vec!["arn:aws:sqs:us-east-1:1:not-in-this-scan"]);
    }

    #[test]
    fn well_known_aws_managed_policies_count_as_what_their_names_say() {
        let g = graph();
        let power = access_of(&g, &[], &["PowerUserAccess".into()]);
        let all = row(&power, "*", true);
        assert_eq!(all.except_actions, vec!["account:*", "iam:*", "organizations:*"]);
        assert!(all.targets.iter().all(|t| !t.0.starts_with("arn:aws:iam::")), "PowerUserAccess leaves IAM alone: {:?}", all.targets);
        let access = access_of(&g, &[], &["AdministratorAccess".into(), "AmazonS3ReadOnlyAccess".into(), "SomeTeamPolicy".into()]);
        let admin = row(&access, "*", true);
        assert_eq!(
            (admin.levels.clone(), admin.everything, admin.policies.clone()),
            (vec![Level::Full], true, vec!["AdministratorAccess".to_string()])
        );
        let s3 = row(&access, "s3", true);
        assert_eq!(s3.levels, vec![Level::Read]);
        assert_eq!(s3.targets.len(), 3, "every bucket of the scan");
        assert_eq!(access.len(), 2, "a managed policy sprawlr doesn't know adds nothing: {access:?}");
    }

    #[test]
    fn apply_keeps_what_each_scanned_role_allows_on_the_role() {
        let mut g = graph();
        let mut placeholder = node("arn:aws:iam::1:role/seen-only-as-a-reference", NodeKind::IamRole);
        placeholder.attrs = json!({"placeholder": true});
        g.upsert_node(placeholder);
        apply(&mut g);
        let worker = g.node(&"arn:aws:iam::1:role/worker".into()).unwrap();
        assert!(worker.attrs["access"].as_array().is_some_and(|a| a.len() >= 5), "{}", worker.attrs["access"]);
        let unread = g.node(&"arn:aws:iam::1:role/seen-only-as-a-reference".into()).unwrap();
        assert!(unread.attrs.get("access").is_none(), "a role whose policies were not read allows nothing we know of");
    }

    #[test]
    fn parses_single_statement_objects() {
        let p = parse_document(
            "x",
            &json!({"Statement": {"Effect": "Allow", "Action": "sns:Publish", "Resource": "*", "Condition": {"StringEquals": {}}}}),
        );
        assert_eq!(p.statements.len(), 1);
        assert!(p.statements[0].conditional);
    }
}
