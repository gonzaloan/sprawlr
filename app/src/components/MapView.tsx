import { useEffect, useMemo, useRef, useState } from "react";
import { Background, BackgroundVariant, Controls, MarkerType, ReactFlow, type Edge, type Node, type ReactFlowInstance } from "@xyflow/react";
import { displayName, kindInfo, metricsOf, primaryMetric, type View } from "../model";
import { layout, type Layout } from "../layout";
import { edgeTypes } from "./RouteEdge";
import { nodeTypes, type Load, type Mark } from "./nodes";
import type { EvidenceKind } from "../types";

interface Props {
  view: View;
  viewKey: string;
  findingsByNode: Map<string, number>;
  selected: string | null;
  blast: Set<string> | null;
  onSelect: (id: string | null) => void;
  onOpen: (id: string) => void;
  /** Load lens: utilisation to draw on each node. */
  loadOf?: (id: string) => Load | undefined;
  children?: React.ReactNode;
}

const LINE: Record<EvidenceKind, { width: number; dash?: string; color: string }> = {
  observed: { width: 2.4, color: "var(--ink)" },
  declared: { width: 1.4, color: "var(--ink-soft)" },
  permitted: { width: 1.4, dash: "2 5", color: "var(--muted)" },
};

export function MapView({ view, viewKey, findingsByNode, selected, blast, onSelect, onOpen, loadOf, children }: Props) {
  const [placed, setPlaced] = useState<{ key: string; items: Layout } | null>(null);

  useEffect(() => {
    let live = true;
    layout(view).then((items) => live && setPlaced({ key: viewKey, items }));
    return () => {
      live = false;
    };
  }, [view, viewKey]);

  const flow = useRef<ReactFlowInstance | null>(null);

  const neighbours = useMemo(() => {
    if (!selected) return null;
    const s = new Set([selected]);
    view.edges.forEach((e) => {
      if (e.from === selected) s.add(e.to);
      if (e.to === selected) s.add(e.from);
    });
    return s;
  }, [selected, view]);

  // Zoom to what the person just selected (the details panel also narrows the map).
  useEffect(() => {
    const f = flow.current;
    if (!f || !placed || placed.key !== viewKey) return;
    const focus = blast ? new Set([...blast, selected!]) : neighbours;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const t = setTimeout(() => {
      f.fitView({
        nodes: focus ? [...focus].map((id) => ({ id })) : undefined,
        padding: focus ? 0.3 : 0.12,
        maxZoom: 1.15,
        duration: reduce ? 0 : 350,
      });
    }, 60);
    return () => clearTimeout(t);
  }, [neighbours, blast, selected, placed, viewKey]);

  const markOf = (id: string): Mark => {
    if (blast) return id === selected ? "origin" : blast.has(id) ? "hot" : "dim";
    if (neighbours) return neighbours.has(id) ? "none" : "dim";
    return "none";
  };

  const nodes: Node[] = useMemo(() => {
    if (!placed || placed.key !== viewKey) return [];
    const byId = new Map(view.nodes.map((n) => [n.id, n]));
    const groups = new Map(view.groups.map((g) => [g.id, g]));
    return placed.items.nodes.map((p): Node => {
      const base = { id: p.id, position: { x: p.x, y: p.y }, parentId: p.parent, width: p.w, height: p.h };
      const g = groups.get(p.id);
      if (g) {
        const cidr = (g.attrs as { cidr?: string } | null)?.cidr;
        return { ...base, type: "territory", selectable: false, draggable: false, zIndex: -1, data: { name: g.name, detail: cidr ? `VPC ${cidr}` : "VPC" } };
      }
      const v = byId.get(p.id)!;
      if (v.app) return { ...base, type: "app", data: { app: v.app, mark: markOf(p.id), load: loadOf?.(p.id) } };
      const n = v.node!;
      const k = kindInfo(n);
      return {
        ...base,
        type: "resource",
        selected: p.id === selected,
        data: {
          name: displayName(n),
          label: k.label,
          glyph: k.glyph,
          layer: k.layer,
          health: n.health,
          role: v.role,
          owner: v.role === "neighbor" ? n.app?.value : undefined,
          findings: findingsByNode.get(n.id) ?? 0,
          mark: markOf(p.id),
          step: v.step,
          load: loadOf?.(p.id),
          spark: (() => {
            const m = primaryMetric(n);
            return m ? { values: m.values, label: m.label, alert: metricsOf(n).some((x) => x.alert) } : undefined;
          })(),
        },
      };
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [placed, viewKey, view, selected, blast, neighbours, findingsByNode, loadOf]);

  const edges: Edge[] = useMemo(
    () =>
      view.edges.map((e) => {
        const line = LINE[e.strength];
        const touches = selected !== null && (e.from === selected || e.to === selected);
        const lit = blast ? (blast.has(e.from) || e.from === selected) && (blast.has(e.to) || e.to === selected) : touches;
        const faded = (blast || selected) && !lit;
        return {
          id: e.id,
          source: e.from,
          target: e.to,
          type: "route",
          animated: e.mode === "async",
          data: { points: placed?.key === viewKey ? placed.items.routes.get(e.id) : undefined },
          label: lit || (!selected && e.count && e.count > 1) ? e.label : undefined,
          className: "route",
          style: {
            stroke: blast && lit ? "var(--alarm)" : line.color,
            strokeWidth: line.width,
            strokeDasharray: line.dash,
            opacity: faded ? 0.18 : 1,
          },
          markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14, color: blast && lit ? "#B3261E" : "#1D2B2A" },
        };
      }),
    [view, selected, blast, placed, viewKey],
  );

  return (
    <div className="map" aria-label="Architecture map">
      <ReactFlow
        key={placed?.key}
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        nodesDraggable={false}
        fitView
        fitViewOptions={{ padding: 0.12, maxZoom: 1.15 }}
        onInit={(f) => (flow.current = f)}
        minZoom={0.2}
        nodesConnectable={false}
        onNodeClick={(_, n) => (n.type === "app" ? onOpen(n.id) : onSelect(n.id))}
        onNodeDoubleClick={(_, n) => onOpen(n.id)}
        onPaneClick={() => onSelect(null)}
        proOptions={{ hideAttribution: true }}
      >
        <Background variant={BackgroundVariant.Dots} gap={24} size={1} color="var(--grid)" />
        <Controls showInteractive={false} position="bottom-right" />
      </ReactFlow>
      <Legend async={view.edges.some((e) => e.mode === "async")} />
      {children}
    </div>
  );
}

function Legend({ async }: { async: boolean }) {
  return (
    <div className="legend" aria-label="Legend">
      <p className="legend__title">How we know two things talk</p>
      {(["observed", "declared", "permitted"] as const).map((k) => (
        <div key={k} className="legend__row">
          <svg width="44" height="10" aria-hidden>
            <line x1="0" y1="5" x2="44" y2="5" stroke={k === "observed" ? "#1D2B2A" : k === "declared" ? "#4A5A55" : "#8A958F"} strokeWidth={LINE[k].width} strokeDasharray={LINE[k].dash} />
          </svg>
          <span>{k === "observed" ? "Seen in traffic" : k === "declared" ? "Configured" : "Allowed by IAM, never configured"}</span>
        </div>
      ))}
      {async && (
        <div className="legend__row">
          <svg width="44" height="10" aria-hidden>
            <line x1="0" y1="5" x2="44" y2="5" stroke="#1D2B2A" strokeWidth={1.4} strokeDasharray="5 4" />
          </svg>
          <span>Moving: happens later, through a queue or event</span>
        </div>
      )}
    </div>
  );
}
