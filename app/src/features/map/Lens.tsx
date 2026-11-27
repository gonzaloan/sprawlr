import { displayName, fmtX, nameList, whatIf, type Index } from "../../domain";
import type { Insights } from "../../domain";

export type LensKind = "health" | "load";

interface Props {
  lens: LensKind;
  onLens: (l: LensKind) => void;
  /** Where the snapshot has load figures: on this map, only on others, or nowhere (no metrics read). */
  capacity: "here" | "elsewhere" | "none";
  factor: number;
  onFactor: (f: number) => void;
  permissions: boolean;
  onPermissions: (p: boolean) => void;
}

/** Health or capacity (with a what-if traffic multiplier), and IAM-only connections. */
export function LensBar({ lens, onLens, capacity, factor, onFactor, permissions, onPermissions }: Props) {
  return (
    <div className="lens">
      {capacity === "here" && (
        <div className="tabs" role="radiogroup" aria-label="What the map shows">
          {(["health", "load"] as const).map((l) => (
            <button key={l} role="radio" aria-checked={lens === l} className={`tabs__opt${lens === l ? " is-on" : ""}`} onClick={() => onLens(l)}>
              {l === "health" ? "Health" : "Capacity"}
            </button>
          ))}
        </div>
      )}
      {capacity === "here" && lens === "load" && (
        <label className="whatif">
          <span>If traffic grows</span>
          <input type="range" min={1} max={4} step={0.1} value={factor} onChange={(e) => onFactor(Number(e.target.value))} aria-valuetext={`${factor.toFixed(1)} times today's traffic`} />
          <output>{factor.toFixed(1)}× today</output>
        </label>
      )}
      {capacity === "none" && (
        <span className="muted small">Capacity needs metrics: scan with alarms, metrics and traces included.</span>
      )}
      <label className="toggle" title="Connections that only an IAM policy allows. Resources mapped from IAM alone always show theirs.">
        <input type="checkbox" checked={permissions} onChange={(e) => onPermissions(e.target.checked)} />
        <span>IAM permissions</span>
      </label>
    </div>
  );
}

/** Banner over the map: what breaks at the chosen traffic level. Shown only on maps with load figures. */
export function WhatIfBanner({ ix, ins, factor, scope }: { ix: Index; ins: Insights; factor: number; scope: ReadonlySet<string> }) {
  const { broken, next } = whatIf(ins, factor, scope);
  const name = (id: string) => {
    const n = ix.byId.get(id);
    return n ? displayName(n) : id;
  };
  return (
    <div className={`whatif-banner${broken.length ? " is-bad" : ""}`} role="status">
      {factor === 1 ? (
        <span>Each bar is how close a resource came to its limit in its busiest hour of the last 24 hours. Drag the slider to see what runs out first as traffic grows.</span>
      ) : broken.length ? (
        <span>
          At {factor.toFixed(1)}× traffic, <strong>{nameList(broken.map((c) => name(c.node)))}</strong> {broken.length === 1 ? "is" : "are"} over the limit.
          {next ? ` Next: ${name(next.node)} at ${fmtX(next.headroom)}.` : ""}
        </span>
      ) : (
        <span>
          At {factor.toFixed(1)}× traffic nothing measured hits its limit.{next ? ` First to break: ${name(next.node)} at ${fmtX(next.headroom)}.` : ""}
        </span>
      )}
      <span className="muted small"> Assumes every part grows with traffic, one to one.</span>
    </div>
  );
}
