//! SNS subscriptions: who receives what a topic publishes (queues, functions, webhooks).

use crate::extract::lambda_arn;
use crate::{evidence, external_node, PendingLink, Ref, ScanContext, ScanOutput};
use sprawlr_domain::{EdgeKind, EvidenceKind, NodeId};

pub async fn scan(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let c = aws_sdk_sns::Client::new(&ctx.sdk);
    let mut pages = c.list_subscriptions().into_paginator().send();
    let mut subs = Vec::new();
    while let Some(page) = pages.next().await {
        for s in page?.subscriptions() {
            if let (Some(t), Some(p), Some(e)) = (s.topic_arn(), s.protocol(), s.endpoint()) {
                subs.push((t.to_string(), p.to_string(), e.to_string()));
            }
        }
    }
    Ok(map_subscriptions(&subs))
}

/// `(topic ARN, protocol, endpoint)` → subscriber `--subscribes-->` topic.
pub fn map_subscriptions(subs: &[(String, String, String)]) -> ScanOutput {
    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    for (topic, protocol, endpoint) in subs {
        let from = match protocol.as_str() {
            "lambda" => lambda_arn(endpoint),
            "sqs" | "firehose" => endpoint.clone(),
            "http" | "https" => {
                let ext = external_node(endpoint);
                let id = ext.id.0.clone();
                out.nodes.push(ext);
                id
            }
            _ => continue, // email / sms: people, not systems
        };
        out.links.push(PendingLink {
            from: NodeId(from),
            to: Ref::Id(NodeId(topic.clone())),
            kind: Some(EdgeKind::Subscribes),
            evidence: evidence(EvidenceKind::Declared, "sns:subscription", Some(protocol.clone())),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribers_become_edges_and_webhooks_external_nodes() {
        let t = "arn:aws:sns:us-east-1:1:orders".to_string();
        let out = map_subscriptions(&[
            (t.clone(), "sqs".into(), "arn:aws:sqs:us-east-1:1:billing".into()),
            (t.clone(), "lambda".into(), "arn:aws:lambda:us-east-1:1:function:mail:live".into()),
            (t.clone(), "https".into(), "https://hooks.slack.com/services/x".into()),
            (t.clone(), "email".into(), "ops@example.com".into()),
        ]);
        assert_eq!(out.links.len(), 3);
        assert_eq!(out.links[1].from.0, "arn:aws:lambda:us-east-1:1:function:mail");
        assert_eq!(out.nodes[0].name, "hooks.slack.com");
    }
}
