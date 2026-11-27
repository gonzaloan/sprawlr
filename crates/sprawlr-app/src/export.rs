//! Export as code: Terraform and CloudFormation for the account or one app, written from each
//! resource's configuration as it is at the moment of the export (ADR 0006).

use crate::ports::{ConfigSource, Progress};
use sprawlr_domain::Snapshot;
use sprawlr_iac::{CodeFile, ConfigRead, Scope, Summary};

/// The files of an export (without the documentation, which drivers add under `docs/`), and
/// what the person should know about them.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeExport {
    pub files: Vec<CodeFile>,
    pub summary: Summary,
}

/// `date` is the day of the export, for the files' headers: the core reads no clock.
pub async fn export_code<S: ConfigSource>(
    source: &S,
    snapshot: &Snapshot,
    scope: &Scope,
    date: &str,
    progress: &Progress,
) -> anyhow::Result<CodeExport> {
    if let Scope::App(app) = scope {
        anyhow::ensure!(
            snapshot.nodes.iter().any(|n| n.app.as_ref().is_some_and(|a| &a.value == app)),
            "there is no app named {app} in this scan"
        );
    }
    let selection = sprawlr_iac::select(scope, &snapshot.nodes);
    let read = if selection.wanted.is_empty() { ConfigRead::default() } else { source.read_configs(&selection.wanted, progress).await? };
    let export = sprawlr_iac::plan(scope, &snapshot.nodes, selection, read, date);
    Ok(CodeExport { files: sprawlr_iac::render(&export), summary: sprawlr_iac::summary(&export) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::{snapshot, Demo};
    use crate::ports::silent;

    async fn export(scope: Scope) -> CodeExport {
        export_code(&Demo, &snapshot(), &scope, "2026-11-27", &silent()).await.unwrap()
    }

    fn file<'a>(e: &'a CodeExport, path: &str) -> &'a str {
        &e.files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("no {path} in {:?}", e.files.iter().map(|f| &f.path).collect::<Vec<_>>()))
            .content
    }

    #[tokio::test]
    async fn the_checkout_app_becomes_terraform_and_cloudformation_with_an_import_per_resource() {
        let e = export(Scope::App("checkout".into())).await;
        let tf = file(&e, "terraform/checkout.tf");
        for r in [
            "awscc_sqs_queue\" \"checkout_order_events\"",
            "awscc_sns_topic\" \"checkout_notifications\"",
            "awscc_iam_role\" \"checkout_api_task_role\"",
            "awscc_lambda_function\" \"checkout_email_sender\"",
        ] {
            assert!(tf.contains(r), "{r} missing from\n{tf}");
        }
        let imports = file(&e, "terraform/imports.tf");
        assert_eq!(imports.matches("import {").count(), e.summary.terraform);
        assert_eq!(e.summary.resources - e.summary.terraform, 1, "the CloudFront distribution is only in CloudFormation");
        let template: serde_json::Value = serde_json::from_str(file(&e, "cloudformation/checkout.json")).unwrap();
        let resources = template["Resources"].as_object().unwrap();
        assert_eq!(resources.len(), e.summary.resources);
        assert!(resources.values().all(|r| r["DeletionPolicy"] == "Retain"));
    }

    #[tokio::test]
    async fn no_value_that_could_be_a_secret_is_written() {
        let e = export(Scope::Account).await;
        for secret in ["orders@example.com", "demo-token-not-real", "IyEvYmluL2Jhc2gKZXhwb3J0IFJFUE9SVFNfVE9LRU49bm90LXJlYWwK"] {
            assert!(e.files.iter().all(|f| !f.content.contains(secret)), "{secret} was written");
        }
        let values: Vec<&str> = e.summary.inputs.iter().map(|i| i.value.as_str()).collect();
        assert!(values.contains(&"REPORTS_API_TOKEN") && values.contains(&"UserData"), "{values:?}");
    }

    #[tokio::test]
    async fn an_app_a_stack_already_manages_is_left_out_with_where_its_code_is() {
        let e = export(Scope::App("billing".into())).await;
        assert_eq!(e.summary.resources, 0);
        assert!(!e.summary.left_out.is_empty());
        assert!(file(&e, "README.md").contains("aws cloudformation get-template --stack-name BillingStack-prod"));
    }

    #[tokio::test]
    async fn the_same_account_gives_the_same_files() {
        assert_eq!(export(Scope::Account).await.files, export(Scope::Account).await.files);
    }

    #[tokio::test]
    async fn an_app_that_does_not_exist_is_said_plainly() {
        let err = export_code(&Demo, &snapshot(), &Scope::App("nope".into()), "2026-11-27", &silent()).await.unwrap_err();
        assert_eq!(err.to_string(), "there is no app named nope in this scan");
    }
}
