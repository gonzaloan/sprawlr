// Pure view logic over a snapshot: no React, no layout. Everything here is testable in isolation.
import type { Alarm, ArchEdge, ArchNode, EdgeKind, Env, EvidenceKind, Finding, Layer, Method, Metric, Snapshot } from "./types";

// ------------------------------------------------------------------ vocabulary

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

export function kindInfo(n: ArchNode): KindInfo {
  const k = KINDS[n.kind.type];
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
  permitted: "Allowed only",
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

export const metricsOf = (n: ArchNode): Metric[] => ((n.attrs?.metrics as Metric[] | undefined) ?? []).filter((m) => Array.isArray(m.values));
export const alarmsOf = (n: ArchNode): Alarm[] => (n.attrs?.alarms as Alarm[] | undefined) ?? [];

/** The series that best shows activity: traffic first, then utilisation. */
export function primaryMetric(n: ArchNode): Metric | undefined {
  const ms = metricsOf(n);
  const order = ["requests", "invocations", "sent", "published", "writes", "started", "cpu"];
  return order.map((k) => ms.find((m) => m.key === k)).find(Boolean) ?? ms[0];
}

export function fmtValue(v: number | null | undefined, unit: string): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "no data";
  const n = (x: number) =>
    Math.abs(x) >= 1e6 ? `${(x / 1e6).toFixed(1)}M` : Math.abs(x) >= 1e4 ? `${Math.round(x / 1e3)}k` : Math.abs(x) >= 1e3 ? `${(x / 1e3).toFixed(1)}k` : Math.abs(x - Math.round(x)) < 0.05 ? `${Math.round(x)}` : x.toFixed(1);
  if (unit === "ms" && v >= 1000) return `${(v / 1000).toFixed(1)} s`;
  if (unit === "count") return n(v);
  if (unit === "%") return `${n(v)}%`;
  return `${n(v)} ${unit}`;
}

export const envName = (e: Env): string => (typeof e === "string" ? e : e.custom);

const RANK: Record<EvidenceKind, number> = { permitted: 0, declared: 1, observed: 2 };

export function strength(e: ArchEdge): EvidenceKind {
  return e.evidence.reduce<EvidenceKind>((best, ev) => (RANK[ev.kind] > RANK[best] ? ev.kind : best), "permitted");
}

/** Edges whose failure propagates (mirrors EdgeKind::is_runtime_dependency in Rust). */
export const isRuntime = (e: ArchEdge) => e.kind !== "network_reachable" && e.kind !== "assumes_role";

// ------------------------------------------------------------------ index

export interface Index {
  snap: Snapshot;
  byId: Map<string, ArchNode>;
  out: Map<string, ArchEdge[]>;
  inc: Map<string, ArchEdge[]>;
}

export function buildIndex(snap: Snapshot): Index {
  const byId = new Map(snap.nodes.map((n) => [n.id, n]));
  const out = new Map<string, ArchEdge[]>();
  const inc = new Map<string, ArchEdge[]>();
  for (const e of snap.edges) {
    (out.get(e.from) ?? out.set(e.from, []).get(e.from)!).push(e);
    (inc.get(e.to) ?? inc.set(e.to, []).get(e.to)!).push(e);
  }
  return { snap, byId, out, inc };
}

/** Nearest VPC containing the node (the "territory" it lives in), if any. */
export function vpcOf(ix: Index, n: ArchNode): ArchNode | undefined {
  let cur = n.parent ? ix.byId.get(n.parent) : undefined;
  for (let i = 0; cur && i < 8; i++) {
    if (cur.kind.type === "vpc") return cur;
    cur = cur.parent ? ix.byId.get(cur.parent) : undefined;
  }
  return undefined;
}

export function envs(ix: Index): string[] {
  const s = new Set<string>();
  ix.snap.nodes.forEach((n) => n.env && s.add(envName(n.env.value)));
  const order = ["prod", "staging", "test", "dev"];
  return [...s].sort((a, b) => (order.indexOf(a) + 99) % 99 - (order.indexOf(b) + 99) % 99 || a.localeCompare(b));
}

