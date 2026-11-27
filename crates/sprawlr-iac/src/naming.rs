//! Names in the code: Terraform attribute and resource type names exactly as the awscc
//! provider derives them from CloudFormation, and names for the resources sprawlr writes.

/// Substrings the awscc provider rewrites before splitting a name into words.
const REPLACEMENTS: [(&str, &str); 6] = [
    ("CloudFormation", "Cloudformation"),
    ("CloudFront", "Cloudfront"),
    ("CloudWatch", "Cloudwatch"),
    ("CNAMEs", "Cnames"),
    ("FSx", "Fsx"),
    ("OTel", "Otel"),
];

/// The awscc provider's attribute name for a CloudFormation property
/// (`KmsMasterKeyId` → `kms_master_key_id`). A port of its `naming` package, so a wrong guess
/// never makes `terraform validate` fail.
pub fn terraform_attribute(property: &str) -> String {
    let mut name = property.trim().to_string();
    for (old, new) in REPLACEMENTS {
        name = name.replace(old, new);
    }
    let bytes = name.as_bytes();
    let mut out = String::with_capacity(bytes.len() + 4);
    for (i, &ch) in bytes.iter().enumerate() {
        let (cap, low, dig) = (ch.is_ascii_uppercase(), ch.is_ascii_lowercase(), ch.is_ascii_digit());
        let lower = ch.to_ascii_lowercase() as char;
        if let Some(&next) = bytes.get(i + 1) {
            let (next_cap, next_low, next_dig) = (next.is_ascii_uppercase(), next.is_ascii_lowercase(), next.is_ascii_digit());
            if (cap && next_low) || (low && (next_cap || next_dig)) || (dig && (next_cap || next_low)) {
                if cap && next_low && i > 0 && bytes[i - 1].is_ascii_uppercase() {
                    out.push('_');
                }
                out.push(lower);
                if low || dig {
                    out.push('_');
                }
                continue;
            }
        }
        out.push(if cap || low || dig { lower } else { '_' });
    }
    out
}

/// The awscc resource type for a CloudFormation type
/// (`AWS::ElasticLoadBalancingV2::LoadBalancer` → `awscc_elasticloadbalancingv2_load_balancer`).
pub fn terraform_type(type_name: &str) -> Option<String> {
    let mut parts = type_name.split("::");
    let (org, service, resource) = (parts.next()?, parts.next()?, parts.next()?);
    if org != "AWS" || parts.next().is_some() {
        return None;
    }
    Some(format!("awscc_{}_{}", service.to_ascii_lowercase(), terraform_attribute(resource)))
}

/// CloudFormation types the awscc provider has no resource for (checked against its schema,
/// version 1.111.0, for every type sprawlr reads). They are written as CloudFormation only.
const NOT_IN_AWSCC: [&str; 1] = ["AWS::CloudFront::Distribution"];

pub fn in_awscc(type_name: &str) -> bool {
    !NOT_IN_AWSCC.contains(&type_name)
}

/// The awscc attribute for a top-level property, where the provider renames a few to keep
/// Terraform's own names free (`Id` is the resource identifier there).
pub fn terraform_top_attribute(type_name: &str, property: &str) -> String {
    match property {
        "Id" => format!("{}_id", type_name.rsplit("::").next().map(terraform_attribute).unwrap_or_default()),
        _ => match terraform_attribute(property).as_str() {
            "provider" => "provider_name".into(),
            other => other.into(),
        },
    }
}

/// Words of a resource name, split where a person would read a break
/// (`checkout-order_events`, `InvoiceProcessor` → checkout, order, events / Invoice, Processor).
fn words(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            continue;
        }
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1);
        let starts = c.is_ascii_uppercase()
            && prev.is_some_and(|p| {
                p.is_ascii_lowercase() || p.is_ascii_digit() || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
            });
        if starts && !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        word.push(c);
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// A CloudFormation logical id: letters and digits, starting with a letter (`CheckoutOrderEvents`).
pub fn logical_id(name: &str, fallback: &str) -> String {
    let id: String = words(name)
        .iter()
        .map(|w| {
            // `STRIPE_API_KEY` reads as StripeApiKey, not STRIPEAPIKEY.
            let w = if w.len() > 1 && !w.chars().any(|c| c.is_ascii_lowercase()) { w.to_ascii_lowercase() } else { w.clone() };
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect();
    match id.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => id,
        Some(_) => format!("{fallback}{id}"),
        None => fallback.to_string(),
    }
}

/// A Terraform name: lowercase words joined by `_`, starting with a letter (`checkout_order_events`).
pub fn terraform_name(name: &str, fallback: &str) -> String {
    let id = words(name).iter().map(|w| w.to_ascii_lowercase()).collect::<Vec<_>>().join("_");
    match id.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => id,
        Some(_) => format!("{fallback}_{id}"),
        None => fallback.to_string(),
    }
}

