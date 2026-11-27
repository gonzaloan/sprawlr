import type { ViewNode } from "../../domain";
import { Tile, type TileProps } from "./Tile";

type Shared = Omit<TileProps, "item" | "findings" | "selected">;

interface Props extends Shared {
  label: string;
  items: ViewNode[];
  findingsOf: (id: string) => number;
  selected: string | null;
  className?: string;
}

/** A labelled set of tiles: an edge lane, a group of regional services, account-wide IAM. */
export function TileGroup({ label, items, findingsOf, selected, className, ...rest }: Props) {
  if (!items.length) return null;
  return (
    <section className={`arch-group${className ? ` ${className}` : ""}`} aria-label={label}>
      <h4 className="arch-group__label">{label}</h4>
      <div className="arch-group__tiles">
        {items.map((v) => (
          <Tile key={v.id} item={v} findings={findingsOf(v.id)} selected={selected === v.id} {...rest} />
        ))}
      </div>
    </section>
  );
}
