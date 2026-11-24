//! App and environment inference.
//!
//! Signals are applied from strongest to weakest; a node keeps the first one that matches:
//!
//! | method        | app                              | env                               |
//! |---------------|----------------------------------|-----------------------------------|
//! | UserOverride  | always kept                      | always kept                       |
//! | Tag           | `app`, `application`, `awsApplication`… (0.95) | `env`, `environment`, `stage` (0.95) |
//! | Stack         | CloudFormation/CDK stack name (0.8) | env token in stack name (0.8)  |
//! | Naming        | shared name token, ≥2 nodes (0.6) | env token in name (0.7)          |
//! | Containment   | —                                | inherited from parent, e.g. VPC (0.6) |
//! | Graph         | unanimous runtime neighbours (0.5) | unanimous runtime neighbours (0.5) |
//!
//! Apps spread over *communication* edges; environments spread over *containment*
//! (a resource inside `prod-vpc` is prod), then over edges.

use crate::{ArchGraph, Environment, InferenceMethod, Inferred, Layer, Node, NodeId};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const APP_TAGS: [&str; 7] = ["app", "application", "awsapplication", "app-name", "service", "project", "system"];
const ENV_TAGS: [&str; 5] = ["env", "environment", "stage", "tier", "aws:servicecatalog:environment"];
const STACK_TAG: &str = "aws:cloudformation:stack-name";

/// Words that describe *what* a resource is, not *which app* it belongs to.
const GENERIC: [&str; 42] = [
    "aws", "stack", "cdk", "cfn", "lambda", "function", "fn", "func", "handler", "api", "apigw", "gateway", "db", "database", "rds",
    "cluster", "instance", "table", "dynamodb", "ddb", "queue", "sqs", "dlq", "topic", "sns", "bucket", "s3", "role", "policy", "sg",
    "secret", "key", "kms", "service", "svc", "worker", "task", "alb", "lb", "elb", "bus", "default",
];

#[derive(Debug, Default)]
pub struct InferenceReport {
    pub apps: BTreeMap<String, usize>,
    pub by_method: BTreeMap<String, usize>,
    /// Eligible nodes with no app.
    pub unassigned: Vec<NodeId>,
    /// Nodes whose neighbours belong to different apps (shared queues, DBs...).
    pub shared: Vec<(NodeId, Vec<String>)>,
    /// Name prefixes detected as organisation-wide and ignored (e.g. `acme`).
    pub ignored_prefixes: Vec<String>,
}

