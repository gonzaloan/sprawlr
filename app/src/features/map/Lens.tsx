import { breakingAt, displayName, fmtX, type Index } from "../../domain";
import type { Insights } from "../../domain";

export type LensKind = "health" | "load";

interface Props {
  lens: LensKind;
  onLens: (l: LensKind) => void;
  factor: number;
  onFactor: (f: number) => void;
  permissions: boolean;
  onPermissions: (p: boolean) => void;
}

/** Health or load, the what-if traffic multiplier for load, and IAM-only connections. */
export function LensBar({ lens, onLens, factor, onFactor, permissions, onPermissions }: Props) {
  return (
    <div className="lens">
      <div className="tabs" role="radiogroup" aria-label="What the map shows">
        {(["health", "load"] as const).map((l) => (
          <button key={l} role="radio" aria-checked={lens === l} className={`tabs__opt${lens === l ? " is-on" : ""}`} onClick={() => onLens(l)}>
            {l === "health" ? "Health" : "Load"}
          </button>
        ))}
      </div>
      {lens === "load" && (
        <label className="whatif">
          <span>Traffic</span>
          <input type="range" min={1} max={4} step={0.1} value={factor} onChange={(e) => onFactor(Number(e.target.value))} aria-valuetext={`${factor.toFixed(1)} times today's traffic`} />
          <output>{factor.toFixed(1)}×</output>
        </label>
      )}
      <label className="toggle" title="Connections that only an IAM policy allows. Resources mapped from IAM alone always show theirs.">
        <input type="checkbox" checked={permissions} onChange={(e) => onPermissions(e.target.checked)} />
        <span>IAM permissions</span>
      </label>
    </div>
  );
}

/** Banner over the map: what breaks at the chosen traffic level. */
export function WhatIfBanner({ ix, ins, factor }: { ix: Index; ins: Insights; factor: number }) {
  const broken = breakingAt(ins, factor);
  const name = (id: string) => {
    const n = ix.byId.get(id);
    return n ? displayName(n) : id;
  };
  const next = ins.capacity.find((c) => c.headroom !== null && c.headroom > factor);
  return (
    <div className={`whatif-banner${broken.length ? " is-bad" : ""}`} role="status">
      {factor === 1 ? (
        <span>Bars show how close each resource is to its limit at today's busiest hour. Drag Traffic to see what breaks first.</span>
      ) : broken.length ? (
        <span>
          At {factor.toFixed(1)}× traffic, <strong>{broken.map((c) => name(c.node)).join(", ")}</strong> {broken.length === 1 ? "is" : "are"} over the limit.
          {next ? ` Next: ${name(next.node)} at ${fmtX(next.headroom!)}.` : ""}
        </span>
      ) : (
        <span>
          At {factor.toFixed(1)}× traffic nothing measured hits its limit.{next ? ` First to break: ${name(next.node)} at ${fmtX(next.headroom!)}.` : ""}
        </span>
      )}
      <span className="muted small"> Assumes every part grows with traffic, one to one.</span>
    </div>
  );
}
