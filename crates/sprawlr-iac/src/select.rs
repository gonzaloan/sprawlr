//! Which resources an export reads, how AWS identifies each one, and which it leaves out
//! before reading anything.

use crate::model::{LeftOut, Scope, Wanted};
use sprawlr_domain::analysis::display_name;
use sprawlr_domain::{Node, NodeId, NodeKind};
use std::collections::HashMap;

const STACK_TAG: &str = "aws:cloudformation:stack-name";

/// What an export will read, and what it will not write with the reason.
#[derive(Debug, Default)]
pub struct Selection {
    pub wanted: Vec<Wanted>,
    pub left_out: Vec<LeftOut>,
}

/// The resources of a scope, by id.
pub fn members<'a>(scope: &Scope, nodes: &'a [Node]) -> Vec<&'a Node> {
    let mut out: Vec<&Node> = nodes
        .iter()
        .filter(|n| match scope {
            Scope::Account => true,
            Scope::App(app) => n.app.as_ref().is_some_and(|a| &a.value == app),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn select(scope: &Scope, nodes: &[Node]) -> Selection {
    let by_id: HashMap<&NodeId, &Node> = nodes.iter().map(|n| (&n.id, n)).collect();
    let mut s = Selection::default();
    for n in members(scope, nodes) {
        let Some(type_name) = cloudformation_type(n) else { continue };
        let identifier = identifier_of(n, &type_name);
        match (owned_elsewhere(n, &by_id), identifier) {
            (None, Some(identifier)) => s.wanted.push(Wanted { node: n.id.clone(), type_name, identifier, region: n.region.clone() }),
            (reason, _) => s.left_out.push(LeftOut {
                node: n.id.clone(),
                name: display_name(n),
                reason: reason.unwrap_or_else(|| UNIDENTIFIED.to_string()),
            }),
        }
    }
    s
}

const UNIDENTIFIED: &str = "sprawlr can't tell how AWS identifies it, so it could not be read.";

/// Resources whose code lives somewhere else, or that AWS itself creates and manages.
fn owned_elsewhere(n: &Node, by_id: &HashMap<&NodeId, &Node>) -> Option<String> {
    if let Some(stack) = n.tags.get(STACK_TAG) {
        return Some(format!(
            "Managed by the CloudFormation stack {stack}: that stack's template is its code. Get it with: aws cloudformation get-template --stack-name {stack}"
        ));
    }
    let is_default = |n: &Node| n.attrs.get("is_default").and_then(|v| v.as_bool()) == Some(true);
    let reason = match n.kind {
        NodeKind::IamRole if n.id.0.contains(":role/aws-service-role/") => "A service-linked role: AWS creates and manages it.",
        NodeKind::IamPolicy if n.id.0.contains(":iam::aws:policy/") => "A policy AWS manages.",
        NodeKind::KmsKey if n.attrs.get("managed_by").and_then(|v| v.as_str()) == Some("AWS") || n.name.starts_with("aws/") => {
            "A key AWS manages for one of its services."
        }
        NodeKind::EventBus if n.name == "default" => "The default event bus: every account has one.",
        NodeKind::SecurityGroup if n.name == "default" => "The default security group of its VPC: AWS creates it with the VPC.",
        NodeKind::Vpc if is_default(n) => "The default VPC: AWS creates one in every region.",
        NodeKind::Subnet if n.parent.as_ref().and_then(|p| by_id.get(p)).is_some_and(|v| is_default(v)) => {
            "A subnet of the default VPC: AWS creates it with the VPC."
        }
        _ => return None,
    };
    Some(reason.to_string())
}

/// The CloudFormation type of a resource, if it has one.
pub fn cloudformation_type(n: &Node) -> Option<String> {
    use NodeKind::*;
    let t = match &n.kind {
        Vpc => "AWS::EC2::VPC",
        Subnet => "AWS::EC2::Subnet",
        SecurityGroup => "AWS::EC2::SecurityGroup",
        RouteTable => "AWS::EC2::RouteTable",
        InternetGateway => "AWS::EC2::InternetGateway",
        NatGateway => "AWS::EC2::NatGateway",
        VpcEndpoint => "AWS::EC2::VPCEndpoint",
        TransitGateway => "AWS::EC2::TransitGateway",
        CloudFront => "AWS::CloudFront::Distribution",
        ApiGateway if n.id.0.contains("/restapis/") => "AWS::ApiGateway::RestApi",
        ApiGateway => "AWS::ApiGatewayV2::Api",
        LoadBalancer => "AWS::ElasticLoadBalancingV2::LoadBalancer",
        TargetGroup => "AWS::ElasticLoadBalancingV2::TargetGroup",
        Route53Zone => "AWS::Route53::HostedZone",
        Ec2Instance => "AWS::EC2::Instance",
        LambdaFunction => "AWS::Lambda::Function",
        EcsCluster => "AWS::ECS::Cluster",
        EcsService => "AWS::ECS::Service",
        EksCluster => "AWS::EKS::Cluster",
        SqsQueue => "AWS::SQS::Queue",
        SnsTopic => "AWS::SNS::Topic",
        EventBus => "AWS::Events::EventBus",
        EventRule => "AWS::Events::Rule",
        StateMachine => "AWS::StepFunctions::StateMachine",
        KinesisStream => "AWS::Kinesis::Stream",
        RdsInstance => "AWS::RDS::DBInstance",
        RdsCluster => "AWS::RDS::DBCluster",
        DynamoTable => "AWS::DynamoDB::Table",
        S3Bucket => "AWS::S3::Bucket",
        ElastiCache if n.id.0.contains(":replicationgroup:") => "AWS::ElastiCache::ReplicationGroup",
        ElastiCache => "AWS::ElastiCache::CacheCluster",
        IamRole => "AWS::IAM::Role",
        IamPolicy => "AWS::IAM::ManagedPolicy",
        Secret => "AWS::SecretsManager::Secret",
        SsmParameter => "AWS::SSM::Parameter",
        KmsKey => "AWS::KMS::Key",
        Other(t) if t.starts_with("AWS::") && t.matches("::").count() == 2 => t.as_str(),
        _ => return None,
    };
    Some(t.to_string())
}

/// The identifier Cloud Control reads a resource by (its primary identifier; parts joined by `|`).
pub fn identifier_of(n: &Node, type_name: &str) -> Option<String> {
    let arn = n.id.0.as_str();
    let after = |sep: &str| arn.split_once(sep).map(|(_, t)| t.to_string()).filter(|t| !t.is_empty());
    let last = || arn.rsplit(['/', ':']).next().filter(|t| !t.is_empty()).map(String::from);
    match type_name {
        "AWS::Lambda::Function" => after(":function:").map(|t| t.split(':').next().unwrap_or_default().to_string()),
        "AWS::IAM::Role"
        | "AWS::EC2::VPC"
        | "AWS::EC2::Subnet"
        | "AWS::EC2::SecurityGroup"
        | "AWS::EC2::NatGateway"
        | "AWS::EC2::Instance"
        | "AWS::EC2::InternetGateway"
        | "AWS::EC2::RouteTable"
        | "AWS::EC2::VPCEndpoint"
        | "AWS::EC2::TransitGateway" => last(),
        "AWS::SQS::Queue" => n.aliases.iter().find(|a| a.starts_with("https://sqs.")).cloned().or_else(|| {
            let p: Vec<&str> = arn.split(':').collect();
            (p.len() == 6).then(|| format!("https://sqs.{}.amazonaws.com/{}/{}", p[3], p[4], p[5]))
        }),
        "AWS::DynamoDB::Table" => after(":table/").map(|t| t.split('/').next().unwrap_or_default().to_string()),
        "AWS::S3::Bucket" => after(":::"),
        "AWS::RDS::DBCluster" | "AWS::ElastiCache::CacheCluster" => after(":cluster:"),
        "AWS::RDS::DBInstance" => after(":db:"),
        "AWS::ElastiCache::ReplicationGroup" => after(":replicationgroup:"),
        "AWS::ECS::Cluster" | "AWS::EKS::Cluster" => after(":cluster/"),
        "AWS::ECS::Service" => {
            let (prefix, rest) = arn.split_once(":service/")?;
            let (cluster, _) = rest.split_once('/')?;
            Some(format!("{arn}|{prefix}:cluster/{cluster}"))
        }
        "AWS::Events::EventBus" => after(":event-bus/"),
        "AWS::Kinesis::Stream" => after(":stream/"),
        "AWS::KMS::Key" => after(":key/"),
        "AWS::ApiGateway::RestApi" => after("/restapis/"),
        "AWS::ApiGatewayV2::Api" => after("/apis/"),
        "AWS::CloudFront::Distribution" => after(":distribution/"),
        "AWS::Route53::HostedZone" => after("hostedzone/"),
        "AWS::EC2::EIP" => last().map(|alloc| format!("{}|{alloc}", n.name)),
        "AWS::SSM::Parameter" => Some(n.name.clone()),
        "AWS::SNS::Topic"
        | "AWS::SecretsManager::Secret"
        | "AWS::IAM::ManagedPolicy"
        | "AWS::StepFunctions::StateMachine"
        | "AWS::Events::Rule"
        | "AWS::ElasticLoadBalancingV2::LoadBalancer"
        | "AWS::ElasticLoadBalancingV2::TargetGroup" => arn.starts_with("arn:").then(|| arn.to_string()),
        // Read through Cloud Control by the scan: the identifier it listed comes first.
        _ => n.aliases.first().cloned().or_else(|| Some(n.name.clone()).filter(|s| !s.is_empty())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprawlr_domain::fixtures::demo_graph;

    fn node(arn: &str, kind: NodeKind, name: &str) -> Node {
        Node::new(arn, kind, name)
    }

    fn id(arn: &str, kind: NodeKind, name: &str) -> Option<String> {
        let n = node(arn, kind, name);
        identifier_of(&n, &cloudformation_type(&n)?)
    }

    #[test]
    fn resources_are_read_by_the_identifier_cloud_control_expects() {
        assert_eq!(
            id("arn:aws:lambda:us-east-1:1:function:checkout-email-sender", NodeKind::LambdaFunction, "x").as_deref(),
            Some("checkout-email-sender")
        );
        assert_eq!(
            id("arn:aws:iam::1:role/service/checkout-api-task-role", NodeKind::IamRole, "x").as_deref(),
            Some("checkout-api-task-role")
        );
        assert_eq!(
            id("arn:aws:sqs:us-east-1:111122223333:checkout-order-events", NodeKind::SqsQueue, "x").as_deref(),
            Some("https://sqs.us-east-1.amazonaws.com/111122223333/checkout-order-events")
        );
        assert_eq!(id("arn:aws:dynamodb:us-east-1:1:table/orders", NodeKind::DynamoTable, "x").as_deref(), Some("orders"));
        assert_eq!(id("arn:aws:s3:::reports-output", NodeKind::S3Bucket, "x").as_deref(), Some("reports-output"));
        assert_eq!(id("arn:aws:rds:us-east-1:1:cluster:orders-db", NodeKind::RdsCluster, "x").as_deref(), Some("orders-db"));
        assert_eq!(
            id("arn:aws:ecs:us-east-1:1:service/prod-cluster/checkout-api", NodeKind::EcsService, "x").as_deref(),
            Some("arn:aws:ecs:us-east-1:1:service/prod-cluster/checkout-api|arn:aws:ecs:us-east-1:1:cluster/prod-cluster")
        );
        assert_eq!(id("arn:aws:ec2:us-east-1:1:security-group/sg-alb", NodeKind::SecurityGroup, "x").as_deref(), Some("sg-alb"));
        assert_eq!(
            id("arn:aws:sns:us-east-1:1:checkout-notifications", NodeKind::SnsTopic, "x").as_deref(),
            Some("arn:aws:sns:us-east-1:1:checkout-notifications")
        );
        assert_eq!(id("arn:aws:apigateway:us-east-1::/restapis/a1b2", NodeKind::ApiGateway, "x").as_deref(), Some("a1b2"));
        assert_eq!(id("arn:aws:apigateway:us-east-1::/apis/h1", NodeKind::ApiGateway, "x").as_deref(), Some("h1"));
        assert_eq!(
            id("arn:aws:ec2:us-east-1:1:eip/eipalloc-1", NodeKind::Other("AWS::EC2::EIP".into()), "203.0.113.24").as_deref(),
            Some("203.0.113.24|eipalloc-1")
        );
        assert_eq!(
            id("arn:aws:es:us-east-1:1:domain/search", NodeKind::Other("AWS::OpenSearchService::Domain".into()), "search").as_deref(),
            Some("search")
        );
        assert_eq!(id("internet", NodeKind::Internet, "Internet"), None);
    }

    fn demo_nodes() -> Vec<Node> {
        let mut g = demo_graph();
        sprawlr_domain::infer::infer(&mut g);
        g.nodes().cloned().collect()
    }

    #[test]
    fn an_app_export_reads_only_the_app_and_says_what_a_stack_already_manages() {
        let nodes = demo_nodes();
        let billing = select(&Scope::App("billing".into()), &nodes);
        let left: Vec<&str> = billing.left_out.iter().map(|l| l.name.as_str()).collect();
        assert!(left.contains(&"InvoiceProcessor"), "{left:?}");
        assert!(billing.left_out.iter().all(|l| l.reason.contains("BillingStack-prod")), "{:?}", billing.left_out);

        let checkout = select(&Scope::App("checkout".into()), &nodes);
        let app_of = |id: &NodeId| nodes.iter().find(|n| &n.id == id).and_then(|n| n.app.as_ref()).map(|a| a.value.clone());
        assert!(checkout.wanted.iter().all(|w| app_of(&w.node).as_deref() == Some("checkout")));
        assert!(checkout.wanted.iter().any(|w| w.identifier.ends_with("/checkout-order-events")));
    }

    #[test]
    fn what_aws_creates_by_itself_is_left_out_with_the_reason() {
        let vpc = {
            let mut v = node("arn:aws:ec2:us-east-1:1:vpc/vpc-def", NodeKind::Vpc, "default");
            v.attrs = serde_json::json!({ "is_default": true });
            v
        };
        let mut sub = node("arn:aws:ec2:us-east-1:1:subnet/subnet-def", NodeKind::Subnet, "default-a");
        sub.parent = Some(vpc.id.clone());
        let nodes = vec![
            vpc,
            sub,
            node("arn:aws:iam::1:role/aws-service-role/ecs.amazonaws.com/AWSServiceRoleForECS", NodeKind::IamRole, "AWSServiceRoleForECS"),
            node("arn:aws:events:us-east-1:1:event-bus/default", NodeKind::EventBus, "default"),
            node("arn:aws:ec2:us-east-1:1:security-group/sg-1", NodeKind::SecurityGroup, "default"),
            node("arn:aws:sqs:us-east-1:1:orders", NodeKind::SqsQueue, "orders"),
            {
                let mut k = node("arn:aws:kms:us-east-1:1:key/k2", NodeKind::KmsKey, "aws/s3");
                k.attrs = serde_json::json!({ "managed_by": "AWS" });
                k
            },
            node("ext:api.stripe.com", NodeKind::ExternalEndpoint, "api.stripe.com"),
        ];
        let s = select(&Scope::Account, &nodes);
        assert_eq!(s.wanted.iter().map(|w| w.type_name.as_str()).collect::<Vec<_>>(), ["AWS::SQS::Queue"]);
        assert_eq!(s.left_out.len(), 6, "{:?}", s.left_out);
        assert!(s.left_out.iter().any(|l| l.reason.starts_with("A service-linked role")));
        assert!(s.left_out.iter().any(|l| l.name == "aws/s3" && l.reason.starts_with("A key AWS manages")));
    }
}
