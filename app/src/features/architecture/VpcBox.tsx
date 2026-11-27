import { Fragment, type CSSProperties } from "react";
import { displayName, tierReading, type VpcPlan } from "../../domain";
import { Tile } from "./Tile";

interface Props {
  plan: VpcPlan;
  findingsOf: (id: string) => number;
  selected: string | null;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
}

/** A VPC as an architecture diagram draws it: a column per availability zone, a row per subnet tier. */
export function VpcBox({ plan, findingsOf, selected, onSelect, onHover }: Props) {
  const { vpc, zones, tiers, loose } = plan;
  const cidr = typeof vpc.attrs?.cidr === "string" ? vpc.attrs.cidr : null;
  return (
    <section className="vpc" aria-label={`VPC ${vpc.name}`}>
      <header className="vpc__head">
        <strong>{vpc.name}</strong>
        <span className="muted small">
          VPC{cidr ? ` ${cidr}` : ""}
          {vpc.attrs?.is_default === true ? ", default" : ""}
        </span>
      </header>
      {zones.length > 0 && (
        <div className="vpc__grid" style={{ "--zones": zones.length } as CSSProperties}>
          <span />
          {zones.map((z) => (
            <span key={z} className="vpc__zone">
              {z}
            </span>
          ))}
          {tiers.map((t) => (
            <Fragment key={t.kind}>
              <span className={`vpc__tier vpc__tier--${t.kind}`}>
                {t.label}
                <span className="vpc__by">{tierReading(t)}</span>
              </span>
              <div className="vpc__row">
                <div className="vpc__cells">
                  {t.subnets.map((col, i) => (
                    <div key={i} className={`vpc__cell vpc__cell--${t.kind}`}>
                      {col.map((s) => (
                        <span key={s.id} className="vpc__subnet">
                          {s.name}
                          {typeof s.attrs?.cidr === "string" && <span className="muted"> {s.attrs.cidr}</span>}
                        </span>
                      ))}
                    </div>
                  ))}
                </div>
                <div className="vpc__placed" style={{ "--labels": Math.max(1, ...t.subnets.map((c) => c.length)) } as CSSProperties}>
                  {t.placed.map((p) => (
                    <div key={`${p.item.id}:${p.from}`} style={{ gridColumn: `${p.from + 1} / ${p.to + 2}` }}>
                      {p.continued ? (
                        <span className="tile tile--continued" title="Its subnets skip the zone in between">
                          <span className="tile__name">{displayName(p.item.node!)}</span>
                          <span className="tile__kind">also here</span>
                        </span>
                      ) : (
                        <Tile item={p.item} note={p.note} findings={findingsOf(p.item.id)} selected={selected === p.item.id} onSelect={onSelect} onHover={onHover} />
                      )}
                    </div>
                  ))}
                </div>
              </div>
            </Fragment>
          ))}
        </div>
      )}
      {loose.length > 0 && (
        <div className="vpc__loose">
          <p className="muted small">{zones.length ? "In this VPC, subnet not known" : "In this VPC"}</p>
          <div className="arch-group__tiles">
            {loose.map((l) => (
              <Tile key={l.item.id} item={l.item} note={l.note} findings={findingsOf(l.item.id)} selected={selected === l.item.id} onSelect={onSelect} onHover={onHover} />
            ))}
          </div>
        </div>
      )}
    </section>
  );
}
