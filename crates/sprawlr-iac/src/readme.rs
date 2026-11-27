//! The export's README: what is in the folder, what to fill in, what was left out and why,
//! and how to check the code against the account before trusting it.

use crate::model::CodeFile;
use crate::plan::{Export, InputReason};
use std::fmt::Write;

/// One value the person fills in, for the README and the app.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InputLine {
    pub resource: String,
    pub value: String,
    pub why: String,
}

pub fn input_lines(e: &Export) -> Vec<InputLine> {
    e.inputs
        .iter()
        .map(|i| {
            let (value, why) = match &i.reason {
                InputReason::Environment(k) => (k.clone(), "Environment variable values are never written."),
                InputReason::Secret(k) => (k.clone(), "It may be a secret."),
                InputReason::Personal(k) => (k.clone(), "It looks like personal data (an email address or a phone number)."),
                InputReason::UserData => ("UserData".into(), "User data may hold secrets."),
                InputReason::ParameterValue => ("Value".into(), "Parameter values are never written."),
                InputReason::NotReturned(k) => (k.clone(), "AWS never returns it: set it to what you deploy."),
            };
            InputLine { resource: e.resources[i.resource].name.clone(), value, why: why.into() }
        })
        .collect()
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn file(e: &Export) -> CodeFile {
    let n = e.resources.len();
    let mut s = format!("# Code for {}\n\n", e.title());
    let account = e.account.as_deref().map(|a| format!(" of account {a}")).unwrap_or_default();
    let _ = writeln!(
        s,
        "Written by sprawlr on {} from the live configuration{account}: {n} {}, as Terraform and as CloudFormation. Use one of them for each resource, not both.\n",
        e.date,
        if n == 1 { "resource" } else { "resources" }
    );
    s.push_str("| Folder | What it holds |\n|---|---|\n");
    s.push_str("| `docs/` | The documentation of the account, as sprawlr shows it. |\n");
    s.push_str("| `terraform/` | A Terraform project with the awscc provider: a file per app and an import block per resource. |\n");
    s.push_str("| `cloudformation/` | A template per app, and the resources to import into its stack. |\n\n");
    s.push_str("This folder holds details of the account a snapshot does not: identifiers, network ranges, policy conditions. Keep it where you keep code.\n\n");

    s.push_str("## Before you use it\n\n");
    let inputs = input_lines(e);
    if inputs.is_empty() {
        s.push_str("Nothing to fill in: sprawlr found no value to leave out.\n\n");
    } else {
        let _ = writeln!(
            s,
            "### Fill in {} {}\n\nsprawlr never writes values that could be secrets. Set them in `terraform/terraform.tfvars` (start from `terraform.tfvars.example`), or pass them as parameters to CloudFormation.\n",
            inputs.len(),
            if inputs.len() == 1 { "value" } else { "values" }
        );
        s.push_str("| Resource | Value | Why it is not written |\n|---|---|---|\n");
        for i in &inputs {
            let _ = writeln!(s, "| {} | `{}` | {} |", cell(&i.resource), cell(&i.value), i.why);
        }
        s.push('\n');
    }
    if !e.left_out.is_empty() {
        let _ = writeln!(s, "### Left out ({})\n\n| Resource | Why |\n|---|---|", e.left_out.len());
        for l in &e.left_out {
            let _ = writeln!(s, "| {} | {} |", cell(&l.name), cell(&l.reason));
        }
        s.push('\n');
    }
    for note in e.notes() {
        let _ = writeln!(s, "**{note}**\n");
    }

    s.push_str("## Check the Terraform\n\n```bash\ncd terraform\n");
    if !inputs.is_empty() {
        s.push_str("cp terraform.tfvars.example terraform.tfvars   # then fill in the values\n");
    }
    s.push_str("terraform init\nterraform plan\n```\n\n");
    let in_tf = e.in_terraform().len();
    let _ = writeln!(
        s,
        "The plan should end with `Plan: {in_tf} to import, 0 to add, 0 to change, 0 to destroy.` Anything else is a difference between this code and the account: read it before you apply. Applying a plan that only imports records the resources in Terraform's state and changes nothing in AWS.\n"
    );
    let only_cfn: Vec<String> = e.resources.iter().filter(|r| !r.in_terraform).map(|r| format!("{} ({})", r.name, r.type_name)).collect();
    if !only_cfn.is_empty() {
        let _ = writeln!(
            s,
            "Only in CloudFormation: {}. The awscc provider has no resource for {}, so Terraform refers to {} by its identifier.\n",
            only_cfn.join(", "),
            if only_cfn.len() == 1 { "its type" } else { "their types" },
            if only_cfn.len() == 1 { "it" } else { "them" }
        );
    }
    if e.resources.iter().any(|r| !r.ignore_changes.is_empty()) {
        s.push_str("Properties AWS never returns, such as a Lambda function's code, are listed under `lifecycle { ignore_changes }`. Terraform does not compare them, so it never deploys code from a guess. Remove the line once the value is yours.\n\n");
    }

    s.push_str("## Check the CloudFormation\n\n```bash\npip install cfn-lint\ncfn-lint cloudformation/*.json\n```\n\n");
    s.push_str("To bring the resources of a template under a stack without changing them, create an IMPORT change set and look at it before you run it:\n\n");
    for (t, members) in &e.templates() {
        let params: Vec<String> = e
            .inputs
            .iter()
            .filter(|i| members.contains(&i.resource))
            .map(|i| format!("ParameterKey={},ParameterValue=...", i.parameter))
            .collect();
        let _ = write!(
            s,
            "```bash\naws cloudformation create-change-set --stack-name {t} --change-set-name sprawlr-import \\\n  --change-set-type IMPORT --template-body file://cloudformation/{t}.json \\\n  --resources-to-import file://cloudformation/{t}.import.json"
        );
        if !params.is_empty() {
            let _ = write!(s, " \\\n  --parameters {}", params.join(" "));
        }
        let _ = writeln!(s, "\naws cloudformation describe-change-set --stack-name {t} --change-set-name sprawlr-import\n```\n");
    }
    s.push_str("Every change should be `Import`. Run it with `aws cloudformation execute-change-set`; then `aws cloudformation detect-stack-drift --stack-name <stack>` compares the template with the account. A template over 51,200 bytes has to go through S3 (`--template-url`). Every resource has `DeletionPolicy: Retain`, so deleting a stack leaves its resources running.\n\n");

    s.push_str("## Resources\n");
    for g in e.groups() {
        let _ = writeln!(s, "\n### {g}\n\n| Resource | Type | Terraform | CloudFormation |\n|---|---|---|---|");
        for (i, r) in e.resources.iter().enumerate().filter(|(_, r)| r.group == g) {
            let _ = writeln!(
                s,
                "| {} | {} | {} | `{}` in `{}.json` |",
                cell(&r.name),
                r.type_name,
                if r.in_terraform { format!("`{}`", e.tf_address(i)) } else { "not in awscc".into() },
                r.logical_id,
                e.template_of(i)
            );
        }
    }
    CodeFile { path: "README.md".into(), content: s }
}
