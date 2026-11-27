// What the resource panel says about one resource: what it does, who may take a role on, what a
// role may do, who IAM lets in, and how it is set up. The rules live in Rust (iam.rs); this is words.
import type { Index } from "./graph";
import { nameList } from "./format";
import type { AccessLevel, ArchNode, ChangeRow, Principal, ServiceAccess, Snapshot } from "./types";
import { displayName } from "./vocabulary";

const PURPOSE: Record<string, string> = {
  lambda_function: "Runs code when something calls it.",
  ecs_service: "Keeps a number of containers running.",
  ecs_cluster: "Groups ECS services and tasks.",
  eks_cluster: "Runs Kubernetes.",
  ec2_instance: "A virtual server.",
  iam_role: "Gives permissions to whatever takes it on.",
  iam_policy: "A set of permissions.",
  dynamo_table: "Stores items by key.",
  s3_bucket: "Stores files (objects).",
  rds_instance: "A relational database.",
  rds_cluster: "A relational database.",
  elasti_cache: "An in-memory cache.",
  sqs_queue: "Holds messages until a worker takes them.",
  sns_topic: "Sends each message to every subscriber.",
  event_bus: "Routes events to the rules that match them.",
  event_rule: "Sends matching events to its targets.",
  state_machine: "Runs a workflow of steps.",
  kinesis_stream: "Carries a stream of records, in order.",
  api_gateway: "Receives HTTP requests and passes them on.",
  cloud_front: "Serves content from locations near the people asking.",
  load_balancer: "Spreads requests across its targets.",
  target_group: "The targets a load balancer sends requests to.",
  route53_zone: "Answers DNS for a domain.",
  secret: "Keeps a secret value, such as a password.",
  ssm_parameter: "Keeps a configuration value.",
  kms_key: "Encrypts and decrypts data.",
  vpc: "A private network.",
  subnet: "A range of addresses in one availability zone.",
  security_group: "A firewall for the resources that use it.",
  nat_gateway: "Lets private subnets reach the internet.",
  internet_gateway: "Connects a VPC to the internet.",
  vpc_endpoint: "Reaches an AWS service without leaving the VPC.",
};

/** One line on what this kind of resource is for, when sprawlr knows the kind. */
export const purposeOf = (n: ArchNode): string | null => PURPOSE[n.kind.type] ?? null;

// ---- who may take a role on

/** The role's trust policy, or null when the scan didn't read it (a snapshot from before). */
export const trustOf = (n: ArchNode): Principal[] | null => (Array.isArray(n.attrs?.trust) ? (n.attrs.trust as Principal[]) : null);

const SERVICE_PRINCIPALS: Record<string, string> = {
  "lambda.amazonaws.com": "Lambda functions",
  "edgelambda.amazonaws.com": "Lambda@Edge functions",
  "ecs-tasks.amazonaws.com": "ECS tasks",
  "ecs.amazonaws.com": "ECS",
  "ec2.amazonaws.com": "EC2 instances",
  "eks.amazonaws.com": "EKS clusters",
  "pods.eks.amazonaws.com": "EKS pods",
  "states.amazonaws.com": "Step Functions state machines",
  "apigateway.amazonaws.com": "API Gateway",
  "events.amazonaws.com": "EventBridge rules",
  "scheduler.amazonaws.com": "EventBridge Scheduler",
  "pipes.amazonaws.com": "EventBridge Pipes",
  "codebuild.amazonaws.com": "CodeBuild projects",
  "codepipeline.amazonaws.com": "CodePipeline pipelines",
  "cloudformation.amazonaws.com": "CloudFormation",
  "firehose.amazonaws.com": "Firehose streams",
  "glue.amazonaws.com": "Glue jobs",
  "sagemaker.amazonaws.com": "SageMaker",
  "appsync.amazonaws.com": "AppSync APIs",
  "amplify.amazonaws.com": "Amplify apps",
  "cognito-idp.amazonaws.com": "Cognito user pools",
  "monitoring.rds.amazonaws.com": "RDS enhanced monitoring",
  "rds.amazonaws.com": "RDS",
  "ssm.amazonaws.com": "Systems Manager",
  "backup.amazonaws.com": "AWS Backup",
  "batch.amazonaws.com": "AWS Batch",
  "s3.amazonaws.com": "S3",
  "sns.amazonaws.com": "SNS",
  "logs.amazonaws.com": "CloudWatch Logs",
  "delivery.logs.amazonaws.com": "CloudWatch Logs delivery",
  "vpc-flow-logs.amazonaws.com": "VPC flow logs",
  "cloudtrail.amazonaws.com": "CloudTrail",
  "config.amazonaws.com": "AWS Config",
  "xray.amazonaws.com": "X-Ray",
  "dms.amazonaws.com": "DMS",
  "es.amazonaws.com": "OpenSearch",
  "opensearchservice.amazonaws.com": "OpenSearch",
};

