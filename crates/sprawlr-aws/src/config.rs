//! Each resource's whole configuration, for an export (ADR 0006): Cloud Control `GetResource`,
//! the call a scan already makes, and each type's schema from CloudFormation `DescribeType`.
//! What is read here goes into the export and is never stored.

use crate::cloudcontrol::{client, failure};
use crate::{context, session, Aws, Progress};
use aws_sdk_cloudformation::types::RegistryType;
use futures::{stream, StreamExt};
use serde_json::Value;
use sprawlr_app::ConfigSource;
use sprawlr_iac::{ConfigRead, NotRead, ResourceConfig, Wanted};
use std::collections::{BTreeMap, BTreeSet};

const CONCURRENCY: usize = 8;

impl ConfigSource for Aws {
    async fn read_configs(&self, wanted: &[Wanted], progress: &Progress) -> anyhow::Result<ConfigRead> {
        progress("signing in".into());
        let (base, account) = session(self.profile.as_deref()).await?;
        if let Some(scanned) = wanted.iter().find_map(|w| account_in(&w.node.0)).filter(|a| *a != account) {
            let who = self.profile.as_deref().map(|p| format!("profile {p}")).unwrap_or_else(|| "the default credentials".into());
            anyhow::bail!(
                "this scan is of account {scanned}, but {who} signs in to account {account}. Export with the profile for {scanned}."
            );
        }
        let fallback = self.regions.first().cloned().unwrap_or_else(|| "us-east-1".into());
        Ok(read_all(&base, &account, &fallback, wanted, progress).await)
    }
}

/// The account an ARN names (`arn:aws:sqs:us-east-1:111122223333:orders`), if any.
fn account_in(id: &str) -> Option<&str> {
    id.strip_prefix("arn:")?.split(':').nth(3).filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()))
}

/// CloudFront answers only in us-east-1. IAM is global and answers in any region.
fn region_of(w: &Wanted, fallback: &str) -> String {
    if w.type_name.starts_with("AWS::CloudFront::") {
        return "us-east-1".into();
    }
    w.region.clone().unwrap_or_else(|| fallback.to_string())
}

async fn read_all(base: &aws_config::SdkConfig, account: &str, fallback: &str, wanted: &[Wanted], progress: &Progress) -> ConfigRead {
    let mut by_region: BTreeMap<String, Vec<&Wanted>> = BTreeMap::new();
    for w in wanted {
        by_region.entry(region_of(w, fallback)).or_default().push(w);
    }
    let mut read = ConfigRead::default();
    for (region, list) in &by_region {
        progress(format!("{region}: reading the configuration of {} resources", list.len()));
        let cc = client(&context(base, account, region).sdk);
        let c = &cc;
        let owned: Vec<Wanted> = list.iter().map(|w| (*w).clone()).collect();
        let results: Vec<(Wanted, Result<Value, String>)> = stream::iter(owned)
            .map(|w| async move {
                let r = read_one(c, &w).await;
                (w, r)
            })
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;
        for (w, r) in results {
            match r {
                Ok(properties) => {
                    read.configs.push(ResourceConfig { node: w.node, type_name: w.type_name, identifier: w.identifier, properties })
                }
                Err(reason) => read.not_read.push(NotRead { node: w.node, reason }),
            }
        }
    }
    let types: BTreeSet<String> = read.configs.iter().map(|c| c.type_name.clone()).collect();
    if !types.is_empty() {
        progress("reading how each type is described".into());
        let sdk = context(base, account, fallback).sdk;
        let retry = aws_config::retry::RetryConfig::adaptive().with_max_attempts(6);
        let cfn =
            aws_sdk_cloudformation::Client::from_conf(aws_sdk_cloudformation::config::Builder::from(&sdk).retry_config(retry).build());
        for t in types {
            match describe(&cfn, &t).await {
                Ok(schema) => {
                    read.schemas.insert(t, schema);
                }
                // Without its schema a type can't be written right: its resources are left out.
                Err(reason) => {
                    let (out, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut read.configs).into_iter().partition(|c| c.type_name == t);
                    read.configs = kept;
                    let why = format!("AWS would not describe its type, {t}: {reason}");
                    read.not_read.extend(out.into_iter().map(|c| NotRead { node: c.node, reason: why.clone() }));
                }
            }
        }
    }
    read.configs.sort_by(|a, b| a.node.cmp(&b.node));
    read.not_read.sort_by(|a, b| a.node.cmp(&b.node));
    read
}

