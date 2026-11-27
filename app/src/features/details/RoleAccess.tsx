import { accessLines, accessState, managedPolicyNote, unlistedManaged, type ArchNode, type Index } from "../../domain";
import { AccessList } from "./AccessList";

interface Props {
  ix: Index;
  role: ArchNode;
  onSelect: (id: string) => void;
  /** Lines shown before "Show all" (the role's own panel shows them all). */
  limit?: number;
}

/** What a role may do: its policies by service, then the AWS managed policies no line counts. */
export function RoleAccess({ ix, role, onSelect, limit }: Props) {
  const state = accessState(role);
  if (state === "scan_again") return <p className="muted small">Scan again to see what it may do: this scan read its policies before sprawlr summed them up.</p>;
  if (state === "not_read") return <p className="muted small">Its policies were not read: it may belong to another account or be managed by AWS.</p>;
  const lines = accessLines(ix, role);
  const managed = unlistedManaged(role);
  return (
    <>
      {lines.length > 0 ? (
        <AccessList lines={lines} onSelect={onSelect} limit={limit} />
      ) : (
        !managed.length && <p className="muted small">No policies attached.</p>
      )}
      {managed.length > 0 && (
        <div className="managed">
          <p className="small">Managed by AWS:</p>
          <ul>
            {managed.map((m) => (
              <li key={m}>
                <code>{m}</code>
                {managedPolicyNote(m) && <span className="muted">: {managedPolicyNote(m)}</span>}
              </li>
            ))}
          </ul>
          <p className="muted small">sprawlr reads the names of AWS managed policies, not their contents.</p>
        </div>
      )}
    </>
  );
}
