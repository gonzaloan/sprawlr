import { conditionWords, displayName, lastUsedOf, principalLabel, takenOnBy, trustOf, type ArchNode, type Index } from "../../domain";

interface Props {
  ix: Index;
  role: ArchNode;
  onSelect: (id: string) => void;
}

/** Who a role's trust policy lets take it on, and what in this scan does. */
export function Trust({ ix, role, onSelect }: Props) {
  const trust = trustOf(role);
  const takers = takenOnBy(ix, role.id);
  const placeholder = role.attrs?.placeholder === true;
  const lastUsed = lastUsedOf(role);
  return (
    <>
      <section>
        <h3>Who can take it on</h3>
        {placeholder ? (
          <p className="muted small">
            This role was referenced by another resource but is not among the account's roles: it may belong to another account or be managed by AWS. If the scan warned about IAM, check its permissions and scan again.
          </p>
        ) : trust === null ? (
          <p className="muted small">Scan again to read who can take this role on.</p>
        ) : trust.length === 0 ? (
          <p className="muted small">Its trust policy lets no one take it on.</p>
        ) : (
          <ul className="trust">
            {trust.map((p, i) => (
              <li key={i} className={p.kind === "anyone" && !p.conditions.length ? "is-bad" : undefined}>
                {principalLabel(p, role)}
                {p.conditions.length > 0 && (
                  <span className="muted">
                    , only when {conditionWords(p)} {p.conditions.length === 1 ? "matches" : "match"}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>
      <section>
        <h3>Taken on by</h3>
        {takers.length ? (
          <ul className="conns">
            {takers.map((t) => (
              <li key={t.id}>
                <button className="conn conn--flow" onClick={() => onSelect(t.id)}>
                  <span className="conn__name">{displayName(t)}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="muted small">
            Nothing in this scan takes it on.{lastUsed && ` Last used: ${lastUsed.charAt(0).toLowerCase()}${lastUsed.slice(1)}.`}
          </p>
        )}
      </section>
    </>
  );
}
