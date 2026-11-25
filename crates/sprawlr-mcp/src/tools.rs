//! The questions an agent can ask, as MCP tools. Each one is a thin call into
//! `sprawlr_app::queries`; answers are JSON an agent can quote back to a person.

use serde_json::{json, Value};
use sprawlr_app::queries;
use sprawlr_domain::Snapshot;

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input: Value,
}

fn none() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn one(field: &str, what: &str) -> Value {
    json!({
        "type": "object",
        "properties": { field: { "type": "string", "description": what } },
        "required": [field],
        "additionalProperties": false
    })
}

fn optional_app() -> Value {
    json!({
        "type": "object",
        "properties": { "app": { "type": "string", "description": "Limit to one app, by name (see list_apps). Leave out for the whole account." } },
        "additionalProperties": false
    })
}

pub fn all() -> Vec<Tool> {
    vec![
        Tool {
            name: "account_status",
            title: "Account status",
            description: "Start here. The account in one line: its level (act_now, plan_for_it, all_clear, not_measured), a headline, the items that need attention in order, flows with errors in the last 24 hours, and what is probably not needed with its monthly cost. Also says when the scan was taken.",
            input: none(),
        },
        Tool {
            name: "list_apps",
            title: "Apps",
            description: "The apps sprawlr recognised (from tags, CloudFormation stacks, naming and how resources connect), with environments, resource and finding counts, their flows, and how each was recognised.",
            input: none(),
        },
        Tool {
            name: "list_flows",
            title: "Flows",
            description: "Every flow: a path requests or events take from an entry point (a public endpoint, an upload, a schedule) through the account, with runs and errors in the last 24 hours.",
            input: none(),
        },
        Tool {
            name: "explain_flow",
            title: "Explain a flow",
            description: "One flow step by step: who calls whom, which steps the caller waits for and which run in the background, the slowest step, the most frequent error messages, and recommendations that touch it.",
            input: one("flow", "The flow's name or part of it, as list_flows shows it (for example \"checkout\")."),
        },
        Tool {
            name: "describe_resource",
            title: "Describe a resource",
            description: "Everything known about one resource: its app and environment, settings, health and metrics, error messages from its logs, what it depends on and what uses it (each connection says whether it is configured, seen in traffic, or only allowed by IAM), findings, the flows it is part of, and whether it is probably not needed.",
            input: one("resource", "A resource's name, the name sprawlr shows, or its ARN. Use find_resources when unsure."),
        },
        Tool {
            name: "blast_radius",
            title: "What breaks if it fails",
            description: "Everything that stops working if a resource fails, through configured or observed connections, plus what IAM would let depend on it without evidence that anything does, and the flows it is on.",
            input: one("resource", "A resource's name, the name sprawlr shows, or its ARN."),
        },
        Tool {
            name: "why_failing",
            title: "Why it is failing",
            description: "Where errors start (the resource whose errors explain the rest), the most frequent error messages in each failing resource's logs (masked: no values or personal data), firing alarms, and flows with errors.",
            input: optional_app(),
        },
        Tool {
            name: "recommendations",
            title: "What to do",
            description: "What needs attention, most important first: bottlenecks, error origins, missing dead-letter queues, single points of failure, then per-resource findings (security, reliability, drift between environments). Each has the evidence and an action.",
            input: optional_app(),
        },
        Tool {
            name: "what_to_clean",
            title: "Probably not needed",
            description: "Resources that look idle (no activity for 14 days), orphaned (nothing uses them) or zombie (still billed, doing nothing), with the reason and a list-price monthly cost where one applies.",
            input: none(),
        },
        Tool {
            name: "find_resources",
            title: "Find resources",
            description: "Resources whose name, id or kind contains some text, best matches first. Use it to get the exact name before describe_resource or blast_radius.",
            input: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Part of a name, an ARN, or a kind such as \"queue\" or \"lambda\"." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
                },
                "required": ["text"],
                "additionalProperties": false
            }),
        },
    ]
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("missing \"{key}\""))
}

fn to_json<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Answer one tool call. `Err` is a message for the agent (unknown name, missing argument,
/// nothing called that), reported as a tool error rather than a protocol error.
pub fn call(name: &str, args: &Value, s: &Snapshot) -> Result<Value, String> {
    let app = args.get("app").and_then(Value::as_str);
    Ok(match name {
        "account_status" => to_json(queries::status(s)),
        "list_apps" => json!({ "apps": queries::apps(s) }),
        "list_flows" => json!({ "flows": queries::flows(s) }),
        "explain_flow" => to_json(queries::flow(s, arg(args, "flow")?).map_err(|e| e.to_string())?),
        "describe_resource" => to_json(queries::describe(s, arg(args, "resource")?).map_err(|e| e.to_string())?),
        "blast_radius" => to_json(queries::blast_radius(s, arg(args, "resource")?).map_err(|e| e.to_string())?),
        "why_failing" => to_json(queries::why_failing(s, app)),
        "recommendations" => json!({ "recommendations": queries::recommendations(s, app) }),
        "what_to_clean" => to_json(queries::cleanup(s)),
        "find_resources" => {
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10).clamp(1, 50) as usize;
            json!({ "resources": queries::find(s, arg(args, "text")?, limit) })
        }
        other => return Err(format!("no tool called \"{other}\"")),
    })
}
