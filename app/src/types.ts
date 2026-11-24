// Mirrors the Rust `Snapshot` (crates/sprawlr-core). Keep in sync with model.rs.

export type Layer = "scope" | "network" | "edge" | "compute" | "integration" | "data" | "identity" | "external";
export type Method = "tag" | "stack" | "account" | "naming" | "containment" | "graph" | "llm" | "user_override";
export type Env = "prod" | "staging" | "dev" | "test" | { custom: string };
export type Health = "ok" | "warning" | "alarm" | "unknown";
export type EdgeKind =
  | "invokes" | "reads" | "writes" | "publishes" | "subscribes" | "routes_to"
  | "exposed_via" | "uses_secret" | "encrypted_with" | "assumes_role" | "network_reachable";
export type EvidenceKind = "permitted" | "declared" | "observed";

export interface Inferred<T> {
  value: T;
  method: Method;
  confidence: number;
}

export interface ArchNode {
  id: string;
  kind: { type: string; name?: string };
  name: string;
  account: string | null;
  region: string | null;
  parent: string | null;
  aliases: string[];
  app: Inferred<string> | null;
  env: Inferred<Env> | null;
  tags: Record<string, string>;
  attrs: Record<string, unknown> | null;
  health: Health;
}

export interface Evidence {
  kind: EvidenceKind;
  source: string;
  detail: string | null;
  seen_at: string;
}

export interface ArchEdge {
  from: string;
  to: string;
  kind: EdgeKind;
  evidence: Evidence[];
}

export type Severity = "high" | "medium" | "low";

/** Computed in Rust (analysis.rs) when the snapshot is taken. */
export interface Finding {
  node: string;
  rule: string;
  severity: Severity;
  title: string;
  detail: string;
}

export interface Snapshot {
  taken_at: string;
  nodes: ArchNode[];
  edges: ArchEdge[];
  findings?: Finding[];
  insights?: Insights;
}

// ---- insights.rs

export interface Hop {
  from: string;
  to: string;
  kind: EdgeKind;
  mode: "sync" | "async";
  stage: number;
  /** Known only from IAM permissions. */
  inferred?: boolean;
}

export interface FlowError {
  node: string;
  message: string;
  count: number;
  last_seen?: string | null;
}

export interface ErrorPattern {
  message: string;
  count: number;
  last_seen?: string | null;
}

export interface LogErrors {
  hours: number;
  log_groups: string[];
  total: number;
  patterns: ErrorPattern[];
}

export interface Flow {
  id: string;
  name: string;
  app: string | null;
  env: string | null;
  trigger: "internet" | "event" | "self_started";
  entry: string;
  nodes: string[];
  hops: Hop[];
  stages: number;
  runs_24h: number | null;
  errors_24h: number;
  sync_depth: number;
  sync_latency_ms: number | null;
  slowest: string | null;
  top_errors?: FlowError[];
}

export interface Capacity {
  node: string;
  utilization: number;
  headroom: number | null; // null when unbounded (serialised infinity)
  basis: string;
  limit: string;
}

export type Priority = "high" | "medium" | "low";

export interface Insight {
  kind: string;
  priority: Priority;
  title: string;
  detail: string;
  action: string | null;
  nodes: string[];
  flow: string | null;
}

export interface Lifecycle {
  node: string;
  state: "idle" | "orphaned" | "zombie";
  reason: string;
  monthly_cost: number | null;
}

export interface Insights {
  flows: Flow[];
  capacity: Capacity[];
  insights: Insight[];
  lifecycle: Lifecycle[];
}

/** Generated Markdown page (docs.rs). */
export interface DocFile {
  path: string;
  title: string;
  content: string;
}

/** attrs.metrics entries written by telemetry.rs (24 hourly values, oldest first). */
export interface Metric {
  key: string;
  label: string;
  unit: string;
  stat: string;
  values: (number | null)[];
  summary: number | null;
  summary_label: string;
  alert: string | null;
}

/** attrs.alarms entries. */
export interface Alarm {
  name: string;
  state: "OK" | "ALARM" | "INSUFFICIENT_DATA";
  metric: string | null;
  reason: string | null;
}

export interface ScanResult {
  snapshot: Snapshot;
  warnings: string[];
}
