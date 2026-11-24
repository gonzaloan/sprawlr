//! Lambda functions: placement (VPC / SGs), role, triggers, and references in env vars.

use crate::refs::{classify, Finding};
use crate::{evidence, sg_id, vpc_id, PendingLink, Ref, ScanContext, ScanOutput};
use aws_sdk_lambda::types::{EventSourceMappingConfiguration, FunctionConfiguration};
use sprawlr_domain::{EdgeKind, EvidenceKind, Node, NodeId, NodeKind};

pub async fn scan(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let client = aws_sdk_lambda::Client::new(&ctx.sdk);
    let fns: Vec<FunctionConfiguration> = client.list_functions().into_paginator().items().send().collect::<Result<_, _>>().await?;
    let esms: Vec<EventSourceMappingConfiguration> =
        client.list_event_source_mappings().into_paginator().items().send().collect::<Result<_, _>>().await?;

    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    for f in &fns {
        let (node, links) = map_function(ctx, f);
        out.nodes.push(node);
        out.links.extend(links);
    }
    out.links.extend(esms.iter().filter_map(map_trigger));
    Ok(out)
}

pub fn map_function(ctx: &ScanContext, f: &FunctionConfiguration) -> (Node, Vec<PendingLink>) {
    let arn = NodeId(f.function_arn().unwrap_or_default().to_string());
    let name = f.function_name().unwrap_or_default().to_string();
    let vpc = f.vpc_config().and_then(|v| v.vpc_id()).filter(|v| !v.is_empty());

    let mut n = Node::new(arn.clone(), NodeKind::LambdaFunction, name.clone());
    n.account = Some(ctx.account.clone());
    n.region = Some(ctx.region.clone());
    n.parent = Some(vpc.map(|v| vpc_id(ctx, v)).unwrap_or_else(|| ctx.region_id()));
    n.aliases.push(name);

    let mut links = Vec::new();
    let link = |to: Ref, kind: Option<EdgeKind>, source: &str, detail: Option<String>| PendingLink {
        from: arn.clone(),
        to,
        kind,
        evidence: evidence(EvidenceKind::Declared, source, detail),
    };

    if let Some(role) = f.role() {
        links.push(link(Ref::Id(NodeId(role.to_string())), Some(EdgeKind::AssumesRole), "lambda:execution_role", None));
    }

    let mut subnets = Vec::new();
    let mut sgs = Vec::new();
    if let Some(vc) = f.vpc_config() {
        subnets = vc.subnet_ids().to_vec();
        sgs = vc.security_group_ids().to_vec();
    }

    // Env vars: keep keys + references, never values.
    let mut env_keys = Vec::new();
    let mut plaintext = Vec::new();
    if let Some(vars) = f.environment().and_then(|e| e.variables()) {
        let mut sorted: Vec<_> = vars.iter().collect();
        sorted.sort();
        for (k, v) in sorted {
            env_keys.push(k.clone());
            match classify(k, v) {
                Finding::Reference(r) => links.push(link(r, None, &format!("env:{k}"), None)),
                Finding::PlaintextSecret => plaintext.push(k.clone()),
                Finding::Nothing => {}
            }
        }
    }

    n.attrs = serde_json::json!({
        "runtime": f.runtime().map(|r| r.as_str()),
        "memory_mb": f.memory_size(),
        "timeout_s": f.timeout(),
        "architectures": f.architectures().iter().map(|a| a.as_str()).collect::<Vec<_>>(),
        "subnets": subnets,
        "security_groups": sgs.iter().map(|s| sg_id(ctx, s).0).collect::<Vec<_>>(),
        "env_keys": env_keys,
        "env_plaintext_secret_keys": plaintext,
        "log_groups": [f.logging_config().and_then(|l| l.log_group()).map(String::from).unwrap_or_else(|| format!("/aws/lambda/{}", f.function_name().unwrap_or_default()))],
    });
    (n, links)
}

/// Event source mapping: the function polls the source (SQS, Kinesis, DynamoDB stream...).
pub fn map_trigger(m: &EventSourceMappingConfiguration) -> Option<PendingLink> {
    let func = m.function_arn()?;
    let src = m.event_source_arn()?;
    // DynamoDB stream ARNs → link to the table itself.
    let target = src.split("/stream/").next().unwrap_or(src);
    Some(PendingLink {
        from: NodeId(func.to_string()),
        to: Ref::Id(NodeId(target.to_string())),
        kind: Some(EdgeKind::Reads),
        evidence: evidence(EvidenceKind::Declared, "lambda:event_source_mapping", m.state().map(String::from)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_lambda::types::{EnvironmentResponse, VpcConfigResponse};

    fn ctx() -> ScanContext {
        ScanContext { sdk: aws_config::SdkConfig::builder().build(), account: "123".into(), region: "us-east-1".into() }
    }

    #[test]
    fn maps_function_without_leaking_values() {
        let f = FunctionConfiguration::builder()
            .function_arn("arn:aws:lambda:us-east-1:123:function:checkout")
            .function_name("checkout")
            .role("arn:aws:iam::123:role/checkout")
            .vpc_config(VpcConfigResponse::builder().vpc_id("vpc-1").subnet_ids("subnet-a").security_group_ids("sg-app").build())
            .environment(
                EnvironmentResponse::builder()
                    .variables("ORDERS_TABLE", "orders")
                    .variables("STRIPE_API_KEY", "sk_live_supersecret")
                    .variables("LOG_LEVEL", "info")
                    .build(),
            )
            .build();
        let (n, links) = map_function(&ctx(), &f);

        assert_eq!(n.parent.as_ref().unwrap().0, "arn:aws:ec2:us-east-1:123:vpc/vpc-1");
        assert_eq!(n.attrs["env_plaintext_secret_keys"][0], "STRIPE_API_KEY");
        assert!(!serde_json::to_string(&n).unwrap().contains("sk_live"), "secret value must never be stored");
        assert_eq!(links.len(), 2); // role + table
        assert!(links.iter().any(|l| l.to == Ref::Alias("dynamodb:orders".into())));
    }

    #[test]
    fn dynamodb_stream_trigger_points_to_table() {
        let m = EventSourceMappingConfiguration::builder()
            .function_arn("arn:aws:lambda:us-east-1:123:function:f")
            .event_source_arn("arn:aws:dynamodb:us-east-1:123:table/orders/stream/2026-01-01T00:00:00.000")
            .build();
        let l = map_trigger(&m).unwrap();
        assert_eq!(l.to, Ref::Id(NodeId("arn:aws:dynamodb:us-east-1:123:table/orders".into())));
    }
}
