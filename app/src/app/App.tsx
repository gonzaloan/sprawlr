import { useEffect, useMemo, useRef, useState } from "react";
import { useWhereabouts } from "./useWhereabouts";
import type { DocFile, Snapshot, Target } from "../domain";
import { DEMO, download, useBackend } from "../platform";
import { DocsView } from "../features/docs/DocsView";
import {
  appView,
  architectureOf,
  everythingView,
  apps as appSummaries,
  blastRadius,
  buildIndex,
  capacityIndex,
  crumbs,
  displayName,
  envChoices,
  envName,
  exportScopeOf,
  flowView,
  HOME,
  insightsOf,
  mapScope,
  orphans,
  openingOf,
  overview,
  projectable,
  projected,
  resolve,
  resourceFocus,
  toHash,
  type Place,
  type Tab,
  type Whereabouts,
} from "../domain";
import { Briefing } from "../features/briefing/Briefing";
import { FlowPanel } from "../features/flows/FlowPanel";
import { Cleanup } from "../features/cleanup/Cleanup";
import { LensBar, WhatIfBanner, type LensKind } from "../features/map/Lens";
import type { Load } from "../features/map/nodes";
import { MapView } from "../features/map/MapView";
import { ArchitectureView } from "../features/architecture/ArchitectureView";
import { Details } from "../features/details/Details";
import { Sidebar } from "../features/navigation/Sidebar";
import { Trail } from "../features/navigation/Trail";
import { ScanPanel } from "../features/scan/ScanPanel";
import { ExportDialog } from "../features/export/ExportDialog";

const DEMO_SOURCE = "Demo account, us-east-1";

