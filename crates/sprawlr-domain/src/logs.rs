//! Error messages from logs, reduced to patterns.
//!
//! The scanner asks CloudWatch Logs Insights for error lines and their counts. Each line is
//! turned into a template: ids, numbers, addresses, tokens and quoted values are masked, so
//! `Timeout calling api.stripe.com after 3012 ms (req 7f1c…)` and the same error a minute later
//! count as one pattern. Only templates are stored: never the raw line, which can carry
//! personal data or secrets.

use crate::Node;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// How many patterns are kept per resource.
pub const KEEP: usize = 8;
const MAX_LEN: usize = 160;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPattern {
    pub message: String,
    pub count: f64,
    /// UTC, as Logs Insights returns it (`2026-11-24 13:05:12.000`).
    #[serde(default)]
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogErrors {
    pub hours: u32,
    pub log_groups: Vec<String>,
    /// Error lines in the window, all patterns included.
    pub total: f64,
    pub patterns: Vec<ErrorPattern>,
    /// When the logs were read. Health refreshes in the background skip logs (they bill per
    /// GB), so patterns can be older than the metrics next to them.
    #[serde(default)]
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One row from Logs Insights: a message (or its first characters), how often, last seen.
#[derive(Debug, Clone)]
pub struct Row {
    pub message: String,
    pub count: f64,
    pub last_seen: Option<String>,
}

struct Masks {
    prefix: Regex,
    level: Regex,
    lambda_id: Regex,
    url_query: Regex,
    email: Regex,
    uuid: Regex,
    ip: Regex,
    aws_id: Regex,
    hex: Regex,
    token: Regex,
    quoted: Regex,
    number: Regex,
    space: Regex,
}

fn masks() -> &'static Masks {
    static M: OnceLock<Masks> = OnceLock::new();
    M.get_or_init(|| Masks {
        // Leading timestamps: 2026-11-24T13:05:12.123Z, [2026-11-24 13:05:12,123], 13:05:12.123
        prefix: Regex::new(r"^\s*\[?(?:\d{4}-\d{2}-\d{2}[T ])?\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?\]?\s*").unwrap(),
        // Lambda's "<request id>\t" after the timestamp.
        // Every line here is an error: the level adds nothing to the template.
        level: Regex::new(r"(?i)^\[?(?:error|err|fatal|critical|crit|severe)\]?\s*[:|-]?\s+").unwrap(),
        lambda_id: Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\s+").unwrap(),
        url_query: Regex::new(r"\?[^\s]*").unwrap(),
        email: Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap(),
        uuid: Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap(),
        ip: Regex::new(r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b").unwrap(),
        // i-0abc12345, sg-1234abcd, vpc-..., eni-...
        aws_id: Regex::new(r"\b[a-z]{1,10}-[0-9a-f]{8,17}\b").unwrap(),
        hex: Regex::new(
            r"\b(?:0x)?[0-9a-fA-F]*\d[0-9a-fA-F]*[a-fA-F][0-9a-fA-F]*\b|\b(?:0x)?[0-9a-fA-F]*[a-fA-F][0-9a-fA-F]*\d[0-9a-fA-F]*\b",
        )
        .unwrap(),
        token: Regex::new(r"[A-Za-z0-9_\-+/=]{24,}").unwrap(),
        quoted: Regex::new(r#""([^"]*)"|'([^']*)'"#).unwrap(),
        number: Regex::new(r"\b\d+(?:\.\d+)?\b").unwrap(),
        space: Regex::new(r"\s+").unwrap(),
    })
}

/// The line that says what went wrong: JSON logs give their message field; a Python
/// traceback gives its last line; otherwise the first line.
fn headline(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            let field = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
            let kind = field("errorType").or_else(|| field("error_type"));
            let msg = field("errorMessage")
                .or_else(|| field("message"))
                .or_else(|| field("msg"))
                .or_else(|| field("error"))
                .or_else(|| v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).map(str::to_string));
            match (kind, msg) {
                (Some(k), Some(m)) => return format!("{k}: {m}"),
                (None, Some(m)) => return headline(&m),
                (Some(k), None) => return k,
                _ => {}
            }
        }
    }
    let lines: Vec<&str> = t.split(['\n', '\r']).map(str::trim).filter(|l| !l.is_empty()).collect();
    match lines.as_slice() {
        [first, .., last] if first.contains("Traceback (most recent call last)") => last.to_string(),
        [first, ..] => first.to_string(),
        [] => String::new(),
    }
}

/// A log line reduced to its pattern. Same error, same template.
pub fn fingerprint(raw: &str) -> String {
    let m = masks();
    let mut s = headline(raw);
    // Prefixes repeat (timestamp, request id, timestamp again in some runtimes).
    for _ in 0..3 {
        let before = s.len();
        s = m.prefix.replace(&s, "").into_owned();
        s = m.lambda_id.replace(&s, "").into_owned();
        s = m.level.replace(&s, "").into_owned();
        if s.len() == before {
            break;
        }
    }
    s = m.url_query.replace_all(&s, "?<query>").into_owned();
    s = m.email.replace_all(&s, "<email>").into_owned();
    s = m.uuid.replace_all(&s, "<id>").into_owned();
    s = m.ip.replace_all(&s, "<ip>").into_owned();
    s = m.aws_id.replace_all(&s, "<id>").into_owned();
    s = m
        .quoted
        .replace_all(&s, |c: &regex::Captures| {
            let (q, inner) = match (c.get(1), c.get(2)) {
                (Some(x), _) => ('"', x.as_str()),
                (_, Some(x)) => ('\'', x.as_str()),
                _ => return c[0].to_string(),
            };
            // Names (a missing key, a field) say what failed; values could be anyone's data.
            let name_like = !inner.is_empty()
                && inner.len() <= 40
                && inner.chars().all(|ch| ch.is_ascii_alphanumeric() || "_.-:".contains(ch))
                && !inner.chars().any(|ch| ch.is_ascii_digit());
            if name_like {
                c[0].to_string()
            } else {
                format!("{q}<value>{q}")
            }
        })
        .into_owned();
    s = m
        .token
        .replace_all(&s, |c: &regex::Captures| {
            // Long words made only of letters are words; anything mixing in digits is a token.
            let t = &c[0];
            if t.chars().any(|ch| ch.is_ascii_digit()) {
                "<token>".to_string()
            } else {
                t.to_string()
            }
        })
        .into_owned();
    s = m.hex.replace_all(&s, |c: &regex::Captures| if c[0].len() >= 8 { "<id>".to_string() } else { c[0].to_string() }).into_owned();
    s = m.number.replace_all(&s, "<n>").into_owned();
    s = m.space.replace_all(s.trim(), " ").into_owned();
    if s.chars().count() > MAX_LEN {
        s = s.chars().take(MAX_LEN - 1).collect::<String>() + "…";
    }
    s
}

