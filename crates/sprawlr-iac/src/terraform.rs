//! Terraform with the awscc provider: a file per app, the inputs as sensitive variables, and an
//! import block per resource, so `terraform plan` checks the code against the account. Written
//! the way `terraform fmt` leaves it.

use crate::cloudformation::describe;
use crate::model::CodeFile;
use crate::naming::{terraform_attribute, terraform_top_attribute};
use crate::plan::{Export, Reference, Val};
use crate::schema::Shape;
use std::collections::BTreeSet;
use std::fmt::Write;

pub const PROVIDER_VERSION: &str = "~> 1.0";

pub fn files(e: &Export) -> Vec<CodeFile> {
    let mut out = vec![CodeFile { path: "terraform/providers.tf".into(), content: providers(e) }];
    if inputs(e).next().is_some() {
        out.push(CodeFile { path: "terraform/variables.tf".into(), content: variables(e) });
        out.push(CodeFile { path: "terraform/terraform.tfvars.example".into(), content: example(e) });
    }
    out.push(CodeFile { path: "terraform/imports.tf".into(), content: imports(e) });
    for group in e.groups() {
        let members: Vec<usize> = e.in_terraform().into_iter().filter(|&i| e.resources[i].group == group).collect();
        if !members.is_empty() {
            out.push(CodeFile { path: format!("terraform/{group}.tf"), content: resources(e, &group, &members) });
        }
    }
    out
}

/// The inputs of resources written as Terraform.
fn inputs(e: &Export) -> impl Iterator<Item = &crate::plan::Input> {
    e.inputs.iter().filter(|i| e.resources[i.resource].in_terraform)
}

fn regions(e: &Export) -> BTreeSet<String> {
    e.in_terraform().into_iter().map(|i| e.resources[i].region.clone()).filter(|r| *r != e.default_region).collect()
}

fn alias(region: &str) -> String {
    region.replace('-', "_")
}

/// `provider = awscc.us_west_2` for a resource outside the default region.
fn provider_of(e: &Export, i: usize) -> Option<String> {
    let r = &e.resources[i].region;
    (*r != e.default_region).then(|| format!("awscc.{}", alias(r)))
}

fn providers(e: &Export) -> String {
    let mut s = format!(
        "terraform {{\n  required_version = \">= 1.5.0\"\n\n  required_providers {{\n    awscc = {{\n      source  = \"hashicorp/awscc\"\n      version = \"{PROVIDER_VERSION}\"\n    }}\n  }}\n}}\n\nprovider \"awscc\" {{\n  region = {}\n}}\n",
        quote(&e.default_region)
    );
    for r in regions(e) {
        let _ = write!(s, "\nprovider \"awscc\" {{\n  alias  = {}\n  region = {}\n}}\n", quote(&alias(&r)), quote(&r));
    }
    s
}

fn variables(e: &Export) -> String {
    let blocks: Vec<String> = inputs(e)
        .map(|i| {
            let mut items =
                vec![Item::attr("description", quote(&describe(e, i.resource, &i.reason))), Item::attr("type", "string".into())];
            if i.sensitive {
                items.push(Item::attr("sensitive", "true".into()));
            }
            format!("variable {} {{\n{}}}\n", quote(&i.variable), body(&items, 1))
        })
        .collect();
    blocks.join("\n")
}

fn example(e: &Export) -> String {
    let mut s = String::from("# Values sprawlr never writes. Copy this file to terraform.tfvars and fill them in.\n");
    for i in inputs(e) {
        let comment = describe(e, i.resource, &i.reason).replace(['\n', '\r'], " ");
        let _ = write!(s, "\n# {comment}\n{} = \"\"\n", i.variable);
    }
    s
}

fn imports(e: &Export) -> String {
    let blocks: Vec<String> = e
        .in_terraform()
        .into_iter()
        .map(|i| {
            let mut items = vec![Item::attr("to", e.tf_address(i)), Item::attr("id", quote(&e.resources[i].identifier))];
            if let Some(p) = provider_of(e, i) {
                items.push(Item::attr("provider", p));
            }
            format!("import {{\n{}}}\n", body(&items, 1))
        })
        .collect();
    format!("# Each resource as it runs today: `terraform plan` imports it and should change nothing.\n\n{}", blocks.join("\n"))
}

fn resources(e: &Export, group: &str, members: &[usize]) -> String {
    let account = e.account.as_deref().map(|a| format!(" of account {a}")).unwrap_or_default();
    let mut s = format!("# {group}: written by sprawlr on {} from the configuration{account}.\n", e.date);
    for &i in members {
        let r = &e.resources[i];
        let mut items = Vec::new();
        if let Some(p) = provider_of(e, i) {
            items.push(Item::attr("provider", p));
            items.push(Item::Blank);
        }
        for (k, v) in &r.properties {
            items.push(Item::Attr(terraform_top_attribute(&r.type_name, k), expr(e, v, 1, false)));
        }
        if !r.ignore_changes.is_empty() {
            let names: Vec<String> = r.ignore_changes.iter().map(|p| terraform_top_attribute(&r.type_name, p)).collect();
            items.push(Item::Blank);
            items.push(Item::Raw(format!("lifecycle {{\n  ignore_changes = [{}]\n}}", names.join(", "))));
        }
        let _ = write!(s, "\nresource {} {} {{\n{}}}\n", quote(&r.tf_type), quote(&r.tf_name), body(&items, 1));
    }
    s
}

