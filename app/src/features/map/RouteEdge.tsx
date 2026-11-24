import { BaseEdge, type Edge, type EdgeProps } from "@xyflow/react";
import type { Point } from "./layout";

export type RouteData = { points?: Point[] };

/** Orthogonal path through ELK's bend points, with softened corners. */
function path(pts: Point[], r = 8): string {
  let d = `M ${pts[0].x} ${pts[0].y}`;
  for (let i = 1; i < pts.length - 1; i++) {
    const [a, b, c] = [pts[i - 1], pts[i], pts[i + 1]];
    const k1 = Math.min(r, Math.hypot(b.x - a.x, b.y - a.y) / 2);
    const k2 = Math.min(r, Math.hypot(c.x - b.x, c.y - b.y) / 2);
    const p1 = { x: b.x - Math.sign(b.x - a.x) * k1, y: b.y - Math.sign(b.y - a.y) * k1 };
    const p2 = { x: b.x + Math.sign(c.x - b.x) * k2, y: b.y + Math.sign(c.y - b.y) * k2 };
    d += ` L ${p1.x} ${p1.y} Q ${b.x} ${b.y} ${p2.x} ${p2.y}`;
  }
  const z = pts[pts.length - 1];
  return `${d} L ${z.x} ${z.y}`;
}

/** Label sits on the longest segment, where it is least likely to collide. */
function labelAt(pts: Point[]): Point {
  let best = { len: -1, x: pts[0].x, y: pts[0].y };
  for (let i = 1; i < pts.length; i++) {
    const [a, b] = [pts[i - 1], pts[i]];
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    if (len > best.len) best = { len, x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
  }
  return best;
}

export function RouteEdge(props: EdgeProps<Edge<RouteData>>) {
  const { sourceX, sourceY, targetX, targetY, data, markerEnd, style, label } = props;
  const pts = data?.points?.length ? data.points : [{ x: sourceX, y: sourceY }, { x: targetX, y: targetY }];
  const at = labelAt(pts);
  return (
    <BaseEdge
      path={path(pts)}
      markerEnd={markerEnd}
      style={style}
      label={label}
      labelX={at.x}
      labelY={at.y}
      labelShowBg
      labelBgPadding={[6, 3]}
      labelBgBorderRadius={3}
    />
  );
}

export const edgeTypes = { route: RouteEdge };
