import { describe, expect, it } from "vitest";
import demo from "../demo/demo-snapshot.json";
import { isStale, logsReadNote, monthlyCost, readAgo, statusOf, timeAgo, type Snapshot } from ".";

const snap = demo as unknown as Snapshot;
const at = (iso: string, plusMin: number) => new Date(new Date(iso).getTime() + plusMin * 60_000);

describe("status", () => {
  it("is computed in Rust and stored with the demo snapshot", () => {
    const s = statusOf(snap)!;
    expect(s.level).toBe("act_now");
    expect(s.headline).toBe("2 to act on now, 3 to plan for");
    expect(s.items.slice(0, 3).map((i) => i.title)).toEqual([
      "Errors start at InvoiceProcessor",
      "checkout-orders-db saturates first",
      "InvoicesTable is on 2 flows",
    ]);
    expect(monthlyCost(s.cleanup.monthly_cost)).toBe("$37");
  });

  it("reads time like a person", () => {
    const t = "2026-11-24T12:00:00Z";
    expect(readAgo(t, at(t, 0))).toBe("just now");
    expect(readAgo(t, at(t, 12))).toBe("12 min ago");
    expect(readAgo(t, at(t, 5 * 60))).toBe("5 h ago");
    expect(timeAgo(t, at(t, 3 * 1440))).toBe("3 days ago");
  });

  it("stops calling a day-old status current", () => {
    const t = "2026-11-24T12:00:00Z";
    const s = statusOf(snap)!;
    expect(s.stale_after_hours).toBe(24);
    expect(isStale(t, at(t, 23 * 60), s)).toBe(false);
    expect(isStale(t, at(t, 25 * 60), s)).toBe(true);
  });

  it("dates error messages older than the health next to them", () => {
    const t = "2026-11-24T12:00:00Z";
    expect(logsReadNote(null, t)).toBeNull();
    expect(logsReadNote("2026-11-24T11:55:00Z", t)).toBeNull();
    expect(logsReadNote("2026-11-24T08:00:00Z", t)).toMatch(/^Error messages from .+; background refreshes skip logs\.$/);
  });
});

describe("changes", () => {
  it("come with the demo snapshot, most telling first", async () => {
    const { changeRows } = await import(".");
    const c = snap.changes!;
    expect(c.summary).toMatch(/^1 new resource, 1 removed, 1 setting changed/);
    const rows = changeRows(c);
    expect(rows[0].tag).toBe("attention");
    expect(rows.find((r) => r.tag === "changed")?.text).toBe("InvoiceProcessor: Timeout (s) 5 → 60");
    expect(rows.find((r) => r.tag === "new")?.id).toBe("arn:aws:s3:::reports-output");
  });
});