/// Run inference over the whole graph. Idempotent: previous non-override inferences are recomputed.
pub fn infer(g: &mut ArchGraph) -> InferenceReport {
    let mut report = InferenceReport::default();
    for n in g.nodes_mut() {
        if !is_override(&n.app) {
            n.app = None;
        }
        if !is_override(&n.env) {
            n.env = None;
        }
    }

    // --- 1. tags and stacks
    for n in g.nodes_mut() {
        if n.app.is_none() && app_eligible(n) {
            n.app = app_from_tags(&n.tags)
                .map(|v| inferred(v, InferenceMethod::Tag, 0.95))
                .or_else(|| n.tags.get(STACK_TAG).and_then(|s| app_from_name(s)).map(|v| inferred(v, InferenceMethod::Stack, 0.8)));
        }
        if n.env.is_none() {
            n.env = env_from_tags(&n.tags)
                .map(|e| inferred(e, InferenceMethod::Tag, 0.95))
                .or_else(|| n.tags.get(STACK_TAG).and_then(|s| env_from_name(s)).map(|e| inferred(e, InferenceMethod::Stack, 0.8)))
                .or_else(|| env_from_name(&n.name).map(|e| inferred(e, InferenceMethod::Naming, 0.7)));
        }
    }

    // --- 2. naming clusters (with organisation-prefix detection)
    let token_lists: Vec<(NodeId, Vec<String>)> = g
        .nodes()
        .filter(|n| app_eligible(n) && n.app.is_none())
        .map(|n| (n.id.clone(), meaningful_tokens(&n.name)))
        .filter(|(_, t)| !t.is_empty())
        .collect();
    let prefixes = org_prefixes(&token_lists);
    let candidates: Vec<(NodeId, String)> = token_lists
        .iter()
        .filter_map(|(id, t)| t.iter().find(|tok| !prefixes.contains(*tok)).map(|tok| (id.clone(), tok.clone())))
        .collect();
    let mut cluster_size: HashMap<&str, usize> = HashMap::new();
    for (_, c) in &candidates {
        *cluster_size.entry(c.as_str()).or_default() += 1;
    }
    let naming: HashMap<NodeId, String> =
        candidates.iter().filter(|(_, c)| cluster_size[c.as_str()] >= 2).map(|(id, c)| (id.clone(), c.clone())).collect();
    for n in g.nodes_mut() {
        if let Some(app) = naming.get(&n.id) {
            n.app = Some(inferred(app.clone(), InferenceMethod::Naming, 0.6));
        }
    }
    report.ignored_prefixes = prefixes.into_iter().collect();

    // --- 3. environment through containment (parent chain)
    let parent_env: HashMap<NodeId, Environment> = g.nodes().filter_map(|n| Some((n.id.clone(), n.env.as_ref()?.value.clone()))).collect();
    let parents: HashMap<NodeId, NodeId> = g.nodes().filter_map(|n| Some((n.id.clone(), n.parent.clone()?))).collect();
    for n in g.nodes_mut() {
        if n.env.is_some() {
            continue;
        }
        let mut cur = n.parent.clone();
        let mut hops = 0;
        while let (Some(p), true) = (cur, hops < 8) {
            if let Some(e) = parent_env.get(&p) {
                n.env = Some(inferred(e.clone(), InferenceMethod::Containment, 0.6));
                break;
            }
            cur = parents.get(&p).cloned();
            hops += 1;
        }
    }

    // --- 4. graph propagation until fixpoint
    let mut shared: BTreeMap<NodeId, Vec<String>> = BTreeMap::new();
    for _ in 0..5 {
        let mut changes: Vec<(NodeId, Option<String>, Option<Environment>)> = Vec::new();
        for n in g.nodes() {
            let need_app = n.app.is_none() && app_eligible(n);
            let need_env = n.env.is_none() && n.layer() != Layer::External;
            if !need_app && !need_env {
                continue;
            }
            let neigh: Vec<&Node> = g.runtime_neighbors(&n.id).iter().filter_map(|id| g.node(id)).collect();
            let apps: BTreeSet<String> = neigh.iter().filter_map(|m| m.app.as_ref().map(|a| a.value.clone())).collect();
            let envs: BTreeSet<Environment> = neigh.iter().filter_map(|m| m.env.as_ref().map(|e| e.value.clone())).collect();

            let app = match (need_app, apps.len()) {
                (true, 1) => apps.iter().next().cloned(),
                (true, k) if k > 1 => {
                    shared.insert(n.id.clone(), apps.iter().cloned().collect());
                    None
                }
                _ => None,
            };
            let env = (need_env && envs.len() == 1).then(|| envs.iter().next().cloned()).flatten();
            if app.is_some() || env.is_some() {
                changes.push((n.id.clone(), app, env));
            }
        }
        if changes.is_empty() {
            break;
        }
        for (id, app, env) in changes {
            let n = g.node_mut(&id).expect("exists");
            if let Some(a) = app {
                n.app = Some(inferred(a, InferenceMethod::Graph, 0.5));
                shared.remove(&id);
            }
            if let Some(e) = env {
                n.env = Some(inferred(e, InferenceMethod::Graph, 0.5));
            }
        }
    }

    // --- report
    for n in g.nodes() {
        if let Some(a) = &n.app {
            *report.apps.entry(a.value.clone()).or_default() += 1;
            *report.by_method.entry(format!("{:?}", a.method)).or_default() += 1;
        } else if app_eligible(n) && !shared.contains_key(&n.id) {
            report.unassigned.push(n.id.clone());
        }
    }
    report.shared = shared.into_iter().collect();
    report
}

fn inferred<T>(value: T, method: InferenceMethod, confidence: f32) -> Inferred<T> {
    Inferred { value, method, confidence }
}

fn is_override<T>(x: &Option<Inferred<T>>) -> bool {
    matches!(x, Some(i) if i.method == InferenceMethod::UserOverride)
}

/// Network and scope resources are shared infrastructure, never owned by one app.
fn app_eligible(n: &Node) -> bool {
    !matches!(n.layer(), Layer::Scope | Layer::Network | Layer::External)
}

fn tag<'a>(tags: &'a BTreeMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| tags.iter().find(|(tk, v)| tk.to_ascii_lowercase() == *k && !v.trim().is_empty()).map(|(_, v)| v.as_str()))
}

pub fn app_from_tags(tags: &BTreeMap<String, String>) -> Option<String> {
    let v = tag(tags, &APP_TAGS)?;
    // `awsApplication` holds an AppRegistry ARN: ...:group/<name>/<id>
    let v = match v.strip_prefix("arn:") {
        Some(_) => v.split("/").nth(1).unwrap_or(v),
        None => v,
    };
    Some(kebab(v)).filter(|s| !s.is_empty())
}

