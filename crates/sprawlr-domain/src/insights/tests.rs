use super::*;
use crate::analysis::display_name;
use crate::fixtures::demo_graph;
use crate::infer::infer;
use std::collections::{BTreeMap, BTreeSet};

fn demo() -> (ArchGraph, Insights) {
    let mut g = demo_graph();
    infer(&mut g);
    let i = analyze(&g);
    (g, i)
}

fn named(g: &ArchGraph, id: &NodeId) -> String {
    g.node(id).map(display_name).unwrap()
}

#[test]
fn checkout_flow_crosses_async_stages_to_stripe() {
    let (g, i) = demo();
    let f = i.flows.iter().find(|f| f.name == "Web requests to checkout").expect("checkout flow");
    let names: Vec<String> = f.nodes.iter().map(|n| named(&g, n)).collect();
    assert_eq!(names[0], "checkout-cdn");
    for expected in ["checkout-api", "checkout-orders-db", "checkout-order-events", "InvoiceProcessor", "api.stripe.com", "search-indexer"]
    {
        assert!(names.contains(&expected.to_string()), "{expected} missing from {names:?}");
    }
    assert!(f.stages >= 3, "api → queue → consumer → bus → indexer: {}", f.stages);
    assert!(f.hops.iter().any(|h| h.mode == Mode::Async));
    assert!(!names.contains(&"checkout/prod/db-credentials".to_string()), "secrets are not steps of a flow");
}

#[test]
fn scheduled_job_is_its_own_flow_and_consumers_are_not_entries() {
    let (g, i) = demo();
    let entries: Vec<String> = i.flows.iter().map(|f| named(&g, &f.entry)).collect();
    assert!(entries.contains(&"MonthlyClose".to_string()), "{entries:?}");
    assert!(!entries.contains(&"InvoiceProcessor".to_string()), "a queue consumer is not an entry point");
    assert!(!entries.contains(&"search-indexer".to_string()), "a bus subscriber is not an entry point");
}

#[test]
fn orders_db_is_the_first_bottleneck() {
    let (g, i) = demo();
    let first = &i.capacity[0];
    assert_eq!(named(&g, &first.node), "checkout-orders-db");
    assert!(first.headroom < 1.5, "{}", first.headroom);
    assert!(i.insights.iter().any(|x| x.kind == "bottleneck" && x.priority == Priority::High));
}

#[test]
fn flow_rules_fire_on_demo() {
    let (_, i) = demo();
    let kinds: BTreeSet<&str> = i.insights.iter().map(|x| x.kind.as_str()).collect();
    for k in ["bottleneck", "shared_dependency", "error_origin", "missing_dlq", "visibility_timeout"] {
        assert!(kinds.contains(k), "missing {k}: {kinds:?}");
    }
}

#[test]
fn lifecycle_finds_idle_orphaned_and_zombie() {
    let (g, i) = demo();
    let states: BTreeMap<String, LifeState> = i.lifecycle.iter().map(|l| (named(&g, &l.node), l.state)).collect();
    assert_eq!(states.get("nat-legacy-dev"), Some(&LifeState::Zombie), "{states:?}");
    assert_eq!(states.get("legacy-export-job"), Some(&LifeState::Idle), "{states:?}");
    assert_eq!(states.get("migration-2024-sg"), Some(&LifeState::Orphaned), "{states:?}");
    assert!(!states.contains_key("checkout-api"), "active resources are not listed");
    assert_eq!(states.get("ci-deploy-2023"), Some(&LifeState::Orphaned), "unused role: {states:?}");
    assert!(!states.contains_key("reports-worker-role"), "a role in use is not listed");
    let savings: f64 = i.lifecycle.iter().filter_map(|l| l.monthly_cost).sum();
    assert!(savings > 30.0);
}

#[test]
fn error_messages_from_logs_reach_flows_and_the_origin() {
    let (g, i) = demo();
    let origin = i.insights.iter().find(|x| x.kind == "error_origin").unwrap();
    assert!(origin.detail.contains("Timeout calling https://api.stripe.com"), "{}", origin.detail);
    assert!(origin.action.as_deref().unwrap().contains("73%"), "{:?}", origin.action);
    let f = i.flows.iter().find(|f| f.name == "Web requests to checkout").unwrap();
    let top: Vec<(String, &str)> = f.top_errors.iter().map(|e| (named(&g, &e.node), e.message.as_str())).collect();
    assert_eq!(top[0].1, "Timeout calling https://api.stripe.com/v1/invoices after <n> ms");
    // ECS has no error metric: its error lines count toward the flow.
    assert!(top.iter().any(|(n, m)| n == "checkout-api" && m.contains("connection pool exhausted")), "{top:?}");
    assert!(f.errors_24h >= 26.0 + 7.0, "{}", f.errors_24h);
}

#[test]
fn upload_flow_reaches_an_ec2_worker_known_only_through_iam() {
    let (g, i) = demo();
    let f = i.flows.iter().find(|f| f.name == "Files uploaded to reports-uploads").expect("upload flow");
    assert_eq!(f.trigger, Trigger::Event);
    let hops: Vec<(String, String, bool)> = f.hops.iter().map(|h| (named(&g, &h.from), named(&g, &h.to), h.inferred)).collect();
    assert!(hops.contains(&("reports-jobs".into(), "reports-worker".into(), true)), "{hops:?}");
    assert!(hops.contains(&("reports-worker".into(), "reports-output".into(), true)), "{hops:?}");
    assert!(hops.contains(&("reports-worker".into(), "reports-uploads".into(), true)), "the worker reads the upload: {hops:?}");
    assert!(!hops.iter().any(|(_, t, _)| t == "reports-jobs-dlq"), "the DLQ is the failure path: {hops:?}");
    assert!(f.runs_24h.is_some(), "counted by the queue it feeds");
    // IAM edges are not followed for resources whose configuration we can read.
    let checkout = i.flows.iter().find(|f| f.name == "Web requests to checkout").unwrap();
    assert!(!checkout.hops.iter().any(|h| h.inferred), "checkout is mapped from configuration");
}
