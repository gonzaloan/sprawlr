// The account in one line, as the tray and the popover show it. The rules live in Rust
// (sprawlr_domain::status); this only reads the stored status and formats time.
import type { AccountStatus, Level, Snapshot } from "./types";

import { logErrorsOf } from "./signals";

/** How many items the popover lists. */
export const TOP = 3;

export const LEVEL_LABEL: Record<Level, string> = {
  act_now: "Act now",
  plan_for_it: "Plan for it",
  all_clear: "All clear",
  not_measured: "Health not read",
};

export function statusOf(s: Snapshot): AccountStatus | null {
  return s.status ?? null;
}

/** Whether a status is too old to show as current; the threshold comes with the status. */
export function isStale(takenAt: string, now: Date, status: Pick<AccountStatus, "stale_after_hours">): boolean {
  return now.getTime() - new Date(takenAt).getTime() > status.stale_after_hours * 3600_000;
}

/** "just now", "12 min ago", "5 h ago", then the date. */
export function readAgo(takenAt: string, now: Date): string {
  const hours = (now.getTime() - new Date(takenAt).getTime()) / 3600_000;
  return hours < 48 ? timeAgo(takenAt, now) : new Date(takenAt).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** "12 min ago", "5 h ago", "3 days ago": always relative. */
export function timeAgo(iso: string, now: Date): string {
  const min = Math.max(0, Math.floor((now.getTime() - new Date(iso).getTime()) / 60_000));
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  if (min < 48 * 60) return `${Math.floor(min / 60)} h ago`;
  return `${Math.floor(min / 1440)} days ago`;
}

/** "$37" style, as the briefing prints it. */
export function monthlyCost(usd: number): string {
  return `$${usd.toFixed(0)}`;
}

/** The note for a snapshot whose error patterns are older than its health, if any. */
export function logsNoteOf(s: Snapshot): string | null {
  const reads = s.nodes.map((n) => logErrorsOf(n)?.read_at).filter((t): t is string => !!t);
  return reads.length ? logsReadNote(reads.sort()[0], s.taken_at) : null;
}

/** Error patterns read longer ago than the health next to them say when. */
export function logsReadNote(readAt: string | null | undefined, takenAt: string): string | null {
  if (!readAt) return null;
  const gap = new Date(takenAt).getTime() - new Date(readAt).getTime();
  if (gap < 10 * 60_000) return null;
  return `Error messages from ${new Date(readAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}; background refreshes skip logs.`;
}
