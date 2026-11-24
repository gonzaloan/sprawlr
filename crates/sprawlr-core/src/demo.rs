//! A realistic fictional AWS account used for the demo, screenshots and the web preview.
//! Every analysis feature has at least one case here: internet exposure, a shared resource,
//! a plaintext secret, a permission-only cross-environment edge, observed traffic, an alarm.

use crate::{ArchGraph, EdgeKind, Evidence, EvidenceKind, Health, Node, NodeId, NodeKind};
use serde_json::{json, Value};

const ACCT: &str = "111122223333";
const REGION: &str = "us-east-1";

struct B {
    g: ArchGraph,
}

fn arn(service: &str, resource: &str) -> String {
    format!("arn:aws:{service}:{REGION}:{ACCT}:{resource}")
}

impl B {
    #[allow(clippy::too_many_arguments)]
    fn node(&mut self, id: &str, kind: NodeKind, name: &str, parent: &str, tags: &[(&str, &str)], attrs: Value, health: Health) {
        let mut n = Node::new(id, kind, name);
        n.account = Some(ACCT.into());
        n.region = Some(REGION.into());
        n.parent = Some(NodeId(parent.into()));
        n.tags = tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        n.attrs = attrs;
        n.health = health;
        self.g.upsert_node(n);
    }

    fn metrics(&mut self, id: &str, list: Vec<Value>) {
        let n = self.g.node_mut(&id.into()).expect("demo node");
        n.attrs["metrics"] = Value::Array(list);
    }

    fn alarm(&mut self, id: &str, name: &str, state: &str, metric: &str, reason: &str) {
        let n = self.g.node_mut(&id.into()).expect("demo node");
        let list = n.attrs.get("alarms").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let mut list = list;
        list.push(json!({"name": name, "state": state, "metric": metric, "reason": reason}));
        n.attrs["alarms"] = Value::Array(list);
    }

    fn edge(&mut self, from: &str, to: &str, kind: EdgeKind, ev: &[(EvidenceKind, &str)]) {
        for (k, source) in ev {
            let e = Evidence { kind: *k, source: source.to_string(), detail: None, seen_at: "2026-11-24T12:00:00Z".parse().unwrap() };
            assert!(self.g.add_evidence(&from.into(), &to.into(), kind, e), "demo edge {from} -> {to}");
        }
    }
}

/// 24 hourly values with a daily shape (quiet at night, busy in the afternoon).
fn daily(base: f64, swing: f64) -> Vec<Option<f64>> {
    (0..24)
        .map(|h| {
            let v = base + swing * ((h as f64 - 9.0) / 24.0 * std::f64::consts::TAU).sin();
            Some((v.max(0.0) * 10.0).round() / 10.0)
        })
        .collect()
}

/// Mostly zero, with the given counts in the last hours.
fn bursts(tail: &[f64]) -> Vec<Option<f64>> {
    let mut v = vec![Some(0.0); 24];
    for (i, x) in tail.iter().enumerate() {
        v[24 - tail.len() + i] = Some(*x);
    }
    v
}

/// Same shape the scanner produces (sprawlr-scan telemetry::summarize).
fn metric(key: &str, label: &str, unit: &str, stat: &str, values: Vec<Option<f64>>, alert: Option<String>) -> Value {
    let total: f64 = values.iter().flatten().sum();
    let latest = values.iter().rev().flatten().next().copied();
    let sum = stat == "Sum";
    json!({
        "key": key, "label": label, "unit": unit, "stat": stat, "values": values,
        "summary": if sum { total } else { latest.unwrap_or(0.0) },
        "summary_label": if sum { "last 24 h" } else { "latest" },
        "alert": alert,
    })
}

