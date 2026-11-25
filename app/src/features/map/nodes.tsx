import { Handle, Position, type NodeProps, type Node } from "@xyflow/react";
import type { AppSummary, LoadState } from "../../domain";
import type { Health, Layer } from "../../domain";
import { Dot, healthLabel } from "../../ui/Dot";
import { Spark } from "../../ui/Spark";

export type Mark = "none" | "dim" | "hot" | "origin";

export type ResourceData = {
  name: string;
  label: string;
  glyph: string;
  layer: Layer;
  health: Health;
  role: "member" | "neighbor" | "external";
  owner?: string; // app of a neighbour
  findings: number;
  mark: Mark;
  spark?: { values: (number | null)[]; label: string; alert: boolean };
  step?: number;
  load?: Load;
};

export type Load = { util: number; state: LoadState; label: string };

function LoadBar({ load }: { load: Load }) {
  const pct = Math.min(load.util, 1.25) / 1.25; // the bar keeps going a little past 100%
  return (
    <span className={`load load--${load.state}`} title={load.label}>
      <span className="load__fill" style={{ width: `${pct * 100}%` }} />
      <span className="load__limit" aria-hidden />
      <span className="load__text">{Math.round(load.util * 100)}%</span>
    </span>
  );
}

export type AppData = { app: AppSummary; mark: Mark; load?: Load };
export type TerritoryData = { name: string; detail: string };


function Ports() {
  return (
    <>
      <Handle type="target" position={Position.Left} className="port" />
      <Handle type="source" position={Position.Right} className="port" />
    </>
  );
}

export function ResourceNode({ data, selected }: NodeProps<Node<ResourceData>>) {
  return (
    <div
      className={`res res--${data.role} mark--${data.mark}${selected ? " is-selected" : ""}${data.load ? ` res--load-${data.load.state}` : ""}`}
      data-layer={data.layer}
      title={`${data.label}: ${data.name}`}
    >
      <Ports />
      {data.step !== undefined && (
        <span className="res__step" aria-label={`Step ${data.step}`}>
          {data.step}
        </span>
      )}
      <span className="res__glyph" aria-hidden>
        {data.glyph}
      </span>
      <span className="res__text">
        <span className="res__name">{data.name}</span>
        <span className="res__kind">{data.owner ? `${data.label} in ${data.owner}` : data.label}</span>
      </span>
      {data.spark && (
        <span className="res__spark" title={`${data.spark.label}, last 24 hours`}>
          <Spark values={data.spark.values} width={44} height={18} tone={data.spark.alert ? "alarm" : "ink"} />
        </span>
      )}
      {data.health !== "unknown" && <Dot tone={data.health} label={healthLabel(data.health)} />}
      {data.findings > 0 && (
        <span className="res__flag" aria-label={`${data.findings} findings`}>
          {data.findings}
        </span>
      )}
      {data.load && <LoadBar load={data.load} />}
    </div>
  );
}

export function AppNode({ data, selected }: NodeProps<Node<AppData>>) {
  const a = data.app;
  return (
    <div className={`appnode mark--${data.mark}${selected ? " is-selected" : ""}`}>
      <Ports />
      <span className="appnode__name">{a.name}</span>
      <span className="appnode__meta">
        {a.count} resources in {a.envs.join(" and ") || "no environment"}
      </span>
      <span className="appnode__foot">
        {a.health !== "unknown" && <Dot tone={a.health} label={healthLabel(a.health)} />}
        {a.findings > 0 ? `${a.findings} to review` : "Nothing to review"}
      </span>
      {data.load && <LoadBar load={data.load} />}
    </div>
  );
}

export function TerritoryNode({ data }: NodeProps<Node<TerritoryData>>) {
  return (
    <div className="territory">
      <span className="territory__name">{data.name}</span>
      <span className="territory__detail">{data.detail}</span>
    </div>
  );
}

export const nodeTypes = { resource: ResourceNode, app: AppNode, territory: TerritoryNode };
