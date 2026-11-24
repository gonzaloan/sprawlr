import { displayName, kindInfo, vpcOf, type Index } from "../../domain";
import type { Lifecycle } from "../../domain";

interface Props {
  ix: Index;
  items: Lifecycle[];
  onShow: (id: string) => void;
}

const STATE: Record<Lifecycle["state"], { label: string; hint: string }> = {
  zombie: { label: "Costs money, unused", hint: "Idle or orphaned, and billed every month." },
  idle: { label: "Idle", hint: "Wired up, but no activity in the measured window." },
  orphaned: { label: "Orphaned", hint: "Nothing uses it and it uses nothing." },
};

/** Resources that look unused, with the evidence and what they cost. Nothing is deleted from here. */
export function Cleanup({ ix, items, onShow }: Props) {
  const total = items.reduce((s, l) => s + (l.monthly_cost ?? 0), 0);
  const order: Lifecycle["state"][] = ["zombie", "idle", "orphaned"];

  if (!items.length)
    return (
      <div className="empty">
        <p>Nothing looks unused. Scan with telemetry on so idle resources can be detected.</p>
      </div>
    );

  return (
    <div className="cleanup">
      <p className="cleanup__lead">
        {items.length} resources look unused{total > 0 ? <>, about <strong>${total.toFixed(2)} a month</strong> at list price</> : null}. Each one shows why. sprawlr never deletes anything: check the evidence, then remove them in your
        infrastructure code or console.
      </p>
      {order.map((state) => {
        const rows = items.filter((l) => l.state === state);
        if (!rows.length) return null;
        return (
          <section key={state}>
            <h2>
              {STATE[state].label} <span className="muted">{STATE[state].hint}</span>
            </h2>
            <table className="cleanup__table">
              <colgroup>
                <col style={{ width: "30%" }} />
                <col />
                <col style={{ width: "120px" }} />
                <col style={{ width: "72px" }} />
              </colgroup>
              <thead>
                <tr>
                  <th>Resource</th>
                  <th>Why</th>
                  <th className="num">Per month</th>
                  <th aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {rows.map((l) => {
                  const n = ix.byId.get(l.node);
                  const vpc = n && vpcOf(ix, n);
                  return (
                    <tr key={l.node}>
                      <td>
                        <span className="cleanup__name">{n ? displayName(n) : l.node}</span>
                        <span className="muted small">
                          {n ? kindInfo(n).label : ""}
                          {vpc ? ` in ${vpc.name}` : ""}
                          {n?.region ? `, ${n.region}` : ""}
                        </span>
                      </td>
                      <td>{l.reason}</td>
                      <td className="num">{l.monthly_cost !== null ? `$${l.monthly_cost.toFixed(2)}` : n?.kind.type === "security_group" ? "free" : "usage-based"}</td>
                      <td>
                        <button className="btn btn--link" onClick={() => onShow(l.node)}>
                          Show
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </section>
        );
      })}
    </div>
  );
}
