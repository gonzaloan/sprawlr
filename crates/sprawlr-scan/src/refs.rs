//! Detect references to other resources inside configuration values (env vars, etc.).

use crate::Ref;
use sprawlr_core::NodeId;

/// What a single configuration entry tells us.
#[derive(Debug, PartialEq)]
pub enum Finding {
    Reference(Ref),
    /// Looks like a secret stored in plain text. The value is never kept.
    PlaintextSecret,
    Nothing,
}

const SECRET_HINTS: [&str; 6] = ["PASSWORD", "PASSWD", "SECRET", "TOKEN", "API_KEY", "PRIVATE_KEY"];

pub fn classify(key: &str, value: &str) -> Finding {
    let v = value.trim();
    let k = key.to_ascii_uppercase();

    if v.starts_with("arn:aws") {
        return Finding::Reference(Ref::Id(NodeId(normalize_arn(v))));
    }
    if let Some(url) = sqs_url(v) {
        return Finding::Reference(Ref::Alias(url));
    }
    if let Some(host) = aws_host(v) {
        return Finding::Reference(Ref::Alias(host));
    }
    // Bare names: only trust them when the key says what they are.
    let is_name = !v.is_empty() && v.len() <= 255 && !v.contains(char::is_whitespace);
    if is_name && (k.ends_with("TABLE") || k.ends_with("TABLE_NAME")) {
        return Finding::Reference(Ref::Alias(format!("dynamodb:{v}")));
    }
    if is_name && (k.ends_with("BUCKET") || k.ends_with("BUCKET_NAME")) {
        return Finding::Reference(Ref::Alias(format!("s3:{v}")));
    }
    // SSM parameter / secret *names* are references, not secrets.
    if v.starts_with('/') && k.contains("PARAM") {
        return Finding::Reference(Ref::Alias(format!("ssm:{v}")));
    }
    if v.len() >= 8 && SECRET_HINTS.iter().any(|h| k.contains(h)) {
        return Finding::PlaintextSecret;
    }
    Finding::Nothing
}

/// Secrets Manager ARNs may be referenced with a JSON-key suffix (`...:secret:name-AbC123:password::`).
pub(crate) fn normalize_arn(arn: &str) -> String {
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() > 7 && parts[2] == "secretsmanager" {
        return parts[..7].join(":");
    }
    arn.to_string()
}

fn sqs_url(v: &str) -> Option<String> {
    let rest = v.strip_prefix("https://")?;
    (rest.starts_with("sqs.") && rest.contains(".amazonaws.com/")).then(|| v.trim_end_matches('/').to_string())
}

/// Hostnames of AWS endpoints (RDS, ElastiCache, API Gateway, ALB...), with or without scheme/port/path.
fn aws_host(v: &str) -> Option<String> {
    let s = v.split("://").last()?;
    let host = s.split(['/', ':', '?']).next()?;
    let known = [".rds.amazonaws.com", ".cache.amazonaws.com", ".execute-api.", ".elb.amazonaws.com", ".cloudfront.net"];
    known.iter().any(|k| host.contains(k)).then(|| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(s: &str) -> Finding {
        Finding::Reference(Ref::Alias(s.into()))
    }

    #[test]
    fn detects_references() {
        assert_eq!(
            classify("ORDERS_QUEUE_URL", "https://sqs.us-east-1.amazonaws.com/123/orders"),
            alias("https://sqs.us-east-1.amazonaws.com/123/orders")
        );
        assert_eq!(classify("DB_HOST", "orders.cluster-abc.us-east-1.rds.amazonaws.com:5432"), alias("orders.cluster-abc.us-east-1.rds.amazonaws.com"));
        assert_eq!(classify("ORDERS_TABLE", "orders-prod"), alias("dynamodb:orders-prod"));
        assert_eq!(classify("UPLOAD_BUCKET", "acme-uploads"), alias("s3:acme-uploads"));
    }

    #[test]
    fn secret_arn_suffix_is_stripped() {
        let f = classify("DB_SECRET_ARN", "arn:aws:secretsmanager:us-east-1:123:secret:db-AbC123:password::");
        assert_eq!(f, Finding::Reference(Ref::Id(NodeId("arn:aws:secretsmanager:us-east-1:123:secret:db-AbC123".into()))));
    }

    #[test]
    fn flags_plaintext_secrets_but_not_references() {
        assert_eq!(classify("STRIPE_API_KEY", "sk_live_51Habc123"), Finding::PlaintextSecret);
        assert_eq!(classify("DB_PASSWORD", "short"), Finding::Nothing);
        assert!(matches!(classify("API_TOKEN_SECRET_ARN", "arn:aws:secretsmanager:us-east-1:1:secret:x-AbC123"), Finding::Reference(_)));
    }

    #[test]
    fn ignores_plain_config() {
        assert_eq!(classify("LOG_LEVEL", "debug"), Finding::Nothing);
    }
}
