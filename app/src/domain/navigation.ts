// What the sidebar lists: apps, and resources shared by several apps or by none.
import type { ArchNode, Finding } from "./types";
import { type Index, inEnv, isRuntime, mappable } from "./graph";
import { envName, layerOf } from "./vocabulary";

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
    // Roles are reached through what assumes them; an account has hundreds.
    if (layerOf(n) === "identity") continue;
    const touching = new Set<string>();
    for (const e of [...(ix.out.get(n.id) ?? []), ...(ix.inc.get(n.id) ?? [])]) {
      const other = ix.byId.get(e.from === n.id ? e.to : e.from);
      if (other?.app && isRuntime(e)) touching.add(other.app.value);
    }
    (touching.size > 1 ? shared : unassigned).push(n);
  }
  return { shared, unassigned };
}
