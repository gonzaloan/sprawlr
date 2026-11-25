//! AWS adapter for sprawlr's ports ([`Aws`] implements `InfraSource`, `HealthSource` and
//! `ErrorLogSource`). Read-only.
//!
//! Discovery: each scanner returns a [`ScanOutput`]; the [`link`] pass merges them into an
//! [`ArchGraph`], resolving references between resources. Extractors in [`extract`] are pure
//! functions from AWS JSON to nodes and links, tested against fixtures.
//!
//! Scanners never read secret values: env var *values* are only inspected in memory to
//! detect references; only keys and resolved references are stored.

pub mod apigw;
pub mod cloudcontrol;
pub mod extract;
pub mod iam;
pub mod lambda;
pub mod link;
pub mod logs;
#[cfg(feature = "ec2")]
pub mod network;
pub mod refs;
pub mod secrets;
pub mod sns;
pub mod tags;
pub mod telemetry;

use sprawlr_domain::{ArchGraph, EdgeKind, Evidence, EvidenceKind, Node, NodeId, NodeKind};
use std::collections::BTreeMap;

pub use sprawlr_app::Progress;
use sprawlr_app::{Discovered, ErrorLogSource, HealthSource, InfraSource, Warnings};

/// An AWS account, reached with a named profile (or the default credential chain), across
/// the given regions.
pub struct Aws {
    profile: Option<String>,
    regions: Vec<String>,
}

impl Aws {
    pub fn new(profile: Option<String>, regions: Vec<String>) -> Self {
        Aws { profile, regions }
    }

