//! CloudFormation: a template per app and region, every resource kept if it leaves the stack
//! (so an import never risks one), and the list of resources an IMPORT change set needs.

use crate::model::CodeFile;
use crate::plan::{Attr, Export, InputReason, Val};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub fn files(e: &Export) -> Vec<CodeFile> {
    let mut out = Vec::new();
    for (name, members) in e.templates() {
        out.push(CodeFile { path: format!("cloudformation/{name}.json"), content: pretty(&template(e, &name, &members)) });
        out.push(CodeFile { path: format!("cloudformation/{name}.import.json"), content: pretty(&imports(e, &members)) });
    }
    out
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default() + "\n"
}

/// Keys inserted in order, so the file is the same whatever map serde_json was built with.
fn object(entries: impl IntoIterator<Item = (String, Value)>) -> Value {
    let sorted: BTreeMap<String, Value> = entries.into_iter().collect();
    let mut m = Map::new();
    for (k, v) in sorted {
        m.insert(k, v);
    }
    Value::Object(m)
}

fn template(e: &Export, name: &str, members: &[usize]) -> Value {
    let account = e.account.as_deref().map(|a| format!(" of account {a}")).unwrap_or_default();
    let mut top = vec![
        ("AWSTemplateFormatVersion".to_string(), json!("2010-09-09")),
        (
            "Description".to_string(),
            json!(format!(
                "{}: written by sprawlr on {} from the configuration{account}. Every resource is kept if it is removed from the stack.",
                capitalised(&e.title()),
                e.date
            )),
        ),
    ];
    let parameters: Vec<(String, Value)> = e
        .inputs
        .iter()
        .filter(|i| members.contains(&i.resource))
        .map(|i| {
            let mut p = vec![("Type".to_string(), json!("String")), ("Description".to_string(), json!(describe(e, i.resource, &i.reason)))];
            if i.sensitive {
                p.push(("NoEcho".to_string(), json!(true)));
            }
            (i.parameter.clone(), object(p))
        })
        .collect();
    if !parameters.is_empty() {
        top.push(("Parameters".to_string(), object(parameters)));
    }
    let resources = members.iter().map(|&i| {
        let r = &e.resources[i];
        let mut body = vec![
            ("Type".to_string(), json!(r.type_name)),
            ("DeletionPolicy".to_string(), json!("Retain")),
            ("UpdateReplacePolicy".to_string(), json!("Retain")),
        ];
        if !r.properties.is_empty() {
            body.push(("Properties".to_string(), object(r.properties.iter().map(|(k, v)| (k.clone(), value(e, name, v))))));
        }
        (r.logical_id.clone(), object(body))
    });
    top.push(("Resources".to_string(), object(resources)));
    object(top)
}

fn value(e: &Export, template: &str, v: &Val) -> Value {
    match v {
        Val::Bool(b) => json!(b),
        Val::Num(n) => Value::Number(n.clone()),
        Val::Str(s) => json!(s),
        Val::List(items) => Value::Array(items.iter().map(|x| value(e, template, x)).collect()),
        Val::Obj(_, m) => object(m.iter().map(|(k, x)| (k.clone(), value(e, template, x)))),
        Val::Input(i) => json!({ "Ref": e.inputs[*i].parameter }),
        // A reference across templates would need exports between stacks: the value stays.
        Val::Ref(r) if e.template_of(r.target) != template => json!(r.literal),
        Val::Ref(r) => {
            let target = &e.resources[r.target].logical_id;
            match &r.attr {
                Attr::Primary(_) => json!({ "Ref": target }),
                Attr::ReadOnly(p) => json!({ "Fn::GetAtt": [target, p] }),
            }
        }
    }
}

fn imports(e: &Export, members: &[usize]) -> Value {
    Value::Array(
        members
            .iter()
            .map(|&i| &e.resources[i])
            .filter(|r| !r.identifier_parts.is_empty())
            .map(|r| {
                object([
                    ("ResourceType".to_string(), json!(r.type_name)),
                    ("LogicalResourceId".to_string(), json!(r.logical_id)),
                    ("ResourceIdentifier".to_string(), object(r.identifier_parts.iter().map(|(k, v)| (k.clone(), json!(v))))),
                ])
            })
            .collect(),
    )
}

