import { useEffect, useMemo, useRef, useState } from "react";
import type { DocFile, Snapshot, Target } from "../domain";
import { DEMO, download, useBackend } from "../platform";
import { DocsView } from "../features/docs/DocsView";
import {
  appView,
  apps as appSummaries,
  blastRadius,
  buildIndex,
  capacityIndex,
  displayName,
  envName,
  envs as envList,
  flowView,
  insightsOf,
  orphans,
  overview,
  projected,
  resourceFocus,
} from "../domain";
import { Briefing } from "../features/briefing/Briefing";
import { FlowPanel } from "../features/flows/FlowPanel";
import { Cleanup } from "../features/cleanup/Cleanup";
import { LensBar, WhatIfBanner, type LensKind } from "../features/map/Lens";
import type { Load } from "../features/map/nodes";
import { MapView } from "../features/map/MapView";
import { Details } from "../features/details/Details";
import { Sidebar, type Place } from "../features/navigation/Sidebar";
import { ScanPanel } from "../features/scan/ScanPanel";

const DEMO_SOURCE = "Demo account, us-east-1";

export default function App() {
  const backend = useBackend();
  const [snap, setSnap] = useState<Snapshot>(DEMO);
  const [source, setSource] = useState(DEMO_SOURCE);
  const [fromScan, setFromScan] = useState(false);
  const ix = useMemo(() => buildIndex(snap), [snap]);
  const envs = useMemo(() => envList(ix), [ix]);
  const [env, setEnv] = useState<string | null>(envs.includes("prod") ? "prod" : null);
  const [place, setPlace] = useState<Place>({ kind: "overview" });
  const [selected, setSelected] = useState<string | null>(null);
  const [blastOn, setBlastOn] = useState(false);
  const [tab, setTab] = useState<"map" | "docs">("map");
  const [docs, setDocs] = useState<DocFile[] | null>(null);
  const [docMsg, setDocMsg] = useState<string | null>(null);
  const [lens, setLens] = useState<LensKind>("health");
  const [factor, setFactor] = useState(1);
  const [permissions, setPermissions] = useState(false);

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

  // Jump to a resource found with search: open its app (or focus it) and select it.
  const find = (id: string) => {
    const n = ix.byId.get(id);
    if (!n) return;
    if (n.env && env && envName(n.env.value) !== env) setEnv(null);
    setTab("map");
    if (n.app) {
      setPlace({ kind: "app", name: n.app.value });
      setSelected(id);
      setBlastOn(false);
    } else go({ kind: "resource", id });
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
  const ins = useMemo(() => insightsOf(snap), [snap]);
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

  // Load lens: utilisation at the chosen traffic level. Apps show their busiest resource.
  const loadOf = useMemo(() => {
    if (lens !== "load") return undefined;
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
  }, [lens, factor, caps, ix, env]);
  const viewKey = `${source}|${JSON.stringify(place)}|${env}`;

  const blast = useMemo(() => (blastOn && selected ? blastRadius(ix, selected) : null), [blastOn, selected, ix]);

  const go = (p: Place) => {
    setPlace(p);
    setSelected(p.kind === "resource" ? p.id : null);
    setBlastOn(false);
  };

  const load = (s: Snapshot, src: string, scanned = false) => {
    setSnap(s);
    setSource(src);
    setFromScan(scanned);
    const e = envList(buildIndex(s));
    setEnv(e.includes("prod") ? "prod" : null);
    go({ kind: "overview" });
  };

  // Clicking an app block opens it; double-clicking a resource opens its app (or focuses it).
  const open = (id: string) => {
    if (id.startsWith("app:")) return go({ kind: "app", name: id.slice(4) });
    const n = ix.byId.get(id);
    if (n?.app) go({ kind: "app", name: n.app.value });
    else if (n) go({ kind: "resource", id });
  };

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
  const showMap = tab === "map" && place.kind !== "cleanup";
  const panel: "details" | "flow" | "briefing" | null =
    tab !== "map" || place.kind === "cleanup" ? null : selected && ix.byId.has(selected) ? "details" : flow ? "flow" : place.kind === "overview" ? "briefing" : null;
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
    setTab("map");
    if (pending.type === "flow") go({ kind: "flow", id: pending.id });
    else if (pending.type === "cleanup") go({ kind: "cleanup" });
    else showNode(pending.id);
    // Runs once the stored scan (and its index) is on screen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, fromScan, ix]);

  const showNode = (id: string) => {
    setTab("map");
    const n = ix.byId.get(id);
    if (n?.app) {
      if (n.env && env && envName(n.env.value) !== env) setEnv(null);
      setPlace({ kind: "app", name: n.app.value });
      setSelected(id);
      setBlastOn(false);
    } else go({ kind: "resource", id });
  };

  return (
    <div className={`shell${panel ? " shell--details" : ""}`}>
      <Sidebar
        ix={ix}
        onFind={find}
        source={source}
        envs={envs}
        env={env}
        onEnv={(e) => {
          setEnv(e);
          setSelected(null);
          setBlastOn(false);
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
          <div className="stage__title">
            <h1>{title}</h1>
            {place.kind !== "cleanup" && (
            <div className="tabs" role="tablist" aria-label="View">
              {(["map", "docs"] as const).map((t) => (
                <button key={t} role="tab" aria-selected={tab === t} className={`tabs__opt${tab === t ? " is-on" : ""}`} onClick={() => setTab(t)}>
                  {t === "map" ? "Map" : "Documentation"}
                </button>
              ))}
            </div>
            )}
          </div>
          {tab === "map" && (
            <div className="stage__sub">
              <p className="muted">{hint}</p>
              {showMap && <LensBar lens={lens} onLens={setLens} factor={factor} onFactor={setFactor} permissions={permissions} onPermissions={setPermissions} />}
            </div>
          )}
          {tab === "docs" && docMsg && <p className="muted">{docMsg}</p>}
        </header>
        {tab === "docs" ? (
          <DocsView docs={docs} path={docPath} onNavigate={navigateDoc} actions={docActions} />
        ) : place.kind === "cleanup" ? (
          <Cleanup ix={ix} items={ins.lifecycle} onShow={showNode} />
        ) : view.nodes.length ? (
          <MapView
            view={view}
            viewKey={viewKey}
            findingsByNode={findingsByNode}
            selected={selected}
            blast={blast}
            onSelect={(id) => {
              setSelected(id);
              setBlastOn(false);
            }}
            onOpen={open}
            loadOf={loadOf}
          >
            {lens === "load" && <WhatIfBanner ix={ix} ins={ins} factor={factor} />}
          </MapView>
        ) : (
          <div className="empty">
            <p>Nothing in this environment yet. Pick another environment or scan more regions.</p>
          </div>
        )}
      </main>

      {panel === "briefing" && (
        <Briefing
          ix={ix}
          ins={ins}
          changes={snap.changes}
          onNode={showNode}
          onFlow={(id) => go({ kind: "flow", id })}
          onCleanup={() => go({ kind: "cleanup" })}
          onWhatIf={() => {
            setLens("load");
            setFactor(1.5);
          }}
        />
      )}
      {panel === "flow" && flow && <FlowPanel ix={ix} flow={flow} insights={ins.insights} capacity={caps} onNode={(id) => setSelected(id)} />}
      {panel === "details" && selected && (
        <Details
          ix={ix}
          id={selected}
          findings={fs}
          blastCount={blast ? blast.size : null}
          onBlast={() => setBlastOn(!blastOn)}
          onSelect={(id) => {
            setSelected(id);
            setBlastOn(false);
          }}
          onClose={() => {
            setSelected(null);
            setBlastOn(false);
          }}
          capacity={caps.get(selected)}
          lifecycle={ins.lifecycle.find((l) => l.node === selected)}
          flows={ins.flows.filter((f) => f.nodes.includes(selected))}
          onFlow={(id) => go({ kind: "flow", id })}
        />
      )}
    </div>
  );
}
