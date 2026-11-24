import { displayName, hopVerb, fmtCount, fmtValue, fmtX, kindInfo, metricsOf, primaryMetric, type Index } from "../model";
import type { Capacity, Flow, Insight } from "../types";
import { Spark } from "./Spark";

interface Props {
  ix: Index;
  flow: Flow;
  insights: Insight[];
  capacity: Map<string, Capacity>;
  onNode: (id: string) => void;
}

/** A flow told as steps: what answers the caller, then what happens in the background. */
export function FlowPanel({ ix, flow, insights, capacity, onNode }: Props) {
  const name = (id: string) => {
    const n = ix.byId.get(id);
    return n ? displayName(n) : id;
  };
  const stages = Array.from({ length: flow.stages }, (_, s) => flow.hops.filter((h) => h.stage === s));
  const related = insights.filter((x) => x.flow === flow.id || x.nodes.some((n) => flow.nodes.includes(n)));
  const weakest = flow.nodes
    .map((id) => capacity.get(id))
    .filter((c): c is Capacity => !!c && c.headroom !== null)
    .sort((a, b) => a.headroom! - b.headroom!)[0];

  const step = (id: string) => flow.nodes.indexOf(id) + 1;

  return (
    <aside className="details flowpanel" aria-label="Flow">
      <header>
        <h2>{flow.name}</h2>
        <p className="muted">
          {flow.trigger === "internet"
            ? "Starts with a request from the internet"
            : flow.trigger === "event"
              ? "Starts when data arrives: an upload or a change notifies the next step"
              : "Starts on its own: a schedule, a manual run or a caller outside the scan"}
          {flow.env ? `, in ${flow.env}` : ""}.
        </p>
      </header>

      <dl className="stats">
        <div>
          <dt>Runs a day</dt>
          <dd>{fmtCount(flow.runs_24h)}</dd>
        </div>
        <div>
          <dt>Errors a day</dt>
          <dd className={flow.errors_24h > 0 ? "is-bad" : undefined}>{fmtCount(flow.errors_24h)}</dd>
        </div>
        <div>
          <dt>Caller waits</dt>
          <dd>{flow.sync_latency_ms !== null ? fmtValue(flow.sync_latency_ms, "ms") : flow.trigger === "internet" ? "no data" : "nobody"}</dd>
        </div>
        <div>
          <dt>Breaks at</dt>
          <dd className={weakest && weakest.headroom! < 1.5 ? "is-bad" : undefined}>{weakest ? fmtX(weakest.headroom!) : "no limit seen"}</dd>
        </div>
      </dl>
      {weakest && (
        <p className="muted small">
          First limit: {name(weakest.node)} ({weakest.basis.toLowerCase()}).
        </p>
      )}

      {stages.map((hops, s) =>
        hops.length === 0 ? null : (
          <section key={s}>
            <h3>
              {flow.trigger === "event"
                ? s <= 1 ? "When data arrives" : s === 2 ? "Then" : "Later still"
                : s === 0 ? "Answers the caller" : s === 1 ? "Then, in the background" : "Later still"}
            </h3>
            <ol className="steps">
              {hops.map((h) => {
                const to = ix.byId.get(h.to);
                const m = to && primaryMetric(to);
                const bad = to && metricsOf(to).some((x) => x.alert);
                return (
                  // A hop back to an earlier step (the worker reading the upload) keeps the worker's number.
                  <li key={`${h.from}${h.to}${h.kind}`} value={step(h.to) < step(h.from) ? step(h.from) : step(h.to)}>
                    <button className="step" onClick={() => onNode(h.to)}>
                      <span className="step__text">
                        <span className="muted">
                          {name(h.from)} {hopVerb(h)}
                          {h.inferred ? " (IAM allows)" : ""}
                        </span>
                        <span className="step__name">{name(h.to)}</span>
                        <span className="muted small">{to ? kindInfo(to).label : ""}</span>
                      </span>
                      {m && <Spark values={m.values} width={56} height={20} tone={bad ? "alarm" : "ink"} label={`${m.label}, last 24 hours`} />}
                    </button>
                  </li>
                );
              })}
            </ol>
          </section>
        ),
      )}

      {related.length > 0 && (
        <section>
          <h3>In this flow</h3>
          {related.map((x, i) => (
            <div key={i} className={`insight insight--${x.priority} insight--static`}>
              <strong>{x.title}</strong>
              <span className="insight__detail">{x.detail}</span>
              {x.action && <span className="insight__action">{x.action}</span>}
            </div>
          ))}
        </section>
      )}
    </aside>
  );
}