/// The first name not taken yet: `name`, then `name2`, `name3`... (Terraform names get `_2`).
pub fn unique(name: String, taken: &mut std::collections::BTreeSet<String>, separator: &str) -> String {
    let mut candidate = name.clone();
    let mut n = 2;
    while taken.contains(&candidate.to_ascii_lowercase()) {
        candidate = format!("{name}{separator}{n}");
        n += 1;
    }
    taken.insert(candidate.to_ascii_lowercase());
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The awscc provider's own cases (internal/naming/naming_test.go).
    #[test]
    fn attribute_names_match_the_awscc_provider() {
        for (cfn, tf) in [
            ("", ""),
            ("  ", ""),
            ("Arn", "arn"),
            ("GlobalReplicationGroupDescription", "global_replication_group_description"),
            ("S3Bucket", "s3_bucket"),
            ("AWS99Thing", "aws99_thing"),
            ("CloudWatchLogsLogGroupArn", "cloudwatch_logs_log_group_arn"),
            ("CNAMEs", "cnames"),
            ("OTelEnrichment", "otel_enrichment"),
        ] {
            assert_eq!(terraform_attribute(cfn), tf, "{cfn}");
        }
    }

    #[test]
    fn attribute_names_keep_acronyms_and_digits_the_way_awscc_does() {
        assert_eq!(terraform_attribute("KmsMasterKeyId"), "kms_master_key_id");
        assert_eq!(terraform_attribute("DBInstanceClass"), "db_instance_class");
        assert_eq!(terraform_attribute("SSESpecification"), "sse_specification");
        assert_eq!(terraform_attribute("Ipv6CidrBlock"), "ipv_6_cidr_block");
    }

    #[test]
    fn resource_types_follow_the_awscc_provider() {
        assert_eq!(terraform_type("AWS::SQS::Queue").as_deref(), Some("awscc_sqs_queue"));
        assert_eq!(terraform_type("AWS::EC2::NatGateway").as_deref(), Some("awscc_ec2_nat_gateway"));
        assert_eq!(terraform_type("AWS::RDS::DBCluster").as_deref(), Some("awscc_rds_db_cluster"));
        assert_eq!(
            terraform_type("AWS::ElasticLoadBalancingV2::LoadBalancer").as_deref(),
            Some("awscc_elasticloadbalancingv2_load_balancer")
        );
        assert_eq!(terraform_type("AWS::EC2::VPC").as_deref(), Some("awscc_ec2_vpc"));
        assert_eq!(terraform_type("Custom::Thing"), None);
        assert!(!in_awscc("AWS::CloudFront::Distribution") && in_awscc("AWS::SQS::Queue"));
    }

    #[test]
    fn top_level_names_terraform_keeps_for_itself_are_renamed() {
        assert_eq!(terraform_top_attribute("AWS::CloudFront::Distribution", "Id"), "distribution_id");
        assert_eq!(terraform_top_attribute("AWS::X::Y", "Provider"), "provider_name");
        assert_eq!(terraform_top_attribute("AWS::SQS::Queue", "QueueName"), "queue_name");
    }

    #[test]
    fn resource_names_read_as_words() {
        assert_eq!(logical_id("checkout-order-events", "Resource"), "CheckoutOrderEvents");
        assert_eq!(logical_id("InvoiceProcessor", "Resource"), "InvoiceProcessor");
        assert_eq!(logical_id("checkout/prod/db-credentials", "Resource"), "CheckoutProdDbCredentials");
        assert_eq!(logical_id("203.0.113.24", "Eip"), "Eip203011324");
        assert_eq!(logical_id("STRIPE_API_KEY", "Value"), "StripeApiKey");
        assert_eq!(terraform_name("checkout-order-events", "resource"), "checkout_order_events");
        assert_eq!(terraform_name("InvoicesTable", "resource"), "invoices_table");
        assert_eq!(terraform_name("PDFRenderer", "resource"), "pdf_renderer");
        assert_eq!(terraform_name("--", "queue"), "queue");
    }

    #[test]
    fn names_already_taken_get_a_number() {
        let mut taken = std::collections::BTreeSet::new();
        assert_eq!(unique("CheckoutApi".into(), &mut taken, ""), "CheckoutApi");
        assert_eq!(unique("CheckoutApi".into(), &mut taken, ""), "CheckoutApi2");
        assert_eq!(unique("checkoutapi".into(), &mut taken, "_"), "checkoutapi_2");
    }
}