const arnAccount = (arn: string) => arn.split(":")[4] ?? "";

function federated(id: string): string {
  if (id.includes("token.actions.githubusercontent.com")) return "GitHub Actions";
  if (id === "cognito-identity.amazonaws.com") return "Cognito identity pools";
  if (/oidc-provider\/oidc\.eks\./.test(id)) return "EKS service accounts";
  if (id === "accounts.google.com") return "Google sign-in";
  const saml = id.match(/saml-provider\/(.+)$/);
  if (saml) return `SAML sign-in through ${saml[1]}`;
  const oidc = id.match(/oidc-provider\/([^/]+)/);
  return oidc ? `Sign-in through ${oidc[1]}` : id;
}

/** A principal in the words of someone running the system, from where `role` stands. */
export function principalLabel(p: Principal, role: ArchNode): string {
  const here = (account: string) => account === role.account;
  switch (p.kind) {
    case "service":
      return SERVICE_PRINCIPALS[p.id] ?? `The ${p.id.replace(/\.amazonaws\.com(\.cn)?$/, "")} service`;
    case "account":
      return here(p.id) ? "This account" : `Account ${p.id}`;
    case "user":
      return here(p.id) ? "Users of this account" : `Users of account ${p.id}`;
    case "role": {
      const path = p.id.split(":").slice(5).join(":").split("/");
      const name = path[0] === "assumed-role" ? path[1] : path[path.length - 1];
      return here(arnAccount(p.id)) ? `Role ${name}` : `Role ${name} in account ${arnAccount(p.id)}`;
    }
    case "federated":
      return federated(p.id);
    case "deleted":
      return "A role or user that no longer exists";
    case "anyone":
      return "Anyone with AWS credentials";
  }
}

/**
 * The condition keys a principal must match, in short: an identity provider's keys name the claim
 * they check (`token.actions.githubusercontent.com:sub` → `sub`).
 */
export const conditionWords = (p: Principal): string =>
  nameList(p.conditions.map((k) => (k.split(":")[0].includes(".") ? k.slice(k.lastIndexOf(":") + 1) : k)));

const assumes = (ix: Index, id: string, side: "out" | "inc") =>
  (ix[side].get(id) ?? [])
    .filter((e) => e.kind === "assumes_role")
    .map((e) => ix.byId.get(side === "out" ? e.to : e.from))
    .filter((n): n is ArchNode => !!n);

/** The resources of this scan that take the role on. */
export const takenOnBy = (ix: Index, roleId: string) => assumes(ix, roleId, "inc");

/** The roles a resource runs as. */
export const runsAs = (ix: Index, id: string) => assumes(ix, id, "out");

// ---- what a role may do

/** What the role may do, by service, or null when its policies were not read. */
export const accessOf = (n: ArchNode): ServiceAccess[] | null => (Array.isArray(n.attrs?.access) ? (n.attrs.access as ServiceAccess[]) : null);

/**
 * Whether what a role may do is known; read by an older sprawlr that kept its policies but not
 * what they add up to (a new scan works it out); or never read (a role only referenced).
 */
