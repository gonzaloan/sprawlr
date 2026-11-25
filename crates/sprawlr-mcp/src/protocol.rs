//! The Model Context Protocol over stdio: JSON-RPC 2.0, one message per line. Only what a
//! read-only tool server needs: initialize, ping, tools/list and tools/call.
//! Spec: https://modelcontextprotocol.io/specification

use crate::source::Source;
use crate::tools;
use serde_json::{json, Value};

/// Newest first; a client asking for one of these gets it back.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub struct Server {
    pub source: Source,
}

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn fail(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

impl Server {
    /// Answer one message. Notifications (no `id`) get no answer.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id")?.clone();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        Some(match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
                ok(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": "sprawlr", "title": "sprawlr", "version": env!("CARGO_PKG_VERSION") },
                        "instructions": format!(
                            "sprawlr answers questions about one AWS account from its last read-only scan ({}). \
                             Start with account_status. Every connection says how it is known: configured, seen in traffic, \
                             or only allowed by IAM; say which when you rely on one. Error messages are masked patterns. \
                             Nothing here changes the account.",
                            self.source.describe()
                        ),
                    }),
                )
            }
            "ping" => ok(&id, json!({})),
            "tools/list" => ok(
                &id,
                json!({ "tools": tools::all().into_iter().map(|t| json!({
                    "name": t.name,
                    "title": t.title,
                    "description": t.description,
                    "inputSchema": t.input,
                    "annotations": { "readOnlyHint": true, "openWorldHint": false }
                })).collect::<Vec<_>>() }),
            ),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let answer = match self.source.snapshot() {
                    Ok(s) => tools::call(name, &args, s),
                    Err(e) => Err(e.to_string()),
                };
                ok(
                    &id,
                    match answer {
                        Ok(v) => json!({
                            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                            "structuredContent": v,
                            "isError": false
                        }),
                        Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
                    },
                )
            }
            _ => fail(&id, -32601, &format!("method not found: {method}")),
        })
    }

    /// One line in, at most one line out. A line that is not JSON gets a parse error.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.handle(&msg)?,
            Err(e) => fail(&Value::Null, -32700, &format!("parse error: {e}")),
        };
        Some(reply.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server { source: Source::demo() }
    }

    fn call(s: &mut Server, line: &str) -> Value {
        serde_json::from_str(&s.handle_line(line).expect("a reply")).unwrap()
    }

    #[test]
    fn handshake_negotiates_the_version() {
        let mut s = server();
        let r = call(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "sprawlr");
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#);
        assert_eq!(r["result"]["protocolVersion"], VERSIONS[0]);
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none(), "notifications get no reply");
    }

    #[test]
    fn lists_read_only_tools_with_schemas() {
        let mut s = server();
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 10);
        assert!(tools.iter().all(|t| t["annotations"]["readOnlyHint"] == true && t["inputSchema"]["type"] == "object"));
    }

    #[test]
    fn answers_questions_and_reports_mistakes_as_tool_errors() {
        let mut s = server();
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"account_status","arguments":{}}}"#);
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(r["result"]["structuredContent"]["status"]["level"], "act_now");
        let r = call(
            &mut s,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"blast_radius","arguments":{"resource":"checkout-orders-db"}}}"#,
        );
        assert!(r["result"]["structuredContent"]["breaks"].as_array().unwrap().iter().any(|x| x["name"] == "checkout-api"));
        let r = call(
            &mut s,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"describe_resource","arguments":{"resource":"nope"}}}"#,
        );
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("find"));
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"explain_flow","arguments":{}}}"#);
        assert_eq!(r["result"]["content"][0]["text"], "missing \"flow\"");
    }

    #[test]
    fn protocol_errors() {
        let mut s = server();
        assert_eq!(call(&mut s, "not json")["error"]["code"], -32700);
        assert_eq!(call(&mut s, r#"{"jsonrpc":"2.0","id":9,"method":"resources/list"}"#)["error"]["code"], -32601);
    }

    #[test]
    fn a_missing_scan_says_how_to_make_one() {
        let mut s = Server { source: Source::file("/nonexistent/last-snapshot.json".into()) };
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"account_status","arguments":{}}}"#);
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Scan an account"));
    }
}