/// A resource's model, or why AWS would not give it, in AWS's words.
async fn read_one(c: &aws_sdk_cloudcontrol::Client, w: &Wanted) -> Result<Value, String> {
    let r = c.get_resource().type_name(&w.type_name).identifier(&w.identifier).send().await.map_err(|e| plain(&failure(&e).message))?;
    let props = r.resource_description().and_then(|d| d.properties()).ok_or("AWS returned no properties")?;
    serde_json::from_str(props).map_err(|e| format!("AWS returned properties that are not JSON: {e}"))
}

async fn describe(c: &aws_sdk_cloudformation::Client, type_name: &str) -> Result<Value, String> {
    use aws_sdk_cloudformation::error::ProvideErrorMetadata;
    let r = c.describe_type().r#type(RegistryType::Resource).type_name(type_name).send().await.map_err(|e| {
        let message = match e.as_service_error() {
            Some(s) => format!("{}: {}", s.code().unwrap_or("Error"), s.message().unwrap_or_default()),
            None => e.to_string(),
        };
        plain(&message)
    })?;
    let schema = r.schema().ok_or("AWS returned no schema")?;
    serde_json::from_str(schema).map_err(|e| format!("AWS returned a schema that is not JSON: {e}"))
}

const ADD_POLICY: &str = "Add the OptionalExport statement of iam-policy.json to the role sprawlr uses.";

/// AWS's message for the README and the app, without who asked: an access denial names the
/// session (with SSO, often a person's email) or the user. A denial says what to add.
fn plain(message: &str) -> String {
    let mut out = mask_after(message, ":assumed-role/", 1);
    out = mask_after(&out, ":user/", 0);
    if out.contains("AccessDenied") || out.contains("not authorized") {
        out = format!("{}. {ADD_POLICY}", out.trim_end_matches('.'));
    }
    out
}

