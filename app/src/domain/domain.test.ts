import { describe, expect, it } from "vitest";
import demo from "../demo/demo-snapshot.json";
import { buildIndex, displayName, flowView, fmtX, hopVerb, insightsOf, overview, apps, orphans, permissionOnly, shown, type Snapshot } from ".";

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
