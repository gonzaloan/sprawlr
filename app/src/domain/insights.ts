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

/** Resources that saturate at or below `factor`, in the order they break. */
export function breakingAt(ins: Insights, factor: number): Capacity[] {
  return ins.capacity.filter((c) => c.headroom !== null && c.headroom <= factor).sort((a, b) => (a.headroom ?? 0) - (b.headroom ?? 0));
}

export function flowsThrough(ins: Insights, id: string): Flow[] {
  return ins.flows.filter((f) => f.nodes.includes(id));
}
