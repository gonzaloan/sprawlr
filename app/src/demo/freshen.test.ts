import { describe, expect, it } from "vitest";
import type { Snapshot } from "../domain";
import raw from "./demo-snapshot.json";
import { freshen } from "./freshen";

const snap = raw as unknown as Snapshot;

describe("freshen", () => {
  it("makes the demo read two minutes old and keeps every gap between times", () => {
    const now = new Date("2030-01-01T12:00:00Z");
    const fresh = freshen(snap, now);
    expect(fresh.taken_at).toBe("2030-01-01T11:58:00.000Z");
    const gap = (s: Snapshot) => Date.parse(s.taken_at) - Date.parse(s.changes!.since);
    expect(gap(fresh)).toBe(gap(snap));
    expect(fresh.nodes.map((n) => n.name)).toEqual(snap.nodes.map((n) => n.name));
    const logTime = (s: Snapshot) => JSON.stringify(s).match(/"last_seen":"([^"]+)"/)![1];
    expect(logTime(fresh)).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$/);
    expect(Date.parse(fresh.taken_at) - Date.parse(logTime(fresh).replace(" ", "T") + "Z")).toBe(
      Date.parse(snap.taken_at) - Date.parse(logTime(snap).replace(" ", "T") + "Z"),
    );
  });
});

describe("freshen with a time it can't read", () => {
  it("keeps the demo as generated", () => {
    const odd = { ...snap, taken_at: "not a time" };
    expect(freshen(odd, new Date())).toBe(odd);
  });
});
