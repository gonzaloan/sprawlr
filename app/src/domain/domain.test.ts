import { describe, expect, it } from "vitest";
import demo from "../demo/demo-snapshot.json";
import { buildIndex, displayName, flowView, fmtX, hopVerb, insightsOf, mapScope, nameList, overview, apps, orphans, permissionOnly, shown, whatIf, type Snapshot } from ".";

const snap = demo as unknown as Snapshot;
const ix = buildIndex(snap);
const byName = (name: string) => snap.nodes.find((n) => n.name === name)!;

describe("vocabulary", () => {
  it("strips CloudFormation suffixes from names", () => {
    expect(displayName(byName("BillingStack-prod-InvoiceProcessor9F3A21-XK2"))).toBe("InvoiceProcessor");
    expect(displayName(byName("checkout-api"))).toBe("checkout-api");
  });
});

describe("format", () => {
  it("rounds headroom down, the safe reading", () => {
    expect(fmtX(1.259)).toBe("1.25×");
    expect(fmtX(2.99)).toBe("2.9×");
  });
});

describe("graph", () => {
  it("knows resources mapped only from IAM", () => {
    const worker = byName("reports-worker");
    expect(ix.iamMapped.has(worker.id)).toBe(true);
    expect(ix.iamMapped.has(byName("checkout-api").id)).toBe(false);
  });

  it("hides permission-only connections unless asked, except for IAM-mapped resources", () => {
    const invoice = byName("BillingStack-prod-InvoiceProcessor9F3A21-XK2");
    const iamOnly = (ix.out.get(invoice.id) ?? []).find(permissionOnly)!;
    expect(shown(ix, iamOnly, false)).toBe(false);
    expect(shown(ix, iamOnly, true)).toBe(true);
    const worker = byName("reports-worker");
    const fromWorker = (ix.out.get(worker.id) ?? []).find(permissionOnly)!;
    expect(shown(ix, fromWorker, false)).toBe(true);
  });
});

describe("views", () => {
  it("reads queue-to-consumer hops as delivery", () => {
    expect(hopVerb({ kind: "reads", mode: "async" })).toBe("delivers to");
    expect(hopVerb({ kind: "reads", mode: "sync" })).toBe("reads");
  });

  it("draws a flow top to bottom with the internet first", () => {
    const f = insightsOf(snap).flows.find((x) => x.name === "Web requests to checkout")!;
    const v = flowView(ix, f);
    expect(v.direction).toBe("DOWN");
    expect(v.nodes[0].node?.kind.type).toBe("internet");
  });

  it("overview has one block per app", () => {
    const summaries = apps(ix, "prod", snap.findings ?? []);
    const { shared } = orphans(ix, "prod");
    const v = overview(ix, "prod", summaries, shared);
    expect(v.nodes.filter((n) => n.app).map((n) => n.app!.name).sort()).toEqual(["billing", "checkout", "reporting", "search"]);
  });
});

describe("what if traffic grows", () => {
  const ins = insightsOf(snap);
  const named = (ids: { node: string }[]) => ids.map((c) => displayName(ix.byId.get(c.node)!));

  it("names only what is on the map shown", () => {
    const flow = ins.flows.find((x) => x.name === "Web requests to checkout")!;
    const here = whatIf(ins, 1.5, mapScope(ix, flowView(ix, flow), null));
    expect(named(here.broken)).toEqual(["checkout-orders-db"]);
    expect(named([here.next!])).toEqual(["checkout-api"]);
    const account = whatIf(ins, 1.5, mapScope(ix, overview(ix, null, apps(ix, null, snap.findings ?? []), []), null));
    expect(named(account.broken)).toEqual(["checkout-orders-db", "reports-worker"]);
  });

  it("an app block stands for its resources in the environment shown", () => {
    const dev = mapScope(ix, overview(ix, "dev", apps(ix, "dev", snap.findings ?? []), []), "dev");
    expect(whatIf(ins, 4, dev).broken).toEqual([]);
  });

  it("says when nothing on the map has load figures", () => {
    expect(whatIf(ins, 2, new Set(["nothing-here"])).measured).toBe(0);
  });
});

describe("nameList", () => {
  it("joins names the way a sentence would", () => {
    expect(nameList([])).toBe("");
    expect(nameList(["a"])).toBe("a");
    expect(nameList(["a", "b"])).toBe("a and b");
    expect(nameList(["a", "b", "c"])).toBe("a, b and c");
  });
});
