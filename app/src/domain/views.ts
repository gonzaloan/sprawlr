// What the map draws: nodes, edges and territories for an app, a resource, the account or a flow.
import type { ArchEdge, ArchNode, EvidenceKind, Flow } from "./types";
import { type Index, RANK, inEnv, isRuntime, mappable, permissionOnly, shown, strength, vpcOf } from "./graph";
import type { AppSummary } from "./navigation";
import { EDGE_VERB, layerOf } from "./vocabulary";

export interface ViewNode {
  id: string;
  node?: ArchNode; // resource view
  app?: AppSummary; // overview
  role: "member" | "neighbor" | "external";
  group?: string; // VPC id
  step?: number; // position in a flow
}

export interface ViewEdge {
  id: string;
  from: string;
  to: string;
  label: string;
  strength: EvidenceKind;
  count?: number;
  mode?: "sync" | "async";
  /** Known only from IAM. */
  iam?: boolean;
}

export interface View {
  nodes: ViewNode[];
  edges: ViewEdge[];
  groups: ArchNode[]; // VPCs drawn as territories
  /** Flows read top to bottom, like a story; maps read left to right. */
  direction?: "RIGHT" | "DOWN";
}

/** One app: its resources, plus the first ring of things it talks to. */
export function appView(ix: Index, app: string, env: string | null, permissions = false): View {
  const members = ix.snap.nodes.filter((n) => n.app?.value === app && inEnv(n, env) && mappable(n));
  return resourceView(ix, members, permissions);
}

/** Every resource of the account in an environment, for views that draw resources, not apps. */
export function everythingView(ix: Index, env: string | null, permissions = false): View {
  return resourceView(ix, ix.snap.nodes.filter((n) => mappable(n) && inEnv(n, env)), permissions);
}

/** A single resource with its neighbours (used for shared / unassigned resources). */
export function resourceFocus(ix: Index, id: string, permissions = false): View {
  const n = ix.byId.get(id);
  return n ? resourceView(ix, [n], permissions) : { nodes: [], edges: [], groups: [] };
}

function resourceView(ix: Index, members: ArchNode[], permissions: boolean): View {
  const ids = new Set(members.map((n) => n.id));
  const nodes = new Map<string, ViewNode>();
  members.forEach((n) => nodes.set(n.id, { id: n.id, node: n, role: "member" }));

  const edges: ViewEdge[] = [];
  for (const e of ix.snap.edges) {
    if (e.kind === "network_reachable") continue;
    if (!ids.has(e.from) && !ids.has(e.to)) continue;
    if (!shown(ix, e, permissions)) continue;
    for (const end of [e.from, e.to]) {
      if (nodes.has(end)) continue;
      const n = ix.byId.get(end);
      if (!n || !(mappable(n) || n.kind.type === "internet")) continue;
      nodes.set(end, { id: end, node: n, role: layerOf(n) === "external" ? "external" : "neighbor" });
    }
    if (nodes.has(e.from) && nodes.has(e.to))
      edges.push({ id: `${e.from}|${e.kind}|${e.to}`, from: e.from, to: e.to, label: EDGE_VERB[e.kind], strength: strength(e), iam: permissionOnly(e) });
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
export function overview(ix: Index, env: string | null, summaries: AppSummary[], shared: ArchNode[], permissions = false): View {
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
    if (!shown(ix, e, permissions)) continue;
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

/** A queue, topic or stream handing work to its consumer reads as delivery, not as the queue reading. */
export const hopVerb = (h: { kind: ArchEdge["kind"]; mode: "sync" | "async" }) =>
  h.mode === "async" && (h.kind === "reads" || h.kind === "subscribes") ? "delivers to" : EDGE_VERB[h.kind];

/**
 * One flow as a story, top to bottom: its resources in order and its hops. No VPC
 * territories here: a flow crosses networks (API in a VPC → event bus → Lambda back in the
 * VPC), and grouping would force the layout to bend edges backwards.
 */
export function flowView(ix: Index, f: Flow): View {
  const nodes: ViewNode[] = f.nodes
    .map((id, i): ViewNode | null => {
      const n = ix.byId.get(id);
      if (!n) return null;
      return { id, node: n, role: layerOf(n) === "external" ? "external" : "member", step: i + 1 };
    })
    .filter((v): v is ViewNode => v !== null);
  const edges: ViewEdge[] = f.hops.map((h) => {
    const e = (ix.out.get(h.from) ?? []).find((x) => x.to === h.to && x.kind === h.kind) ?? (ix.out.get(h.to) ?? []).find((x) => x.to === h.from);
    const iam = h.inferred === true;
    return { id: `${h.from}|${h.kind}|${h.to}`, from: h.from, to: h.to, label: iam ? `${hopVerb(h)} (IAM)` : hopVerb(h), strength: e ? strength(e) : "declared", mode: h.mode, iam };
  });
  if (f.trigger === "internet") {
    const internet = ix.snap.nodes.find((n) => n.kind.type === "internet");
    if (internet) {
      nodes.unshift({ id: internet.id, node: internet, role: "external" });
      edges.unshift({ id: `internet|${f.entry}`, from: internet.id, to: f.entry, label: "routes to", strength: "declared", mode: "sync" });
    }
  }
  return { nodes, edges, groups: [], direction: "DOWN" };
}

/**
 * The resources a map covers, so a banner over it never names anything off the map. App blocks
 * stand for their resources in the environment shown.
 */
export function mapScope(ix: Index, view: View, env: string | null): Set<string> {
  const ids = new Set<string>();
  for (const v of view.nodes) {
    if (!v.id.startsWith("app:")) {
      ids.add(v.id);
      continue;
    }
    const app = v.id.slice(4);
    ix.snap.nodes.filter((n) => n.app?.value === app && inEnv(n, env)).forEach((n) => ids.add(n.id));
  }
  return ids;
}
