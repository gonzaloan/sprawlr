// Words for things: kinds of resource, connection verbs, evidence, how apps were inferred.
import type { ArchNode, EdgeKind, Env, EvidenceKind, Layer, Method } from "./types";



interface KindInfo {
  layer: Layer;
  label: string; // what a person calls it
  glyph: string; // short mark shown on the map
}

const KINDS: Record<string, KindInfo> = {
  account: { layer: "scope", label: "AWS account", glyph: "ACC" },
  region: { layer: "scope", label: "Region", glyph: "RGN" },
  availability_zone: { layer: "scope", label: "Availability zone", glyph: "AZ" },
  vpc: { layer: "network", label: "VPC", glyph: "VPC" },
  subnet: { layer: "network", label: "Subnet", glyph: "SN" },
  security_group: { layer: "network", label: "Security group", glyph: "SG" },
  route_table: { layer: "network", label: "Route table", glyph: "RT" },
  internet_gateway: { layer: "network", label: "Internet gateway", glyph: "IGW" },
  nat_gateway: { layer: "network", label: "NAT gateway", glyph: "NAT" },
  vpc_endpoint: { layer: "network", label: "VPC endpoint", glyph: "VPE" },
  transit_gateway: { layer: "network", label: "Transit gateway", glyph: "TGW" },
  cloud_front: { layer: "edge", label: "CloudFront distribution", glyph: "CF" },
  api_gateway: { layer: "edge", label: "API Gateway", glyph: "API" },
  load_balancer: { layer: "edge", label: "Load balancer", glyph: "ALB" },
  target_group: { layer: "edge", label: "Target group", glyph: "TG" },
  route53_zone: { layer: "edge", label: "Route 53 zone", glyph: "R53" },
  ec2_instance: { layer: "compute", label: "EC2 instance", glyph: "EC2" },
  lambda_function: { layer: "compute", label: "Lambda function", glyph: "λ" },
  ecs_cluster: { layer: "compute", label: "ECS cluster", glyph: "ECS" },
  ecs_service: { layer: "compute", label: "ECS service", glyph: "ECS" },
  eks_cluster: { layer: "compute", label: "EKS cluster", glyph: "EKS" },
  sqs_queue: { layer: "integration", label: "SQS queue", glyph: "SQS" },
  sns_topic: { layer: "integration", label: "SNS topic", glyph: "SNS" },
  event_bus: { layer: "integration", label: "EventBridge bus", glyph: "EVB" },
  event_rule: { layer: "integration", label: "EventBridge rule", glyph: "EVR" },
  state_machine: { layer: "integration", label: "Step Functions", glyph: "SFN" },
  kinesis_stream: { layer: "integration", label: "Kinesis stream", glyph: "KDS" },
  rds_instance: { layer: "data", label: "RDS instance", glyph: "RDS" },
  rds_cluster: { layer: "data", label: "Aurora / RDS cluster", glyph: "RDS" },
  dynamo_table: { layer: "data", label: "DynamoDB table", glyph: "DDB" },
  s3_bucket: { layer: "data", label: "S3 bucket", glyph: "S3" },
  elasti_cache: { layer: "data", label: "ElastiCache", glyph: "EC" },
  iam_role: { layer: "identity", label: "IAM role", glyph: "IAM" },
  iam_policy: { layer: "identity", label: "IAM policy", glyph: "POL" },
  secret: { layer: "identity", label: "Secret", glyph: "SEC" },
  ssm_parameter: { layer: "identity", label: "SSM parameter", glyph: "SSM" },
  kms_key: { layer: "identity", label: "KMS key", glyph: "KMS" },
  internet: { layer: "external", label: "Internet", glyph: "WWW" },
  external_endpoint: { layer: "external", label: "External service", glyph: "EXT" },
};

/** Readable names for types without a dedicated kind. */
const OTHER: Record<string, KindInfo> = {
  "AWS::EC2::EIP": { layer: "network", label: "Elastic IP", glyph: "EIP" },
  "AWS::OpenSearchService::Domain": { layer: "data", label: "OpenSearch domain", glyph: "OS" },
};

export function kindInfo(n: ArchNode): KindInfo {
  const k = KINDS[n.kind.type] ?? (n.kind.name ? OTHER[n.kind.name] : undefined);
  if (k) return k;
  // `Other("AWS::OpenSearchService::Domain")` → "OpenSearchService Domain"
  const parts = (n.kind.name ?? "Resource").split("::").slice(1);
  return { layer: "compute", label: parts.join(" ") || "Resource", glyph: (parts[0] ?? "?").slice(0, 3).toUpperCase() };
}

export const layerOf = (n: ArchNode): Layer => kindInfo(n).layer;

export const EDGE_VERB: Record<EdgeKind, string> = {
  invokes: "invokes",
  reads: "reads",
  writes: "writes",
  publishes: "publishes to",
  subscribes: "subscribes to",
  routes_to: "routes to",
  exposed_via: "exposed via",
  uses_secret: "uses secret",
  encrypted_with: "encrypted with",
  assumes_role: "assumes",
  network_reachable: "can reach",
};

/**
 * Human name for a resource. CloudFormation physical names look like
 * `BillingStack-prod-InvoiceProcessor9F3A21-XK2`; people know it as `InvoiceProcessor`.
 */
export function displayName(n: ArchNode): string {
  const stack = n.tags["aws:cloudformation:stack-name"];
  if (!stack || !n.name.toLowerCase().startsWith(stack.toLowerCase() + "-")) return n.name;
  const rest = n.name.slice(stack.length + 1);
  // Generated suffix: starts with a digit (`9F3A21`, `3c2b1a`), optionally followed by `-XK2`.
  const m = rest.match(/^(.+?)(\d[A-Z0-9]{5,}|\d[a-z0-9]{5,})(-[A-Za-z0-9]+)?$/);
  return (m ? m[1] : rest) || n.name;
}

export const EVIDENCE_LABEL: Record<EvidenceKind, string> = {
  observed: "Seen in traffic",
  declared: "Configured",
  permitted: "Allowed by IAM",
};

export const METHOD_LABEL: Record<Method, string> = {
  tag: "from a tag",
  stack: "from its CloudFormation stack",
  account: "from its account",
  naming: "guessed from its name",
  containment: "inherited from where it runs",
  graph: "inferred from its connections",
  llm: "suggested by AI",
  user_override: "set by you",
};

export const envName = (e: Env): string => (typeof e === "string" ? e : e.custom);
