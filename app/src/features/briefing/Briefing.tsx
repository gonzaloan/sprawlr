import { displayName, fmtCount, fmtX, monthlyCost, type Index } from "../../domain";
import type { Changes, Insights, Priority } from "../../domain";
import { ChangesCard } from "../changes/ChangesCard";

interface Props {
  ix: Index;
  ins: Insights;
  /** What changed since the previous scan, when known. */
  changes?: Changes | null;
  onNode: (id: string) => void;
  onFlow: (id: string) => void;
  onCleanup: () => void;
  onWhatIf: () => void;
}

const PRIORITY: Record<Priority, string> = { high: "Act now", medium: "Plan for it", low: "Good to know" };

/** What deserves attention in this account, most important first. */
export function Briefing({ ix, ins, changes, onNode, onFlow, onCleanup, onWhatIf }: Props) {
  const name = (id: string) => {
    const n = ix.byId.get(id);
    return n ? displayName(n) : id;
  };
  const first = ins.capacity.find((c) => c.headroom !== null && c.headroom < 10);
  const waste = ins.lifecycle.reduce((s, l) => s + (l.monthly_cost ?? 0), 0);
  const runs = ins.flows.reduce((s, f) => s + (f.runs_24h ?? 0), 0);

  return (
    <aside className="details briefing" aria-label="What needs attention">
      <header>
        <h2>What needs attention</h2>
        <p className="muted">
          {ins.flows.length} flows handle about {fmtCount(runs)} requests and runs a day.
        </p>
      </header>

      {changes && <ChangesCard changes={changes} ix={ix} onNode={onNode} />}

      {first && (
        <button className="brief-card brief-card--capacity" onClick={onWhatIf}>
          <span className="brief-card__eyebrow">Breaks first if traffic grows</span>
          <strong>
            {name(first.node)} at {fmtX(first.headroom!)} today's traffic
          </strong>
          <span className="muted">{first.basis}. Try the traffic slider to see what follows.</span>
        </button>
      )}

      <section>
        {ins.insights.map((x, i) => (
          <button
            key={i}
            className={`insight insight--${x.priority}`}
            onClick={() => (x.flow ? onFlow(x.flow) : x.nodes[0] && onNode(x.nodes[0]))}
          >
            <span className="insight__head">
              <span className={`prio prio--${x.priority}`}>{PRIORITY[x.priority]}</span>
            </span>
            <strong>{x.title}</strong>
            <span className="insight__detail">{x.detail}</span>
            {x.action && <span className="insight__action">{x.action}</span>}
          </button>
        ))}
        {ins.insights.length === 0 && <p className="muted">Nothing stands out. Scan with telemetry on to see load and errors.</p>}
      </section>

      {ins.lifecycle.length > 0 && (
        <button className="brief-card" onClick={onCleanup}>
          <span className="brief-card__eyebrow">Probably not needed</span>
          <strong>
            {ins.lifecycle.length} resources{waste > 0 ? `, about ${monthlyCost(waste)} a month` : ""}
          </strong>
          <span className="muted">Idle, orphaned or left behind. Review the evidence before removing anything.</span>
        </button>
      )}
    </aside>
  );
}