const inEnv = (n: ArchNode, env: string | null) => env === null || (n.env ? envName(n.env.value) === env : true);

/** Resources the map shows: everything except account/region scaffolding and raw network plumbing. */
const mappable = (n: ArchNode) => !["scope", "network"].includes(layerOf(n));

// ------------------------------------------------------------------ sidebar

export interface AppSummary {
  name: string;
  count: number;
  envs: string[];
  findings: number;
  health: "ok" | "warning" | "alarm" | "unknown";
}

export function apps(ix: Index, env: string | null, fs: Finding[]): AppSummary[] {
  const m = new Map<string, AppSummary>();
  const fByNode = new Map<string, number>();
  fs.forEach((f) => fByNode.set(f.node, (fByNode.get(f.node) ?? 0) + 1));
  for (const n of ix.snap.nodes) {
    if (!n.app || !inEnv(n, env)) continue;
    const s = m.get(n.app.value) ?? { name: n.app.value, count: 0, envs: [], findings: 0, health: "unknown" };
    s.count++;
    s.findings += fByNode.get(n.id) ?? 0;
    if (n.env && !s.envs.includes(envName(n.env.value))) s.envs.push(envName(n.env.value));
    const rank = { unknown: 0, ok: 1, warning: 2, alarm: 3 } as const;
    if (rank[n.health] > rank[s.health]) s.health = n.health;
    m.set(s.name, s);
  }
  return [...m.values()].sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

/** Resources connected to several apps, or to none. */
export function orphans(ix: Index, env: string | null) {
  const shared: ArchNode[] = [];
  const unassigned: ArchNode[] = [];
  for (const n of ix.snap.nodes) {
    if (n.app || !mappable(n) || layerOf(n) === "external" || !inEnv(n, env)) continue;
    const touching = new Set<string>();
    for (const e of [...(ix.out.get(n.id) ?? []), ...(ix.inc.get(n.id) ?? [])]) {
      const other = ix.byId.get(e.from === n.id ? e.to : e.from);
      if (other?.app && isRuntime(e)) touching.add(other.app.value);
    }
    (touching.size > 1 ? shared : unassigned).push(n);
  }
  return { shared, unassigned };
}

// ------------------------------------------------------------------ map views

export interface ViewNode {
  id: string;
  node?: ArchNode; // resource view
  app?: AppSummary; // overview
  role: "member" | "neighbor" | "external";
  group?: string; // VPC id
}

export interface ViewEdge {
  id: string;
  from: string;
  to: string;
  label: string;
  strength: EvidenceKind;
  count?: number;
}

export interface View {
  nodes: ViewNode[];
  edges: ViewEdge[];
  groups: ArchNode[]; // VPCs drawn as territories
}

/** One app: its resources, plus the first ring of things it talks to. */
export function appView(ix: Index, app: string, env: string | null): View {
  const members = ix.snap.nodes.filter((n) => n.app?.value === app && inEnv(n, env) && mappable(n));
  return resourceView(ix, members);
}

/** A single resource with its neighbours (used for shared / unassigned resources). */
export function resourceFocus(ix: Index, id: string): View {
  const n = ix.byId.get(id);
  return n ? resourceView(ix, [n]) : { nodes: [], edges: [], groups: [] };
}

function resourceView(ix: Index, members: ArchNode[]): View {
  const ids = new Set(members.map((n) => n.id));
  const nodes = new Map<string, ViewNode>();
  members.forEach((n) => nodes.set(n.id, { id: n.id, node: n, role: "member" }));

  const edges: ViewEdge[] = [];
  for (const e of ix.snap.edges) {
    if (e.kind === "network_reachable") continue;
    if (!ids.has(e.from) && !ids.has(e.to)) continue;
    for (const end of [e.from, e.to]) {
      if (nodes.has(end)) continue;
      const n = ix.byId.get(end);
      if (!n || !(mappable(n) || n.kind.type === "internet")) continue;
      nodes.set(end, { id: end, node: n, role: layerOf(n) === "external" ? "external" : "neighbor" });
    }
    if (nodes.has(e.from) && nodes.has(e.to))
      edges.push({ id: `${e.from}|${e.kind}|${e.to}`, from: e.from, to: e.to, label: EDGE_VERB[e.kind], strength: strength(e) });
  }

  const groups = new Map<string, ArchNode>();
  for (const v of nodes.values()) {
    const vpc = v.node && vpcOf(ix, v.node);
    if (vpc) {
      v.group = vpc.id;
      groups.set(vpc.id, vpc);
    }
  }
  return { nodes: [...nodes.values()], edges, groups: [...groups.values()] };
}

/** System context: apps as single blocks, shared resources between them, internet entry points. */
export function overview(ix: Index, env: string | null, summaries: AppSummary[], shared: ArchNode[]): View {
  const nodes = new Map<string, ViewNode>();
  summaries.forEach((a) => nodes.set(`app:${a.name}`, { id: `app:${a.name}`, app: a, role: "member" }));
  shared.forEach((n) => nodes.set(n.id, { id: n.id, node: n, role: "neighbor" }));

  const key = (n: ArchNode | undefined): string | undefined => {
    if (!n || !inEnv(n, env)) return undefined;
    if (n.app) return `app:${n.app.value}`;
    if (nodes.has(n.id)) return n.id;
    if (n.kind.type === "internet" || n.kind.type === "external_endpoint") return n.id;
    return undefined;
  };

  const agg = new Map<string, ViewEdge>();
  for (const e of ix.snap.edges) {
    if (!isRuntime(e) && e.kind !== "routes_to") continue;
    const a = key(ix.byId.get(e.from));
    const b = key(ix.byId.get(e.to));
    if (!a || !b || a === b) continue;
    for (const [k, n] of [[a, e.from], [b, e.to]] as const)
      if (!nodes.has(k)) nodes.set(k, { id: k, node: ix.byId.get(n), role: "external" });
    const id = `${a}->${b}`;
    const s = strength(e);
    const prev = agg.get(id);
    if (prev) {
      prev.count = (prev.count ?? 1) + 1;
      if (RANK[s] > RANK[prev.strength]) prev.strength = s;
    } else agg.set(id, { id, from: a, to: b, label: "", strength: s, count: 1 });
  }
  for (const e of agg.values()) e.label = e.count && e.count > 1 ? `${e.count} connections` : "";
  return { nodes: [...nodes.values()], edges: [...agg.values()], groups: [] };
}

/** Everything that breaks if `id` fails. Mirrors ArchGraph::blast_radius. */
export function blastRadius(ix: Index, id: string): Set<string> {
  const out = new Set<string>();
  const queue = [id];
  while (queue.length) {
    const cur = queue.shift()!;
    for (const e of ix.inc.get(cur) ?? []) {
      if (!isRuntime(e) || out.has(e.from) || e.from === id) continue;
      out.add(e.from);
      queue.push(e.from);
    }
  }
  return out;
}

/** Resources whose name, kind, app or identifier matches every word typed. */
export function search(ix: Index, q: string, limit = 8): ArchNode[] {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const hay = (n: ArchNode) => `${displayName(n)} ${n.name} ${kindInfo(n).label} ${n.app?.value ?? ""} ${n.id}`.toLowerCase();
  return ix.snap.nodes
    .filter((n) => mappable(n) && words.every((w) => hay(n).includes(w)))
    .sort((a, b) => Number(!displayName(a).toLowerCase().startsWith(words[0])) - Number(!displayName(b).toLowerCase().startsWith(words[0])) || displayName(a).localeCompare(displayName(b)))
    .slice(0, limit);
}
