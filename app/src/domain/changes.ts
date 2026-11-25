// What changed since the previous scan, as rows for a list. The comparison itself is done in
// Rust (sprawlr_domain::changes) and stored with the snapshot.
import type { Changes } from "./types";

export type ChangeTag = "new" | "removed" | "changed" | "attention" | "resolved" | "error" | "health" | "connection";

export interface ChangeRow {
  tag: ChangeTag;
  label: string;
  text: string;
  /** A resource to open, when the row is about one that still exists. */
  id?: string;
}

const HEALTH: Record<string, string> = { ok: "OK", warning: "warning", alarm: "alarm firing", unknown: "unknown" };

/** Most telling first: what needs attention now, new errors, health, then structure. */
export function changeRows(c: Changes): ChangeRow[] {
  return [
    ...c.attention_new.map((t) => ({ tag: "attention" as const, label: "Needs attention", text: t })),
    ...c.new_errors.map((t) => ({ tag: "error" as const, label: "New error", text: t })),
    ...c.health.map((h) => ({ tag: "health" as const, label: "Health", text: `${h.resource.name}: ${HEALTH[h.before]} → ${HEALTH[h.after]}`, id: h.resource.id })),
    ...c.added.map((i) => ({ tag: "new" as const, label: "New", text: `${i.name} (${i.kind})`, id: i.id })),
    ...c.removed.map((i) => ({ tag: "removed" as const, label: "Removed", text: `${i.name} (${i.kind})` })),
    ...c.settings.map((s) => ({ tag: "changed" as const, label: "Changed", text: `${s.resource.name}: ${s.label} ${s.before} → ${s.after}`, id: s.resource.id })),
    ...c.connections_added.map((x) => ({ tag: "connection" as const, label: "New connection", text: `${x.from} ${x.verb} ${x.to}` })),
    ...c.connections_removed.map((x) => ({ tag: "connection" as const, label: "Gone", text: `${x.from} ${x.verb} ${x.to}` })),
    ...c.findings_new.map((t) => ({ tag: "attention" as const, label: "New finding", text: t })),
    ...c.attention_resolved.map((t) => ({ tag: "resolved" as const, label: "Resolved", text: t })),
    ...c.findings_resolved.map((t) => ({ tag: "resolved" as const, label: "Resolved", text: t })),
  ];
}

/** "Nov 23" style date for "Since …". */
export function sinceLabel(c: Changes): string {
  return new Date(c.since).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
