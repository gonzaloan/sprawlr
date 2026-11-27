import { EDGE_VERB, EVIDENCE_LABEL, displayName, strength, type ArchEdge, type Index } from "../../domain";

interface Props {
  title: string;
  edges: ArchEdge[];
  pick: (e: ArchEdge) => string;
  ix: Index;
  onSelect: (id: string) => void;
}

/**
 * What a resource talks to, or what talks to it, with how sprawlr knows. Roles taken on are left
 * to the permission sections, which say what the role allows.
 */
export function Connections({ title, edges, pick, ix, onSelect }: Props) {
  const shown = edges.filter((e) => e.kind !== "network_reachable" && e.kind !== "assumes_role");
  if (!shown.length) return null;
  return (
    <section>
      <h3>{title}</h3>
      <ul className="conns">
        {shown.map((e) => {
          const other = ix.byId.get(pick(e));
          const s = strength(e);
          return (
            <li key={`${e.from}${e.kind}${e.to}`}>
              <button className="conn" onClick={() => other && onSelect(other.id)}>
                <span className="conn__verb">{EDGE_VERB[e.kind]}</span>
                <span className="conn__name">{other ? displayName(other) : pick(e)}</span>
                <span className={`chip chip--${s}`} title={e.evidence.map((v) => v.source).join(", ")}>
                  {EVIDENCE_LABEL[s]}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
