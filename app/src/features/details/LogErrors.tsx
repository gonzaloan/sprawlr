import { fmtCount, type LogErrors as Errors } from "../../domain";

/** "2026-11-24 11:52:03.120" → "11:52" today, "Nov 23, 11:52" before. Times are UTC. */
function when(t?: string | null): string {
  if (!t) return "";
  const d = new Date(t.replace(" ", "T") + "Z");
  if (Number.isNaN(d.getTime())) return "";
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return d.toDateString() === new Date().toDateString() ? time : `${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}, ${time}`;
}

/** Error patterns read from a resource's logs, masked. */
export function LogErrors({ logs }: { logs: Errors }) {
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
