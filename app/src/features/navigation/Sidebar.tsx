import type { ArchNode, Flow } from "../../domain";
import { displayName, fmtCount, type AppSummary, type Index } from "../../domain";
import { Find } from "./Find";
import type { ReactNode } from "react";

export type Place =
  | { kind: "overview" }
  | { kind: "app"; name: string }
  | { kind: "resource"; id: string }
  | { kind: "flow"; id: string }
  | { kind: "cleanup" };

interface Props {
  ix: Index;
  onFind: (id: string) => void;
  source: string;
  envs: string[];
  env: string | null;
  onEnv: (e: string | null) => void;
  apps: AppSummary[];
  shared: ArchNode[];
  unassigned: ArchNode[];
  flows: Flow[];
  cleanup: { count: number; monthly: number };
  place: Place;
  onPlace: (p: Place) => void;
  footer: ReactNode;
}

export function Sidebar({ ix, onFind, source, envs, env, onEnv, apps, shared, unassigned, flows, cleanup, place, onPlace, footer }: Props) {
  const is = (p: Place) => JSON.stringify(p) === JSON.stringify(place);
  return (
    <nav className="side" aria-label="Apps and resources">
      <div className="side__brand">
        <span className="wordmark">sprawlr</span>
        <span className="muted">{source}</span>
      </div>

      <Find ix={ix} onPick={onFind} />

      <div className="envs" role="radiogroup" aria-label="Environment">
        {[null, ...envs].map((e) => (
          <button key={e ?? "all"} role="radio" aria-checked={env === e} className={`envs__opt${env === e ? " is-on" : ""}`} onClick={() => onEnv(e)}>
            {e ?? "All"}
          </button>
        ))}
      </div>

      <div className="side__scroll">
        <button className={`side__item side__item--map${is({ kind: "overview" }) ? " is-on" : ""}`} onClick={() => onPlace({ kind: "overview" })}>
          <span>Whole account</span>
          <span className="muted">{apps.length} apps</span>
        </button>

        {flows.length > 0 && <h3 className="side__h">Flows</h3>}
        {flows.map((f) => (
          <button key={f.id} className={`side__item${is({ kind: "flow", id: f.id }) ? " is-on" : ""}`} onClick={() => onPlace({ kind: "flow", id: f.id })}>
            <span className="side__name">
              {f.errors_24h > 0 && <span className="dot dot--alarm" aria-label="Errors in the last 24 hours" />}
              {f.name}
            </span>
            <span className="muted">{f.runs_24h ? `${fmtCount(f.runs_24h)}/day` : ""}</span>
          </button>
        ))}

        <h3 className="side__h">Apps</h3>
        {apps.map((a) => (
          <button key={a.name} className={`side__item${is({ kind: "app", name: a.name }) ? " is-on" : ""}`} onClick={() => onPlace({ kind: "app", name: a.name })}>
            <span className="side__name">
              {a.health !== "unknown" && <span className={`dot dot--${a.health}`} />}
              {a.name}
            </span>
            <span className="muted">{a.findings > 0 ? `${a.findings} to review` : a.count}</span>
          </button>
        ))}

        {shared.length > 0 && <h3 className="side__h">Shared by several apps</h3>}
        {shared.map((n) => (
          <button key={n.id} className={`side__item${is({ kind: "resource", id: n.id }) ? " is-on" : ""}`} onClick={() => onPlace({ kind: "resource", id: n.id })}>
            <span className="side__name">{displayName(n)}</span>
          </button>
        ))}

        {unassigned.length > 0 && <h3 className="side__h">Not linked to an app</h3>}
        {unassigned.map((n) => (
          <button key={n.id} className={`side__item${is({ kind: "resource", id: n.id }) ? " is-on" : ""}`} onClick={() => onPlace({ kind: "resource", id: n.id })}>
            <span className="side__name">{displayName(n)}</span>
          </button>
        ))}

        {cleanup.count > 0 && (
          <button className={`side__item side__item--cleanup${is({ kind: "cleanup" }) ? " is-on" : ""}`} onClick={() => onPlace({ kind: "cleanup" })}>
            <span>Probably not needed</span>
            <span className="muted">{cleanup.monthly > 0 ? `$${cleanup.monthly.toFixed(0)}/mo` : cleanup.count}</span>
          </button>
        )}
      </div>

      <div className="side__foot">{footer}</div>
    </nav>
  );
}