/// A line in a block: `name = value`, a nested block, or a blank line.
enum Item {
    Attr(String, String),
    Raw(String),
    Blank,
}

impl Item {
    fn attr(name: &str, value: String) -> Item {
        Item::Attr(name.into(), value)
    }
}

/// Items at `depth`, with the `=` of consecutive one-line attributes aligned, as `terraform fmt` does.
fn body(items: &[Item], depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let mut out = String::new();
    let mut i = 0;
    while i < items.len() {
        match &items[i] {
            Item::Blank => {
                out.push('\n');
                i += 1;
            }
            Item::Raw(text) => {
                for line in text.lines() {
                    let _ = writeln!(out, "{pad}{line}");
                }
                i += 1;
            }
            Item::Attr(name, value) if value.contains('\n') => {
                let _ = writeln!(out, "{pad}{name} = {value}");
                i += 1;
            }
            Item::Attr(..) => {
                let run: Vec<(&String, &String)> = items[i..]
                    .iter()
                    .map_while(|it| match it {
                        Item::Attr(n, v) if !v.contains('\n') => Some((n, v)),
                        _ => None,
                    })
                    .collect();
                let width = run.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
                for (n, v) in &run {
                    let _ = writeln!(out, "{pad}{n:width$} = {v}");
                }
                i += run.len();
            }
        }
    }
    out
}

/// A value as a Terraform expression at `depth`; continuation lines are indented.
fn expr(e: &Export, v: &Val, depth: usize, in_json: bool) -> String {
    let pad = "  ".repeat(depth);
    match v {
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => n.to_string(),
        Val::Str(s) => quote(s),
        Val::Input(i) => format!("var.{}", e.inputs[*i].variable),
        Val::Ref(r) => reference(e, r),
        Val::List(items) if items.is_empty() => "[]".into(),
        Val::List(items) if items.iter().all(|x| !matches!(x, Val::List(_) | Val::Obj(..))) => {
            format!("[{}]", items.iter().map(|x| expr(e, x, depth, in_json)).collect::<Vec<_>>().join(", "))
        }
        Val::List(items) => {
            let inner: String = items.iter().map(|x| format!("{pad}  {},\n", expr(e, x, depth + 1, in_json))).collect();
            format!("[\n{inner}{pad}]")
        }
        Val::Obj(Shape::Json, m) if !in_json => format!("jsonencode({})", object(e, m, depth, true, Shape::Json)),
        Val::Obj(shape, m) => object(e, m, depth, in_json, if in_json { Shape::Json } else { *shape }),
    }
}

fn object(e: &Export, m: &std::collections::BTreeMap<String, Val>, depth: usize, in_json: bool, shape: Shape) -> String {
    if m.is_empty() {
        return "{}".into();
    }
    let items: Vec<Item> = m
        .iter()
        .map(|(k, v)| {
            let key = match shape {
                Shape::Attrs | Shape::Unknown | Shape::List | Shape::Scalar => terraform_attribute(k),
                Shape::Map | Shape::Json => quote(k),
            };
            Item::Attr(key, expr(e, v, depth + 1, in_json))
        })
        .collect();
    format!("{{\n{}{}}}", body(&items, depth + 1), "  ".repeat(depth))
}

/// `awscc_sqs_queue.orders.arn`; the Cloud Control identifier is `.id`.
fn reference(e: &Export, r: &Reference) -> String {
    let target = &e.resources[r.target];
    if !target.in_terraform {
        return quote(&r.literal);
    }
    let prop = r.attr.property();
    let attr =
        if prop == "Id" && r.literal == target.identifier { "id".to_string() } else { terraform_top_attribute(&target.type_name, prop) };
    format!("{}.{attr}", e.tf_address(r.target))
}