pub fn demo_graph() -> ArchGraph {
    use EdgeKind::*;
    use EvidenceKind::*;
    use Health::*;
    use NodeKind::*;

    let mut b = B { g: ArchGraph::new() };
    let account = format!("account:{ACCT}");
    let region = format!("region:{ACCT}/{REGION}");
    b.g.upsert_node(Node::new(account.as_str(), Account, ACCT));
    let mut r = Node::new(region.as_str(), Region, REGION);
    r.parent = Some(NodeId(account.clone()));
    b.g.upsert_node(r);
    b.g.upsert_node(Node::new("internet", Internet, "Internet"));

    // ---------------------------------------------------------------- network
    let prod_vpc = arn("ec2", "vpc/vpc-0a1prod");
    let dev_vpc = arn("ec2", "vpc/vpc-0b2dev");
    b.node(&prod_vpc, Vpc, "prod-vpc", &region, &[("Name", "prod-vpc")], json!({"cidr": "10.0.0.0/16"}), Unknown);
    b.node(&dev_vpc, Vpc, "dev-vpc", &region, &[("Name", "dev-vpc")], json!({"cidr": "10.1.0.0/16"}), Unknown);
    for (id, name, az, public) in [
        ("subnet-pub-a", "prod-public-a", "us-east-1a", true),
        ("subnet-pub-b", "prod-public-b", "us-east-1b", true),
        ("subnet-priv-a", "prod-private-a", "us-east-1a", false),
        ("subnet-priv-b", "prod-private-b", "us-east-1b", false),
    ] {
        b.node(&arn("ec2", &format!("subnet/{id}")), Subnet, name, &prod_vpc, &[], json!({"az": az, "auto_public_ip": public}), Unknown);
    }
    b.node(&arn("ec2", "subnet/subnet-dev-a"), Subnet, "dev-private-a", &dev_vpc, &[], json!({"az": "us-east-1a", "auto_public_ip": false}), Unknown);

    let sg = |id: &str| arn("ec2", &format!("security-group/{id}"));
    b.node(&sg("sg-alb"), SecurityGroup, "checkout-alb-sg", &prod_vpc, &[], json!({"ingress": ["tcp 443"], "open_to_internet": true}), Unknown);
    b.node(&sg("sg-api"), SecurityGroup, "checkout-api-sg", &prod_vpc, &[], json!({"ingress": ["tcp 8080"]}), Unknown);
    b.node(&sg("sg-db"), SecurityGroup, "orders-db-sg", &prod_vpc, &[], json!({"ingress": ["tcp 5432"]}), Unknown);
    b.edge("internet", &sg("sg-alb"), NetworkReachable, &[(Permitted, "ec2:security_group_ingress")]);
    b.edge(&sg("sg-alb"), &sg("sg-api"), NetworkReachable, &[(Permitted, "ec2:security_group_ingress")]);
    b.edge(&sg("sg-api"), &sg("sg-db"), NetworkReachable, &[(Permitted, "ec2:security_group_ingress")]);

    // ---------------------------------------------------------------- shared platform
    let bus = arn("events", "event-bus/platform-events");
    b.node(&bus, EventBus, "platform-events", &region, &[], json!({}), Ok);
    let kms = arn("kms", "key/1f2e3d4c-data");
    b.node(&kms, KmsKey, "data-key", &region, &[], json!({"rotation_enabled": true}), Unknown);

    // ---------------------------------------------------------------- checkout (prod) — tagged
    let ck = [("app", "checkout"), ("env", "prod")];
    let cdn = format!("arn:aws:cloudfront::{ACCT}:distribution/E2CHECKOUT");
    let alb = arn("elasticloadbalancing", "loadbalancer/app/checkout-alb/50dc6c495c0c9188");
    let api = format!("arn:aws:ecs:{REGION}:{ACCT}:service/prod-cluster/checkout-api");
    let db = arn("rds", "cluster:checkout-orders-db");
    let secret = arn("secretsmanager", "secret:checkout/prod/db-credentials-AbC123");
    let orders_q = arn("sqs", "checkout-order-events");
    let notif = arn("sns", "checkout-notifications");
    let mailer = arn("lambda", "function:checkout-email-sender");
    let role = format!("arn:aws:iam::{ACCT}:role/checkout-api-task-role");

    b.node(&cdn, CloudFront, "checkout-cdn", &region, &ck, json!({"domain": "d111.cloudfront.net"}), Ok);
    b.node(&alb, LoadBalancer, "checkout-alb", &prod_vpc, &ck, json!({"scheme": "internet-facing", "subnets": ["prod-public-a", "prod-public-b"], "security_groups": [sg("sg-alb")]}), Ok);
    b.node(&api, EcsService, "checkout-api", &prod_vpc, &ck, json!({"cluster": "prod-cluster", "launch_type": "FARGATE", "desired": 4, "task_definition": "checkout-api:42", "images": ["111122223333.dkr.ecr.us-east-1.amazonaws.com/checkout-api:2026.11.3"], "cpu": "1024", "memory": "2048", "subnets": ["prod-private-a", "prod-private-b"], "security_groups": [sg("sg-api")]}), Ok);
    b.node(&db, RdsCluster, "checkout-orders-db", &prod_vpc, &ck, json!({"engine": "aurora-postgresql", "multi_az": true, "encrypted": true, "security_groups": [sg("sg-db")]}), Warning);
    b.node(&secret, Secret, "checkout/prod/db-credentials", &region, &ck, json!({"rotation_enabled": true, "last_rotated_days": 12}), Unknown);
    b.node(&orders_q, SqsQueue, "checkout-order-events", &region, &ck, json!({"fifo": false, "visibility_timeout_s": 30}), Ok);
    b.node(&notif, SnsTopic, "checkout-notifications", &region, &ck, json!({}), Ok);
    b.node(&mailer, LambdaFunction, "checkout-email-sender", &region, &ck, json!({"runtime": "nodejs22.x", "memory_mb": 256}), Ok);
    b.node(&role, IamRole, "checkout-api-task-role", &region, &[], json!({"placeholder": true}), Unknown);

    b.edge("internet", &cdn, RoutesTo, &[(Declared, "cloudfront:viewer")]);
    b.edge(&cdn, &alb, RoutesTo, &[(Declared, "cloudfront:origin")]);
    b.edge(&alb, &api, RoutesTo, &[(Declared, "elb:target_group"), (Observed, "vpc_flow_logs")]);
    b.edge(&api, &db, Writes, &[(Declared, "env:DB_HOST"), (Permitted, "iam:role/checkout-api-task-role"), (Observed, "vpc_flow_logs")]);
    b.edge(&api, &secret, UsesSecret, &[(Declared, "ecs:task_definition.secrets")]);
    b.edge(&api, &orders_q, Publishes, &[(Declared, "env:ORDER_EVENTS_QUEUE_URL"), (Permitted, "iam:role/checkout-api-task-role")]);
    b.edge(&api, &notif, Publishes, &[(Declared, "env:NOTIFICATIONS_TOPIC_ARN")]);
    b.edge(&api, &bus, Publishes, &[(Declared, "env:EVENT_BUS_NAME")]);
    b.edge(&api, &role, AssumesRole, &[(Declared, "ecs:task_role")]);
    b.edge(&mailer, &notif, Subscribes, &[(Declared, "sns:subscription")]);
    b.edge(&db, &kms, EncryptedWith, &[(Declared, "rds:kms_key_id")]);

    // ---------------------------------------------------------------- billing (prod) — CloudFormation stack only
    let bl = [("aws:cloudformation:stack-name", "BillingStack-prod")];
    let invoices = arn("lambda", "function:BillingStack-prod-InvoiceProcessor9F3A21-XK2");
    let renderer = arn("lambda", "function:BillingStack-prod-PdfRenderer4B7C11-QW8");
    let close = arn("states", "stateMachine:BillingStack-prod-MonthlyClose");
    let table = arn("dynamodb", "table/BillingStack-prod-InvoicesTable7D1E0A-1ABC");
    let pdfs = "arn:aws:s3:::billingstack-prod-invoicepdfs3c2b1a-9zz".to_string();
    b.node(&invoices, LambdaFunction, "BillingStack-prod-InvoiceProcessor9F3A21-XK2", &region, &bl,
        json!({"runtime": "python3.13", "memory_mb": 512, "timeout_s": 60, "env_keys": ["INVOICES_TABLE", "STRIPE_API_KEY"], "env_plaintext_secret_keys": ["STRIPE_API_KEY"]}), Alarm);
    b.node(&renderer, LambdaFunction, "BillingStack-prod-PdfRenderer4B7C11-QW8", &region, &bl, json!({"runtime": "python3.13", "memory_mb": 1024}), Ok);
    b.node(&close, StateMachine, "BillingStack-prod-MonthlyClose", &region, &bl, json!({"type": "STANDARD"}), Ok);
    b.node(&table, DynamoTable, "BillingStack-prod-InvoicesTable7D1E0A-1ABC", &region, &bl, json!({"billing_mode": "PAY_PER_REQUEST", "pitr": true}), Ok);
    b.node(&pdfs, S3Bucket, "billingstack-prod-invoicepdfs3c2b1a-9zz", &region, &bl, json!({"public_access_block": true, "versioning": true}), Ok);

    b.edge(&invoices, &orders_q, Reads, &[(Declared, "lambda:event_source_mapping"), (Observed, "cloudwatch:NumberOfMessagesReceived")]);
    b.edge(&invoices, &table, Writes, &[(Declared, "env:INVOICES_TABLE"), (Permitted, "iam:role/InvoiceProcessorRole")]);
    b.edge(&invoices, &bus, Publishes, &[(Permitted, "iam:role/InvoiceProcessorRole")]);
    b.edge(&close, &invoices, Invokes, &[(Declared, "states:definition")]);
    b.edge(&close, &renderer, Invokes, &[(Declared, "states:definition")]);
    b.edge(&renderer, &table, Reads, &[(Declared, "env:INVOICES_TABLE")]);
    b.edge(&renderer, &pdfs, Writes, &[(Declared, "env:OUTPUT_BUCKET")]);
    b.edge(&pdfs, &kms, EncryptedWith, &[(Declared, "s3:bucket_encryption")]);

    // ---------------------------------------------------------------- search (prod) — naming only
    let sapi = format!("arn:aws:apigateway:{REGION}::/restapis/a1b2c3search");
    let squery = arn("lambda", "function:search-query-handler");
    let sindexer = arn("lambda", "function:search-indexer");
    let domain = arn("es", "domain/search-products");
    b.node(&sapi, ApiGateway, "search-public-api", &region, &[], json!({"endpoint_type": "REGIONAL", "authorizer": "none"}), Ok);
    b.node(&squery, LambdaFunction, "search-query-handler", &prod_vpc, &[], json!({"runtime": "java21", "memory_mb": 1024}), Ok);
    b.node(&sindexer, LambdaFunction, "search-indexer", &prod_vpc, &[], json!({"runtime": "java21", "memory_mb": 2048}), Ok);
    b.node(&domain, Other("AWS::OpenSearchService::Domain".into()), "search-products", &prod_vpc, &[], json!({"engine": "OpenSearch_2.17", "instances": 3}), Ok);
    b.edge("internet", &sapi, RoutesTo, &[(Declared, "apigateway:public_endpoint")]);
    b.edge(&sapi, &squery, Invokes, &[(Declared, "apigateway:integration"), (Observed, "xray")]);
    b.edge(&squery, &domain, Reads, &[(Declared, "env:OPENSEARCH_ENDPOINT"), (Observed, "xray")]);
    b.edge(&sindexer, &bus, Subscribes, &[(Declared, "events:rule_target")]);
    b.edge(&sindexer, &domain, Writes, &[(Declared, "env:OPENSEARCH_ENDPOINT")]);

    // ---------------------------------------------------------------- dev environment
    let dk = [("app", "checkout"), ("env", "dev")];
    let api_dev = format!("arn:aws:ecs:{REGION}:{ACCT}:service/dev-cluster/checkout-api");
    let db_dev = arn("rds", "cluster:checkout-orders-db-dev");
    b.node(&api_dev, EcsService, "checkout-api", &dev_vpc, &dk, json!({"cluster": "dev-cluster", "launch_type": "FARGATE", "desired": 1, "task_definition": "checkout-api:47", "images": ["111122223333.dkr.ecr.us-east-1.amazonaws.com/checkout-api:2026.11.5"], "cpu": "512", "memory": "1024"}), Ok);
    b.node(&db_dev, RdsCluster, "checkout-orders-db-dev", &dev_vpc, &dk, json!({"engine": "aurora-postgresql", "multi_az": false, "encrypted": true}), Ok);
    b.edge(&api_dev, &db_dev, Writes, &[(Declared, "env:DB_HOST")]);

    // Reporting job in dev whose IAM role can read the PROD database (never used): permission-only edge.
    let report = arn("lambda", "function:report-generator");
    b.node(&report, LambdaFunction, "report-generator", &dev_vpc, &[("app", "reporting"), ("env", "dev")], json!({"runtime": "python3.12"}), Unknown);
    b.edge(&report, &db, Reads, &[(Permitted, "iam:role/report-generator-role")]);
    b.edge(&report, &db_dev, Reads, &[(Declared, "env:DB_HOST")]);

    // ---------------------------------------------------------------- forgotten things
    // An export job nobody calls any more, a NAT gateway from an old dev setup, an IP and a
    // security group left behind by a 2024 migration.
    let legacy = arn("lambda", "function:legacy-export-job");
    b.node(&legacy, LambdaFunction, "legacy-export-job", &region, &[], json!({"runtime": "python3.9", "memory_mb": 1024}), Unknown);
    b.metrics(&legacy, vec![metric("invocations", "Invocations", "count", "Sum", bursts(&[]), None)]);
    let nat = arn("ec2", "natgateway/nat-0legacy");
    b.node(&nat, NatGateway, "nat-legacy-dev", &dev_vpc, &[("env", "dev")], json!({"subnets": ["dev-private-a"]}), Unknown);
    b.metrics(&nat, vec![metric("bytes_out", "Bytes out", "bytes", "Sum", bursts(&[]), None)]);
    b.node(&arn("ec2", "eip/eipalloc-0legacy"), Other("AWS::EC2::EIP".into()), "203.0.113.24", &region, &[], json!({"associated": false}), Unknown);
    b.node(&sg("sg-migration"), SecurityGroup, "migration-2024-sg", &prod_vpc, &[], json!({"ingress": ["tcp 5432"]}), Unknown);

    // ---------------------------------------------------------------- third parties
    let stripe = "ext:api.stripe.com";
    let slack = "ext:hooks.slack.com";
    let mut st = Node::new(stripe, ExternalEndpoint, "api.stripe.com");
    st.aliases.push("api.stripe.com".into());
    b.g.upsert_node(st);
    let mut sl = Node::new(slack, ExternalEndpoint, "hooks.slack.com");
    sl.aliases.push("hooks.slack.com".into());
    b.g.upsert_node(sl);
    b.edge(&invoices, stripe, Invokes, &[(Observed, "xray")]);
    b.edge(slack, &notif, Subscribes, &[(Declared, "sns:subscription")]);

    // ---------------------------------------------------------------- telemetry (last 24 h)
    b.metrics(&alb, vec![
        metric("requests", "Requests", "count", "Sum", daily(1800.0, 900.0), None),
        metric("errors_5xx", "5xx from targets", "count", "Sum", bursts(&[0.0, 4.0, 0.0, 8.0]), Some("12 5xx from targets in the last 24 hours".into())),
        metric("latency_p95", "Response time p95", "ms", "p95", daily(180.0, 40.0), None),
    ]);
    b.alarm(&alb, "checkout-alb-5xx-rate", "OK", "HTTPCode_Target_5XX_Count", "Threshold not crossed");
    b.metrics(&api, vec![
        metric("cpu", "CPU", "%", "Average", daily(35.0, 15.0), None),
        metric("memory", "Memory", "%", "Average", daily(58.0, 5.0), None),
    ]);
    b.metrics(&db, vec![
        metric("cpu", "CPU", "%", "Average", daily(62.0, 18.0), None),
        metric("connections", "Connections", "count", "Average", daily(140.0, 40.0), None),
    ]);
    b.alarm(&db, "orders-db-freeable-memory", "OK", "FreeableMemory", "Threshold not crossed");
    b.metrics(&orders_q, vec![
        metric("sent", "Messages sent", "count", "Sum", daily(400.0, 220.0), None),
        metric("backlog", "Messages waiting", "count", "Maximum", daily(6.0, 5.0), None),
        metric("oldest_age", "Oldest message age", "s", "Maximum", daily(20.0, 18.0), None),
    ]);
    b.metrics(&mailer, vec![
        metric("invocations", "Invocations", "count", "Sum", daily(300.0, 160.0), None),
        metric("errors", "Errors", "count", "Sum", bursts(&[]), None),
        metric("duration_p95", "Duration p95", "ms", "p95", daily(420.0, 60.0), None),
    ]);
    b.metrics(&notif, vec![
        metric("published", "Messages published", "count", "Sum", daily(300.0, 160.0), None),
        metric("failed", "Failed deliveries", "count", "Sum", bursts(&[]), None),
    ]);
    b.metrics(&invoices, vec![
        metric("invocations", "Invocations", "count", "Sum", daily(400.0, 220.0), None),
        metric("errors", "Errors", "count", "Sum", bursts(&[3.0, 9.0, 14.0]), Some("26 errors in the last 24 hours".into())),
        metric("throttles", "Throttles", "count", "Sum", bursts(&[]), None),
        metric("duration_p95", "Duration p95", "ms", "p95", {
            let mut d = daily(900.0, 120.0);
            d[21] = Some(4100.0); d[22] = Some(6900.0); d[23] = Some(8800.0);
            d
        }, None),
    ]);
    b.alarm(&invoices, "invoice-processor-errors", "ALARM", "Errors", "Threshold Crossed: 3 datapoints were greater than the threshold (5.0)");
    b.metrics(&renderer, vec![
        metric("invocations", "Invocations", "count", "Sum", daily(30.0, 20.0), None),
        metric("duration_p95", "Duration p95", "ms", "p95", daily(2400.0, 300.0), None),
    ]);
    b.metrics(&close, vec![
        metric("started", "Executions", "count", "Sum", bursts(&[0.0, 1.0, 0.0, 0.0, 0.0]), None),
        metric("failed", "Failed executions", "count", "Sum", bursts(&[]), None),
    ]);
    b.metrics(&table, vec![
        metric("writes", "Write units", "count", "Sum", daily(250.0, 140.0), None),
        metric("reads", "Read units", "count", "Sum", daily(600.0, 300.0), None),
        metric("write_throttles", "Write throttles", "count", "Sum", bursts(&[]), None),
    ]);
    b.metrics(&sapi, vec![
        metric("requests", "Requests", "count", "Sum", daily(5200.0, 2600.0), None),
        metric("errors_5xx", "5xx", "count", "Sum", bursts(&[]), None),
        metric("latency_p95", "Latency p95", "ms", "p95", daily(120.0, 25.0), None),
    ]);
    b.metrics(&squery, vec![
        metric("invocations", "Invocations", "count", "Sum", daily(5100.0, 2500.0), None),
        metric("errors", "Errors", "count", "Sum", bursts(&[]), None),
        metric("duration_p95", "Duration p95", "ms", "p95", daily(85.0, 15.0), None),
    ]);
    b.metrics(&sindexer, vec![
        metric("invocations", "Invocations", "count", "Sum", daily(900.0, 400.0), None),
        metric("duration_p95", "Duration p95", "ms", "p95", daily(640.0, 90.0), None),
    ]);

    b.g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infer::infer;

    #[test]
    fn demo_exercises_every_inference_path() {
        let mut g = demo_graph();
        let r = infer(&mut g);
        assert!(r.apps.contains_key("checkout"));
        assert!(r.apps.contains_key("billing"), "{:?}", r.apps);
        assert!(r.apps.contains_key("search"), "{:?}", r.apps);
        assert!(r.shared.iter().any(|(id, _)| id.0.contains("platform-events")), "{:?}", r.shared);
        let methods: Vec<_> = r.by_method.keys().cloned().collect();
        for m in ["Tag", "Stack", "Naming"] {
            assert!(methods.contains(&m.to_string()), "{methods:?}");
        }
    }
}
