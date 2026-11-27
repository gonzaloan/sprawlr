//! App and environment inference.
//!
//! Signals are applied from strongest to weakest; a node keeps the first one that matches:
//!
//! | method        | app                              | env                               |
//! |---------------|----------------------------------|-----------------------------------|
//! | UserOverride  | always kept                      | always kept                       |
//! | Tag           | `app`, `application`, `awsApplication`… (0.95) | `env`, `environment`, `stage` (0.95) |
//! | Stack         | CloudFormation/CDK stack name (0.8) | env token in stack name (0.8)  |
//! | Naming        | an app from a tag or stack named in it; else a shared first word (verb + next), ≥2 nodes (0.6) | env token in name, never a domain's extension (0.7) |
//! | Containment   | —                                | inherited from parent, e.g. VPC (0.6) |
//! | Graph         | unanimous runtime neighbours (0.5) | unanimous runtime neighbours (0.5) |
//!
//! Apps spread over *communication* edges; environments spread over *containment*
//! (a resource inside `prod-vpc` is prod), then over edges.

use crate::{ArchGraph, Environment, InferenceMethod, Inferred, Layer, Node, NodeId, NodeKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const APP_TAGS: [&str; 7] = ["app", "application", "awsapplication", "app-name", "service", "project", "system"];
const ENV_TAGS: [&str; 5] = ["env", "environment", "stage", "tier", "aws:servicecatalog:environment"];
const STACK_TAG: &str = "aws:cloudformation:stack-name";

/// Words that describe *what* a resource is, not *which app* it belongs to.
const GENERIC: [&str; 44] = [
    "aws",
    "stack",
    "cdk",
    "cfn",
    "lambda",
    "function",
    "fn",
    "func",
    "handler",
    "api",
    "apigw",
    "gateway",
    "db",
    "database",
    "rds",
    "cluster",
    "instance",
    "table",
    "dynamodb",
    "ddb",
    "queue",
    "sqs",
    "dlq",
    "topic",
    "sns",
    "bucket",
    "s3",
    "role",
    "policy",
    "sg",
    "secret",
    "key",
    "kms",
    "service",
    "svc",
    "worker",
    "task",
    "alb",
    "lb",
    "elb",
    "bus",
    "default",
    "www",
    "cloudfront",
];

/// Words that start a name for what it does (`get-quotes-*`, `sendDailyDigest`): never an app
/// on their own.
const VERBS: [&str; 14] =
    ["get", "put", "post", "list", "send", "process", "handle", "create", "update", "delete", "fetch", "run", "check", "notify"];

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
                .or_else(|| env_from_name(name_words(n)).map(|e| inferred(e, InferenceMethod::Naming, 0.7)));
        }
    }

    // --- 2. names: organisation prefixes set aside, then apps tags and stacks already name,
    // then groups that share a first word
    let token_lists: Vec<Words> = g
        .nodes()
        .filter(|n| app_eligible(n) && n.app.is_none())
        .map(|n| (n.id.clone(), meaningful_tokens(name_words(n))))
        .filter(|(_, t)| !t.is_empty())
        .collect();
    let prefixes = org_prefixes(&token_lists);
    let unprefixed: Vec<Words> = token_lists
        .iter()
        .filter_map(|(id, t)| t.iter().position(|tok| !prefixes.contains(tok)).map(|start| (id.clone(), t[start..].to_vec())))
        .collect();
    let known: Vec<(Vec<String>, String)> = g
        .nodes()
        .filter_map(|n| n.app.as_ref())
        .filter(|a| matches!(a.method, InferenceMethod::Tag | InferenceMethod::Stack | InferenceMethod::UserOverride))
        .map(|a| a.value.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|app| (meaningful_tokens(&app), app))
        .filter(|(t, _)| !t.is_empty())
        .collect();
    let (carried, rest): (Vec<_>, Vec<_>) =
        unprefixed.into_iter().map(|(id, t)| (known_app_in(&t, &known), (id, t))).partition(|(app, _)| app.is_some());
    let mut by_name: HashMap<NodeId, String> = carried.into_iter().filter_map(|(app, (id, _))| Some((id, app?))).collect();
    by_name.extend(cluster_names(&rest.into_iter().map(|(_, w)| w).collect::<Vec<_>>()));
    for n in g.nodes_mut() {
        if let Some(app) = by_name.remove(&n.id) {
            n.app = Some(inferred(app, InferenceMethod::Naming, 0.6));
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

/// Network and scope resources are shared infrastructure, never owned by one app; neither are
/// resources AWS creates for itself (`aws/s3` keys).
fn app_eligible(n: &Node) -> bool {
    !matches!(n.layer(), Layer::Scope | Layer::Network | Layer::External)
        && n.attrs.get("managed_by").and_then(|v| v.as_str()) != Some("AWS")
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

/// The part of a resource's name that can say its app or environment. Resources named by a
/// hostname (`learn.example.dev`) leave out the domain's extension, which is never either;
/// subdomains still count (`dev.example.com`). Other names keep their dots (`orders.dev`).
fn name_words(n: &Node) -> &str {
    match n.kind {
        NodeKind::CloudFront | NodeKind::Route53Zone | NodeKind::S3Bucket | NodeKind::ExternalEndpoint => without_tld(&n.name),
        _ => &n.name,
    }
}

fn without_tld(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((rest, tld)) if (2..=24).contains(&tld.len()) && tld.chars().all(|c| c.is_ascii_alphabetic()) => rest,
        _ => name,
    }
}

/// The known app a name carries: its words in order, earliest in the name, the longer app on a
/// tie (`github-actions-deploy-quiz-guide` → `quiz-guide`). A one-word app only counts at the
/// start of a name, where a name says what it belongs to (`billing-web-assets` is billing's).
fn known_app_in(name: &[String], known: &[(Vec<String>, String)]) -> Option<String> {
    known
        .iter()
        .filter_map(|(words, app)| {
            let at = name.windows(words.len()).position(|w| w == words.as_slice())?;
            (at == 0 || words.len() > 1).then_some((at, words.len(), app))
        })
        .min_by_key(|(at, len, _)| (*at, std::cmp::Reverse(*len)))
        .map(|(_, _, app)| app.clone())
}

/// A resource and the meaningful words of its name.
type Words = (NodeId, Vec<String>);

/// Apps from groups of at least two resources that share their first word. A name that starts
/// with a verb is grouped by the verb and what follows (`get-quotes-*`, apart from
/// `getWeatherReport`), since the verb alone says what a resource does, not what it is part of.
fn cluster_names(candidates: &[Words]) -> HashMap<NodeId, String> {
    let key = |t: &[String]| match t {
        [verb, what, ..] if VERBS.contains(&verb.as_str()) => format!("{verb}-{what}"),
        _ => t[0].clone(),
    };
    let mut groups: BTreeMap<String, Vec<&NodeId>> = BTreeMap::new();
    for (id, t) in candidates {
        groups.entry(key(t)).or_default().push(id);
    }
    groups
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 2)
        .flat_map(|(app, ids)| ids.into_iter().map(move |id| (id.clone(), app.clone())))
        .collect()
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
    use crate::{EdgeKind, Evidence, EvidenceKind};

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
    fn a_site_s_domain_extension_is_not_an_environment() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("cdn", NodeKind::CloudFront, "learn.example.dev", &[]));
        g.upsert_node(node("api", NodeKind::ApiGateway, "learn", &[]));
        g.upsert_node(node("dev-cdn", NodeKind::CloudFront, "dev.example.com", &[]));
        g.upsert_node(node("stg-cdn", NodeKind::CloudFront, "api.staging.example.com", &[]));
        g.upsert_node(node("table", NodeKind::DynamoTable, "orders.dev", &[]));
        link(&mut g, "cdn", "api", EdgeKind::RoutesTo);
        infer(&mut g);
        let env = |id: &str| g.node(&id.into()).unwrap().env.as_ref().map(|e| e.value.clone());
        assert_eq!(env("cdn"), None);
        assert_eq!(env("api"), None, "nor is what the site calls");
        assert_eq!(env("dev-cdn"), Some(Environment::Dev), "a subdomain still counts");
        assert_eq!(env("stg-cdn"), Some(Environment::Staging));
        assert_eq!(env("table"), Some(Environment::Dev), "only hostnames have an extension");
    }

    #[test]
    fn names_that_carry_a_known_app_join_it_by_its_exact_name() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("sync", NodeKind::LambdaFunction, "aws-quiz-guide-sync", &[(STACK_TAG, "aws-quiz-guide")]));
        g.upsert_node(node("progress", NodeKind::DynamoTable, "aws-quiz-guide-progress", &[]));
        g.upsert_node(node("deploy", NodeKind::IamRole, "github-actions-deploy-quiz-guide", &[]));
        g.upsert_node(node("pay", NodeKind::LambdaFunction, "x", &[("App", "Payments API")]));
        g.upsert_node(node("events", NodeKind::SqsQueue, "payments-api-events", &[]));
        infer(&mut g);
        assert_eq!(app(&g, "progress"), Some(("quiz-guide".into(), InferenceMethod::Naming)));
        assert_eq!(app(&g, "deploy"), Some(("quiz-guide".into(), InferenceMethod::Naming)), "a two-word app counts anywhere");
        assert_eq!(app(&g, "events"), Some(("payments-api".into(), InferenceMethod::Naming)), "the app's own name, not its words");
    }

    #[test]
    fn a_one_word_known_app_counts_only_at_the_start_of_a_name() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("b", NodeKind::LambdaFunction, "billing-fn", &[("app", "billing")]));
        g.upsert_node(node("w", NodeKind::LambdaFunction, "web-fn", &[("app", "web")]));
        g.upsert_node(node("q", NodeKind::SqsQueue, "billing-web-assets", &[]));
        g.upsert_node(node("o", NodeKind::S3Bucket, "assets-web", &[]));
        infer(&mut g);
        assert_eq!(app(&g, "q"), Some(("billing".into(), InferenceMethod::Naming)));
        assert!(app(&g, "o").is_none());
    }

    #[test]
    fn an_organisation_prefix_is_set_aside_before_known_apps() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("tools", NodeKind::LambdaFunction, "tools", &[(STACK_TAG, "acme")]));
        for (i, name) in
            ["acme-checkout-api", "acme-checkout-db", "acme-billing-fn", "acme-billing-queue", "acme-search-fn", "acme-lonely-thing"]
                .iter()
                .enumerate()
        {
            g.upsert_node(node(&format!("n{i}"), NodeKind::LambdaFunction, name, &[]));
        }
        infer(&mut g);
        assert_eq!(app(&g, "n0").unwrap().0, "checkout");
        assert_eq!(app(&g, "n2").unwrap().0, "billing");
    }

    #[test]
    fn a_name_that_starts_with_a_verb_is_grouped_by_what_follows() {
        let mut g = ArchGraph::new();
        let names = ["get-quotes-cfn-exec", "get-quotes-gh-plan", "get-quotes-gh-apply", "getWeatherReport-role-x9k2m7qp"];
        for (i, name) in names.iter().enumerate() {
            g.upsert_node(node(&format!("n{i}"), NodeKind::IamRole, name, &[]));
        }
        infer(&mut g);
        assert_eq!(app(&g, "n0").unwrap().0, "get-quotes");
        assert_eq!(app(&g, "n2").unwrap().0, "get-quotes");
        assert!(app(&g, "n3").is_none(), "getWeatherReport is not part of get-quotes");
    }

    #[test]
    fn one_entity_named_most_often_does_not_split_an_app() {
        let mut g = ArchGraph::new();
        let names = ["shop-orders-table", "shop-orders-queue", "shop-orders-fn", "shop-payments-fn", "shop-refunds-fn"];
        for (i, name) in names.iter().enumerate() {
            g.upsert_node(node(&format!("n{i}"), NodeKind::LambdaFunction, name, &[]));
        }
        infer(&mut g);
        for i in 0..names.len() {
            assert_eq!(app(&g, &format!("n{i}")).unwrap().0, "shop");
        }
    }

    #[test]
    fn a_naming_cluster_is_named_by_its_first_word() {
        let mut g = ArchGraph::new();
        g.upsert_node(node("cdn", NodeKind::CloudFront, "docs.example.dev", &[]));
        g.upsert_node(node("site", NodeKind::S3Bucket, "docs-example-111122223333", &[]));
        g.upsert_node(node("r1", NodeKind::IamRole, "AmplifySSRLoggingRole-1a2b3c4d-1111-2222-3333-444455556666", &[]));
        g.upsert_node(node("r2", NodeKind::IamRole, "AmplifySSRLoggingRole-5e6f7a8b-1111-2222-3333-444455556666", &[]));
        infer(&mut g);
        assert_eq!(app(&g, "cdn").unwrap().0, "docs");
        assert_eq!(app(&g, "site").unwrap().0, "docs");
        assert_eq!(app(&g, "r1").unwrap().0, "amplify");
    }

    #[test]
    fn aws_managed_resources_belong_to_no_app() {
        let mut g = ArchGraph::new();
        for (id, name) in [("k1", "aws/ebs"), ("k2", "aws/ebs"), ("k3", "aws/backup")] {
            let mut k = node(id, NodeKind::KmsKey, name, &[]);
            k.attrs = serde_json::json!({ "managed_by": "AWS" });
            g.upsert_node(k);
        }
        g.upsert_node(node("role", NodeKind::IamRole, "backup-vault-role", &[]));
        infer(&mut g);
        assert!(app(&g, "k1").is_none());
        assert!(app(&g, "k3").is_none());
        assert!(app(&g, "role").is_none());
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
