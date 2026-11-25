//! Error lines from CloudWatch Logs, counted with Logs Insights and kept only as patterns
//! (see `sprawlr_domain::logs`). Opt-in: Logs Insights reads log content and bills per GB scanned.

use crate::{Progress, ScanContext};
use aws_sdk_cloudwatchlogs::types::{QueryStatus, ResultField};
use sprawlr_domain::logs::{group, LogErrors, Row};
use sprawlr_domain::{ArchGraph, NodeId};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Duration;

pub const HOURS: u32 = 24;
/// StartQuery accepts at most 50 log groups.
const PER_QUERY: usize = 50;

/// Error lines, grouped by log group and their first 400 characters. Patterns are merged
/// afterwards, on our side, so ids and numbers do not split one error into many rows.
pub const QUERY: &str = r#"fields @timestamp, @log, substr(@message, 0, 400) as msg
| filter @message like /(?i)(\berror\b|exception|\bfatal\b|\bpanic\b|traceback|timed out|\bunhandled\b)/
| stats count(*) as n, max(@timestamp) as last by @log, msg
| sort n desc
| limit 5000"#;

/// Log groups a resource writes to, recorded by the scanner on the node.
fn groups_of(n: &sprawlr_domain::Node) -> Vec<String> {
    n.attrs.get("log_groups").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect()
}

/// `@log` is `<account>:<log group>`.
pub fn log_group_of(field: &str) -> &str {
    match field.split_once(':') {
        Some((acct, rest)) if acct.chars().all(|c| c.is_ascii_digit()) => rest,
        _ => field,
    }
}

pub fn rows_by_group(results: &[Vec<ResultField>]) -> BTreeMap<String, Vec<Row>> {
    let mut out: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for r in results {
        let get = |k: &str| r.iter().find(|f| f.field() == Some(k)).and_then(|f| f.value());
        let (Some(log), Some(msg)) = (get("@log"), get("msg")) else { continue };
        let count = get("n").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
        out.entry(log_group_of(log).to_string()).or_default().push(Row {
            message: msg.to_string(),
            count,
            last_seen: get("last").map(String::from),
        });
    }
    out
}

pub async fn apply(g: &mut ArchGraph, ctx: &ScanContext, progress: &Progress) -> Vec<String> {
    match run(g, ctx, progress).await {
        Ok(w) => w,
        Err(e) => vec![format!("{}/logs: {}", ctx.region, crate::describe(&e))],
    }
}

async fn run(g: &mut ArchGraph, ctx: &ScanContext, progress: &Progress) -> anyhow::Result<Vec<String>> {
    let mut wanted: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
    for n in g.nodes().filter(|n| n.region.as_deref() == Some(ctx.region.as_str())) {
        for lg in groups_of(n) {
            wanted.entry(lg).or_default().push(n.id.clone());
        }
    }
    if wanted.is_empty() {
        return Ok(vec![]);
    }
    let c = aws_sdk_cloudwatchlogs::Client::new(&ctx.sdk);

    // A query fails if any group is missing (a function that never ran has none).
    let mut existing: HashSet<String> = HashSet::new();
    let mut pages = c.describe_log_groups().into_paginator().send();
    while let Some(page) = pages.next().await {
        existing.extend(page?.log_groups().iter().filter_map(|l| l.log_group_name().map(String::from)));
    }
    let groups: Vec<String> = wanted.keys().filter(|lg| existing.contains(*lg)).cloned().collect();
    if groups.is_empty() {
        return Ok(vec![]);
    }

    progress(format!("{}: reading error lines from {} log groups", ctx.region, groups.len()));
    let end = chrono::Utc::now().timestamp();
    let start = end - i64::from(HOURS) * 3600;
    let mut rows: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    let mut warnings = Vec::new();
    for batch in groups.chunks(PER_QUERY) {
        let id = c
            .start_query()
            .set_log_group_names(Some(batch.to_vec()))
            .start_time(start)
            .end_time(end)
            .query_string(QUERY)
            .send()
            .await?
            .query_id()
            .unwrap_or_default()
            .to_string();
        let mut done = false;
        for _ in 0..90 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let r = c.get_query_results().query_id(&id).send().await?;
            match r.status() {
                Some(QueryStatus::Complete) => {
                    for (lg, list) in rows_by_group(r.results()) {
                        rows.entry(lg).or_default().extend(list);
                    }
                    done = true;
                    break;
                }
                Some(QueryStatus::Failed | QueryStatus::Cancelled | QueryStatus::Timeout) => {
                    warnings.push(format!("{}/logs: a Logs Insights query ended as {:?}", ctx.region, r.status()));
                    done = true;
                    break;
                }
                _ => {}
            }
        }
        if !done {
            let _ = c.stop_query().query_id(&id).send().await;
            warnings.push(format!("{}/logs: a Logs Insights query took over 90 s and was stopped", ctx.region));
        }
    }

    // Per resource: every group it writes to, merged into patterns.
    let mut per_node: BTreeMap<NodeId, BTreeSet<String>> = BTreeMap::new();
    for lg in &groups {
        for id in &wanted[lg] {
            per_node.entry(id.clone()).or_default().insert(lg.clone());
        }
    }
    for (id, lgs) in per_node {
        let (total, patterns) = group(lgs.iter().flat_map(|lg| rows.get(lg).cloned().unwrap_or_default()));
        let l = LogErrors { hours: HOURS, log_groups: lgs.into_iter().collect(), total, patterns, read_at: Some(chrono::Utc::now()) };
        if let Some(n) = g.node_mut(&id) {
            if !n.attrs.is_object() {
                n.attrs = serde_json::json!({});
            }
            n.attrs["log_errors"] = serde_json::to_value(l).unwrap_or_default();
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(k: &str, v: &str) -> ResultField {
        ResultField::builder().field(k).value(v).build()
    }

    #[test]
    fn rows_are_grouped_by_log_group() {
        let results = vec![
            vec![
                field("@log", "123456789012:/aws/lambda/Invoice"),
                field("msg", "[ERROR] Timeout after 3012 ms"),
                field("n", "11"),
                field("last", "2026-11-24 11:52:03.120"),
            ],
            vec![field("@log", "123456789012:/aws/lambda/Invoice"), field("msg", "[ERROR] Timeout after 2999 ms"), field("n", "8")],
            vec![field("@log", "123456789012:/ecs/checkout-api"), field("msg", "pool exhausted"), field("n", "2")],
        ];
        let by = rows_by_group(&results);
        assert_eq!(by["/aws/lambda/Invoice"].len(), 2);
        let (total, p) = group(by["/aws/lambda/Invoice"].clone());
        assert_eq!(total, 19.0);
        assert_eq!(p.len(), 1, "one template: {p:?}");
        assert_eq!(log_group_of("/no/account"), "/no/account");
    }
}