export function accessState(n: ArchNode): "known" | "scan_again" | "not_read" {
  if (accessOf(n) !== null) return "known";
  return Array.isArray(n.attrs?.policies) ? "scan_again" : "not_read";
}

const SERVICES: Record<string, [label: string, noun: string]> = {
  "*": ["Every service", "resource"],
  dynamodb: ["DynamoDB", "table"],
  s3: ["S3", "bucket"],
  sqs: ["SQS", "queue"],
  sns: ["SNS", "topic"],
  lambda: ["Lambda", "function"],
  kms: ["KMS", "key"],
  secretsmanager: ["Secrets Manager", "secret"],
  ssm: ["Systems Manager", "parameter"],
  events: ["EventBridge", "event bus"],
  states: ["Step Functions", "state machine"],
  kinesis: ["Kinesis", "stream"],
  firehose: ["Firehose", "stream"],
  "execute-api": ["API Gateway", "API"],
  apigateway: ["API Gateway", "API"],
  iam: ["IAM", "role or policy"],
  sts: ["STS", "role"],
  logs: ["CloudWatch Logs", "log group"],
  cloudwatch: ["CloudWatch", "metric or alarm"],
  xray: ["X-Ray", "trace"],
  ec2: ["EC2", "EC2 resource"],
  ecs: ["ECS", "ECS resource"],
  ecr: ["ECR", "repository"],
  rds: ["RDS", "database"],
  "rds-db": ["RDS (database sign-in)", "database user"],
  es: ["OpenSearch", "domain"],
  cloudfront: ["CloudFront", "distribution"],
  route53: ["Route 53", "zone"],
  cloudformation: ["CloudFormation", "stack"],
};

const serviceLabel = (s: string) => SERVICES[s]?.[0] ?? s;
const every = (s: string) => `every ${SERVICES[s]?.[1] ?? `${serviceLabel(s)} resource`}`;

const LEVEL_WORDS: Record<AccessLevel, string> = { read: "read", write: "write", permissions: "change permissions", full: "full access" };

function levelWords(levels: AccessLevel[]): string {
  if (levels.includes("full")) return LEVEL_WORDS.full;
  const order: AccessLevel[] = ["read", "write", "permissions"];
  return nameList(order.filter((l) => levels.includes(l)).map((l) => LEVEL_WORDS[l]));
}

/** `arn:aws:sqs:us-east-1:1:jobs` → `jobs`, `arn:aws:s3:::archive/*` → `archive/*`. */
const shortPattern = (r: string) => r.replace(/^arn:aws:[a-z0-9-]+:[a-z0-9*-]*:(\d*|\*):/, "");

export interface AccessLine {
  allow: boolean;
  service: string;
  /** "read and write", "full access", "everything except iam:*". */
  what: string;
  /** Resources of this scan it reaches, when it doesn't reach every one. */
  targets: ArchNode[];
  /** "every table", "jobs, not in this scan", or null when the targets say it all. */
  scope: string | null;
  actions: string[];
  conditional: boolean;
  /** The policies that grant it. */
  via: string[];
}

function scopeOf(a: ServiceAccess): string | null {
  if (a.everything) {
    const except = a.except_resources.map(shortPattern);
    return except.length ? `${every(a.service)} except ${nameList(except)}` : every(a.service);
  }
  return a.elsewhere.length ? `${nameList(a.elsewhere.map(shortPattern))}, not in this scan` : null;
}

/** What a role may do, one line per service and set of resources: allows first, every service first. */
export function accessLines(ix: Index, role: ArchNode): AccessLine[] {
  const managed = new Set(managedPoliciesOf(role));
  return (accessOf(role) ?? [])
    .map((a) => ({
      allow: a.allow,
      service: serviceLabel(a.service),
      what: a.except_actions.length ? `everything except ${nameList(a.except_actions)}` : levelWords(a.levels),
      targets: a.everything ? [] : a.targets.map((t) => ix.byId.get(t)).filter((n): n is ArchNode => !!n),
      scope: scopeOf(a),
      actions: a.actions,
      conditional: a.conditional,
      via: a.policies.map((p) => (managed.has(p) ? `${p}, managed by AWS` : p)),
    }))
    .sort((x, y) => Number(!x.allow) - Number(!y.allow) || Number(x.service !== "Every service") - Number(y.service !== "Every service") || x.service.localeCompare(y.service));
}

