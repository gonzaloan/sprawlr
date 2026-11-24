// The account in one line, as the tray and the popover show it. The rules live in Rust
// (sprawlr_domain::status); this only reads the stored status and formats time.
import type { AccountStatus, Level, Snapshot } from "./types";

/** How many items the popover lists. */
export const TOP = 3;
/** Same as Rust's STALE_AFTER_HOURS: an older status is not shown as current. */
export const STALE_AFTER_HOURS = 24;

export const LEVEL_LABEL: Record<Level, string> = {
  act_now: "Act now",
  plan_for_it: "Plan for it",
  all_clear: "All clear",
  not_measured: "Health not read",
};

export function statusOf(s: Snapshot): AccountStatus | null {
  return s.status ?? null;
}

export function isStale(takenAt: string, now: Date): boolean {
  return now.getTime() - new Date(takenAt).getTime() > STALE_AFTER_HOURS * 3600_000;
}

/** "just now", "12 min ago", "5 h ago", then the date. */
export function readAgo(takenAt: string, now: Date): string {
  const then = new Date(takenAt);
  const min = Math.max(0, Math.floor((now.getTime() - then.getTime()) / 60_000));
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  if (min < 48 * 60) return `${Math.floor(min / 60)} h ago`;
  return then.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** "$37" style, as the briefing prints it. */
export function monthlyCost(usd: number): string {
  return `$${usd.toFixed(0)}`;
}

/** Error patterns read longer ago than the health next to them say when. */
export function logsReadNote(readAt: string | null | undefined, takenAt: string): string | null {
  if (!readAt) return null;
  const gap = new Date(takenAt).getTime() - new Date(readAt).getTime();
  if (gap < 10 * 60_000) return null;
  return `Error messages from ${new Date(readAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}; background refreshes skip logs.`;
}
