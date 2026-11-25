// Numbers as people read them.

export function fmtValue(v: number | null | undefined, unit: string): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "no data";
  const n = (x: number) =>
    Math.abs(x) >= 1e6 ? `${(x / 1e6).toFixed(1)}M` : Math.abs(x) >= 1e4 ? `${Math.round(x / 1e3)}k` : Math.abs(x) >= 1e3 ? `${(x / 1e3).toFixed(1)}k` : Math.abs(x - Math.round(x)) < 0.05 ? `${Math.round(x)}` : x.toFixed(1);
  if (unit === "ms" && v >= 1000) return `${(v / 1000).toFixed(1)} s`;
  if (unit === "bytes") return v >= 1e9 ? `${(v / 1e9).toFixed(1)} GB` : v >= 1e6 ? `${(v / 1e6).toFixed(1)} MB` : v >= 1e3 ? `${(v / 1e3).toFixed(1)} KB` : `${Math.round(v)} B`;
  if (unit === "count") return n(v);
  if (unit === "%") return `${n(v)}%`;
  return `${n(v)} ${unit}`;
}

/** Headroom rounded down (the safe reading): two decimals below 2× where it matters, one above. */
export const fmtX = (h: number) => (h < 2 ? `${(Math.floor(h * 100) / 100).toFixed(2)}×` : `${(Math.floor(h * 10) / 10).toFixed(1)}×`);

export function fmtCount(v: number | null | undefined): string {
  return v === null || v === undefined ? "unknown" : fmtValue(v, "count");
}

/** "a", "a and b", "a, b and c". */
export function nameList(names: string[]): string {
  return names.length < 2 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}
