import { useState } from "react";
import { EDGE_VERB, EVIDENCE_LABEL, METHOD_LABEL, alarmsOf, displayName, envName, fmtCount, fmtValue, fmtX, kindInfo, logErrorsOf, metricsOf, strength, vpcOf, type Index } from "../../domain";
import { Spark } from "../../ui/Spark";
import type { ArchEdge, Capacity, Finding, Flow, Lifecycle, LogErrors } from "../../domain";

interface Props {
  ix: Index;
  id: string;
  findings: Finding[];
  blastCount: number | null;
  onBlast: () => void;
  onSelect: (id: string) => void;
  onClose: () => void;
  capacity?: Capacity;
  lifecycle?: Lifecycle;
  flows: Flow[];
  onFlow: (id: string) => void;
}

const LIFE: Record<Lifecycle["state"], string> = { zombie: "Costs money, looks unused", idle: "Idle", orphaned: "Orphaned" };

export function Details({ ix, id, findings, blastCount, onBlast, onSelect, onClose, capacity, lifecycle, flows, onFlow }: Props) {
  const [raw, setRaw] = useState(false);
  const n = ix.byId.get(id);
  if (!n) return null;
  const k = kindInfo(n);
  const layerOfNode = k.layer;
  const vpc = vpcOf(ix, n);
  const out = ix.out.get(id) ?? [];
  const inc = ix.inc.get(id) ?? [];
  const mine = findings.filter((f) => f.node === id);
  const attrs = Object.entries(n.attrs ?? {}).filter(([key]) => !["placeholder", "metrics", "alarms", "policies", "broad_permissions", "activity"].includes(key));
  const metrics = metricsOf(n);
  const logs = logErrorsOf(n);
  const alarms = alarmsOf(n);
  const a = (n.attrs ?? {}) as Record<string, unknown>;
  const runs = Array.isArray(a.images) && a.images.length
    ? (a.images as string[]).map((i) => i.split("/").pop()).join(", ")
    : typeof a.runtime === "string" ? a.runtime : null;

  return (
    <aside className="details" aria-label="Resource details">
      <header className="details__head">
        <span className="res__glyph" data-layer={k.layer} aria-hidden>
          {k.glyph}
        </span>
        <div>
          <h2>{displayName(n)}</h2>
          <p className="muted">{displayName(n) === n.name ? k.label : `${k.label}, deployed as ${n.name}`}</p>
        </div>
        <button className="icon-btn" onClick={onClose} aria-label="Close details">
          ×
        </button>
      </header>

      {mine.length > 0 && (
        <section>
          {mine.map((f, i) => (
            <div key={i} className={`finding finding--${f.severity}`}>
              <strong>{f.title}</strong>
              <p>{f.detail}</p>
            </div>
          ))}
        </section>
      )}

      {lifecycle && (
        <div className="finding finding--medium">
          <strong>{LIFE[lifecycle.state]}{lifecycle.monthly_cost !== null ? `, about $${lifecycle.monthly_cost.toFixed(2)} a month` : ""}</strong>
          <p>{lifecycle.reason}</p>
        </div>
      )}

      {ix.iamMapped.has(id) && (
        <p className="note">
          Mapped from IAM. sprawlr can't see what this {/^[A-Z]{2}/.test(k.label) ? k.label : k.label.toLowerCase()} runs, so its connections come from what its role allows. Dotted lines are allowed, not seen in use.
        </p>
      )}

      {n.kind.type === "iam_role" && <Policies attrs={a} />}

      {capacity && (
        <section>
          <h3>Headroom</h3>
          <div className={`headroom${capacity.headroom !== null && capacity.headroom < 1.5 ? " is-bad" : ""}`}>
            <span className="load load--static">
              <span className="load__fill" style={{ width: `${Math.min(capacity.utilization, 1) * 100}%` }} />
            </span>
            <p>
              {capacity.basis}. {capacity.headroom !== null ? <>It reaches its limit at <strong>{fmtX(capacity.headroom)}</strong> today's traffic.</> : "No limit in sight."}
            </p>
            <p className="muted small">Limit: {capacity.limit}.</p>
          </div>
        </section>
      )}

      {flows.length > 0 && (
        <section>
          <h3>Part of</h3>
          <ul className="conns">
            {flows.map((f) => (
              <li key={f.id}>
                <button className="conn conn--flow" onClick={() => onFlow(f.id)}>
                  <span className="conn__name">{f.name}</span>
                  <span className="muted">step {f.nodes.indexOf(id) + 1}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {(metrics.length > 0 || alarms.length > 0) && (
        <section>
          <h3>Last 24 hours</h3>
          {alarms.map((al) => (
            <div key={al.name} className={`alarm alarm--${al.state.toLowerCase()}`} title={al.reason ?? undefined}>
              <span className={`dot dot--${al.state === "ALARM" ? "alarm" : al.state === "OK" ? "ok" : "unknown"}`} />
              <span className="alarm__name">{al.name}</span>
              <span className="muted">{al.state === "ALARM" ? "firing" : al.state === "OK" ? "OK" : "no data"}</span>
            </div>
          ))}
          <ul className="metrics">
            {metrics.map((m) => (
              <li key={m.key} className={m.alert ? "is-bad" : undefined}>
                <span className="metrics__label">{m.label}</span>
                <Spark values={m.values} width={96} height={22} tone={m.alert ? "alarm" : "ink"} label={`${m.label}, last 24 hours`} />
                <span className="metrics__value">
                  {fmtValue(m.summary, m.unit)}
                  <small>{m.summary_label}</small>
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {logs && <LogErrorsSection logs={logs} />}

      <section>
        {layerOfNode === "external" ? (
          <p className="muted">{n.kind.type === "internet" ? "Anyone on the internet." : "A service outside your AWS account, seen in traces or configuration."}</p>
        ) : (
        <dl className="facts">
          <dt>App</dt>
          <dd>{n.app ? <>{n.app.value} <span className="muted">{METHOD_LABEL[n.app.method]}</span></> : <span className="muted">Not assigned</span>}</dd>
          <dt>Environment</dt>
          <dd>{n.env ? <>{envName(n.env.value)} <span className="muted">{METHOD_LABEL[n.env.method]}</span></> : <span className="muted">Unknown</span>}</dd>
          {runs && (
            <>
              <dt>Runs</dt>
              <dd>{runs}</dd>
            </>
          )}
          <dt>Runs in</dt>
          <dd>{vpc ? vpc.name : "AWS-managed (no VPC)"}{n.region ? `, ${n.region}` : ""}</dd>
        </dl>
        )}
        <button className="btn btn--ghost" onClick={onBlast}>
          {blastCount === null ? "Show what breaks if this fails" : blastCount === 0 ? "Nothing depends on this" : `Hide the ${blastCount} resources that depend on this`}
        </button>
      </section>

      <Connections title="Depends on" edges={out} pick={(e) => e.to} ix={ix} onSelect={onSelect} />
      <Connections title="Used by" edges={inc} pick={(e) => e.from} ix={ix} onSelect={onSelect} />

      {Object.keys(n.tags).length > 0 && (
        <section>
          <h3>Tags</h3>
          <dl className="facts facts--tags">
            {Object.entries(n.tags).map(([key, v]) => (
              <div key={key} className="facts__pair">
                <dt>{key}</dt>
                <dd>{v}</dd>
              </div>
            ))}
          </dl>
        </section>
      )}

      <section>
        <h3>Identifier</h3>
        <p className="arn">{n.id}</p>
        {attrs.length > 0 && (
          <>
            <button className="btn btn--link" onClick={() => setRaw(!raw)} aria-expanded={raw}>
              {raw ? "Hide configuration" : "Show configuration"}
            </button>
            {raw && <pre className="raw">{JSON.stringify(Object.fromEntries(attrs), null, 2)}</pre>}
          </>
        )}
      </section>
    </aside>
  );
}

/** "2026-11-24 11:52:03.120" → "11:52" today, "Nov 23, 11:52" before. Times are UTC. */
function when(t?: string | null): string {
  if (!t) return "";
  const d = new Date(t.replace(" ", "T") + "Z");
  if (Number.isNaN(d.getTime())) return "";
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return d.toDateString() === new Date().toDateString() ? time : `${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}, ${time}`;
}

function LogErrorsSection({ logs }: { logs: LogErrors }) {
  return (
    <section>
      <h3>Errors in logs</h3>
      {logs.total === 0 ? (
        <p className="muted small">No error lines in the last {logs.hours} hours.</p>
      ) : (
        <ul className="errs">
          {logs.patterns.map((p) => (
            <li key={p.message}>
              <code className="errs__msg">{p.message}</code>
              <span className="errs__meta">
                <strong>{fmtCount(p.count)}</strong> {p.count === 1 ? "time" : "times"}
                {p.last_seen ? <>, last at {when(p.last_seen)}</> : null}
              </span>
            </li>
          ))}
        </ul>
      )}
      <p className="muted small">
        From {logs.log_groups.join(", ")}, last {logs.hours} hours. Numbers, ids and values are masked; raw lines are never stored.
      </p>
    </section>
  );
}

interface Stmt {
  allow: boolean;
  actions: string[];
  not_actions?: string[];
  resources: string[];
  not_resources?: string[];
  conditional?: boolean;
}

const short = (r: string) => (r === "*" ? "everything" : r.replace(/^arn:aws:[a-z0-9-]+:[a-z0-9-]*:\d*:/, "").replace(/^arn:aws:s3:::/, ""));

/** A role's policies, statement by statement, in words a reader can check against the map. */
function Policies({ attrs }: { attrs: Record<string, unknown> }) {
  const policies = (Array.isArray(attrs.policies) ? attrs.policies : []) as { name: string; statements: Stmt[] }[];
  const managed = (Array.isArray(attrs.aws_managed_policies) ? attrs.aws_managed_policies : []) as string[];
  const used = typeof attrs.last_used_days === "number" ? (attrs.last_used_days === 0 ? "Used today" : `Last used ${attrs.last_used_days} days ago`) : "Never used";
  if (!policies.length && !managed.length) return null;
  return (
    <section>
      <h3>Permissions</h3>
      <p className="muted small">{used}.</p>
      {policies.map((p) => (
        <div key={p.name} className="policy">
          <div className="policy__name">{p.name}</div>
          {p.statements.map((st, i) => {
            const acts = st.not_actions?.length ? `everything except ${st.not_actions.join(", ")}` : st.actions.join(", ");
            const res = st.not_resources?.length ? `everything except ${st.not_resources.map(short).join(", ")}` : st.resources.map(short).join(", ");
            return (
              <div key={i} className={`policy__stmt${st.allow ? "" : " policy__stmt--deny"}`}>
                {st.allow ? "Allows" : "Denies"} <code>{acts}</code> on <code>{res}</code>
                {st.conditional ? <span className="muted"> when its conditions hold</span> : null}
              </div>
            );
          })}
        </div>
      ))}
      {managed.length > 0 && (
        <p className="muted small">
          Also attached, managed by AWS: {managed.join(", ")}. Their contents are not analysed.
        </p>
      )}
    </section>
  );
}

function Connections({ title, edges, pick, ix, onSelect }: { title: string; edges: ArchEdge[]; pick: (e: ArchEdge) => string; ix: Index; onSelect: (id: string) => void }) {
  const shown = edges.filter((e) => e.kind !== "network_reachable");
  if (!shown.length) return null;
  return (
    <section>
      <h3>{title}</h3>
      <ul className="conns">
        {shown.map((e) => {
          const other = ix.byId.get(pick(e));
          const s = strength(e);
          return (
            <li key={`${e.from}${e.kind}${e.to}`}>
              <button className="conn" onClick={() => other && onSelect(other.id)}>
                <span className="conn__verb">{EDGE_VERB[e.kind]}</span>
                <span className="conn__name">{other ? displayName(other) : pick(e)}</span>
                <span className={`chip chip--${s}`} title={e.evidence.map((v) => v.source).join(", ")}>
                  {EVIDENCE_LABEL[s]}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
