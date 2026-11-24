// Automatic layout with ELK (layered, left → right, VPCs as nested territories).
import ELK, { type ElkExtendedEdge, type ElkNode } from "elkjs/lib/elk.bundled.js";
import type { View } from "../../domain";

const elk = new ELK();

export const SIZE = {
  resource: { w: 252, h: 58 },
  app: { w: 252, h: 96 },
};

export interface Placed {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  parent?: string; // relative to this group
}

export interface Point {
  x: number;
  y: number;
}

export interface Layout {
  nodes: Placed[];
  /** Orthogonal routes computed by ELK, in absolute map coordinates. */
  routes: Map<string, Point[]>;
}

const ROOT = {
  "elk.algorithm": "layered",
  "elk.direction": "RIGHT",
  "elk.hierarchyHandling": "INCLUDE_CHILDREN",
  "elk.layered.spacing.nodeNodeBetweenLayers": "96",
  "elk.spacing.nodeNode": "26",
  "elk.spacing.componentComponent": "48",
  "elk.layered.nodePlacement.strategy": "NETWORK_SIMPLEX",
  "elk.layered.crossingMinimization.strategy": "LAYER_SWEEP",
  "elk.edgeRouting": "ORTHOGONAL",
  "elk.layered.spacing.edgeNodeBetweenLayers": "24",
  "elk.json.edgeCoords": "ROOT",
};

const GROUP = { "elk.padding": "[top=44,left=28,bottom=28,right=28]" };

export async function layout(view: View): Promise<Layout> {
  const size = (id: string) => (view.nodes.find((n) => n.id === id)?.app ? SIZE.app : SIZE.resource);
  const leaf = (id: string): ElkNode => ({ id, width: size(id).w, height: size(id).h });

  const groups: ElkNode[] = view.groups.map((g) => ({
    id: g.id,
    layoutOptions: GROUP,
    children: view.nodes.filter((n) => n.group === g.id).map((n) => leaf(n.id)),
  }));
  const loose = view.nodes.filter((n) => !n.group).map((n) => leaf(n.id));

  const res = await elk.layout({
    id: "root",
    layoutOptions:
      view.direction === "DOWN"
        ? { ...ROOT, "elk.direction": "DOWN", "elk.layered.spacing.nodeNodeBetweenLayers": "56", "elk.spacing.nodeNode": "40" }
        : ROOT,
    children: [...groups, ...loose],
    edges: view.edges.map((e) => ({ id: e.id, sources: [e.from], targets: [e.to] })),
  });

  const out: Placed[] = [];
  for (const c of res.children ?? []) {
    out.push({ id: c.id, x: c.x ?? 0, y: c.y ?? 0, w: c.width ?? 0, h: c.height ?? 0 });
    for (const k of c.children ?? [])
      out.push({ id: k.id, x: k.x ?? 0, y: k.y ?? 0, w: k.width ?? 0, h: k.height ?? 0, parent: c.id });
  }
  const routes = new Map<string, Point[]>();
  const collect = (n: ElkNode) => {
    for (const e of (n.edges ?? []) as ElkExtendedEdge[]) {
      const s = e.sections?.[0];
      if (s) routes.set(e.id, [s.startPoint, ...(s.bendPoints ?? []), s.endPoint]);
    }
    (n.children ?? []).forEach(collect);
  };
  collect(res);
  return { nodes: out, routes };
}
