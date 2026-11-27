// Where things run, drawn the way an AWS architecture diagram does: the internet, edge services,
// then each region with its VPCs (availability zones by subnet tier) and what runs outside them.
// Placement only: every fact comes from the snapshot.
import { inEnv, mappable, vpcOf, type Index } from "./graph";
import type { ArchNode, Layer } from "./types";
import type { View, ViewEdge, ViewNode } from "./views";
import { displayName, layerOf } from "./vocabulary";

export type TierKind = "public" | "private" | "subnets";

/**
 * What the scan says about zones, as configured: "Multi-AZ" or "Single-AZ" (a database setting),
 * "Subnets in 2 AZs". Configuration, not a judgement: how many zones something runs in also
 * depends on how many copies run, and findings judge it.
 */
export interface ZoneNote {
  label: string;
}

/**
 * A resource in a tier, across the zone columns `from`..`to` (indexes into the VPC's zones). A
 * resource whose subnets skip a zone is placed once per run of zones; the runs after the first
 * are `continued`.
 */
export interface Placed {
  item: ViewNode;
  from: number;
  to: number;
  note?: ZoneNote;
  continued?: boolean;
}

/** How a subnet's tier was read: from its name, or because it gives new instances public IPs. */
export type TierBasis = "name" | "public_ips";

export interface Tier {
  kind: TierKind;
  label: string;
  /** How its subnets' tier was read; empty when it is not known. */
  by: TierBasis[];
  /** The tier's subnets in each zone column. */
  subnets: ArchNode[][];
  placed: Placed[];
}

/** In the VPC, but in no subnet the scan knows (a database, a function with no subnets read). */
export interface Loose {
  item: ViewNode;
  note?: ZoneNote;
}

export interface VpcPlan {
  vpc: ArchNode;
  zones: string[];
  tiers: Tier[];
  loose: Loose[];
}

/** A VPC with nothing of the place in it: one line, not a box. */
export interface QuietVpc {
  vpc: ArchNode;
  subnets: number;
  zones: number;
  /** What runs in it in any environment, gateways included: 0 means nothing does. */
  resources: number;
}

export interface ServiceGroup {
  layer: Layer;
  label: string;
  items: ViewNode[];
}

export interface RegionPlan {
  /** "" when the scan doesn't say. */
  region: string;
  vpcs: VpcPlan[];
  quiet: QuietVpc[];
  services: ServiceGroup[];
}

export interface Architecture {
  users: ViewNode[];
  edge: ViewNode[];
  /** Account-wide, not regional: IAM. */
  accountWide: ViewNode[];
  regions: RegionPlan[];
  outside: ViewNode[];
  edges: ViewEdge[];
}

const EDGE_KINDS = new Set(["cloud_front", "route53_zone"]);
const ACCOUNT_WIDE = new Set(["iam_role", "iam_policy"]);
/** Network pieces an architecture diagram draws in their VPC, though the map leaves them out. */
const GATEWAYS = new Set(["nat_gateway", "internet_gateway", "vpc_endpoint"]);
/** Kinds that can run in several zones, so having subnets in one is worth saying. */
const CAN_SPAN = new Set(["ecs_service", "load_balancer", "rds_cluster", "rds_instance", "elasti_cache", "eks_cluster"]);

const GROUPS: [Layer, string][] = [
  ["edge", "Entry points"],
  ["compute", "Compute"],
  ["integration", "Messaging and events"],
  ["data", "Data"],
  ["identity", "Keys and secrets"],
];

const TIER_LABEL: Record<TierKind, string> = { public: "Public subnets", private: "Private subnets", subnets: "Subnets" };
const TIER_ORDER: TierKind[] = ["public", "private", "subnets"];
const PUBLIC_WORDS = new Set(["public", "pub", "dmz"]);
const PRIVATE_WORDS = new Set(["private", "priv", "isolated", "internal", "db", "data", "database"]);

/**
 * A subnet's tier and how it was read: its name first ("prod-public-a", "PrivateSubnet1"), then
 * whether it gives new instances public IPs. Without either, sprawlr doesn't guess: route tables
 * would say, and they aren't read.
 */
export function subnetReading(subnet: ArchNode): { tier: TierKind; by: TierBasis | null } {
  const words = subnet.name
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z0-9]+/);
  if (words.some((w) => PUBLIC_WORDS.has(w))) return { tier: "public", by: "name" };
  if (words.some((w) => PRIVATE_WORDS.has(w))) return { tier: "private", by: "name" };
  return subnet.attrs?.auto_public_ip === true ? { tier: "public", by: "public_ips" } : { tier: "subnets", by: null };
}