const MANAGED: Record<string, string> = {
  AdministratorAccess: "full access to everything",
  PowerUserAccess: "full access to everything except IAM and Organizations",
  ReadOnlyAccess: "reads everything",
  AWSLambdaBasicExecutionRole: "writes its logs to CloudWatch Logs",
  AWSLambdaVPCAccessExecutionRole: "writes its logs and attaches to a VPC",
  AWSLambdaSQSQueueExecutionRole: "reads its SQS queue and writes its logs",
  AWSLambdaDynamoDBExecutionRole: "reads its DynamoDB stream and writes its logs",
  AWSLambdaKinesisExecutionRole: "reads its Kinesis stream and writes its logs",
  AmazonECSTaskExecutionRolePolicy: "pulls container images and writes their logs",
  AmazonSSMManagedInstanceCore: "lets Systems Manager manage the instance",
  CloudWatchAgentServerPolicy: "sends metrics and logs to CloudWatch",
  CloudWatchFullAccess: "full access to CloudWatch",
  AWSXRayDaemonWriteAccess: "sends traces to X-Ray",
  AmazonAPIGatewayPushToCloudWatchLogs: "lets API Gateway write its logs",
  AmazonS3FullAccess: "full access to every bucket",
  AmazonS3ReadOnlyAccess: "reads every bucket",
  AmazonDynamoDBFullAccess: "full access to every table",
  AmazonDynamoDBReadOnlyAccess: "reads every table",
  AmazonSQSFullAccess: "full access to every queue",
  AmazonSNSFullAccess: "full access to every topic",
};

/** What a common AWS managed policy is for, in plain words. */
export const managedPolicyNote = (name: string): string | null => MANAGED[name] ?? null;

export const managedPoliciesOf = (n: ArchNode): string[] => (Array.isArray(n.attrs?.aws_managed_policies) ? (n.attrs.aws_managed_policies as string[]) : []);

/** AWS managed policies attached to a role that no access line already counts. */
export function unlistedManaged(role: ArchNode): string[] {
  const counted = new Set((accessOf(role) ?? []).flatMap((a) => a.policies));
  return managedPoliciesOf(role).filter((m) => !counted.has(m));
}

// ---- who IAM lets in

export interface Allowed {
  role: ArchNode;
  what: string;
  /** "every table", "every resource", or null when the role names this one. */
  scope: string | null;
  conditional: boolean;
}

/** The roles whose policies reach a resource: by name first, then those that reach every one. */
export function allowedIn(ix: Index, id: string): Allowed[] {
  if (!id.startsWith("arn:")) return [];
  const out: Allowed[] = [];
  for (const role of ix.snap.nodes) {
    if (role.id === id) continue;
    // Rust names every resource a row reaches, service-wide grants included, after denies.
    const rows = (accessOf(role) ?? []).filter((a) => a.allow && a.targets.includes(id));
    if (!rows.length) continue;
    const broadest = rows.find((a) => a.service === "*") ?? rows.find((a) => a.everything);
    out.push({
      role,
      what: levelWords([...new Set(rows.flatMap((a) => a.levels))]),
      scope: broadest?.everything ? every(broadest.service) : null,
      conditional: rows.every((a) => a.conditional),
    });
  }
  return out.sort((a, b) => Number(a.scope !== null) - Number(b.scope !== null) || displayName(a.role).localeCompare(displayName(b.role)));
}

// ---- how it is set up

export interface Setting {
  label: string;
  value: string;
  tone?: "bad";
}

