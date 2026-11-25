// What changed since the previous scan. The comparison and its rows are computed in Rust
// (sprawlr_domain::changes) and stored with the snapshot; this only reads and dates them.
import type { Changes, Snapshot } from "./types";

/** "Nov 23" style date for "Since …". */
export function sinceLabel(c: Changes): string {
  return new Date(c.since).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** One line for the tray, or null when nothing changed or nothing is known. */
export function changesLine(s: Snapshot): string | null {
  const c = s.changes;
  if (!c || !c.rows?.length) return null;
  return `Since ${sinceLabel(c)}: ${c.summary.charAt(0).toLowerCase()}${c.summary.slice(1)}.`;
}
