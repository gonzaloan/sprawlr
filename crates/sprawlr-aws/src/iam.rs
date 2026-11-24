//! IAM roles with their policies, in one paginated call (`GetAccountAuthorizationDetails`).
//! Only roles and customer-managed policies are requested: AWS-managed policy documents
//! are thousands and add nothing specific (their names are kept and judged as broad grants).

use crate::{ScanContext, ScanOutput};
use aws_sdk_iam::types::EntityType;
use serde_json::{json, Value};
use sprawlr_domain::iam::{parse_document, Policy};
use sprawlr_domain::{Node, NodeKind};
use std::collections::HashMap;

/// Policy documents come URL-encoded from IAM.
pub fn decode_document(raw: &str) -> Value {
    let decoded = percent_encoding::percent_decode_str(raw).decode_utf8_lossy();
    serde_json::from_str(&decoded).or_else(|_| serde_json::from_str(raw)).unwrap_or_default()
}

pub struct RoleInfo {
    pub arn: String,
    pub name: String,
    pub inline: Vec<(String, String)>,            // (policy name, raw document)
    pub attached: Vec<(String, String)>,          // (policy name, policy ARN)
    pub instance_profiles: Vec<(String, String)>, // (name, ARN)
    pub created_days: Option<i64>,
    pub last_used_days: Option<i64>,
}

pub async fn scan(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let c = aws_sdk_iam::Client::new(&ctx.sdk);
    let now = chrono::Utc::now().timestamp();
    let mut roles = Vec::new();
    let mut managed: HashMap<String, String> = HashMap::new(); // ARN → default version document
    let mut pages =
        c.get_account_authorization_details().filter(EntityType::Role).filter(EntityType::LocalManagedPolicy).into_paginator().send();
    while let Some(page) = pages.next().await {
        let page = page?;
        for r in page.role_detail_list() {
            let (Some(arn), Some(name)) = (r.arn(), r.role_name()) else { continue };
            // Service-linked and SSO roles are managed by AWS: nothing to map or clean up.
            if r.path().is_some_and(|p| p.starts_with("/aws-service-role/") || p.starts_with("/aws-reserved/")) {
                continue;
            }
            roles.push(RoleInfo {
                arn: arn.into(),
                name: name.into(),
                inline: r
                    .role_policy_list()
                    .iter()
                    .filter_map(|p| Some((p.policy_name()?.to_string(), p.policy_document()?.to_string())))
                    .collect(),
                attached: r
                    .attached_managed_policies()
                    .iter()
                    .filter_map(|p| Some((p.policy_name()?.to_string(), p.policy_arn()?.to_string())))
                    .collect(),
                instance_profiles: r
                    .instance_profile_list()
                    .iter()
                    .map(|p| (p.instance_profile_name().to_string(), p.arn().to_string()))
                    .collect(),
                created_days: r.create_date().map(|d| (now - d.secs()) / 86_400),
                last_used_days: r.role_last_used().and_then(|u| u.last_used_date()).map(|d| (now - d.secs()) / 86_400),
            });
        }
        for p in page.policies() {
            let (Some(arn), Some(default)) = (p.arn(), p.default_version_id()) else { continue };
            if let Some(doc) = p.policy_version_list().iter().find(|v| v.version_id() == Some(default)).and_then(|v| v.document()) {
                managed.insert(arn.to_string(), doc.to_string());
            }
        }
    }
    Ok(map_roles(ctx, roles, &managed))
}

pub fn map_roles(ctx: &ScanContext, roles: Vec<RoleInfo>, managed: &HashMap<String, String>) -> ScanOutput {
    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    for r in roles {
        let mut policies: Vec<Policy> = r.inline.iter().map(|(n, doc)| parse_document(n, &decode_document(doc))).collect();
        let mut aws_managed = Vec::new();
        for (name, arn) in &r.attached {
            match managed.get(arn) {
                Some(doc) => policies.push(parse_document(name, &decode_document(doc))),
                None if arn.starts_with("arn:aws:iam::aws:policy/") => aws_managed.push(name.clone()),
                None => {}
            }
        }
        let mut n = Node::new(r.arn.as_str(), NodeKind::IamRole, r.name.clone());
        n.account = Some(ctx.account.clone());
        n.parent = Some(ctx.account_id());
        n.aliases.push(r.name.clone());
        for (name, arn) in &r.instance_profiles {
            n.aliases.push(name.clone());
            n.aliases.push(arn.clone());
        }
        n.attrs = json!({
            "policies": policies,
            "aws_managed_policies": aws_managed,
            "instance_profiles": r.instance_profiles.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            "created_days": r.created_days,
            "last_used_days": r.last_used_days,
        });
        out.nodes.push(n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ScanContext {
        ScanContext { sdk: aws_config::SdkConfig::builder().build(), account: "1".into(), region: "us-east-1".into() }
    }

    #[test]
    fn decodes_url_encoded_documents() {
        let raw = "%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%7B%22Effect%22%3A%22Allow%22%2C%22Action%22%3A%22s3%3AGetObject%22%2C%22Resource%22%3A%22arn%3Aaws%3As3%3A%3A%3Areports%2F%2A%22%7D%5D%7D";
        let doc = decode_document(raw);
        assert_eq!(doc["Statement"][0]["Action"], "s3:GetObject");
        assert_eq!(decode_document(r#"{"Statement": []}"#)["Statement"], json!([]));
    }

    #[test]
    fn roles_carry_policies_profiles_and_managed_names() {
        let managed = HashMap::from([(
            "arn:aws:iam::1:policy/reports-rw".to_string(),
            r#"{"Statement":{"Effect":"Allow","Action":"s3:PutObject","Resource":"arn:aws:s3:::out/*"}}"#.to_string(),
        )]);
        let out = map_roles(
            &ctx(),
            vec![RoleInfo {
                arn: "arn:aws:iam::1:role/worker".into(),
                name: "worker".into(),
                inline: vec![(
                    "inline".into(),
                    r#"{"Statement":[{"Effect":"Allow","Action":"sqs:ReceiveMessage","Resource":"arn:aws:sqs:us-east-1:1:jobs"}]}"#.into(),
                )],
                attached: vec![
                    ("reports-rw".into(), "arn:aws:iam::1:policy/reports-rw".into()),
                    ("AmazonS3ReadOnlyAccess".into(), "arn:aws:iam::aws:policy/AmazonS3ReadOnlyAccess".into()),
                ],
                instance_profiles: vec![("worker-profile".into(), "arn:aws:iam::1:instance-profile/worker-profile".into())],
                created_days: Some(400),
                last_used_days: Some(1),
            }],
            &managed,
        );
        let n = &out.nodes[0];
        assert_eq!(n.attrs["policies"].as_array().unwrap().len(), 2);
        assert_eq!(n.attrs["aws_managed_policies"][0], "AmazonS3ReadOnlyAccess");
        assert!(n.aliases.contains(&"worker-profile".to_string()), "EC2 instance profiles resolve to the role");
    }
}