type Show = (v: unknown, attrs: Record<string, unknown>) => Omit<Setting, "label"> | string | null;

const onOff: Show = (v) => (v ? "On" : "Off");
const yesNo: Show = (v) => (v ? "Yes" : "No");
const unit = (u: string): Show => (v) => `${v} ${u}`;
const lastPart = (s: string) => s.split(/[/:]/).pop() ?? s;
const list = (v: unknown) => (Array.isArray(v) ? v.map(String) : []);
const named = (words: Record<string, string>): Show => (v) => words[String(v)] ?? String(v);
const daysAgo = (d: number) => (d === 0 ? "Today" : d === 1 ? "Yesterday" : `${d} days ago`);

/** When a role was last used and where; long unused reads as bad, as cleanup judges it. */
function lastUse(a: Record<string, unknown>): Omit<Setting, "label"> {
  const v = a.last_used_days;
  if (v === null || v === undefined) return { value: "Never" };
  const d = Number(v);
  const value = daysAgo(d) + (typeof a.last_used_region === "string" ? `, in ${a.last_used_region}` : "");
  return d > 90 ? { value, tone: "bad" } : { value };
}

/** When a role was last used, or null for anything that doesn't record it. */
export const lastUsedOf = (n: ArchNode): string | null => (n.attrs && "last_used_days" in n.attrs ? lastUse(n.attrs).value : null);

/** Settings by attribute key, in the order the panel shows them. */
const SETTINGS: [key: string, label: string, show?: Show][] = [
  ["runtime", "Runtime"],
  ["memory_mb", "Memory", unit("MB")],
  ["timeout_s", "Timeout", unit("s")],
  ["architectures", "Architecture"],
  ["env_keys", "Environment variables"],
  ["env_plaintext_secret_keys", "Secrets in plain text", (v) => (list(v).length ? { value: list(v).join(", "), tone: "bad" } : null)],
  ["cluster", "Cluster"],
  ["launch_type", "Launch type", named({ FARGATE: "Fargate", EC2: "EC2", EXTERNAL: "External" })],
  ["desired", "Tasks wanted"],
  ["cpu", "CPU", (v) => `${Number(v) / 1024} vCPU`],
  ["memory", "Memory", unit("MB")],
  ["task_definition", "Task definition"],
  ["images", "Images", (v) => list(v).map(lastPart).join(", ")],
  ["instance_type", "Instance type"],
  ["state", "State"],
  ["public_ip", "Public IP", yesNo],
  ["engine", "Engine"],
  ["multi_az", "Multi-AZ", yesNo],
  ["encrypted", "Encrypted", yesNo],
  ["billing_mode", "Capacity mode", named({ PAY_PER_REQUEST: "On demand", PROVISIONED: "Provisioned" })],
  ["pitr", "Point-in-time recovery", onOff],
  ["stream", "Stream"],
  ["encrypted_with_cmk", "Encryption", (v) => (v ? "Your own KMS key" : "Key owned by AWS")],
  ["public_access_block", "Block public access", (v) => (v ? "On" : { value: "Off", tone: "bad" })],
  ["versioning", "Versioning", onOff],
  ["website", "Static website", onOff],
  ["eventbridge_notifications", "Events to EventBridge", onOff],
  ["fifo", "Order", (v) => (v ? "FIFO: in order, once" : "Standard")],
  ["visibility_timeout_s", "Visibility timeout", unit("s")],
  ["dead_letter_queue", "Dead-letter queue", (v) => lastPart(String(v))],
  ["max_receive_count", "Tries before the dead-letter queue"],
  ["type", "Type", named({ STANDARD: "Standard", EXPRESS: "Express" })],
  ["endpoint_type", "Endpoint type", named({ REGIONAL: "Regional", EDGE: "Edge-optimized", PRIVATE: "Private" })],
  ["protocol", "Protocol"],
  ["endpoint", "Endpoint"],
  ["public", "Reachable from the internet", yesNo],
  ["authorizer", "Authorizer", named({ none: "None" })],
  ["domain", "Domain"],
  ["aliases", "Domains"],
  ["enabled", "Enabled", yesNo],
  ["waf", "WAF", onOff],
  ["scheme", "Scheme", named({ "internet-facing": "Internet-facing", internal: "Internal" })],
  ["usage", "Use", named({ ENCRYPT_DECRYPT: "Encrypt and decrypt", SIGN_VERIFY: "Sign and verify", GENERATE_VERIFY_MAC: "Generate and verify MACs" })],
  ["managed_by", "Managed by"],
  ["rotation_enabled", "Rotation", onOff],
  ["last_rotated_days", "Last rotated", (v) => daysAgo(Number(v))],
  ["ingress", "Inbound", (v) => (list(v).length ? list(v).join(", ") : "Nothing")],
  ["open_to_internet", "Open to the internet", (v) => (v ? { value: "Yes", tone: "bad" } : "No")],
  ["group_id", "Group id"],
  ["az", "Availability zone"],
  ["cidr", "CIDR"],
  ["subnet_id", "Subnet id"],
  ["auto_public_ip", "Public IPs for new instances", yesNo],
  ["vpc_id", "VPC id"],
  ["is_default", "Default VPC", yesNo],
  ["associated", "Attached", yesNo],
  ["instance_id", "Instance id"],
  ["instances", "Instances"],
  ["subnets", "Subnets"],
  ["security_groups", "Security groups", (v) => list(v).map(lastPart).join(", ")],
  ["log_groups", "Log groups"],
  ["created_days", "Created", (v) => daysAgo(Number(v))],
  ["last_used_days", "Last used", (_, a) => lastUse(a)],
  ["instance_profiles", "Instance profiles"],
  ["permissions_boundary", "Permissions boundary"],
];

