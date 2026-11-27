//! IAM roles with their policies and trust policies, in one paginated call
//! (`GetAccountAuthorizationDetails`).
//! Only roles and customer-managed policies are requested: AWS-managed policy documents
//! are thousands and add nothing specific (their names are kept and judged as broad grants).

use crate::{ScanContext, ScanOutput};
use aws_sdk_iam::types::EntityType;
use serde_json::{json, Value};
use sprawlr_domain::iam::{parse_document, trusted_by, Policy};
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
    pub last_used_region: Option<String>,
    /// The trust policy (`AssumeRolePolicyDocument`), URL-encoded as IAM returns it.
    pub trust: Option<String>,
    /// The permissions boundary's policy ARN.
    pub boundary: Option<String>,
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
                last_used_region: r.role_last_used().and_then(|u| u.region()).map(String::from),
                trust: r.assume_role_policy_document().map(String::from),
                boundary: r.permissions_boundary().and_then(|b| b.permissions_boundary_arn()).map(String::from),
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
        // Who may take the role on; condition values are dropped by `trusted_by` (ADR 0003).
        // A document that doesn't decode leaves who may take the role on unknown, not "nobody".
        if let Some(doc) = r.trust.as_deref().map(decode_document).filter(|d| d.get("Statement").is_some()) {
            n.attrs["trust"] = json!(trusted_by(&doc));
        }
        if let Some(arn) = &r.boundary {
            n.attrs["permissions_boundary"] = json!(arn.rsplit('/').next().unwrap_or(arn));
        }
        if let Some(region) = &r.last_used_region {
            n.attrs["last_used_region"] = json!(region);
        }
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
    fn a_trust_policy_keeps_condition_keys_and_drops_their_values() {
        let trust = r#"{"Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::444455556666:root"},"Action":"sts:AssumeRole","Condition":{"StringEquals":{"sts:ExternalId":"vendor-shared-secret"}}}]}"#;
        let out = map_roles(&ctx(), vec![role_trusted_by(trust)], &HashMap::new());
        let attrs = out.nodes[0].attrs.to_string();
        assert!(attrs.contains("sts:ExternalId") && attrs.contains("444455556666"), "{attrs}");
        assert!(!attrs.contains("vendor-shared-secret"), "an external id is a shared secret: {attrs}");
    }

    #[test]
    fn a_trust_policy_that_cannot_be_read_is_unknown_not_empty() {
        let out = map_roles(&ctx(), vec![role_trusted_by("not a policy {")], &HashMap::new());
        assert!(out.nodes[0].attrs.get("trust").is_none(), "{}", out.nodes[0].attrs);
    }

    #[test]
    fn a_role_read_before_its_trust_policy_says_nothing_about_it() {
        let mut r = role_trusted_by("");
        r.trust = None;
        let out = map_roles(&ctx(), vec![r], &HashMap::new());
        assert!(out.nodes[0].attrs.get("trust").is_none(), "unknown, not 'nobody'");
    }

    fn role_trusted_by(doc: &str) -> RoleInfo {
        RoleInfo {
            arn: "arn:aws:iam::1:role/vendor-audit".into(),
            name: "vendor-audit".into(),
            inline: vec![],
            attached: vec![],
            instance_profiles: vec![],
            created_days: None,
            last_used_days: None,
            last_used_region: None,
            trust: Some(doc.into()),
            boundary: None,
        }
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
                last_used_region: Some("us-east-1".into()),
                trust: Some("%7B%22Statement%22%3A%5B%7B%22Effect%22%3A%22Allow%22%2C%22Principal%22%3A%7B%22Service%22%3A%22ec2.amazonaws.com%22%7D%2C%22Action%22%3A%22sts%3AAssumeRole%22%7D%5D%7D".into()),
                boundary: Some("arn:aws:iam::1:policy/team-boundary".into()),
            }],
            &managed,
        );
        let n = &out.nodes[0];
        assert_eq!(n.attrs["policies"].as_array().unwrap().len(), 2);
        assert_eq!(n.attrs["trust"][0]["id"], "ec2.amazonaws.com");
        assert_eq!(n.attrs["permissions_boundary"], "team-boundary");
        assert_eq!(n.attrs["last_used_region"], "us-east-1");
        assert_eq!(n.attrs["aws_managed_policies"][0], "AmazonS3ReadOnlyAccess");
        assert!(n.aliases.contains(&"worker-profile".to_string()), "EC2 instance profiles resolve to the role");
    }
}
