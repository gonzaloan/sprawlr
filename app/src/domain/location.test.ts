import { describe, expect, it } from "vitest";
import demo from "../demo/demo-snapshot.json";
import { buildIndex, crumbs, fromHash, HOME, insightsOf, openingOf, popped, pushed, resolve, startAt, toHash, type Snapshot, type Whereabouts } from ".";

const snap = demo as unknown as Snapshot;
const ix = buildIndex(snap);
const flows = insightsOf(snap).flows;
const byName = (name: string) => snap.nodes.find((n) => n.name === name)!;
const at = (place: Whereabouts["place"], selected: string | null = null): Whereabouts => ({ place, selected, tab: "map" });

describe("whereabouts in the URL", () => {
  it("round-trip through the hash", () => {
    const role = byName("checkout-api-task-role").id;
    const places: Whereabouts[] = [
      HOME,
      at({ kind: "app", name: "checkout" }, role),
      { place: { kind: "resource", id: role }, selected: role, tab: "docs" },
      at({ kind: "flow", id: flows[0].id }),
      at({ kind: "cleanup" }),
    ];
    for (const w of places) expect(fromHash(toHash(w))).toEqual(w);
    expect(toHash(HOME)).toBe("#/");
  });

  it("fall back to the whole account for a hash they do not know or cannot decode", () => {
    expect(fromHash("")).toEqual(HOME);
    expect(fromHash("#/nowhere/x")).toEqual(HOME);
    expect(fromHash("#/resource/arn%3Aaws%3")).toEqual(HOME);
  });
});

describe("resolve", () => {
  it("keeps what the snapshot on screen has", () => {
    const role = byName("checkout-api-task-role").id;
    expect(resolve(ix, flows, at({ kind: "app", name: "checkout" }, role))).toEqual(at({ kind: "app", name: "checkout" }, role));
  });

  it("sends a place the snapshot lacks to the whole account, and drops a selection it lacks", () => {
    expect(resolve(ix, flows, at({ kind: "app", name: "nope" }))).toEqual(HOME);
    expect(resolve(ix, flows, at({ kind: "flow", id: "nope" }))).toEqual(HOME);
    expect(resolve(ix, flows, at({ kind: "resource", id: "nope" }, "nope"))).toEqual(HOME);
    expect(resolve(ix, flows, at({ kind: "app", name: "checkout" }, "nope"))).toEqual(at({ kind: "app", name: "checkout" }));
  });

  it("shows the cleanup list as a list, never as documentation", () => {
    expect(resolve(ix, flows, { place: { kind: "cleanup" }, selected: null, tab: "docs" }).tab).toBe("map");
  });
});

describe("where a resource opens", () => {
  it("in its app, or on its own when it belongs to none", () => {
    const role = byName("checkout-api-task-role").id;
    const bus = byName("platform-events").id;
    expect(openingOf(ix, role)).toEqual(at({ kind: "app", name: "checkout" }, role));
    expect(openingOf(ix, bus)).toEqual(at({ kind: "resource", id: bus }, bus));
    expect(openingOf(ix, "nope")).toBeNull();
  });
});

describe("history steps", () => {
  it("a new place adds a step and drops the ones ahead", () => {
    expect(pushed({ at: 2, last: 5 })).toEqual({ at: 3, last: 3 });
  });

  it("Back and Forward move along the steps", () => {
    expect(popped({ at: 3, last: 3 }, { at: 2 })).toEqual({ at: 2, last: 3 });
  });

  it("a hash typed in the address bar counts as a new step", () => {
    expect(popped({ at: 1, last: 4 }, null)).toEqual({ at: 2, last: 2 });
  });

  it("a reload starts where its history entry says", () => {
    expect(startAt({ at: 3 })).toEqual({ at: 3, last: 3 });
    expect(startAt(null)).toEqual({ at: 0, last: 0 });
  });
});

describe("breadcrumbs", () => {
  const labels = (w: Whereabouts) => crumbs(ix, flows, w).map((c) => c.label);

  it("go from the whole account to the app, then to the resource selected", () => {
    const role = byName("checkout-api-task-role").id;
    const c = crumbs(ix, flows, at({ kind: "app", name: "checkout" }, role));
    expect(c.map((x) => x.label)).toEqual(["Whole account", "checkout", "checkout-api-task-role"]);
    expect(c[0].to).toEqual(HOME);
    expect(c[1].to).toEqual(at({ kind: "app", name: "checkout" }));
  });

  it("name a flow and the cleanup list", () => {
    const f = flows.find((x) => x.name === "Web requests to checkout")!;
    expect(labels(at({ kind: "flow", id: f.id }))).toEqual(["Whole account", "Web requests to checkout"]);
    expect(labels(at({ kind: "cleanup" }))).toEqual(["Whole account", "Probably not needed"]);
  });

  it("put a resource that belongs to no app right under the whole account", () => {
    const bus = byName("platform-events").id;
    expect(labels(at({ kind: "resource", id: bus }, bus))).toEqual(["Whole account", "platform-events"]);
  });
});
