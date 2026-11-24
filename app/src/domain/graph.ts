// The snapshot as a graph: index, evidence strength, filters, search, blast radius.
import type { ArchEdge, ArchNode, EvidenceKind, Snapshot } from "./types";
import { displayName, envName, kindInfo, layerOf } from "./vocabulary";

export const RANK: Record<EvidenceKind, number> = { permitted: 0, declared: 1, observed: 2 };

export function strength(e: ArchEdge): EvidenceKind {
  return e.evidence.reduce<EvidenceKind>((best, ev) => (RANK[ev.kind] > RANK[best] ? ev.kind : best), "permitted");
}

/** Edges whose failure propagates (mirrors EdgeKind::is_runtime_dependency in Rust). */
export const isRuntime = (e: ArchEdge) => e.kind !== "network_reachable" && e.kind !== "assumes_role";

export interface Index {
  snap: Snapshot;
  byId: Map<string, ArchNode>;
  out: Map<string, ArchEdge[]>;
  inc: Map<string, ArchEdge[]>;
  /** Resources whose connections are known only from IAM (an EC2 worker: its code is out of sight). */
  iamMapped: Set<string>;
}

/** Connection that only an IAM policy allows: nothing in configuration or traffic shows it. */
export const permissionOnly = (e: ArchEdge) => strength(e) === "permitted";

const WORK = new Set(["reads", "writes", "publishes", "invokes", "subscribes", "routes_to"]);

/** Permission-only connections are hidden unless asked for, except where they are the only map. */
export function shown(ix: Index, e: ArchEdge, permissions: boolean): boolean {
  return permissions || !permissionOnly(e) || ix.iamMapped.has(e.from);
}

export function buildIndex(snap: Snapshot): Index {
  const byId = new Map(snap.nodes.map((n) => [n.id, n]));
  const out = new Map<string, ArchEdge[]>();
  const inc = new Map<string, ArchEdge[]>();
  for (const e of snap.edges) {
    (out.get(e.from) ?? out.set(e.from, []).get(e.from)!).push(e);
    (inc.get(e.to) ?? inc.set(e.to, []).get(e.to)!).push(e);
  }
  const iamMapped = new Set<string>();
  for (const [id, list] of out) {
    const work = list.filter((e) => WORK.has(e.kind));
    if (work.length && work.every(permissionOnly)) iamMapped.add(id);
  }
  return { snap, byId, out, inc, iamMapped };
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

export const inEnv = (n: ArchNode, env: string | null) => env === null || (n.env ? envName(n.env.value) === env : true);

/** Resources the map shows: everything except account/region scaffolding and raw network plumbing. */
export const mappable = (n: ArchNode) => !["scope", "network"].includes(layerOf(n));

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
