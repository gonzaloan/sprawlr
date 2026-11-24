//! REST API integrations (API Gateway v1). Cloud Control only exposes them per resource and
//! method, so one `GetResources?embed=methods` call per API is far cheaper.

use crate::extract::{integration_target, rest_api_arn};
use crate::{evidence, PendingLink, ScanContext, ScanOutput};
use sprawlr_core::{EvidenceKind, NodeId};

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
                        out.links.push(PendingLink { from: from.clone(), to, kind: Some(kind), evidence: evidence(EvidenceKind::Declared, "apigateway:integration", Some(route)) });
                    }
                }
            }
        }
    }
    Ok(out)
}
