import { displayName, type ArchNode, type Index } from "../../domain";
import { RoleAccess } from "./RoleAccess";

interface Props {
  ix: Index;
  roles: ArchNode[];
  onSelect: (id: string) => void;
}

/** A resource's permissions are its role's: name the role and what it allows. */
export function RunsAs({ ix, roles, onSelect }: Props) {
  return (
    <section>
      <h3>Permissions</h3>
      {roles.map((r) => (
        <div key={r.id} className="runs-as">
          <p className="small">
            Runs as{" "}
            <button className="btn--link" onClick={() => onSelect(r.id)}>
              {displayName(r)}
            </button>
            , so it may do what that role allows.
          </p>
          <RoleAccess ix={ix} role={r} onSelect={onSelect} limit={4} />
        </div>
      ))}
    </section>
  );
}
