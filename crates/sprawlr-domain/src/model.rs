//! Architecture graph data model.
//!
//! Three ideas drive the design:
//! 1. **Containment is not communication.** `parent` (account → region → VPC → subnet → resource)
//!    lives on the node; edges only describe interactions.
//! 2. **Every edge carries evidence.** An edge can be Declared (config), Permitted (IAM / SG)
//!    or Observed (flow logs / traces). Comparing the three is the core analysis.
//! 3. **Inferences are explicit.** App and environment assignments record *how* they were inferred
//!    and with what confidence, so the UI can show it and the user can override it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable identifier: the ARN when one exists, otherwise a synthetic id
/// (e.g. `internet`, `ext:api.stripe.com`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        NodeId(s.to_string())
    }
}

// ---------------------------------------------------------------- nodes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Scope,       // account, region, AZ
    Network,     // VPC, subnet, SG, gateways
    Edge,        // CloudFront, API Gateway, ALB, Route 53
    Compute,     // EC2, Lambda, ECS, EKS
    Integration, // SQS, SNS, EventBridge, Step Functions, Kinesis
    Data,        // RDS, DynamoDB, S3, ElastiCache
    Identity,    // IAM, Secrets Manager, SSM, KMS
    External,    // internet, third-party endpoints
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "name", rename_all = "snake_case")]
pub enum NodeKind {
    // scope
    Account,
    Region,
    AvailabilityZone,
    // network
    Vpc,
    Subnet,
    SecurityGroup,
    RouteTable,
    InternetGateway,
    NatGateway,
    VpcEndpoint,
    TransitGateway,
    // edge
    CloudFront,
    ApiGateway,
    LoadBalancer,
    TargetGroup,
    Route53Zone,
    // compute
    Ec2Instance,
    LambdaFunction,
    EcsCluster,
    EcsService,
    EksCluster,
    // integration
    SqsQueue,
    SnsTopic,
    EventBus,
    EventRule,
    StateMachine,
    KinesisStream,
    // data
    RdsInstance,
    RdsCluster,
    DynamoTable,
    S3Bucket,
    ElastiCache,
    // identity
    IamRole,
    IamPolicy,
    Secret,
    SsmParameter,
    KmsKey,
    // external
    Internet,
    ExternalEndpoint,
    /// Anything discovered through Cloud Control we don't model yet,
    /// e.g. `AWS::Glue::Job`. Keeps the inventory complete.
    Other(String),
}

impl NodeKind {
    pub fn layer(&self) -> Layer {
        use NodeKind::*;
        match self {
            Account | Region | AvailabilityZone => Layer::Scope,
            Vpc | Subnet | SecurityGroup | RouteTable | InternetGateway | NatGateway | VpcEndpoint | TransitGateway => Layer::Network,
            CloudFront | ApiGateway | LoadBalancer | TargetGroup | Route53Zone => Layer::Edge,
            Ec2Instance | LambdaFunction | EcsCluster | EcsService | EksCluster => Layer::Compute,
            SqsQueue | SnsTopic | EventBus | EventRule | StateMachine | KinesisStream => Layer::Integration,
            RdsInstance | RdsCluster | DynamoTable | S3Bucket | ElastiCache => Layer::Data,
            IamRole | IamPolicy | Secret | SsmParameter | KmsKey => Layer::Identity,
            Internet | ExternalEndpoint => Layer::External,
            Other(_) => Layer::Compute,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceMethod {
    Tag,          // e.g. `app=checkout`, `awsApplication`
    Stack,        // CloudFormation / CDK stack
    Account,      // account or Organizations OU
    Naming,       // `checkout-prod-api`
    Containment,  // inherited from parent (resource inside `prod-vpc`)
    Graph,        // connected to nodes already assigned
    Llm,          // Bedrock fallback
    UserOverride, // always wins
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inferred<T> {
    pub value: T,
    pub method: InferenceMethod,
    /// 0.0..=1.0
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Prod,
    Staging,
    Dev,
    Test,
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ok,
    Warning,
    Alarm,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub account: Option<String>,
    pub region: Option<String>,
    /// Containment parent (subnet for a Lambda in VPC, VPC for a subnet, ...).
    pub parent: Option<NodeId>,
    /// Other strings that refer to this node (queue URL, DB endpoint host, table name...).
    /// Used to resolve references found in env vars and configs.
    #[serde(default)]
    pub aliases: Vec<String>,
    pub app: Option<Inferred<String>>,
    pub env: Option<Inferred<Environment>>,
    pub tags: BTreeMap<String, String>,
    /// Raw, service-specific properties (Cloud Control / Config JSON). Never secret values.
    pub attrs: serde_json::Value,
    pub health: Health,
}

impl Node {
    pub fn new(id: impl Into<NodeId>, kind: NodeKind, name: impl Into<String>) -> Self {
        Node {
            id: id.into(),
            kind,
            name: name.into(),
            account: None,
            region: None,
            parent: None,
            aliases: Vec::new(),
            app: None,
            env: None,
            tags: BTreeMap::new(),
            attrs: serde_json::Value::Null,
            health: Health::Unknown,
        }
    }

    pub fn layer(&self) -> Layer {
        self.kind.layer()
    }
}

// ---------------------------------------------------------------- edges

/// Direction convention: `from` depends on / acts on `to`.
/// (Lambda --reads--> DynamoTable, ALB --routes_to--> EcsService, SNS --publishes--> SQS)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Invokes,
    Reads,
    Writes,
    Publishes,
    Subscribes,
    RoutesTo,
    ExposedVia,
    UsesSecret,
    EncryptedWith,
    AssumesRole,
    NetworkReachable,
}

impl EdgeKind {
    /// Whether a failure of `to` propagates to `from` (used by blast radius).
    pub fn is_runtime_dependency(&self) -> bool {
        !matches!(self, EdgeKind::NetworkReachable | EdgeKind::AssumesRole)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// IAM policy or security group allows it. Weakest signal.
    Permitted,
    /// Configuration wires it: trigger, subscription, env var, integration.
    Declared,
    /// Traffic seen in flow logs, X-Ray or Application Signals. Strongest signal.
    Observed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    /// Where it came from, e.g. `lambda:event_source_mapping`, `env:ORDERS_QUEUE_URL`,
    /// `iam:role/checkout-api`, `vpc_flow_logs`.
    pub source: String,
    pub detail: Option<String>,
    pub seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub kind: EdgeKind,
    pub evidence: Vec<Evidence>,
}

impl Edge {
    /// Strongest evidence on this edge.
    pub fn strength(&self) -> Option<EvidenceKind> {
        self.evidence.iter().map(|e| e.kind).max()
    }

    pub fn has(&self, kind: EvidenceKind) -> bool {
        self.evidence.iter().any(|e| e.kind == kind)
    }

    /// Permitted but never declared nor observed → likely over-permission.
    pub fn is_permission_only(&self) -> bool {
        self.strength() == Some(EvidenceKind::Permitted)
    }
}
