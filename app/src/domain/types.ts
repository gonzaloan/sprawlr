// Mirrors the Rust `Snapshot` (crates/sprawlr-domain: model.rs, graph.rs, insights, logs). Keep in sync (see docs/adr/0004).

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
  /** Absent in snapshots written before it existed. */
  status?: AccountStatus | null;
  /** What changed since the previous scan of the account, when there is one. */
  changes?: Changes | null;
}

// ---- changes.rs

export interface ChangeItem {
  id: string;
  name: string;
  kind: string;
  app: string | null;
}

export interface Changes {
  since: string;
  summary: string;
  added: ChangeItem[];
  removed: ChangeItem[];
  settings: { resource: ChangeItem; setting: string; label: string; before: string; after: string }[];
  connections_added: { from: string; verb: string; to: string }[];
  connections_removed: { from: string; verb: string; to: string }[];
  findings_new: string[];
  findings_resolved: string[];
  attention_new: string[];
  attention_resolved: string[];
  level: [Level, Level] | null;
  health: { resource: ChangeItem; before: Health; after: Health }[];
  new_errors: string[];
  cleanup_monthly: [number, number];
  /** Every change, most telling first, in the words every view uses. */
  rows: ChangeRow[];
}

export interface ChangeRow {
  tag: "attention" | "error" | "health" | "new" | "removed" | "changed" | "connection" | "resolved";
  label: string;
  text: string;
  id: string | null;
}

// ---- status.rs

export type Level = "act_now" | "plan_for_it" | "all_clear" | "not_measured";

export type Target = { type: "flow"; id: string } | { type: "node"; id: string } | { type: "cleanup" };

export interface StatusItem {
  key: string;
  priority: Priority;
  title: string;
  open: Target;
}

export interface AccountStatus {
  level: Level;
  headline: string;
  account: string | null;
  counts: { act_now: number; plan_for_it: number; good_to_know: number };
  items: StatusItem[];
  flows_with_errors: { flow: string; name: string; errors: number }[];
  cleanup: { resources: number; monthly_cost: number };
  /** After this many hours the status is not shown as current. */
  stale_after_hours: number;
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
  /** When the logs were read; background refreshes skip logs, so this can be older. */
  read_at?: string | null;
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

// ---- iam.rs (role attributes)

/** attrs.trust entries: who a role's trust policy lets take it on. Condition values are never kept. */
export interface Principal {
  kind: "service" | "account" | "role" | "user" | "federated" | "deleted" | "anyone";
  id: string;
  conditions: string[];
}

export type AccessLevel = "read" | "write" | "permissions" | "full";

/** attrs.access entries: what a role may do on one service. */
export interface ServiceAccess {
  /** IAM's service prefix (`dynamodb`), or `*` for every service. */
  service: string;
  allow: boolean;
  levels: AccessLevel[];
  actions: string[];
  except_actions: string[];
  /** Resources of this scan the grant covers. */
  targets: string[];
  everything: boolean;
  except_resources: string[];
  /** Resource patterns that match nothing in this scan. */
  elsewhere: string[];
  conditional: boolean;
  policies: string[];
}

export interface ScanResult {
  snapshot: Snapshot;
  warnings: string[];
}
