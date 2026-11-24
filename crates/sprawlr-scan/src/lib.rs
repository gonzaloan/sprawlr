//! Read-only AWS discovery. Each scanner returns a [`ScanOutput`]; the [`link`] pass then
//! merges everything into an [`ArchGraph`], resolving references between resources.
//!
//! Scanners never read secret values: env var *values* are only inspected in memory to
//! detect references; only keys and resolved references are stored.

pub mod apigw;
pub mod cloudcontrol;
pub mod extract;
pub mod iam;
pub mod lambda;
pub mod link;
#[cfg(feature = "ec2")]
pub mod network;
pub mod refs;
pub mod secrets;
pub mod sns;
pub mod tags;
pub mod telemetry;

use sprawlr_core::{ArchGraph, EdgeKind, Evidence, EvidenceKind, Node, NodeId, NodeKind};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Human-readable progress messages ("us-east-1: reading ECS services").
pub type Progress = Arc<dyn Fn(String) + Send + Sync>;

pub fn quiet() -> Progress {
    Arc::new(|_| {})
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// CloudWatch alarms + metrics and X-Ray traces. GetMetricData is billed (cents per scan).
    pub telemetry: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions { telemetry: true }
    }
}

/// A reference to another resource that may not be scanned yet.
#[derive(Debug, Clone, PartialEq)]
pub enum Ref {
    /// ARN or exact node id.
    Id(NodeId),
    /// Alias such as a queue URL, DB host, `dynamodb:orders`.
    Alias(String),
    /// A hostname: one of our resources if an alias matches, otherwise a third-party endpoint.
    External(String),
}

/// Node for a host outside the account (webhook, SaaS API, custom origin).
pub fn external_node(url_or_host: &str) -> Node {
    let host = url_or_host.split("://").last().unwrap_or(url_or_host).split(['/', ':', '?']).next().unwrap_or("").to_ascii_lowercase();
    let mut n = Node::new(format!("ext:{host}").as_str(), NodeKind::ExternalEndpoint, host.clone());
    n.aliases.push(host);
    n
}

#[derive(Debug, Clone)]
pub struct PendingLink {
    pub from: NodeId,
    pub to: Ref,
    /// `None` → infer from the target's kind during linking.
    pub kind: Option<EdgeKind>,
    pub evidence: Evidence,
}

#[derive(Debug, Default)]
pub struct ScanOutput {
    pub nodes: Vec<Node>,
    pub links: Vec<PendingLink>,
    pub tags: Vec<(NodeId, BTreeMap<String, String>)>,
    /// Specific scanners are authoritative; generic inventory (Cloud Control)
    /// only adds nodes nobody else produced.
    pub authoritative: bool,
    pub warnings: Vec<String>,
}

pub struct ScanContext {
    pub sdk: aws_config::SdkConfig,
    pub account: String,
    pub region: String,
}

impl ScanContext {
    pub fn account_id(&self) -> NodeId {
        NodeId(format!("account:{}", self.account))
    }
    pub fn region_id(&self) -> NodeId {
        NodeId(format!("region:{}/{}", self.account, self.region))
    }
    pub fn arn(&self, service: &str, resource: &str) -> NodeId {
        NodeId(format!("arn:aws:{service}:{}:{}:{resource}", self.region, self.account))
    }
}

pub(crate) fn vpc_id(ctx: &ScanContext, id: &str) -> NodeId {
    ctx.arn("ec2", &format!("vpc/{id}"))
}

#[allow(dead_code)]
pub(crate) fn subnet_id(ctx: &ScanContext, id: &str) -> NodeId {
    ctx.arn("ec2", &format!("subnet/{id}"))
}

pub(crate) fn sg_id(ctx: &ScanContext, id: &str) -> NodeId {
    ctx.arn("ec2", &format!("security-group/{id}"))
}

pub(crate) fn evidence(kind: EvidenceKind, source: &str, detail: Option<String>) -> Evidence {
    Evidence { kind, source: source.to_string(), detail, seen_at: chrono::Utc::now() }
}

pub struct ScanReport {
    pub graph: ArchGraph,
    pub warnings: Vec<String>,
    pub unresolved: Vec<String>,
}

/// Full error chain, including the service's own message (AccessDenied, throttling...).
fn describe(e: &anyhow::Error) -> String {
    let chain: Vec<String> = e.chain().map(|c| c.to_string()).collect();
    chain.join(": ")
}