/// A Terraform string: escapes, and `${` / `%{` kept literal.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' | '%' if chars.peek() == Some(&'{') => {
                out.push(c);
                out.push(c);
            }
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConfigRead, ResourceConfig, Scope};
    use crate::plan::plan;
    use crate::select::Selection;
    use serde_json::json;
    use sprawlr_domain::NodeKind;

    fn export(second_region: bool) -> Export {
        let mut nodes = vec![
            crate::plan::tests::node("arn:aws:sqs:us-east-1:111122223333:orders", NodeKind::SqsQueue, "orders", "billing"),
            crate::plan::tests::node(
                "arn:aws:lambda:us-east-1:111122223333:function:invoice-processor",
                NodeKind::LambdaFunction,
                "invoice-processor",
                "billing",
            ),
        ];
        if second_region {
            nodes[1].region = Some("eu-west-1".into());
        }
        let url = "https://sqs.us-east-1.amazonaws.com/111122223333/orders";
        let configs = vec![
            ResourceConfig {
                node: nodes[0].id.clone(),
                type_name: "AWS::SQS::Queue".into(),
                identifier: url.into(),
                properties: json!({ "Arn": nodes[0].id.0, "QueueUrl": url, "QueueName": "orders", "VisibilityTimeout": 30,
                    "RedrivePolicy": { "deadLetterTargetArn": "arn:aws:sqs:us-east-1:111122223333:dlq", "maxReceiveCount": 5 },
                    "Tags": [{"Key": "app", "Value": "billing"}] }),
            },
            ResourceConfig {
                node: nodes[1].id.clone(),
                type_name: "AWS::Lambda::Function".into(),
                identifier: "invoice-processor".into(),
                properties: json!({ "FunctionName": "invoice-processor", "Role": "arn:aws:iam::111122223333:role/r",
                    "Environment": { "Variables": { "QUEUE_URL": url, "STRIPE_API_KEY": "sk_live_x" } } }),
            },
        ];
        let read = ConfigRead { configs, schemas: crate::plan::tests::schemas(), not_read: vec![] };
        plan(&Scope::App("billing".into()), &nodes, Selection::default(), read, "2026-11-27")
    }

    fn file(e: &Export, path: &str) -> String {
        files(e).into_iter().find(|f| f.path == path).map(|f| f.content).unwrap_or_else(|| panic!("no {path}"))
    }

    #[test]
    fn resources_are_written_as_terraform_fmt_leaves_them() {
        let tf = file(&export(false), "terraform/billing.tf");
        let expected = r#"# billing: written by sprawlr on 2026-11-27 from the configuration of account 111122223333.

resource "awscc_lambda_function" "invoice_processor" {
  code = {
    s3_bucket = var.invoice_processor_code_s3_bucket
    s3_key    = var.invoice_processor_code_s3_key
  }
  environment = {
    variables = {
      "QUEUE_URL"      = awscc_sqs_queue.orders.queue_url
      "STRIPE_API_KEY" = var.invoice_processor_stripe_api_key
    }
  }
  function_name = "invoice-processor"
  role          = "arn:aws:iam::111122223333:role/r"

  lifecycle {
    ignore_changes = [code]
  }
}

resource "awscc_sqs_queue" "orders" {
  queue_name = "orders"
  redrive_policy = jsonencode({
    "deadLetterTargetArn" = "arn:aws:sqs:us-east-1:111122223333:dlq"
    "maxReceiveCount"     = 5
  })
  tags = [
    {
      key   = "app"
      value = "billing"
    },
  ]
  visibility_timeout = 30
}
"#;
        assert_eq!(tf, expected);
    }

    #[test]
    fn every_resource_is_imported_by_its_cloud_control_identifier() {
        let imports = file(&export(false), "terraform/imports.tf");
        assert!(
            imports.contains(
                "import {\n  to = awscc_sqs_queue.orders\n  id = \"https://sqs.us-east-1.amazonaws.com/111122223333/orders\"\n}\n"
            ),
            "{imports}"
        );
    }

    #[test]
    fn inputs_are_sensitive_variables_with_nothing_filled_in() {
        let e = export(false);
        let vars = file(&e, "terraform/variables.tf");
        assert!(vars.contains(
            "variable \"invoice_processor_stripe_api_key\" {\n  description = \"Value of the environment variable STRIPE_API_KEY of invoice-processor. sprawlr never writes it.\"\n  type        = string\n  sensitive   = true\n}\n"
        ), "{vars}");
        assert!(file(&e, "terraform/terraform.tfvars.example").contains("invoice_processor_stripe_api_key = \"\""));
        assert!(!files(&e).iter().any(|f| f.content.contains("sk_live_x")));
    }

    #[test]
    fn resources_in_other_regions_use_their_own_provider() {
        let e = export(true);
        assert_eq!(e.default_region, "eu-west-1", "a tie goes to the first region by name");
        let providers = file(&e, "terraform/providers.tf");
        assert!(providers.contains("provider \"awscc\" {\n  alias  = \"us_east_1\"\n  region = \"us-east-1\"\n}\n"), "{providers}");
        assert!(file(&e, "terraform/billing.tf").contains("resource \"awscc_sqs_queue\" \"orders\" {\n  provider = awscc.us_east_1\n\n"));
        assert!(file(&e, "terraform/imports.tf").contains("  provider = awscc.us_east_1\n"));
    }

    #[test]
    fn inputs_of_resources_only_in_cloudformation_are_not_terraform_variables() {
        let mut e = export(false);
        e.resources[0].in_terraform = false;
        assert!(files(&e).iter().all(|f| f.path != "terraform/variables.tf"), "only the CloudFormation resource had inputs");
        let tf = file(&e, "terraform/billing.tf");
        assert!(!tf.contains("awscc_lambda_function") && tf.contains("awscc_sqs_queue"), "{tf}");
    }

    #[test]
    fn strings_keep_what_terraform_would_read_as_templates() {
        assert_eq!(quote("a\"b\\c\n${x}%{y}$z"), r#""a\"b\\c\n$${x}%%{y}$z""#);
    }
}