/// Merge rows by template, most frequent first.
pub fn group(rows: impl IntoIterator<Item = Row>) -> (f64, Vec<ErrorPattern>) {
    let mut by: HashMap<String, ErrorPattern> = HashMap::new();
    let mut total = 0.0;
    for r in rows {
        let message = fingerprint(&r.message);
        if message.is_empty() {
            continue;
        }
        total += r.count;
        let p = by.entry(message.clone()).or_insert(ErrorPattern { message, count: 0.0, last_seen: None });
        p.count += r.count;
        if r.last_seen.as_deref() > p.last_seen.as_deref() {
            p.last_seen = r.last_seen;
        }
    }
    let mut list: Vec<ErrorPattern> = by.into_values().collect();
    list.sort_by(|a, b| b.count.total_cmp(&a.count).then(a.message.cmp(&b.message)));
    list.truncate(KEEP);
    (total, list)
}

/// Error patterns the scanner stored on a resource.
pub fn of(n: &Node) -> Option<LogErrors> {
    n.attrs.get("log_errors").and_then(|v| serde_json::from_value(v.clone()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lambda_python_lines_lose_time_and_request_id() {
        let a = fingerprint("2026-11-24T13:05:12.123Z\t7f1c2a9e-1b2c-4d5e-8f90-a1b2c3d4e5f6\t[ERROR] KeyError: 'customer_id'\nTraceback (most recent call last):\n  File \"/var/task/app.py\", line 41");
        assert_eq!(a, "KeyError: 'customer_id'");
        assert_eq!(fingerprint("ERROR: payment declined for order 42"), "payment declined for order <n>");
        assert_eq!(fingerprint("Error: ENOTFOUND db.internal"), "ENOTFOUND db.internal", "a level word followed by the message");
    }

    #[test]
    fn same_error_different_values_is_one_template() {
        let a = fingerprint("Timeout calling https://api.stripe.com/v1/charges?key=sk_live_abc after 3012 ms (req 9f8e7d6c5b4a)");
        let b = fingerprint("Timeout calling https://api.stripe.com/v1/charges?key=sk_live_xyz after 2950 ms (req 1a2b3c4d5e6f)");
        assert_eq!(a, b);
        assert_eq!(a, "Timeout calling https://api.stripe.com/v1/charges?<query> after <n> ms (req <id>)");
    }

    #[test]
    fn personal_data_and_tokens_are_masked() {
        let s = fingerprint(
            r#"User ana.perez@example.com from 10.0.3.17:443 failed: invalid token eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 for "Ana Pérez""#,
        );
        assert!(!s.contains("ana.perez") && !s.contains("10.0.3.17") && !s.contains("eyJ") && !s.contains("Ana"), "{s}");
        assert_eq!(s, r#"User <email> from <ip> failed: invalid token <token> for "<value>""#);
    }

    #[test]
    fn json_logs_use_their_message() {
        let s = fingerprint(
            r#"{"level":"error","errorType":"ConditionalCheckFailedException","errorMessage":"invoice 88412 already exists","requestId":"x"}"#,
        );
        assert_eq!(s, "ConditionalCheckFailedException: invoice <n> already exists");
        assert_eq!(
            fingerprint(r#"{"level":"error","msg":"pool exhausted (max 20) on i-0abc12345def"}"#),
            "pool exhausted (max <n>) on <id>"
        );
    }

    #[test]
    fn plain_python_traceback_uses_its_last_line() {
        let s = fingerprint(
            "Traceback (most recent call last):\n  File \"x.py\", line 3, in <module>\nValueError: amount must be positive, got -12",
        );
        assert_eq!(s, "ValueError: amount must be positive, got -<n>");
    }

    #[test]
    fn grouping_merges_and_orders() {
        let rows = vec![
            Row { message: "Task timed out after 6.01 seconds".into(), count: 3.0, last_seen: Some("2026-11-24 10:00:00.000".into()) },
            Row { message: "Task timed out after 5.99 seconds".into(), count: 4.0, last_seen: Some("2026-11-24 12:00:00.000".into()) },
            Row { message: "KeyError: 'sku'".into(), count: 2.0, last_seen: None },
        ];
        let (total, p) = group(rows);
        assert_eq!(total, 9.0);
        assert_eq!(p[0].message, "Task timed out after <n> seconds");
        assert_eq!(p[0].count, 7.0);
        assert_eq!(p[0].last_seen.as_deref(), Some("2026-11-24 12:00:00.000"));
        assert_eq!(p[1].message, "KeyError: 'sku'");
    }
}
