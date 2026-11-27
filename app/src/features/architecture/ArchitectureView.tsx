import { useRef, useState } from "react";
import type { Architecture } from "../../domain";
import { TileGroup } from "./TileGroup";
import { VpcBox } from "./VpcBox";
import { Wires } from "./Wires";

interface Props {
  arch: Architecture;
  account: string | null;
  /** The environment shown, to say why a VPC looks empty. */
  env: string | null;
  findingsByNode: Map<string, number>;
  selected: string | null;
  onSelect: (id: string) => void;
}

/**
 * The place's resources where they run: what comes from the internet and through which edge
 * services, each region with its VPCs and the services outside them, and what lives outside AWS.
 */
export function ArchitectureView({ arch, account, env, findingsByNode, selected, onSelect }: Props) {
  const canvas = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const tiles = { findingsOf: (id: string) => findingsByNode.get(id) ?? 0, selected, onSelect, onHover: setHover };
  const focus = hover ?? selected;

  return (
    <div className="arch">
      <div className="arch__canvas" ref={canvas}>
        <div className="arch__body">
          <div className="arch__bar">
            <label className="check">
              <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
              All connections
            </label>
            <span className="arch__legend">
              <span className="arch__key arch__key--observed">Seen in traffic</span>
              <span className="arch__key arch__key--declared">Configured</span>
              <span className="arch__key arch__key--permitted">Allowed by IAM</span>
            </span>
            {!all && <span className="muted small">Hover or select a resource to see its connections.</span>}
          </div>

          {(arch.users.length > 0 || arch.edge.length > 0) && (
            <div className="arch__lane">
              <TileGroup label="From the internet" items={arch.users} {...tiles} />
              <TileGroup label="Edge" items={arch.edge} {...tiles} />
            </div>
          )}

          <section className="arch__cloud" aria-label="AWS">
            <header className="arch__cloud-head">
              <strong>AWS</strong>
              {account && <span className="muted small">account {account}</span>}
            </header>
            {arch.regions.map((r) => (
              <section key={r.region} className="arch__region" aria-label={`Region ${r.region || "not known"}`}>
                <h3 className="arch__region-name">{r.region || "Region not known"}</h3>
                {r.vpcs.map((v) => (
                  <VpcBox key={v.vpc.id} plan={v} {...tiles} />
                ))}
                {r.quiet.map((q) => (
                  <p key={q.vpc.id} className="arch__quiet">
                    <strong>{q.vpc.name}</strong>
                    {q.vpc.attrs?.is_default === true ? " (default VPC)" : " (VPC)"}: {q.subnets} {q.subnets === 1 ? "subnet" : "subnets"} in {q.zones}{" "}
                    {q.zones === 1 ? "zone" : "zones"},{" "}
                    {q.resources === 0 ? "nothing runs in it." : `${q.resources} ${q.resources === 1 ? "resource" : "resources"}, none ${env ? `in ${env}` : "of them here"}.`}
                  </p>
                ))}
                {r.services.length > 0 && (
                  <div className="arch__services">
                    <p className="muted small">Outside any VPC</p>
                    {r.services.map((g) => (
                      <TileGroup key={g.layer} label={g.label} items={g.items} {...tiles} />
                    ))}
                  </div>
                )}
              </section>
            ))}
            <TileGroup label="Account-wide: IAM" items={arch.accountWide} className="arch__global" {...tiles} />
          </section>

          {arch.outside.length > 0 && (
            <div className="arch__lane">
              <TileGroup label="Outside AWS" items={arch.outside} {...tiles} />
            </div>
          )}
        </div>
        <Wires canvas={canvas} edges={arch.edges} focus={focus} all={all} />
      </div>
    </div>
  );
}