/// What an input is and why it is not written, for the person filling it in.
pub fn describe(e: &Export, resource: usize, reason: &InputReason) -> String {
    let name = &e.resources[resource].name;
    match reason {
        InputReason::Environment(k) => format!("Value of the environment variable {k} of {name}. sprawlr never writes it."),
        InputReason::Secret(k) => format!("{k} of {name}: it may be a secret, so sprawlr never writes it."),
        InputReason::Personal(k) => format!("{k} of {name}: it looks like personal data, so sprawlr never writes it."),
        InputReason::UserData => format!("User data of {name}: it may hold secrets, so sprawlr never writes it."),
        InputReason::ParameterValue => format!("Value of the parameter {name}. sprawlr never writes it."),
        InputReason::NotReturned(k) => format!("{k} of {name}: AWS never returns it. Set it to what you deploy."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConfigRead, ResourceConfig, Scope};
    use crate::plan::plan;
    use crate::select::Selection;
    use serde_json::json;
    use sprawlr_domain::NodeKind;

    fn export() -> Export {
        let nodes = vec![
            crate::plan::tests::node("arn:aws:sqs:us-east-1:111122223333:orders", NodeKind::SqsQueue, "orders", "billing"),
            crate::plan::tests::node(
                "arn:aws:lambda:us-east-1:111122223333:function:invoice-processor",
                NodeKind::LambdaFunction,
                "invoice-processor",
                "billing",
            ),
        ];
        let url = "https://sqs.us-east-1.amazonaws.com/111122223333/orders";
        let configs = vec![
            ResourceConfig {
                node: nodes[0].id.clone(),
                type_name: "AWS::SQS::Queue".into(),
                identifier: url.into(),
                properties: json!({ "Arn": nodes[0].id.0, "QueueUrl": url, "QueueName": "orders" }),
            },
            ResourceConfig {
                node: nodes[1].id.clone(),
                type_name: "AWS::Lambda::Function".into(),
                identifier: "invoice-processor".into(),
                properties: json!({ "FunctionName": "invoice-processor", "Role": "arn:aws:iam::111122223333:role/r",
                    "Environment": { "Variables": { "QUEUE_ARN": nodes[0].id.0, "STRIPE_API_KEY": "sk_live_x" } } }),
            },
        ];
        let read = ConfigRead { configs, schemas: crate::plan::tests::schemas(), not_read: vec![] };
        plan(&Scope::App("billing".into()), &nodes, Selection::default(), read, "2026-11-27")
    }

    fn parsed(path: &str) -> Value {
        let f = files(&export()).into_iter().find(|f| f.path == path).unwrap();
        serde_json::from_str(&f.content).unwrap()
    }

    #[test]
    fn a_template_keeps_every_resource_and_refers_to_the_others() {
        let t = parsed("cloudformation/billing.json");
        let q = &t["Resources"]["Orders"];
        assert_eq!(q["Type"], "AWS::SQS::Queue");
        assert_eq!(q["DeletionPolicy"], "Retain");
        assert_eq!(q["UpdateReplacePolicy"], "Retain");
        let vars = &t["Resources"]["InvoiceProcessor"]["Properties"]["Environment"]["Variables"];
        assert_eq!(vars["QUEUE_ARN"], json!({ "Fn::GetAtt": ["Orders", "Arn"] }));
        assert_eq!(vars["STRIPE_API_KEY"], json!({ "Ref": "InvoiceProcessorStripeApiKey" }));
        let p = &t["Parameters"]["InvoiceProcessorStripeApiKey"];
        assert_eq!((p["Type"].as_str(), p["NoEcho"].as_bool()), (Some("String"), Some(true)));
        assert!(!t.to_string().contains("sk_live_x"));
    }

    #[test]
    fn the_import_list_names_each_resource_by_the_properties_that_identify_it() {
        let i = parsed("cloudformation/billing.import.json");
        assert_eq!(
            i[0],
            json!({ "LogicalResourceId": "InvoiceProcessor", "ResourceIdentifier": { "FunctionName": "invoice-processor" }, "ResourceType": "AWS::Lambda::Function" })
        );
        assert_eq!(i[1]["ResourceIdentifier"], json!({ "QueueUrl": "https://sqs.us-east-1.amazonaws.com/111122223333/orders" }));
    }
}
