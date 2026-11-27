import { describe, expect, it } from "vitest";
import demo from "../demo/demo-snapshot.json";
import {
  appView,
  architectureOf,
  buildIndex,
  displayName,
  everythingView,
  fromHash,
  subnetReading,
  subnetTier,
  tierReading,
  toHash,
  zonesOf,
  type ArchNode,
  type Architecture,
  type Snapshot,
  type VpcPlan,
} from ".";

const snap = demo as unknown as Snapshot;
const ix = buildIndex(snap);
const byName = (name: string) => snap.nodes.find((n) => n.name === name || displayName(n) === name)!;
const names = (items: { node?: ArchNode }[]) => items.map((i) => (i.node ? displayName(i.node) : "?"));
const vpc = (a: Architecture, name: string): VpcPlan => a.regions.flatMap((r) => r.vpcs).find((v) => v.vpc.name === name)!;
const placed = (v: VpcPlan, name: string) => v.tiers.flatMap((t) => t.placed.map((p) => ({ ...p, tier: t.kind }))).find((p) => p.item.node && displayName(p.item.node) === name);

const subnet = (name: string, attrs: Record<string, unknown>): ArchNode => ({
  id: `arn:aws:ec2:us-east-1:1:subnet/${name}`,
  kind: { type: "subnet" },
  name,
  account: "1",
  region: "us-east-1",
  parent: null,
  aliases: [],
  app: null,
  env: null,
  tags: {},
  attrs,
  health: "unknown",
});

describe("where things run", () => {
  const checkout = architectureOf(ix, appView(ix, "checkout", null));

  it("puts the internet first, edge services next, and services outside AWS last", () => {
    expect(names(checkout.users)).toEqual(["Internet"]);
    expect(names(checkout.edge)).toEqual(["checkout-cdn"]);
    expect(checkout.regions.map((r) => r.region)).toEqual(["us-east-1"]);
  });

  it("draws a VPC as availability zones by subnet tier, with each resource across the zones of its subnets", () => {
    const prod = vpc(checkout, "prod-vpc");
    expect(prod.zones).toEqual(["us-east-1a", "us-east-1b"]);
    expect(prod.tiers.map((t) => t.kind)).toEqual(["public", "private"]);
    expect(placed(prod, "checkout-alb")).toMatchObject({ tier: "public", from: 0, to: 1, note: { label: "Subnets in 2 AZs" } });
    expect(placed(prod, "checkout-api")).toMatchObject({ tier: "private", from: 0, to: 1 });
    expect(prod.tiers[0].subnets.map((col) => col.map((s) => s.name))).toEqual([["prod-public-a"], ["prod-public-b"]]);
    expect(prod.tiers[0].by).toEqual(["name"]);
    expect(tierReading(prod.tiers[0])).toBe("From their names");
    expect(tierReading({ ...prod.tiers[0], by: [] })).toBe("Tier not known: route tables aren't read");
  });

  it("reads the subnet ids a scan stores, not only names", () => {
    const ids = ix.snap.nodes.map((n) => (n.name === "checkout-api" && n.id.includes("prod-cluster") ? { ...n, attrs: { ...n.attrs, subnets: ["subnet-priv-a", "subnet-priv-b"] } } : n));
    const byId = buildIndex({ ...snap, nodes: ids });
    expect(placed(vpc(architectureOf(byId, appView(byId, "checkout", null)), "prod-vpc"), "checkout-api")).toMatchObject({ tier: "private", from: 0, to: 1 });
  });

  it("keeps only the connections between what it draws, with their evidence", () => {
    const drawn = new Set([...checkout.users, ...checkout.edge, ...checkout.accountWide, ...checkout.outside].map((v) => v.id));
    checkout.regions.forEach((r) => {
      r.services.forEach((g) => g.items.forEach((v) => drawn.add(v.id)));
      r.vpcs.forEach((v) => [...v.loose.map((l) => l.item), ...v.tiers.flatMap((t) => t.placed.map((p) => p.item))].forEach((i) => drawn.add(i.id)));
    });
    expect(checkout.edges.length).toBeGreaterThan(0);
    expect(checkout.edges.every((e) => drawn.has(e.from) && drawn.has(e.to))).toBe(true);
    const albToApi = checkout.edges.find((e) => e.from.includes("checkout-alb") && e.to.endsWith("prod-cluster/checkout-api"));
    expect(albToApi?.strength).toBe("observed");
  });

  it("draws a resource whose subnets skip a zone once per run of zones it covers", () => {
    const c = { ...subnet("prod-public-c", { az: "us-east-1c", auto_public_ip: true }), parent: "arn:aws:ec2:us-east-1:111122223333:vpc/vpc-0a1prod" };
    const nodes = [...ix.snap.nodes.map((n) => (n.name === "checkout-alb" ? { ...n, attrs: { ...n.attrs, subnets: ["prod-public-a", "prod-public-c"] } } : n)), c];
    const skip = buildIndex({ ...snap, nodes });
    const prod = vpc(architectureOf(skip, appView(skip, "checkout", null)), "prod-vpc");
    const alb = prod.tiers.flatMap((t) => t.placed).filter((p) => p.item.node?.name === "checkout-alb");
    expect(prod.zones).toEqual(["us-east-1a", "us-east-1b", "us-east-1c"]);
    expect(alb.map((p) => [p.from, p.to, p.continued ?? false])).toEqual([
      [0, 0, false],
      [2, 2, true],
    ]);
  });

  it("keeps in the VPC what has no subnet, with its availability-zone setting", () => {
    const prod = vpc(checkout, "prod-vpc");
    const db = prod.loose.find((l) => l.item.node?.name === "checkout-orders-db");
    expect(db?.note).toEqual({ label: "Multi-AZ" });
  });

  it("groups what runs outside any VPC by what it does", () => {
    const [region] = checkout.regions;
    const group = (label: string) => names(region.services.find((g) => g.label === label)?.items ?? []);
    expect(group("Messaging and events")).toEqual(expect.arrayContaining(["checkout-order-events", "checkout-notifications"]));
    expect(group("Keys and secrets")).toContain("checkout/prod/db-credentials");
    expect(names(checkout.accountWide)).toContain("checkout-api-task-role");
  });

  it("draws a serverless app as its region's groups, with no VPC", () => {
    const billing = architectureOf(ix, appView(ix, "billing", null));
    expect(billing.regions[0].vpcs).toEqual([]);
    expect(names(billing.regions[0].services.find((g) => g.label === "Compute")!.items)).toContain("InvoiceProcessor");
    expect(names(billing.outside)).toContain("api.stripe.com");
  });

  it("puts NAT gateways in their subnets when their VPC is drawn", () => {
    const all = architectureOf(ix, everythingView(ix, null), { quietVpcs: true });
    expect(placed(vpc(all, "dev-vpc"), "nat-legacy-dev")).toMatchObject({ tier: "private", from: 0, to: 0 });
  });

  it("says when something that could span zones has subnets in only one", () => {
    const only = ix.snap.nodes.map((n) => (n.name === "checkout-api" && n.id.includes("prod-cluster") ? { ...n, attrs: { ...n.attrs, subnets: ["prod-private-a"] } } : n));
    const one = buildIndex({ ...snap, nodes: only });
    const prod = vpc(architectureOf(one, everythingView(one, null)), "prod-vpc");
    expect(placed(prod, "checkout-api")?.note).toEqual({ label: "Subnets in 1 AZ" });
    expect(placed(prod, "reports-worker")?.note).toBeUndefined();
  });
});

