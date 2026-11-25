import { useState } from "react";
import { sinceLabel, timeAgo, type Changes, type Index } from "../../domain";

const SHOWN = 5;

/** What changed since the previous scan, most telling first. Rows about a resource open it. */
export function ChangesCard({ changes, ix, onNode }: { changes: Changes; ix: Index; onNode: (id: string) => void }) {
  const [all, setAll] = useState(false);
  const rows = changes.rows ?? [];
  if (!rows.length) {
    return (
      <section className="changes">
        <h3>
          Since {sinceLabel(changes)} <span className="muted small">({timeAgo(changes.since, new Date())})</span>
        </h3>
        <p className="muted small">Nothing changed.</p>
      </section>
    );
  }
  const shown = all ? rows : rows.slice(0, SHOWN);
  return (
    <section className="changes" aria-label="What changed since the previous scan">
      <h3>
        Since {sinceLabel(changes)} <span className="muted small">({timeAgo(changes.since, new Date())})</span>
      </h3>
      <p className="changes__summary">{changes.summary}.</p>
      <ul className="changes__list">
        {shown.map((r, i) => {
          const body = (
            <>
              <span className={`changes__tag changes__tag--${r.tag}`}>{r.label}</span>
              <span className="changes__text">{r.text}</span>
            </>
          );
          return (
            <li key={i}>
              {r.id && ix.byId.has(r.id) ? (
                <button className="changes__row" onClick={() => onNode(r.id!)}>
                  {body}
                </button>
              ) : (
                <div className="changes__row changes__row--static">{body}</div>
              )}
            </li>
          );
        })}
      </ul>
      {rows.length > SHOWN && (
        <button className="btn--link small" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${rows.length}`}
        </button>
      )}
    </section>
  );
}
