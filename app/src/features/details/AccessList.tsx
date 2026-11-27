import { useState } from "react";
import { displayName, nameList, type AccessLine } from "../../domain";

const TARGETS = 5;

interface Props {
  lines: AccessLine[];
  onSelect: (id: string) => void;
  /** Lines shown before "Show all". */
  limit?: number;
}

/** What a role may do, one line per service: the access, the resources of this scan it reaches, and the policy behind it. */
export function AccessList({ lines, onSelect, limit = lines.length }: Props) {
  const [all, setAll] = useState(false);
  const shown = all ? lines : lines.slice(0, limit);
  return (
    <>
      <ul className="access">
        {shown.map((l, i) => (
          <li key={i} className={`access__row${l.allow ? "" : " access__row--deny"}`}>
            <p className="access__what">
              <strong>{l.service}</strong>: {l.allow ? l.what : `denies ${l.what}`}
              {l.conditional && <span className="muted">, when its conditions hold</span>}
            </p>
            {(l.targets.length > 0 || l.scope) && <Reach line={l} onSelect={onSelect} />}
            <p className="access__how">
              {l.actions.length > 0 && <code>{l.actions.join(", ")}</code>}
              {l.actions.length > 0 && " · "}
              from {nameList(l.via)}
            </p>
          </li>
        ))}
      </ul>
      {lines.length > limit && (
        <button className="btn--link small" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${lines.length}`}
        </button>
      )}
    </>
  );
}

function Reach({ line, onSelect }: { line: AccessLine; onSelect: (id: string) => void }) {
  const [all, setAll] = useState(false);
  const shown = all ? line.targets : line.targets.slice(0, TARGETS);
  const more = line.targets.length - shown.length;
  return (
    <p className="access__on">
      on{" "}
      {shown.map((t, i) => (
        <span key={t.id}>
          {i > 0 && ", "}
          <button className="btn--link" onClick={() => onSelect(t.id)}>
            {displayName(t)}
          </button>
        </span>
      ))}
      {more > 0 && (
        <>
          {" "}
          <button className="btn--link" onClick={() => setAll(true)}>
            and {more} more
          </button>
        </>
      )}
      {line.scope && `${line.targets.length ? ", and " : ""}${line.scope}`}
    </p>
  );
}