export default function App() {
  const backend = useBackend();
  const [snap, setSnap] = useState<Snapshot>(DEMO);
  const [source, setSource] = useState(DEMO_SOURCE);
  const [fromScan, setFromScan] = useState(false);
  const ix = useMemo(() => buildIndex(snap), [snap]);
  const ins = useMemo(() => insightsOf(snap), [snap]);
  const envs = useMemo(() => envChoices(ix), [ix]);
  const [envPick, setEnv] = useState<string | null>(envs.includes("prod") ? "prod" : null);
  // An environment this snapshot no longer offers (after a refresh or another scan) shows them all.
  const env = envPick && envs.includes(envPick) ? envPick : null;
  const nav = useWhereabouts((w) => resolve(ix, ins.flows, w), backend.can.historyKeys);
  const { place, selected, tab } = nav.here;
  const heading = useRef<HTMLHeadingElement>(null);
  const [blastOn, setBlastOn] = useState(false);
  const [docs, setDocs] = useState<DocFile[] | null>(null);
  const [docMsg, setDocMsg] = useState<string | null>(null);
  const [lens, setLens] = useState<LensKind>("health");
  const [factor, setFactor] = useState(1);
  const [permissions, setPermissions] = useState(false);
  const [exporting, setExporting] = useState(false);

  // Desktop: reopen the last scan instead of the demo.
  useEffect(() => {
    backend
      .lastSnapshot()
      .then((s) => s && load(s, `Last scan, ${new Date(s.taken_at).toLocaleString()}`, true))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The tray: a new snapshot (background refresh, the popover, another scan) replaces the one
  // on screen when that is the stored scan, keeping place and selection. Opening an item from
  // the popover switches to the stored scan first if a file or the demo is open.
  const live = useRef({ fromScan, openTarget: (_t: Target) => {} });
  live.current.fromScan = fromScan;
  useEffect(() => {
    if (!backend.can.tray) return;
    const stops = [
      backend.onSnapshotChanged((s) => {
        if (!live.current.fromScan) return;
        setSnap(s);
        setSource(`Last scan, ${new Date(s.taken_at).toLocaleString()}`);
      }),
      backend.onOpenTarget((t) => live.current.openTarget(t)),
    ];
    return () => stops.forEach((p) => p.then((stop) => stop()));
  }, [backend]);

  // A new snapshot on screen: keep the whereabouts to what it has.
  useEffect(() => {
    const kept = resolve(ix, ins.flows, nav.here);
    if (toHash(kept) !== toHash(nav.here)) nav.replace(kept);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ix]);

  // A resource opened from search, a link or Back can be outside the environment shown.
  useEffect(() => {
    const n = selected ? ix.byId.get(selected) : undefined;
    if (n?.env && env && envName(n.env.value) !== env) setEnv(null);
  }, [selected, env, ix]);

  useEffect(() => setBlastOn(false), [place, selected]);

  /** Open a resource where it lives (its app, or on its own) with its details. */
  const reveal = (id: string) => {
    const w = openingOf(ix, id);
    if (w) nav.go(w);
  };

  // Documentation is generated in Rust (desktop) or precomputed for the demo (web).
  useEffect(() => {
    if (tab !== "docs") return;
    let live = true;
    setDocMsg(null);
    backend
      .renderDocs(snap)
      .then((d) => live && setDocs(d))
      .catch((e) => live && setDocMsg(`Could not generate documentation: ${e}`));
    return () => {
      live = false;
    };
  }, [backend, tab, snap]);

  const fs = useMemo(() => snap.findings ?? [], [snap]);
  const findingsByNode = useMemo(() => {
    const m = new Map<string, number>();
    fs.forEach((f) => m.set(f.node, (m.get(f.node) ?? 0) + 1));
    return m;
  }, [fs]);
  const summaries = useMemo(() => appSummaries(ix, env, fs), [ix, env, fs]);
  const { shared, unassigned } = useMemo(() => orphans(ix, env), [ix, env]);
  const caps = useMemo(() => capacityIndex(ins), [ins]);
  const flows = useMemo(() => ins.flows.filter((f) => env === null || f.env === null || f.env === env), [ins, env]);
  const flow = place.kind === "flow" ? ins.flows.find((f) => f.id === place.id) : undefined;
  const leftover = useMemo(() => ({ count: ins.lifecycle.length, monthly: ins.lifecycle.reduce((s, l) => s + (l.monthly_cost ?? 0), 0) }), [ins]);

  const view = useMemo(() => {
    if (place.kind === "app") return appView(ix, place.name, env, permissions);
    if (place.kind === "resource") return resourceFocus(ix, place.id, permissions);
    if (place.kind === "flow" && flow) return flowView(ix, flow);
    return overview(ix, env, summaries, shared, permissions);
  }, [ix, place, env, summaries, shared, flow, permissions]);

  // The what-if banner names only what is on the map shown, and only maps with load figures
  // offer the capacity lens at all.
  const scope = useMemo(() => mapScope(ix, view, env), [ix, view, env]);
  const capacityHere = useMemo(() => projectable(ins, scope), [ins, scope]);
  const shownLens: LensKind = capacityHere ? lens : "health";

  // Load lens: utilisation at the chosen traffic level. Apps show their busiest resource.
  const loadOf = useMemo(() => {
    if (shownLens !== "load") return undefined;
    const pct = (u: number) => `${Math.round(u * 100)}%`;
    const one = (id: string): Load | undefined => {
      const c = caps.get(id);
      const p = projected(c, factor);
      return c && p ? { ...p, label: `${c.basis}. At ${factor.toFixed(1)}× traffic: ${pct(p.util)} of ${c.limit}.` } : undefined;
    };
    return (id: string): Load | undefined => {
      if (!id.startsWith("app:")) return one(id);
      const app = id.slice(4);
      return ix.snap.nodes
        .filter((n) => n.app?.value === app && (env === null || !n.env || envName(n.env.value) === env))
        .map((n) => one(n.id))
        .filter((l): l is Load => !!l)
        .sort((a, b) => b.util - a.util)[0];
    };
  }, [shownLens, factor, caps, ix, env]);
  const viewKey = `${source}|${JSON.stringify(place)}|${env}`;

  // The architecture draws resources, so the whole account is every resource, not app blocks.
  const arch = useMemo(() => {
    if (tab !== "architecture" || place.kind === "cleanup") return null;
    const resources = place.kind === "overview" ? everythingView(ix, env, permissions) : view;
    return architectureOf(ix, resources, { quietVpcs: place.kind === "overview", env });
  }, [tab, place, ix, env, permissions, view]);
  const nothingHere = env ? "Nothing in this environment yet. Pick another environment or scan more regions." : "Nothing here yet. Scan more regions, or open another snapshot.";

  const blast = useMemo(() => (blastOn && selected ? blastRadius(ix, selected) : null), [blastOn, selected, ix]);

  const go = (p: Place) => nav.go({ place: p, selected: p.kind === "resource" ? p.id : null, tab });
  const select = (id: string | null) => nav.go({ ...nav.here, selected: id });
  const setTab = (t: Tab) => nav.go({ ...nav.here, tab: t });

  const load = (s: Snapshot, src: string, scanned = false) => {
    setSnap(s);
    setSource(src);
    setFromScan(scanned);
    const e = envChoices(buildIndex(s));
    setEnv(e.includes("prod") ? "prod" : null);
    nav.replace(HOME);
  };

  // Clicking an app block opens it; double-clicking a resource opens it where it lives.
  const open = (id: string) => (id.startsWith("app:") ? go({ kind: "app", name: id.slice(4) }) : reveal(id));

  const docPath = place.kind === "app" ? `apps/${place.name}.md` : "README.md";
  const navigateDoc = (path: string) => {
    const m = path.match(/^apps\/(.+)\.md$/);
    go(m ? { kind: "app", name: m[1] } : { kind: "overview" });
  };
  const saveDocs = async () => {
    try {
      const dir = await backend.saveDocs(snap);
      if (dir) setDocMsg(`Saved ${docs?.length ?? "all"} pages to ${dir}`);
    } catch (e) {
      setDocMsg(`Could not save: ${e}`);
    }
  };
  const docActions = backend.can.saveDocsToFolder ? (
    <button className="btn btn--small" onClick={saveDocs}>
      Save all pages to a folder
    </button>
  ) : (
    <button
      className="btn btn--small"
      onClick={() => {
        const d = docs?.find((f) => f.path === docPath);
        if (d) download(d.path.split("/").pop()!, d.content);
      }}
    >
      Download this page
    </button>
  );

  const title =
    place.kind === "overview"
      ? "Whole account"
      : place.kind === "app"
        ? place.name
        : place.kind === "flow"
          ? (flow?.name ?? "Flow")
          : place.kind === "cleanup"
            ? "Probably not needed"
            : ix.byId.get(place.id)
              ? displayName(ix.byId.get(place.id)!)
              : "Resource";
  const hint =
    place.kind === "overview"
      ? "Light blocks are your apps, dark blocks are outside your account. Lines show who calls whom. Open an app to see its resources."
      : place.kind === "flow"
        ? "Numbers follow a request through the flow. Moving lines happen later, through a queue or event."
        : place.kind === "cleanup"
          ? "Resources that look unused, with the evidence and what they cost."
          : "Select a resource to inspect it. Dashed ones belong to other apps; double-click to open them.";
  const exportScope = exportScopeOf(place);
  const showMap = tab === "map" && place.kind !== "cleanup";
  const drawn = tab === "map" || tab === "architecture";
  const panel: "details" | "flow" | "briefing" | null =
    !drawn || place.kind === "cleanup" ? null : selected && ix.byId.has(selected) ? "details" : flow ? "flow" : place.kind === "overview" ? "briefing" : null;
  // An item opened from the popover, waiting for the stored scan to be on screen.
  const [pending, setPending] = useState<Target | null>(null);
  live.current.openTarget = async (t: Target) => {
    if (!live.current.fromScan) {
      const s = await backend.lastSnapshot().catch(() => null);
      if (!s) return;
      load(s, `Last scan, ${new Date(s.taken_at).toLocaleString()}`, true);
    }
    setPending(t);
  };
  useEffect(() => {
    if (!pending || !fromScan) return;
    setPending(null);
    if (pending.type === "flow") nav.go({ place: { kind: "flow", id: pending.id }, selected: null, tab: "map" });
    else if (pending.type === "cleanup") nav.go({ place: { kind: "cleanup" }, selected: null, tab: "map" });
    else reveal(pending.id);
    // Runs once the stored scan (and its index) is on screen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, fromScan, ix]);

  return (
    <div className={`shell${panel ? " shell--details" : ""}`}>
      <Sidebar
        ix={ix}
        onFind={reveal}
        source={source}
        envs={envs}
        env={env}
        onEnv={(e) => {
          setEnv(e);
          const n = selected ? ix.byId.get(selected) : undefined;
          if (n?.env && e && envName(n.env.value) !== e) nav.replace({ ...nav.here, selected: null });
        }}
        apps={summaries}
        shared={shared}
        unassigned={unassigned}
        flows={flows}
        cleanup={leftover}
        place={place}
        onPlace={go}
        footer={<ScanPanel current={snap} fromScan={fromScan} onLoad={load} onDemo={() => load(DEMO, DEMO_SOURCE)} />}
      />

      <main className="stage">
        <header className="stage__head">
          <Trail
            crumbs={crumbs(ix, ins.flows, nav.here)}
            canBack={nav.canBack}
            canForward={nav.canForward}
            onBack={nav.back}
            onForward={nav.forward}
            onGo={(to: Whereabouts) => {
              nav.go(to);
              heading.current?.focus();
            }}
          />
          <div className="stage__title">
            <h1 ref={heading} tabIndex={-1}>
              {title}
            </h1>
            {place.kind !== "cleanup" && (
            <div className="tabs" role="tablist" aria-label="View">
              {(["map", "architecture", "docs"] as const).map((t) => (
                <button key={t} role="tab" aria-selected={tab === t} className={`tabs__opt${tab === t ? " is-on" : ""}`} onClick={() => setTab(t)}>
                  {t === "map" ? "Map" : t === "architecture" ? "Architecture" : "Documentation"}
                </button>
              ))}
            </div>
            )}
            {exportScope && (
              <button className="btn btn--small btn--quiet stage__action" onClick={() => setExporting(true)}>
                Export as code
              </button>
            )}
          </div>
          {tab === "map" && (
            <div className="stage__sub">
              <p className="muted">{hint}</p>
              {showMap && (
                <LensBar
                  lens={shownLens}
                  onLens={setLens}
                  capacity={capacityHere ? "here" : ins.capacity.length ? "elsewhere" : "none"}
                  factor={factor}
                  onFactor={setFactor}
                  permissions={permissions}
                  onPermissions={setPermissions}
                />
              )}
            </div>
          )}
          {tab === "architecture" && place.kind !== "cleanup" && (
            <div className="stage__sub">
              <p className="muted">Where each resource runs: region, VPC, availability zones and subnet tiers, and what runs outside any VPC.</p>
            </div>
          )}
          {tab === "docs" && docMsg && <p className="muted">{docMsg}</p>}
        </header>
        {tab === "docs" ? (
          <DocsView docs={docs} path={docPath} onNavigate={navigateDoc} actions={docActions} />
        ) : place.kind === "cleanup" ? (
          <Cleanup ix={ix} items={ins.lifecycle} onShow={reveal} />
        ) : arch ? (
          arch.regions.length || arch.edge.length || arch.accountWide.length || arch.outside.length ? (
            <ArchitectureView arch={arch} account={snap.nodes.find((n) => n.account)?.account ?? null} env={env} findingsByNode={findingsByNode} selected={selected} onSelect={select} />
          ) : (
            <div className="empty">
              <p>{nothingHere}</p>
            </div>
          )
        ) : view.nodes.length ? (
          <MapView
            view={view}
            viewKey={viewKey}
            findingsByNode={findingsByNode}
            selected={selected}
            blast={blast}
            onSelect={select}
            onOpen={open}
            loadOf={loadOf}
          >
            {shownLens === "load" && <WhatIfBanner ix={ix} ins={ins} factor={factor} scope={scope} />}
          </MapView>
        ) : (
          <div className="empty">
            <p>{nothingHere}</p>
          </div>
        )}
      </main>
      {exporting && exportScope && <ExportDialog scope={exportScope} snapshot={snap} demo={snap === DEMO} onClose={() => setExporting(false)} />}

      {panel === "briefing" && (
        <Briefing
          ix={ix}
          ins={ins}
          changes={snap.changes}
          onNode={reveal}
          onFlow={(id) => go({ kind: "flow", id })}
          onCleanup={() => go({ kind: "cleanup" })}
          onWhatIf={() => {
            // The account-wide answer: in every environment, so the resource it names is on the map.
            setEnv(null);
            setLens("load");
            setFactor(1.5);
          }}
        />
      )}
      {panel === "flow" && flow && <FlowPanel ix={ix} flow={flow} insights={ins.insights} capacity={caps} onNode={select} />}
      {panel === "details" && selected && (
        <Details
          ix={ix}
          id={selected}
          findings={fs}
          blastCount={blast ? blast.size : null}
          onBlast={() => setBlastOn(!blastOn)}
          onSelect={select}
          onClose={() => select(null)}
          capacity={caps.get(selected)}
          lifecycle={ins.lifecycle.find((l) => l.node === selected)}
          flows={ins.flows.filter((f) => f.nodes.includes(selected))}
          onFlow={(id) => go({ kind: "flow", id })}
        />
      )}
    </div>
  );
}