export const subnetTier = (subnet: ArchNode): TierKind => subnetReading(subnet).tier;

/** How a tier's subnets were read, for the label next to it. */
export function tierReading(t: Tier): string {
  if (!t.by.length) return "Tier not known: route tables aren't read";
  if (t.by.length > 1) return "From their names, or the public IPs they give instances";
  return t.by[0] === "name" ? "From their names" : "They give new instances public IPs";
}

const zoneOf = (subnet: ArchNode) => (typeof subnet.attrs?.az === "string" ? subnet.attrs.az : "");
const byName = (a: ViewNode, b: ViewNode) => displayName(a.node!).localeCompare(displayName(b.node!));

function subnetsIn(ix: Index, vpc: ArchNode): ArchNode[] {
  return ix.snap.nodes.filter((n) => n.kind.type === "subnet" && n.parent === vpc.id);
}

/** The subnets a resource names, by id, name or alias (scans store ids, people use names). */
function subnetsOf(n: ArchNode, candidates: ArchNode[]): ArchNode[] {
  const refs = Array.isArray(n.attrs?.subnets) ? (n.attrs.subnets as string[]) : [];
  return candidates.filter((s) => refs.some((r) => s.id.endsWith(`/${r}`) || s.name === r || s.aliases.includes(r)));
}

/** The availability zones a resource runs in, from its subnets. */
export function zonesOf(ix: Index, n: ArchNode): string[] {
  const vpc = vpcOf(ix, n);
  if (!vpc) return [];
  return [...new Set(subnetsOf(n, subnetsIn(ix, vpc)).map(zoneOf).filter(Boolean))].sort();
}

function noteFor(n: ArchNode, zones: number, vpcZones: number): ZoneNote | undefined {
  if (n.attrs?.multi_az === true) return { label: "Multi-AZ" };
  if (n.attrs?.multi_az === false) return { label: "Single-AZ" };
  if (zones > 1) return { label: `Subnets in ${zones} AZs` };
  if (zones === 1 && vpcZones > 1 && CAN_SPAN.has(n.kind.type)) return { label: "Subnets in 1 AZ" };
  return undefined;
}

/** Runs of consecutive columns: [0, 2, 3] → [[0, 0], [2, 3]]. */
function runs(columns: number[]): [number, number][] {
  const out: [number, number][] = [];
  for (const c of [...new Set(columns)].sort((a, b) => a - b)) {
    const last = out[out.length - 1];
    if (last && c === last[1] + 1) last[1] = c;
    else out.push([c, c]);
  }
  return out;
}

/** The tier most of a resource's subnets are in; ties go to the more exposed one. */
function tierOf(subnets: ArchNode[]): TierKind {
  const count = (k: TierKind) => subnets.filter((s) => subnetTier(s) === k).length;
  return TIER_ORDER.reduce((best, k) => (count(k) > count(best) ? k : best), TIER_ORDER[0]);
}

function planVpc(ix: Index, vpc: ArchNode, items: ViewNode[]): VpcPlan {
  const subnets = subnetsIn(ix, vpc).filter((s) => zoneOf(s));
  const inSubnets: { item: ViewNode; subnets: ArchNode[]; tier: TierKind }[] = [];
  const loose: Loose[] = [];
  for (const item of [...items].sort(byName)) {
    const own = subnetsOf(item.node!, subnets);
    if (own.length) inSubnets.push({ item, subnets: own, tier: tierOf(own) });
    else loose.push({ item, note: noteFor(item.node!, 0, 0) });
  }
  const tiers = TIER_ORDER.filter((k) => inSubnets.some((p) => p.tier === k));
  const zones = [...new Set(subnets.filter((s) => tiers.includes(subnetTier(s))).map(zoneOf))].sort();
  return {
    vpc,
    zones,
    loose,
    tiers: tiers.map((kind) => {
      const mine = subnets.filter((s) => subnetTier(s) === kind);
      return {
        kind,
        label: TIER_LABEL[kind],
        by: (["name", "public_ips"] as TierBasis[]).filter((b) => mine.some((s) => subnetReading(s).by === b)),
        subnets: zones.map((z) => mine.filter((s) => zoneOf(s) === z).sort((a, b) => a.name.localeCompare(b.name))),
        placed: inSubnets
          .filter((p) => p.tier === kind)
          .flatMap((p) => {
            const own = new Set(p.subnets.map(zoneOf)).size;
            const note = noteFor(p.item.node!, own, zones.length);
            const columns = p.subnets.filter((s) => subnetTier(s) === kind).map((s) => zones.indexOf(zoneOf(s)));
            return runs(columns).map(([from, to], i): Placed => ({ item: p.item, from, to, note, ...(i > 0 ? { continued: true } : {}) }));
          })
          .sort((a, b) => a.from - b.from || b.to - b.from - (a.to - a.from) || byName(a.item, b.item)),
      };
    }),
  };
}

