import type { ChangeRow } from "../../domain";

/** The changes of the last comparison that are about this resource. */
export function SinceLastScan({ rows }: { rows: ChangeRow[] }) {
  if (!rows.length) return null;
  return (
    <section>
      <h3>Since the last scan</h3>
      <ul className="changes__list">
        {rows.map((r, i) => (
          <li key={i} className="changes__row changes__row--static">
            <span className={`changes__tag changes__tag--${r.tag}`}>{r.label}</span>
            <span className="changes__text">{r.text}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
