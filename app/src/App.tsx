import { useEffect, useMemo, useState } from "react";
import demo from "./demo/demo-snapshot.json";
import demoDocs from "./demo/demo-docs.json";
import type { DocFile, Snapshot } from "./types";
import { call, download, inTauri, pickFolder } from "./platform";
import { DocsView } from "./components/DocsView";
import { displayName, envName, appView, apps as appSummaries, blastRadius, buildIndex, envs as envList, orphans, overview, resourceFocus } from "./model";
import { MapView } from "./components/MapView";
import { Details } from "./components/Details";
import { Sidebar, type Place } from "./components/Sidebar";
import { ScanPanel } from "./components/ScanPanel";

const DEMO = demo as unknown as Snapshot;
const DEMO_SOURCE = "Demo account, us-east-1";

export default function App() {
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

  // Desktop: reopen the last scan instead of the demo.
  useEffect(() => {
    if (!inTauri()) return;
    call<Snapshot | null>("last_snapshot")
      .then((s) => s && load(s, `Last scan, ${new Date(s.taken_at).toLocaleString()}`, true))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
    if (inTauri())
      call<DocFile[]>("render_docs", { snapshot: snap })
        .then((d) => live && setDocs(d))
        .catch((e) => live && setDocMsg(`Could not generate documentation: ${e}`));
    else setDocs(source === DEMO_SOURCE ? (demoDocs as DocFile[]) : null);
    return () => {
      live = false;
    };
  }, [tab, snap, source]);

  const fs = useMemo(() => snap.findings ?? [], [snap]);
  const findingsByNode = useMemo(() => {
    const m = new Map<string, number>();
    fs.forEach((f) => m.set(f.node, (m.get(f.node) ?? 0) + 1));
    return m;
  }, [fs]);
  const summaries = useMemo(() => appSummaries(ix, env, fs), [ix, env, fs]);
  const { shared, unassigned } = useMemo(() => orphans(ix, env), [ix, env]);

  const view = useMemo(() => {
    if (place.kind === "app") return appView(ix, place.name, env);
    if (place.kind === "resource") return resourceFocus(ix, place.id);
    return overview(ix, env, summaries, shared);
  }, [ix, place, env, summaries, shared]);
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
      const dir = await pickFolder();
      if (!dir) return;
      const n = await call<number>("save_docs", { snapshot: snap, dir });
      setDocMsg(`Saved ${n} pages to ${dir}`);
    } catch (e) {
      setDocMsg(`Could not save: ${e}`);
    }
  };
  const docActions = inTauri() ? (
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
    place.kind === "overview" ? "Whole account" : place.kind === "app" ? place.name : (ix.byId.get(place.id) ? displayName(ix.byId.get(place.id)!) : "Resource");
  const hint =
    place.kind === "overview"
      ? "Light blocks are your apps, dark blocks are outside your account. Lines show who calls whom. Open an app to see its resources."
      : "Select a resource to inspect it. Dashed ones belong to other apps; double-click to open them.";

  return (
    <div className={`shell${selected && tab === "map" ? " shell--details" : ""}`}>
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
        place={place}
        onPlace={go}
        footer={<ScanPanel current={snap} fromScan={fromScan} onLoad={load} onDemo={() => load(DEMO, DEMO_SOURCE)} />}
      />

      <main className="stage">
        <header className="stage__head">
          <div className="stage__title">
            <h1>{title}</h1>
            <div className="tabs" role="tablist" aria-label="View">
              {(["map", "docs"] as const).map((t) => (
                <button key={t} role="tab" aria-selected={tab === t} className={`tabs__opt${tab === t ? " is-on" : ""}`} onClick={() => setTab(t)}>
                  {t === "map" ? "Map" : "Documentation"}
                </button>
              ))}
            </div>
          </div>
          {tab === "map" && <p className="muted">{hint}</p>}
          {tab === "docs" && docMsg && <p className="muted">{docMsg}</p>}
        </header>
        {tab === "docs" ? (
          <DocsView docs={docs} path={docPath} onNavigate={navigateDoc} actions={docActions} />
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
          />
        ) : (
          <div className="empty">
            <p>Nothing in this environment yet. Pick another environment or scan more regions.</p>
          </div>
        )}
      </main>

      {tab === "map" && selected && ix.byId.has(selected) && (
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
        />
      )}
    </div>
  );
}