/** What runs in a VPC in any environment, its gateways included. */
const runningIn = (ix: Index, vpc: ArchNode) =>
  ix.snap.nodes.filter((n) => (mappable(n) || GATEWAYS.has(n.kind.type)) && vpcOf(ix, n)?.id === vpc.id).length;

/** A node's region, from the node, its VPC, or its ARN (a reference to another region's resource). */
function regionOf(n: ArchNode, vpc: ArchNode | undefined): string {
  return n.region ?? vpc?.region ?? (n.id.startsWith("arn:") ? (n.id.split(":")[3] ?? "") : "");
}

interface Options {
  /** Show every VPC of the account, those with nothing of the place in it as one line. */
  quietVpcs?: boolean;
  /** The environment shown: gateways added to a drawn VPC follow it. */
  env?: string | null;
}

/** The place's resources, drawn where they run. */
export function architectureOf(ix: Index, view: View, { quietVpcs = false, env = null }: Options = {}): Architecture {
  const users: ViewNode[] = [];
  const edge: ViewNode[] = [];
  const accountWide: ViewNode[] = [];
  const outside: ViewNode[] = [];
  const regions = new Map<string, { vpcs: Map<string, ViewNode[]>; services: ViewNode[] }>();
  const region = (key: string) => {
    if (!regions.has(key)) regions.set(key, { vpcs: new Map(), services: [] });
    return regions.get(key)!;
  };
  const drawn = new Set<string>();

  for (const v of view.nodes) {
    const n = v.node;
    if (!n) continue;
    drawn.add(n.id);
    const kind = n.kind.type;
    if (kind === "internet") users.push(v);
    else if (layerOf(n) === "external") outside.push(v);
    else if (EDGE_KINDS.has(kind)) edge.push(v);
    else if (ACCOUNT_WIDE.has(kind)) accountWide.push(v);
    else {
      const vpc = vpcOf(ix, n);
      const r = region(regionOf(n, vpc));
      if (!vpc) r.services.push(v);
      else r.vpcs.set(vpc.id, [...(r.vpcs.get(vpc.id) ?? []), v]);
    }
  }

  // Gateways belong in the picture of a VPC that is drawn.
  for (const r of regions.values())
    for (const [vpcId, items] of r.vpcs)
      for (const n of ix.snap.nodes)
        if (GATEWAYS.has(n.kind.type) && !drawn.has(n.id) && inEnv(n, env) && vpcOf(ix, n)?.id === vpcId) {
          // Shared network plumbing, not part of the place: drawn like a neighbour.
          items.push({ id: n.id, node: n, role: "neighbor" });
          drawn.add(n.id);
        }

  if (quietVpcs)
    for (const vpc of ix.snap.nodes.filter((n) => n.kind.type === "vpc")) {
      const r = region(vpc.region ?? "");
      if (!r.vpcs.has(vpc.id)) r.vpcs.set(vpc.id, []);
    }

  const plans: RegionPlan[] = [...regions.entries()]
    .sort(([a], [b]) => (a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)))
    .map(([key, r]) => {
      const vpcs = [...r.vpcs.entries()]
        .flatMap(([id, items]) => {
          const vpc = ix.byId.get(id);
          return vpc ? [{ vpc, items }] : [];
        })
        .sort((a, b) => a.vpc.name.localeCompare(b.vpc.name));
      return {
        region: key,
        vpcs: vpcs.filter((v) => v.items.length).map((v) => planVpc(ix, v.vpc, v.items)),
        quiet: vpcs
          .filter((v) => !v.items.length)
          .map((v) => {
            const subnets = subnetsIn(ix, v.vpc);
            return { vpc: v.vpc, subnets: subnets.length, zones: new Set(subnets.map(zoneOf).filter(Boolean)).size, resources: runningIn(ix, v.vpc) };
          }),
        services: GROUPS.map(([layer, label]) => ({
          layer,
          label,
          items: r.services.filter((v) => layerOf(v.node!) === layer).sort((a, b) => a.node!.kind.type.localeCompare(b.node!.kind.type) || byName(a, b)),
        })).filter((g) => g.items.length),
      };
    });

  return {
    users: users.sort(byName),
    edge: edge.sort(byName),
    accountWide: accountWide.sort(byName),
    regions: plans,
    outside: outside.sort(byName),
    edges: view.edges.filter((e) => drawn.has(e.from) && drawn.has(e.to)),
  };
}