describe("a VPC with nothing of the place in it", () => {
  const quiet = {
    ...snap,
    nodes: [
      ...snap.nodes,
      { ...subnet("default-a", { az: "us-east-1a", auto_public_ip: true }), parent: "arn:aws:ec2:us-east-1:111122223333:vpc/vpc-default" },
      { ...subnet("default-b", { az: "us-east-1b", auto_public_ip: true }), parent: "arn:aws:ec2:us-east-1:111122223333:vpc/vpc-default" },
      { ...subnet("x", {}), id: "arn:aws:ec2:us-east-1:111122223333:vpc/vpc-default", kind: { type: "vpc" }, name: "default", parent: "region:111122223333/us-east-1", attrs: { is_default: true } },
    ],
  };
  const qix = buildIndex(quiet);

  it("is one line on the whole account", () => {
    const d = architectureOf(qix, everythingView(qix, null), { quietVpcs: true }).regions[0].quiet.find((v) => v.vpc.name === "default");
    expect(d).toMatchObject({ subnets: 2, zones: 2 });
  });

  it("is left out of an app", () => {
    expect(architectureOf(qix, appView(qix, "checkout", null), { quietVpcs: false }).regions[0].quiet).toEqual([]);
  });

  it("says nothing runs in it only when nothing does, in any environment", () => {
    const prod = architectureOf(qix, everythingView(qix, "prod"), { quietVpcs: true, env: "prod" }).regions[0];
    expect(prod.quiet.find((v) => v.vpc.name === "default")?.resources).toBe(0);
    expect(prod.quiet.find((v) => v.vpc.name === "dev-vpc")?.resources).toBeGreaterThan(0);
    expect(prod.vpcs.flatMap((v) => v.tiers.flatMap((t) => t.placed)).some((p) => p.item.node?.name === "nat-legacy-dev")).toBe(false);
  });
});

describe("subnet tiers", () => {
  it("come from the subnet's name, then from whether it gives instances public IPs", () => {
    expect(subnetTier(subnet("prod-public-a", { auto_public_ip: false }))).toBe("public");
    expect(subnetTier(subnet("prod-private-a", { auto_public_ip: true }))).toBe("private");
    expect(subnetTier(subnet("app-a", { auto_public_ip: true }))).toBe("public");
    expect(subnetTier(subnet("app-a", { auto_public_ip: false }))).toBe("subnets");
  });

  it("say how they were read", () => {
    expect(subnetReading(subnet("prod-public-a", {}))).toEqual({ tier: "public", by: "name" });
    expect(subnetReading(subnet("app-a", { auto_public_ip: true }))).toEqual({ tier: "public", by: "public_ips" });
    expect(subnetReading(subnet("app-a", {}))).toEqual({ tier: "subnets", by: null });
  });

  it("read the names CloudFormation, CDK and eksctl give subnets", () => {
    expect(subnetTier(subnet("PrivateSubnet1", {}))).toBe("private");
    expect(subnetTier(subnet("MyStack-PublicSubnetA", {}))).toBe("public");
    expect(subnetTier(subnet("eksctl-x/SubnetPrivateUSEAST1A", {}))).toBe("private");
  });
});

describe("availability zones of a resource", () => {
  it("are those of its subnets", () => {
    const api = ix.byId.get("arn:aws:ecs:us-east-1:111122223333:service/prod-cluster/checkout-api")!;
    expect(zonesOf(ix, api)).toEqual(["us-east-1a", "us-east-1b"]);
    expect(zonesOf(ix, byName("InvoicesTable"))).toEqual([]);
  });
});

describe("the architecture tab", () => {
  it("is part of the link", () => {
    const w = { place: { kind: "app" as const, name: "checkout" }, selected: null, tab: "architecture" as const };
    expect(toHash(w)).toBe("#/app/checkout?tab=architecture");
    expect(fromHash("#/app/checkout?tab=architecture").tab).toBe("architecture");
  });
});