    /// Regions of the resources already in a graph (for health and logs on a snapshot).
    fn regions_of(graph: &ArchGraph) -> Vec<String> {
        graph.nodes().filter_map(|n| n.region.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect()
    }
}

impl InfraSource for Aws {
    async fn discover(&self, progress: &Progress) -> anyhow::Result<Discovered> {
        anyhow::ensure!(!self.regions.is_empty(), "add at least one region");
        let report = discover(self.profile.as_deref(), &self.regions, progress).await?;
        Ok(Discovered { graph: report.graph, warnings: report.warnings })
    }
}

impl HealthSource for Aws {
    async fn read_health(&self, graph: &mut ArchGraph, progress: &Progress) -> anyhow::Result<Warnings> {
        let (base, account) = session(self.profile.as_deref()).await?;
        ensure_same_account(graph, &account, self.profile.as_deref())?;
        let mut warnings = Vec::new();
        for region in Self::regions_of(graph) {
            let ctx = context(&base, &account, &region);
            warnings.extend(telemetry::apply(graph, &ctx, progress).await);
        }
        Ok(warnings)
    }
}

impl ErrorLogSource for Aws {
    async fn read_errors(&self, graph: &mut ArchGraph, progress: &Progress) -> anyhow::Result<Warnings> {
        let (base, account) = session(self.profile.as_deref()).await?;
        ensure_same_account(graph, &account, self.profile.as_deref())?;
        let mut warnings = Vec::new();
        for region in Self::regions_of(graph) {
            let ctx = context(&base, &account, &region);
            warnings.extend(logs::apply(graph, &ctx, progress).await);
        }
        Ok(warnings)
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
pub(crate) fn describe(e: &anyhow::Error) -> String {
    let chain: Vec<String> = e.chain().map(|c| c.to_string()).collect();
    chain.join(": ")
}

async fn session(profile: Option<&str>) -> anyhow::Result<(aws_config::SdkConfig, String)> {
    let mut loader = aws_config::from_env();
    if let Some(p) = profile {
        loader = loader.profile_name(p);
    }
    let base = loader.load().await;
    let ident = aws_sdk_sts::Client::new(&base).get_caller_identity().send().await.map_err(|e| {
        use aws_sdk_sts::error::ProvideErrorMetadata;
        let detail = aws_sdk_sts::error::DisplayErrorContext(&e).to_string();
        let code = e.as_service_error().and_then(|s| s.code()).map(String::from);
        match sign_in_hint(credentials_failed(&e), code.as_deref(), &detail, profile) {
            Some(hint) => anyhow::Error::new(sprawlr_app::SignInNeeded { profile: profile.map(String::from), hint, reason: detail }),
            None => anyhow::Error::new(e),
        }
    })?;
    Ok((base, ident.account().unwrap_or("unknown").to_string()))
}

/// Whether the credential provider itself failed (as opposed to the network or AWS), found by
/// type in the error chain. A provider timeout is a network problem, not a sign-in one.
fn credentials_failed(e: &(dyn std::error::Error + 'static)) -> bool {
    use aws_credential_types::provider::error::CredentialsError;
    let mut cur = Some(e);
    while let Some(err) = cur {
        if let Some(c) = err.downcast_ref::<CredentialsError>() {
            return !matches!(c, CredentialsError::ProviderTimedOut(_));
        }
        cur = err.source();
    }
    false
}

/// What a person must do when credentials are the problem, or `None` when they are not.
fn sign_in_hint(credentials_failed: bool, code: Option<&str>, detail: &str, profile: Option<&str>) -> Option<String> {
    let flag = profile.map(|p| format!(" --profile {p}")).unwrap_or_default();
    let who = profile.map(|p| format!("profile {p}")).unwrap_or_else(|| "the default credentials".into());
    let sso = detail.to_ascii_lowercase().contains("sso");
    match code {
        Some("ExpiredToken" | "ExpiredTokenException") if sso => Some(format!("Run `aws sso login{flag}`, then try again.")),
        Some("ExpiredToken" | "ExpiredTokenException") => {
            Some(format!("The session token of {who} has expired. Get new credentials, then try again."))
        }
        Some("InvalidClientTokenId" | "UnrecognizedClientException" | "SignatureDoesNotMatch" | "InvalidAccessKeyId") => {
            Some(format!("AWS does not recognise the access keys of {who}. Check them with `aws configure{flag}`."))
        }
        _ if credentials_failed && sso => Some(format!("Run `aws sso login{flag}`, then try again.")),
        _ if credentials_failed => Some(format!("No usable credentials for {who}. Set them up with `aws configure{flag}` or sign in.")),
        _ => None,
    }
}

/// Health for a snapshot must come from the account it describes: same resource names in
/// another account would paint the wrong health on it.
fn ensure_same_account(graph: &ArchGraph, account: &str, profile: Option<&str>) -> anyhow::Result<()> {
    if let Some(scanned) = graph.nodes().find_map(|n| n.account.clone()) {
        let who = profile.map(|p| format!("profile {p}")).unwrap_or_else(|| "the default credentials".into());
        anyhow::ensure!(
            scanned == account,
            "this scan is of account {scanned}, but {who} signs in to account {account}. Refresh with the profile for {scanned}."
        );
    }
    Ok(())
}

fn context(base: &aws_config::SdkConfig, account: &str, region: &str) -> ScanContext {
    let sdk = base.to_builder().region(aws_config::Region::new(region.to_string())).build();
    ScanContext { sdk, account: account.to_string(), region: region.to_string() }
}

/// Discover one account's structure across regions with the given profile (None = default
/// credential chain). Health, logs and IAM reach are added by the use case.
pub async fn discover(profile: Option<&str>, regions: &[String], progress: &Progress) -> anyhow::Result<ScanReport> {
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
            cloudcontrol::scan(&ctx, i == 0, progress),
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
    match cloudcontrol::scan_cloudfront(&cf, progress).await {
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
    Ok(link::link(outputs))
}

#[cfg(feature = "ec2")]
async fn scan_network(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    network::scan(ctx).await
}

#[cfg(not(feature = "ec2"))]
async fn scan_network(_: &ScanContext) -> anyhow::Result<ScanOutput> {
    anyhow::bail!("built without the `ec2` feature")
}

#[cfg(test)]
mod sign_in_tests {
    use super::*;

    #[test]
    fn credentials_problems_get_a_hint_and_outages_do_not() {
        let p = Some("prod-admin");
        assert_eq!(
            sign_in_hint(true, None, "the SSO session has expired", p).unwrap(),
            "Run `aws sso login --profile prod-admin`, then try again."
        );
        assert!(sign_in_hint(true, None, "no credentials in the property bag", p).unwrap().contains("aws configure --profile prod-admin"));
        assert!(sign_in_hint(false, Some("ExpiredToken"), "The security token included in the request is expired", None)
            .unwrap()
            .contains("expired"));
        assert!(sign_in_hint(false, Some("InvalidClientTokenId"), "", p).unwrap().contains("does not recognise the access keys"));
        // A network failure on the way to the SSO endpoint is not a sign-in problem.
        assert_eq!(sign_in_hint(false, None, "dispatch failure: io error reaching portal.sso.us-east-1.amazonaws.com", p), None);
        assert_eq!(sign_in_hint(false, Some("AccessDenied"), "not authorized to perform sts:GetCallerIdentity", p), None);
    }

    #[test]
    fn credential_errors_are_found_by_type_in_the_chain() {
        use aws_credential_types::provider::error::CredentialsError;
        #[derive(Debug)]
        struct Wrapper(CredentialsError);
        impl std::fmt::Display for Wrapper {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "dispatch failure")
            }
        }
        impl std::error::Error for Wrapper {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        assert!(credentials_failed(&Wrapper(CredentialsError::not_loaded("no profile"))));
        assert!(!credentials_failed(&Wrapper(CredentialsError::provider_timed_out(std::time::Duration::from_secs(5)))));
    }

    #[test]
    fn health_comes_from_the_scanned_account() {
        let mut g = ArchGraph::new();
        let mut n = Node::new("arn:aws:lambda:us-east-1:111:function:a", NodeKind::LambdaFunction, "a");
        n.account = Some("111".into());
        g.upsert_node(n);
        assert!(ensure_same_account(&g, "111", None).is_ok());
        let e = ensure_same_account(&g, "222", Some("dev-admin")).unwrap_err().to_string();
        assert!(e.contains("account 111") && e.contains("profile dev-admin") && e.contains("account 222"), "{e}");
    }
}
