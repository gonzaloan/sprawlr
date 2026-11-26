// The demo account was read once, when the demo was generated. Shown as is, it would say
// "read 3 weeks ago" a few weeks later. This moves every time in it forward so the read is
// always a couple of minutes old, keeping the gaps between times exactly as they were.
import type { Snapshot } from "../domain";

const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;
/** CloudWatch Logs Insights times ("2026-11-24 11:52:03.120"), always UTC. */
const LOGS = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$/;

/** How old the demo read looks. */
const AGE_MS = 2 * 60_000;

export function freshen(snapshot: Snapshot, now: Date): Snapshot {
  const shift = now.getTime() - AGE_MS - Date.parse(snapshot.taken_at);
  // A browser that can't parse the stored time keeps the demo as generated rather than failing.
  if (!Number.isFinite(shift)) return snapshot;
  const move = (v: unknown): unknown => {
    if (typeof v === "string" && ISO.test(v)) return new Date(Date.parse(v) + shift).toISOString();
    if (typeof v === "string" && LOGS.test(v)) return new Date(Date.parse(v.replace(" ", "T") + "Z") + shift).toISOString().replace("T", " ").slice(0, 23);
    if (Array.isArray(v)) return v.map(move);
    if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, move(x)]));
    return v;
  };
  return move(snapshot) as Snapshot;
}
