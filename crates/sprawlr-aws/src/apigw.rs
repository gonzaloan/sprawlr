//! API Gateway integrations, read natively.
//! - REST (v1): Cloud Control only exposes them per resource and method, so one
//!   `GetResources?embed=methods` call per API is far cheaper.
//! - HTTP (v2): Cloud Control's read handler fails on some integration types.

use crate::extract::{http_api_arn, integration_target, rest_api_arn};
use crate::{evidence, PendingLink, ScanContext, ScanOutput};
use sprawlr_domain::{EvidenceKind, NodeId};

pub async fn scan(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let c = aws_sdk_apigateway::Client::new(&ctx.sdk);
    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    let mut apis = c.get_rest_apis().into_paginator().send();
    while let Some(page) = apis.next().await {
        for api in page?.items() {
            let Some(id) = api.id() else { continue };
            let from = NodeId(rest_api_arn(&ctx.region, id));
            let mut res = c.get_resources().rest_api_id(id).embed("methods").into_paginator().send();
            while let Some(rp) = res.next().await {
                for r in rp?.items() {
                    for (verb, m) in r.resource_methods().into_iter().flatten() {
                        let Some(uri) = m.method_integration().and_then(|i| i.uri()) else { continue };
                        let Some((to, kind)) = integration_target(uri) else { continue };
                        let route = format!("{verb} {}", r.path().unwrap_or("/"));
                        out.links.push(PendingLink {
                            from: from.clone(),
                            to,
                            kind: Some(kind),
                            evidence: evidence(EvidenceKind::Declared, "apigateway:integration", Some(route)),
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

/// HTTP API (v2) integrations: route target of each API.
pub async fn scan_http(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let c = aws_sdk_apigatewayv2::Client::new(&ctx.sdk);
    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    let mut api_ids = Vec::new();
    let mut next: Option<String> = None;
    loop {
        let page = c.get_apis().set_next_token(next.take()).send().await?;
        api_ids.extend(page.items().iter().filter_map(|a| a.api_id().map(String::from)));
        match page.next_token() {
            Some(t) if !t.is_empty() => next = Some(t.to_string()),
            _ => break,
        }
    }
    for id in api_ids {
        let from = NodeId(http_api_arn(&ctx.region, &id));
        let mut next: Option<String> = None;
        loop {
            let page = c.get_integrations().api_id(&id).set_next_token(next.take()).send().await?;
            for i in page.items() {
                let Some(uri) = i.integration_uri() else { continue };
                let Some((to, kind)) = integration_target(uri) else { continue };
                let detail = i.integration_type().map(|t| t.as_str().to_string());
                out.links.push(PendingLink {
                    from: from.clone(),
                    to,
                    kind: Some(kind),
                    evidence: evidence(EvidenceKind::Declared, "apigateway:integration", detail),
                });
            }
            match page.next_token() {
                Some(t) if !t.is_empty() => next = Some(t.to_string()),
                _ => break,
            }
        }
    }
    Ok(out)
}