pub fn env_from_tags(tags: &BTreeMap<String, String>) -> Option<Environment> {
    let v = tag(tags, &ENV_TAGS)?;
    Some(env_token(&v.to_ascii_lowercase()).unwrap_or_else(|| Environment::Custom(kebab(v))))
}

/// `checkout-service-prod` / `CheckoutStack` → `checkout`
pub fn app_from_name(name: &str) -> Option<String> {
    let t = meaningful_tokens(name);
    (!t.is_empty()).then(|| t.join("-"))
}

pub fn env_from_name(name: &str) -> Option<Environment> {
    tokens(name).iter().find_map(|t| env_token(t))
}

fn env_token(t: &str) -> Option<Environment> {
    Some(match t {
        "prod" | "prd" | "production" | "live" => Environment::Prod,
        "stg" | "stage" | "staging" | "preprod" | "uat" => Environment::Staging,
        "dev" | "develop" | "development" | "sandbox" | "sbx" => Environment::Dev,
        "test" | "tst" | "qa" => Environment::Test,
        _ => return None,
    })
}

/// Lowercase tokens split on separators and camelCase: `CheckoutApi-prod_v2` → [checkout, api, prod, v2]
pub fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in s.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut cur = String::new();
        let mut prev_lower = false;
        for c in part.chars() {
            if c.is_ascii_uppercase() && prev_lower && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = c.is_ascii_lowercase();
            cur.push(c.to_ascii_lowercase());
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

/// Tokens that can identify an app: no env words, generic type words, numbers or generated hashes.
pub fn meaningful_tokens(name: &str) -> Vec<String> {
    tokens(name)
        .into_iter()
        .filter(|t| t.len() >= 2)
        .filter(|t| env_token(t).is_none())
        .filter(|t| !GENERIC.contains(&t.as_str()))
        .filter(|t| !looks_generated(t))
        .collect()
}

/// CloudFormation physical-id suffixes (`1a2b3c4d`), versions (`v2`), plain numbers.
fn looks_generated(t: &str) -> bool {
    let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
    digits == t.len() || (t.len() >= 6 && digits >= 2) || (t.starts_with('v') && digits == t.len() - 1)
}

/// A first token present on most named resources (`acme-…`) is an organisation prefix.
fn org_prefixes(lists: &[(NodeId, Vec<String>)]) -> BTreeSet<String> {
    let mut first: HashMap<&str, usize> = HashMap::new();
    for (_, t) in lists {
        *first.entry(t[0].as_str()).or_default() += 1;
    }
    let total = lists.len();
    first
        .into_iter()
        .filter(|(_, c)| total >= 6 && *c * 2 > total)
        // Only a prefix if the *next* token splits those names into ≥2 real groups
        // (`acme-checkout-*`, `acme-billing-*`). Otherwise it is just a dominant app (`search-*`).
        .filter(|(tok, _)| {
            let mut second: HashMap<&str, usize> = HashMap::new();
            for (_, t) in lists.iter().filter(|(_, t)| t[0] == *tok && t.len() > 1) {
                *second.entry(t[1].as_str()).or_default() += 1;
            }
            second.values().filter(|c| **c >= 2).count() >= 2
        })
        .map(|(t, _)| t.to_string())
        .collect()
}

fn kebab(s: &str) -> String {
    tokens(s).join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgeKind, Evidence, EvidenceKind, NodeKind};

    fn node(id: &str, kind: NodeKind, name: &str, tags: &[(&str, &str)]) -> Node {
        let mut n = Node::new(id, kind, name);
        n.tags = tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        n
    }

    fn link(g: &mut ArchGraph, a: &str, b: &str, kind: EdgeKind) {
        let ev = Evidence { kind: EvidenceKind::Declared, source: "test".into(), detail: None, seen_at: chrono::Utc::now() };
        assert!(g.add_evidence(&a.into(), &b.into(), kind, ev));
    }

    fn app(g: &ArchGraph, id: &str) -> Option<(String, InferenceMethod)> {
        g.node(&id.into())?.app.as_ref().map(|a| (a.value.clone(), a.method))
    }

    #[test]
    fn tokenizer_handles_camel_case_and_hashes() {
        assert_eq!(tokens("CheckoutApi-prod_v2"), vec!["checkout", "api", "prod", "v2"]);
        assert_eq!(meaningful_tokens("CheckoutStack-OrdersTable1A2B3C-XYZ123ABC"), vec!["checkout", "orders"]);
        assert_eq!(app_from_name("checkout-service-prod"), Some("checkout".into()));
    }

    #[test]
    fn tag_beats_stack_beats_naming() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("a", NodeKind::LambdaFunction, "orders-handler", &[("App", "Payments API"), (STACK_TAG, "checkout-prod")]));
        g.upsert_node(node("b", NodeKind::LambdaFunction, "orders-worker", &[(STACK_TAG, "CheckoutStack-prod")]));
        g.upsert_node(node("c", NodeKind::SqsQueue, "orders-queue", &[]));
        g.upsert_node(node("d", NodeKind::DynamoTable, "orders-table", &[]));
        infer(&mut g);
        assert_eq!(app(&g, "a"), Some(("payments-api".into(), InferenceMethod::Tag)));
        assert_eq!(app(&g, "b"), Some(("checkout".into(), InferenceMethod::Stack)));
        assert_eq!(app(&g, "c"), Some(("orders".into(), InferenceMethod::Naming)));
    }

    #[test]
    fn aws_application_arn_is_parsed() {
        let t = BTreeMap::from([("awsApplication".to_string(), "arn:aws:resource-groups:us-east-1:1:group/checkout/0abc".to_string())]);
        assert_eq!(app_from_tags(&t), Some("checkout".into()));
    }

    #[test]
    fn naming_needs_a_cluster_and_skips_org_prefix() {
        let mut g = ArchGraph::new();
        for (i, name) in
            ["acme-checkout-api", "acme-checkout-db", "acme-billing-fn", "acme-billing-queue", "acme-search-fn", "acme-lonely-thing"]
                .iter()
                .enumerate()
        {
            g.upsert_node(node(&format!("n{i}"), NodeKind::LambdaFunction, name, &[]));
        }
        let r = infer(&mut g);
        assert_eq!(r.ignored_prefixes, vec!["acme"]);
        assert_eq!(app(&g, "n0").unwrap().0, "checkout");
        assert_eq!(app(&g, "n3").unwrap().0, "billing");
        assert!(app(&g, "n4").is_none(), "singletons are not an app");
        assert_eq!(r.apps.len(), 2);
    }

    #[test]
    fn env_inherits_through_containment() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("vpc", NodeKind::Vpc, "prod-vpc", &[]));
        let mut sub = node("sub", NodeKind::Subnet, "private-a", &[]);
        sub.parent = Some("vpc".into());
        g.upsert_node(sub);
        let mut f = node("fn", NodeKind::LambdaFunction, "report", &[]);
        f.parent = Some("sub".into());
        g.upsert_node(f);
        infer(&mut g);
        let env = g.node(&"fn".into()).unwrap().env.clone().unwrap();
        assert_eq!(env.value, Environment::Prod);
        assert!(g.node(&"vpc".into()).unwrap().app.is_none(), "network is shared, never owned by an app");
    }

    #[test]
    fn graph_propagation_and_shared_detection() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("co", NodeKind::LambdaFunction, "x", &[("app", "checkout"), ("env", "prod")]));
        g.upsert_node(node("bi", NodeKind::LambdaFunction, "y", &[("app", "billing")]));
        g.upsert_node(node("sec", NodeKind::Secret, "db-creds", &[]));
        g.upsert_node(node("q", NodeKind::SqsQueue, "events", &[]));
        link(&mut g, "co", "sec", EdgeKind::UsesSecret);
        link(&mut g, "co", "q", EdgeKind::Publishes);
        link(&mut g, "bi", "q", EdgeKind::Reads);
        let r = infer(&mut g);
        assert_eq!(app(&g, "sec"), Some(("checkout".into(), InferenceMethod::Graph)));
        assert_eq!(g.node(&"sec".into()).unwrap().env.as_ref().unwrap().value, Environment::Prod);
        assert!(app(&g, "q").is_none());
        assert_eq!(r.shared, vec![(NodeId("q".into()), vec!["billing".into(), "checkout".into()])]);
    }

    #[test]
    fn user_override_survives_reinference() {
        let mut g = ArchGraph::new();
        let mut n = node("a", NodeKind::LambdaFunction, "x", &[("app", "checkout")]);
        n.app = Some(inferred("legacy".into(), InferenceMethod::UserOverride, 1.0));
        g.upsert_node(n);
        infer(&mut g);
        infer(&mut g);
        assert_eq!(app(&g, "a"), Some(("legacy".into(), InferenceMethod::UserOverride)));
    }
}
