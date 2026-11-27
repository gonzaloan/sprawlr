import { useState } from "react";
import { displayName, type Allowed } from "../../domain";

const SHOWN = 6;

/** The roles whose policies reach a resource: named ones first, then those that reach every one like it. */
export function AllowedIn({ allowed, onSelect }: { allowed: Allowed[]; onSelect: (id: string) => void }) {
  const [all, setAll] = useState(false);
  if (!allowed.length) return null;
  const shown = all ? allowed : allowed.slice(0, SHOWN);
  return (
    <section>
      <h3>Allowed in by IAM</h3>
      <ul className="conns">
        {shown.map((a) => (
          <li key={a.role.id}>
            <button className="conn conn--flow" onClick={() => onSelect(a.role.id)}>
              <span className="conn__name">{displayName(a.role)}</span>
              <span className="muted">
                {a.what}
                {a.scope ? `, ${a.scope}` : ""}
                {a.conditional ? ", with conditions" : ""}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {allowed.length > SHOWN && (
        <button className="btn--link small" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${allowed.length}`}
        </button>
      )}
      <p className="muted small">Roles whose policies reach it, after denies of every action. Conditions, permissions boundaries and service control policies are not evaluated.</p>
    </section>
  );
}
