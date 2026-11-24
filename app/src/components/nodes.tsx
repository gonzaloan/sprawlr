import { Handle, Position, type NodeProps, type Node } from "@xyflow/react";
import type { AppSummary } from "../model";
import type { Health, Layer } from "../types";
import { Spark } from "./Spark";

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
};

export type AppData = { app: AppSummary; mark: Mark };
export type TerritoryData = { name: string; detail: string };

const HEALTH_TEXT: Record<Health, string> = { ok: "Healthy", warning: "Warning", alarm: "Alarm", unknown: "No health data" };

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
      className={`res res--${data.role} mark--${data.mark}${selected ? " is-selected" : ""}`}
      data-layer={data.layer}
      title={`${data.label}: ${data.name}`}
    >
      <Ports />
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
      {data.health !== "unknown" && <span className={`dot dot--${data.health}`} aria-label={HEALTH_TEXT[data.health]} />}
      {data.findings > 0 && (
        <span className="res__flag" aria-label={`${data.findings} findings`}>
          {data.findings}
        </span>
      )}
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
        {a.health !== "unknown" && <span className={`dot dot--${a.health}`} />}
        {a.findings > 0 ? `${a.findings} to review` : "Nothing to review"}
      </span>
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
