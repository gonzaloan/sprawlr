import { METHOD_LABEL, alarmsOf, allowedIn, changesAbout, displayName, envName, fmtValue, fmtX, kindInfo, logErrorsOf, metricsOf, purposeOf, runsAs, vpcOf, zonesOf, type Index } from "../../domain";
import { Dot } from "../../ui/Dot";
import { Spark } from "../../ui/Spark";
import type { Capacity, Finding, Flow, Lifecycle } from "../../domain";
import { AllowedIn } from "./AllowedIn";
import { Connections } from "./Connections";
import { LogErrors } from "./LogErrors";
import { RoleAccess } from "./RoleAccess";
import { RunsAs } from "./RunsAs";
import { Settings } from "./Settings";
import { SinceLastScan } from "./SinceLastScan";
import { Trust } from "./Trust";

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

/**
 * Everything about one resource, in the order someone asks: what it is, what needs attention,
 * what it is part of and talks to, what it may do, how it behaved, how it is set up.
 */
export function Details({ ix, id, findings, blastCount, onBlast, onSelect, onClose, capacity, lifecycle, flows, onFlow }: Props) {
  const n = ix.byId.get(id);
  if (!n) return null;
  const k = kindInfo(n);
  const vpc = vpcOf(ix, n);
  const isRole = n.kind.type === "iam_role";
  const external = k.layer === "external";
  const roles = runsAs(ix, id);
  const zones = zonesOf(ix, n);
  const mine = findings.filter((f) => f.node === id);
  const metrics = metricsOf(n);
  const logs = logErrorsOf(n);
  const alarms = alarmsOf(n);
  const purpose = purposeOf(n);
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
        {purpose && <p className="details__purpose">{purpose}</p>}
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

      <section>
        {external ? (
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
            {!isRole && (
              <>
                <dt>Runs in</dt>
                <dd>
                  {vpc ? vpc.name : "AWS-managed (no VPC)"}
                  {n.region ? `, ${n.region}` : ""}
                  {zones.length > 0 && <span className="muted">{zones.length > 1 ? `Subnets in ${zones.length} availability zones: ` : "Subnets in one availability zone: "}{zones.join(", ")}</span>}
                </dd>
              </>
            )}
          </dl>
        )}
        <button className="btn btn--ghost" onClick={onBlast}>
          {blastCount === null ? "Show what breaks if this fails" : blastCount === 0 ? "Nothing depends on this" : `Hide the ${blastCount} resources that depend on this`}
        </button>
      </section>

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

      {isRole && <Trust ix={ix} role={n} onSelect={onSelect} />}
      <Connections title="Used by" edges={ix.inc.get(id) ?? []} pick={(e) => e.from} ix={ix} onSelect={onSelect} />
      <Connections title="Depends on" edges={ix.out.get(id) ?? []} pick={(e) => e.to} ix={ix} onSelect={onSelect} />

      {isRole ? (
        <section>
          <h3>What it may do</h3>
          <RoleAccess ix={ix} role={n} onSelect={onSelect} />
        </section>
      ) : (
        roles.length > 0 && <RunsAs ix={ix} roles={roles} onSelect={onSelect} />
      )}
      {!isRole && <AllowedIn allowed={allowedIn(ix, id)} onSelect={onSelect} />}

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

      {(metrics.length > 0 || alarms.length > 0) && (
        <section>
          <h3>Last 24 hours</h3>
          {alarms.map((al) => (
            <div key={al.name} className={`alarm alarm--${al.state.toLowerCase()}`} title={al.reason ?? undefined}>
              <Dot tone={al.state === "ALARM" ? "alarm" : al.state === "OK" ? "ok" : "unknown"} />
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

      {logs && <LogErrors logs={logs} />}

      <Settings node={n} />
      <SinceLastScan rows={changesAbout(ix.snap, id)} />

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
      </section>
    </aside>
  );
}