/** Shown by other sections of the panel, or folded into another setting. */
const ELSEWHERE = new Set(["placeholder", "metrics", "alarms", "policies", "broad_permissions", "activity", "log_errors", "access", "trust", "aws_managed_policies", "last_used_region"]);

const humanise = (key: string) => key.charAt(0).toUpperCase() + key.slice(1).replace(/_/g, " ");

function plain(v: unknown): string | null {
  if (v === null || v === undefined) return null;
  if (typeof v === "boolean") return v ? "Yes" : "No";
  if (Array.isArray(v)) return v.length ? v.map(String).join(", ") : null;
  if (typeof v === "object") return null; // left to the JSON view
  return String(v);
}

/** A resource's settings as labelled values: known ones in a reading order, the rest as written. */
export function settingsOf(n: ArchNode): Setting[] {
  const a = (n.attrs ?? {}) as Record<string, unknown>;
  const known = new Set(SETTINGS.map(([k]) => k));
  const out: Setting[] = [];
  for (const [key, label, show] of SETTINGS) {
    if (!(key in a)) continue;
    const v = a[key];
    const shown = show ? (v === null && key !== "last_used_days" ? null : show(v, a)) : plain(v);
    if (shown === null || shown === "") continue;
    out.push(typeof shown === "string" ? { label, value: shown } : { label, ...shown });
  }
  for (const [key, v] of Object.entries(a)) {
    if (known.has(key) || ELSEWHERE.has(key)) continue;
    const value = plain(v);
    if (value !== null) out.push({ label: humanise(key), value });
  }
  return out;
}

/** What sprawlr worked out or measured: other sections show it, so the JSON view leaves it out. */
const DERIVED = new Set(["placeholder", "metrics", "alarms", "broad_permissions", "activity", "access", "trust", "log_errors"]);

/** The attributes as read, for the JSON view. */
export const rawAttrs = (n: ArchNode): Record<string, unknown> => Object.fromEntries(Object.entries(n.attrs ?? {}).filter(([k]) => !DERIVED.has(k)));

/** The rows of the last comparison that are about this resource. */
export const changesAbout = (snap: Snapshot, id: string): ChangeRow[] => (snap.changes?.rows ?? []).filter((r) => r.id === id);
