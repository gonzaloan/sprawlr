import { useId, useLayoutEffect, useState, type RefObject } from "react";
import type { ViewEdge } from "../../domain";

interface Props {
  /** The positioned box the tiles and the lines share. */
  canvas: RefObject<HTMLDivElement | null>;
  edges: ViewEdge[];
  /** The resource whose connections to draw; all of them when `all`. */
  focus: string | null;
  all: boolean;
}

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A point along a side, as close to `toward` as the side allows: wires leave toward where they go. */
const along = (start: number, length: number, toward: number) => Math.min(Math.max(toward, start + 12), start + length - 12);

/**
 * Where two tiles meet: side to side when one is clear of the other horizontally, otherwise
 * top to bottom. A gentle curve, so lines crossing a zone grid stay readable.
 */
function route(a: Box, b: Box): string {
  const pull = (n: number) => Math.max(24, Math.abs(n) / 2);
  const mid = (t: Box) => ({ x: t.x + t.w / 2, y: t.y + t.h / 2 });
  if (b.x >= a.x + a.w || a.x >= b.x + b.w) {
    const right = b.x >= a.x + a.w;
    const [x1, y1] = [right ? a.x + a.w : a.x, along(a.y, a.h, mid(b).y)];
    const [x2, y2] = [right ? b.x : b.x + b.w, along(b.y, b.h, mid(a).y)];
    const c = pull(x2 - x1) * (right ? 1 : -1);
    return `M${x1},${y1} C${x1 + c},${y1} ${x2 - c},${y2} ${x2},${y2}`;
  }
  const down = b.y >= a.y;
  const [x1, y1] = [along(a.x, a.w, mid(b).x), down ? a.y + a.h : a.y];
  const [x2, y2] = [along(b.x, b.w, mid(a).x), down ? b.y : b.y + b.h];
  const c = pull(y2 - y1) * (down ? 1 : -1);
  return `M${x1},${y1} C${x1},${y1 + c} ${x2},${y2 - c} ${x2},${y2}`;
}

/**
 * Connections drawn over the architecture. Tiles are measured once per layout (a new place, a
 * resize of the canvas); hovering only picks which lines to draw.
 */
export function Wires({ canvas, edges, focus, all }: Props) {
  const marker = `wire-end-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const [boxes, setBoxes] = useState<Map<string, Box>>(new Map());

  useLayoutEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const measure = () => {
      const base = el.getBoundingClientRect();
      const found = new Map<string, Box>();
      el.querySelectorAll<HTMLElement>("[data-tile]").forEach((t) => {
        const r = t.getBoundingClientRect();
        found.set(t.dataset.tile!, { x: r.left - base.left, y: r.top - base.top, w: r.width, h: r.height });
      });
      setBoxes(found);
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, [canvas, edges]);

  const touches = (e: ViewEdge) => focus !== null && (e.from === focus || e.to === focus);
  const wires = edges
    .filter((e) => all || touches(e))
    .flatMap((e) => {
      const [a, b] = [boxes.get(e.from), boxes.get(e.to)];
      return a && b ? [{ id: e.id, d: route(a, b), strength: e.strength, lit: touches(e) }] : [];
    });
  if (!wires.length) return null;
  return (
    <svg className="wires" aria-hidden>
      <defs>
        <marker id={marker} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M0,0 L8,4 L0,8 z" className="wires__head" />
        </marker>
      </defs>
      {wires.map((w) => (
        <path key={w.id} d={w.d} className={`wires__line wires__line--${w.strength}${w.lit ? " is-lit" : ""}`} markerEnd={`url(#${marker})`} />
      ))}
    </svg>
  );
}