/// Replaces what follows `marker` with `…`, keeping `keep` path segments (a role's name).
fn mask_after(message: &str, marker: &str, keep: usize) -> String {
    let mut out = String::new();
    let mut rest = message;
    while let Some(at) = rest.find(marker) {
        let (before, after) = rest.split_at(at + marker.len());
        out.push_str(before);
        let end = after.find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',').unwrap_or(after.len());
        let kept: Vec<&str> = after[..end].split('/').take(keep).collect();
        out.push_str(&kept.iter().map(|k| format!("{k}/")).collect::<String>());
        out.push('…');
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sdk_answering;
    use sprawlr_app::ports::silent;

    fn queue() -> Wanted {
        Wanted {
            node: "arn:aws:sqs:us-east-1:111122223333:orders".into(),
            type_name: "AWS::SQS::Queue".into(),
            identifier: "https://sqs.us-east-1.amazonaws.com/111122223333/orders".into(),
            region: Some("us-east-1".into()),
        }
    }

    const QUEUE: (u16, &str) = (
        200,
        r#"{"TypeName":"AWS::SQS::Queue","ResourceDescription":{"Identifier":"https://sqs.us-east-1.amazonaws.com/111122223333/orders","Properties":"{\"QueueName\":\"orders\",\"VisibilityTimeout\":30}"}}"#,
    );
    const SCHEMA: (u16, &str) = (
        200,
        r#"<DescribeTypeResponse xmlns="http://cloudformation.amazonaws.com/doc/2010-05-15/"><DescribeTypeResult><Schema>{"typeName":"AWS::SQS::Queue","primaryIdentifier":["/properties/QueueUrl"]}</Schema></DescribeTypeResult></DescribeTypeResponse>"#,
    );
    const DENIED: (u16, &str) =
        (400, r#"{"__type":"AccessDeniedException","Message":"not authorized to perform: sqs:GetQueueAttributes"}"#);

    #[tokio::test]
    async fn reads_each_resource_and_the_schema_of_its_type() {
        let sdk = sdk_answering("ap-south-1", |call| if call == 0 { QUEUE } else { SCHEMA });
        let read = read_all(&sdk, "111122223333", "ap-south-1", &[queue()], &silent()).await;
        assert_eq!(read.configs[0].properties["VisibilityTimeout"], 30);
        assert_eq!(read.schemas["AWS::SQS::Queue"]["primaryIdentifier"][0], "/properties/QueueUrl");
        assert!(read.not_read.is_empty());
    }

    #[tokio::test]
    async fn a_resource_aws_refuses_is_reported_in_aws_s_words() {
        let sdk = sdk_answering("ap-south-2", |_| DENIED);
        let read = read_all(&sdk, "111122223333", "ap-south-2", &[queue()], &silent()).await;
        assert_eq!(
            read.not_read[0].reason,
            format!("AccessDeniedException: not authorized to perform: sqs:GetQueueAttributes. {ADD_POLICY}")
        );
        assert!(read.configs.is_empty() && read.schemas.is_empty());
    }

    #[tokio::test]
    async fn a_type_aws_won_t_describe_leaves_its_resources_out_with_the_reason() {
        let refused = (
            400,
            r#"<ErrorResponse><Error><Type>Sender</Type><Code>AccessDenied</Code><Message>User: arn:aws:sts::111122223333:assumed-role/ReadOnly/jane@example.com is not authorized to perform: cloudformation:DescribeType</Message></Error></ErrorResponse>"#,
        );
        let sdk = sdk_answering("ap-southeast-3", move |call| if call == 0 { QUEUE } else { refused });
        let read = read_all(&sdk, "111122223333", "ap-southeast-3", &[queue()], &silent()).await;
        assert!(read.configs.is_empty());
        let reason = &read.not_read[0].reason;
        assert!(reason.starts_with("AWS would not describe its type, AWS::SQS::Queue: AccessDenied"), "{reason}");
        assert!(!reason.contains("jane"), "the session is not written: {reason}");
        assert!(reason.contains("assumed-role/ReadOnly/…") && reason.ends_with(ADD_POLICY), "{reason}");
    }

    #[test]
    fn messages_keep_the_role_and_drop_who_asked() {
        assert_eq!(
            plain("User: arn:aws:iam::1:user/jane.doe is not authorized to perform: sqs:GetQueueAttributes"),
            format!("User: arn:aws:iam::1:user/… is not authorized to perform: sqs:GetQueueAttributes. {ADD_POLICY}")
        );
        assert_eq!(plain("ResourceNotFoundException: no such queue"), "ResourceNotFoundException: no such queue");
    }

    #[test]
    fn cloudfront_is_read_where_it_answers_and_arns_name_their_account() {
        let cdn = Wanted {
            node: "arn:aws:cloudfront::1:distribution/E1".into(),
            type_name: "AWS::CloudFront::Distribution".into(),
            identifier: "E1".into(),
            region: Some("eu-west-1".into()),
        };
        assert_eq!(region_of(&cdn, "eu-west-1"), "us-east-1");
        assert_eq!(region_of(&Wanted { region: None, ..queue() }, "eu-west-1"), "eu-west-1");
        assert_eq!(account_in("arn:aws:sqs:us-east-1:111122223333:orders"), Some("111122223333"));
        assert_eq!(account_in("arn:aws:s3:::reports"), None);
        assert_eq!(account_in("internet"), None);
    }
}
