// Where a person is in the app, how that reads as a URL (so Back, Forward and links work), the
// steps of history behind it, and the trail back to the whole account.
import type { Flow } from "./types";
import type { Index } from "./graph";
import { displayName } from "./vocabulary";

export type Place =
  | { kind: "overview" }
  | { kind: "app"; name: string }
  | { kind: "resource"; id: string }
  | { kind: "flow"; id: string }
  | { kind: "cleanup" };

export type Tab = "map" | "architecture" | "docs";

export interface Whereabouts {
  place: Place;
  /** The resource whose details are open. */
  selected: string | null;
  tab: Tab;
}

export const HOME: Readonly<Whereabouts> = Object.freeze({ place: { kind: "overview" }, selected: null, tab: "map" } as const);

const enc = encodeURIComponent;

/** `#/`, `#/app/checkout?selected=<id>`, `#/flow/<id>?tab=docs`… */
export function toHash(w: Whereabouts): string {
  const p = w.place;
  const path = p.kind === "overview" ? "" : p.kind === "cleanup" ? "cleanup" : `${p.kind}/${enc(p.kind === "app" ? p.name : p.id)}`;
  const query = new URLSearchParams();
  if (w.selected) query.set("selected", w.selected);
  if (w.tab !== "map") query.set("tab", w.tab);
  const q = query.toString();
  return `#/${path}${q ? `?${q}` : ""}`;
}

/** The whereabouts a hash names; the whole account for anything else, a mangled link included. */
export function fromHash(hash: string): Whereabouts {
  try {
    const [path, q = ""] = hash.replace(/^#\/?/, "").split("?");
    const query = new URLSearchParams(q);
    const [kind, raw] = path.split("/");
    const value = raw === undefined ? "" : decodeURIComponent(raw);
    const place: Place | null =
      kind === "" ? { kind: "overview" }
      : kind === "cleanup" ? { kind: "cleanup" }
      : kind === "app" && value ? { kind: "app", name: value }
      : kind === "resource" && value ? { kind: "resource", id: value }
      : kind === "flow" && value ? { kind: "flow", id: value }
      : null;
    if (!place) return HOME;
    const tab = query.get("tab");
    return { place, selected: query.get("selected"), tab: tab === "docs" || tab === "architecture" ? tab : "map" };
  } catch {
    return HOME;
  }
}

/**
 * The same whereabouts, kept to what the snapshot on screen has: a place it lacks becomes the
 * whole account, a selection it lacks is dropped. Links and Back can name another snapshot's.
 */
export function resolve(ix: Index, flows: Flow[], w: Whereabouts): Whereabouts {
  const p = w.place;
  const known =
    p.kind === "app" ? ix.snap.nodes.some((n) => n.app?.value === p.name)
    : p.kind === "resource" ? ix.byId.has(p.id)
    : p.kind === "flow" ? flows.some((f) => f.id === p.id)
    : true;
  if (!known) return HOME;
  const selected = w.selected && ix.byId.has(w.selected) ? w.selected : null;
  return { place: p, selected, tab: p.kind === "cleanup" ? "map" : w.tab };
}

/** Where a resource opens: selected in its app, or on its own when it belongs to none. */
export function openingOf(ix: Index, id: string): Whereabouts | null {
  const n = ix.byId.get(id);
  if (!n) return null;
  return { place: n.app ? { kind: "app", name: n.app.value } : { kind: "resource", id }, selected: id, tab: "map" };
}

/** Steps of history: the one on screen and the furthest one Forward can reach. */
export interface Steps {
  at: number;
  last: number;
}

/** A new place: one step on, and nothing ahead any more. */
export const pushed = (s: Steps): Steps => ({ at: s.at + 1, last: s.at + 1 });

/** Back or Forward to a history entry; one without a step (a hash typed in the address bar) is a new step. */
export function popped(s: Steps, state: unknown): Steps {
  const at = stepOf(state);
  return at === null ? pushed(s) : { at, last: Math.max(s.last, at) };
}

/** The steps a page opens on, from its history entry (a reload keeps it). */
export function startAt(state: unknown): Steps {
  const at = stepOf(state) ?? 0;
  return { at, last: at };
}

const stepOf = (state: unknown): number | null => {
  const at = (state as { at?: unknown } | null)?.at;
  return typeof at === "number" ? at : null;
};

export interface Crumb {
  label: string;
  to: Whereabouts;
}

/** The trail to some whereabouts: the whole account, the app or flow, the resource open. */
export function crumbs(ix: Index, flows: Flow[], w: Whereabouts): Crumb[] {
  const there = (place: Place, selected: string | null = null): Whereabouts => ({ place, selected, tab: w.tab });
  const nodeName = (id: string) => {
    const n = ix.byId.get(id);
    return n ? displayName(n) : "Resource";
  };
  const out: Crumb[] = [{ label: "Whole account", to: { ...HOME, tab: w.tab } }];
  const p = w.place;
  if (p.kind === "app") out.push({ label: p.name, to: there(p) });
  if (p.kind === "flow") out.push({ label: flows.find((f) => f.id === p.id)?.name ?? "Flow", to: there(p) });
  if (p.kind === "cleanup") out.push({ label: "Probably not needed", to: there(p) });
  if (p.kind === "resource") out.push({ label: nodeName(p.id), to: there(p, p.id) });
  if (w.selected && !(p.kind === "resource" && p.id === w.selected)) out.push({ label: nodeName(w.selected), to: there(p, w.selected) });
  return out;
}