async fn session(profile: Option<&str>) -> anyhow::Result<(aws_config::SdkConfig, String)> {
    let mut loader = aws_config::from_env();
    if let Some(p) = profile {
        loader = loader.profile_name(p);
    }
    let base = loader.load().await;
    let ident = aws_sdk_sts::Client::new(&base).get_caller_identity().send().await?;
    Ok((base, ident.account().unwrap_or("unknown").to_string()))
}

fn context(base: &aws_config::SdkConfig, account: &str, region: &str) -> ScanContext {
    let sdk = base.to_builder().region(aws_config::Region::new(region.to_string())).build();
    ScanContext { sdk, account: account.to_string(), region: region.to_string() }
}

/// Scan one account across regions with the given profile (None = default credential chain).
pub async fn scan(profile: Option<&str>, regions: &[String], opts: &ScanOptions, progress: Progress) -> anyhow::Result<ScanReport> {
    progress("signing in".into());
    let (base, account) = session(profile).await?;

    let mut outputs = Vec::new();
    let mut scope = ScanOutput { authoritative: true, ..Default::default() };
    scope.nodes.push(Node::new(format!("account:{account}").as_str(), NodeKind::Account, account.clone()));
    scope.nodes.push(Node::new("internet", NodeKind::Internet, "Internet"));
    outputs.push(scope);

    for (i, region) in regions.iter().enumerate() {
        let ctx = context(&base, &account, region);
        let mut region_node = Node::new(ctx.region_id(), NodeKind::Region, region.clone());
        region_node.parent = Some(ctx.account_id());
        region_node.region = Some(region.clone());
        outputs.push(ScanOutput { nodes: vec![region_node], authoritative: true, ..Default::default() });

        progress(format!("{region}: reading networks, functions and tags"));
        let (net, fns, tg, cc, sn, sec, rest, http) = tokio::join!(
            scan_network(&ctx),
            lambda::scan(&ctx),
            tags::scan(&ctx),
            cloudcontrol::scan(&ctx, i == 0, &progress),
            sns::scan(&ctx),
            secrets::scan(&ctx),
            apigw::scan(&ctx),
            apigw::scan_http(&ctx),
        );
        let parts = [
            ("network", net),
            ("lambda", fns),
            ("tags", tg),
            ("cloudcontrol", cc),
            ("sns", sn),
            ("secrets", sec),
            ("apigateway REST", rest),
            ("apigateway HTTP", http),
        ];
        for (name, res) in parts {
            match res {
                Ok(o) => outputs.push(o),
                // Missing permissions for one service must not kill the whole scan.
                Err(e) => outputs.push(ScanOutput { warnings: vec![format!("{region}/{name}: {}", describe(&e))], ..Default::default() }),
            }
        }
    }

    // CloudFront is global: read once, from us-east-1.
    let cf = context(&base, &account, "us-east-1");
    match cloudcontrol::scan_cloudfront(&cf, &progress).await {
        Ok(o) => outputs.push(o),
        Err(e) => outputs.push(ScanOutput { warnings: vec![format!("cloudfront: {e:#}")], ..Default::default() }),
    }

    // IAM is global: roles and what their policies allow.
    progress("reading IAM roles and policies".into());
    match iam::scan(&cf).await {
        Ok(o) => outputs.push(o),
        Err(e) => outputs.push(ScanOutput { warnings: vec![format!("iam: {}", describe(&e))], ..Default::default() }),
    }

    progress("linking resources".into());
    let mut report = link::link(outputs);
    sprawlr_core::iam::apply(&mut report.graph);
    if opts.telemetry {
        for region in regions {
            let ctx = context(&base, &account, region);
            let w = telemetry::apply(&mut report.graph, &ctx, &progress).await;
            report.warnings.extend(w);
        }
    }
    Ok(report)
}

/// Re-read alarms, metrics and traces for an existing graph (no structural rescan).
pub async fn refresh_telemetry(graph: &mut ArchGraph, profile: Option<&str>, progress: Progress) -> anyhow::Result<Vec<String>> {
    let (base, account) = session(profile).await?;
    let regions: std::collections::BTreeSet<String> = graph.nodes().filter_map(|n| n.region.clone()).collect();
    let mut warnings = Vec::new();
    for region in regions {
        let ctx = context(&base, &account, &region);
        warnings.extend(telemetry::apply(graph, &ctx, &progress).await);
    }
    Ok(warnings)
}

#[cfg(feature = "ec2")]
async fn scan_network(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    network::scan(ctx).await
}

#[cfg(not(feature = "ec2"))]
async fn scan_network(_: &ScanContext) -> anyhow::Result<ScanOutput> {
    anyhow::bail!("built without the `ec2` feature")
}
