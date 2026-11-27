// Flows, capacity and what-if, read from the snapshot's insights.
import type { Capacity, Flow, Insights, Snapshot } from "./types";

export const EMPTY_INSIGHTS: Insights = { flows: [], capacity: [], insights: [], lifecycle: [] };

export const insightsOf = (snap: Snapshot): Insights => ({ ...EMPTY_INSIGHTS, ...(snap.insights ?? {}) });

export function capacityIndex(ins: Insights): Map<string, Capacity> {
  return new Map(ins.capacity.map((c) => [c.node, c]));
}

export type LoadState = "ok" | "busy" | "saturated";

/** Utilisation if all traffic grows by `factor` (linear model, stated in the UI). */
export function projected(c: Capacity | undefined, factor: number): { util: number; state: LoadState } | undefined {
  if (!c) return undefined;
  const util = c.utilization * factor;
  return { util, state: util >= 1 ? "saturated" : util >= 0.7 ? "busy" : "ok" };
}

/** Capacity with a limit it can reach (headroom is null when measured idle). */
export type Bounded = Capacity & { headroom: number };
const bounded = (c: Capacity): c is Bounded => c.headroom !== null;

/**
 * Load of the resources in `scope`: how many have load figures at all, and those that can reach
 * a limit, in the order they break. The headroom itself is computed in Rust.
 */
export function limitsIn(capacity: Iterable<Capacity>, scope: ReadonlySet<string>): { measured: number; order: Bounded[] } {
  const here = [...capacity].filter((c) => scope.has(c.node));
  return { measured: here.length, order: here.filter(bounded).sort((a, b) => a.headroom - b.headroom) };
}

/** What on a map breaks at a traffic multiplier, and what breaks next. */
export function whatIf(ins: Insights, factor: number, scope: ReadonlySet<string>) {
  const { measured, order } = limitsIn(ins.capacity, scope);
  return { measured, broken: order.filter((c) => c.headroom <= factor), next: order.find((c) => c.headroom > factor) };
}

/** Whether the map has load figures to project traffic growth on. */
export const projectable = (ins: Insights, scope: ReadonlySet<string>) => limitsIn(ins.capacity, scope).measured > 0;

export function flowsThrough(ins: Insights, id: string): Flow[] {
  return ins.flows.filter((f) => f.nodes.includes(id));
}
