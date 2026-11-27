import { displayName, kindInfo, type ViewNode, type ZoneNote } from "../../domain";
import { Dot, healthLabel } from "../../ui/Dot";

export interface TileProps {
  item: ViewNode;
  note?: ZoneNote;
  findings: number;
  selected: boolean;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
}

/** One resource where it runs: its mark, name, kind, health from CloudWatch and findings. */
export function Tile({ item, note, findings, selected, onSelect, onHover }: TileProps) {
  const n = item.node!;
  const k = kindInfo(n);
  const owner = item.role === "neighbor" && n.app ? ` in ${n.app.value}` : "";
  // Outside the account (the internet, third parties) reads dark wherever it is drawn.
  const role = k.layer === "external" ? "external" : item.role;
  return (
    <button
      className={`tile tile--${role} tile--${n.health}${selected ? " is-selected" : ""}`}
      data-layer={k.layer}
      data-tile={n.id}
      aria-current={selected || undefined}
      title={`${k.label}: ${displayName(n)}`}
      onClick={() => onSelect(n.id)}
      onMouseEnter={() => onHover(n.id)}
      onMouseLeave={() => onHover(null)}
      onFocus={() => onHover(n.id)}
      onBlur={() => onHover(null)}
    >
      <span className="res__glyph" aria-hidden>
        {k.glyph}
      </span>
      <span className="tile__text">
        <span className="tile__name">{displayName(n)}</span>
        <span className="tile__kind">
          {k.label}
          {owner}
        </span>
      </span>
      {note && <span className="tile__note">{note.label}</span>}
      {n.health !== "unknown" && <Dot tone={n.health} label={healthLabel(n.health)} />}
      {findings > 0 && (
        <span className="res__flag" title={`${findings} to review`}>
          {findings}
          <span className="sr-only"> to review</span>
        </span>
      )}
    </button>
  );
}
