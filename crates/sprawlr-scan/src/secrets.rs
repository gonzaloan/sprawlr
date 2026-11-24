//! Secrets Manager metadata: rotation and last use. Values are never requested
//! (`ListSecrets` does not return them, and the IAM policy denies `GetSecretValue`).

use crate::{evidence, PendingLink, Ref, ScanContext, ScanOutput};
use sprawlr_core::{EdgeKind, EvidenceKind, Node, NodeId, NodeKind};

pub struct SecretMeta {
    pub arn: String,
    pub name: String,
    pub rotation_enabled: bool,
    pub last_rotated_days: Option<i64>,
    pub last_accessed_days: Option<i64>,
    pub kms_key: Option<String>,
    pub tags: Vec<(String, String)>,
}

pub async fn scan(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let c = aws_sdk_secretsmanager::Client::new(&ctx.sdk);
    let now = chrono::Utc::now().timestamp();
    let days = |d: Option<&aws_sdk_secretsmanager::primitives::DateTime>| d.map(|t| (now - t.secs()) / 86_400);
    let mut pages = c.list_secrets().into_paginator().send();
    let mut metas = Vec::new();
    while let Some(page) = pages.next().await {
        for s in page?.secret_list() {
            let (Some(arn), Some(name)) = (s.arn(), s.name()) else { continue };
            metas.push(SecretMeta {
                arn: arn.into(),
                name: name.into(),
                rotation_enabled: s.rotation_enabled().unwrap_or(false),
                last_rotated_days: days(s.last_rotated_date()),
                last_accessed_days: days(s.last_accessed_date()),
                kms_key: s.kms_key_id().map(String::from),
                tags: s.tags().iter().filter_map(|t| Some((t.key()?.to_string(), t.value().unwrap_or_default().to_string()))).collect(),
            });
        }
    }
    Ok(map_secrets(ctx, metas))
}

pub fn map_secrets(ctx: &ScanContext, metas: Vec<SecretMeta>) -> ScanOutput {
    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    for m in metas {
        let mut n = Node::new(m.arn.as_str(), NodeKind::Secret, m.name.clone());
        n.account = Some(ctx.account.clone());
        n.region = Some(ctx.region.clone());
        n.parent = Some(ctx.region_id());
        n.aliases.push(m.name.clone());
        n.tags = m.tags.into_iter().collect();
        n.attrs = serde_json::json!({
            "rotation_enabled": m.rotation_enabled,
            "last_rotated_days": m.last_rotated_days,
            "last_accessed_days": m.last_accessed_days,
        });
        if let Some(k) = m.kms_key.filter(|k| !k.starts_with("alias/aws/")) {
            let to = if k.starts_with("arn:") { Ref::Id(NodeId(k)) } else { Ref::Alias(k) };
            out.links.push(PendingLink { from: n.id.clone(), to, kind: Some(EdgeKind::EncryptedWith), evidence: evidence(EvidenceKind::Declared, "secretsmanager:kms_key", None) });
        }
        out.nodes.push(n);
    }
    out
}
