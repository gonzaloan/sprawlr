// Tiny 24-hour sparkline. Gaps (no datapoint) break the line instead of pretending zero.
interface Props {
  values: (number | null)[];
  width: number;
  height: number;
  tone?: "ink" | "alarm";
  label?: string;
}

export function Spark({ values, width, height, tone = "ink", label }: Props) {
  const nums = values.filter((v): v is number => v !== null);
  if (nums.length < 2) return null;
  const max = Math.max(...nums);
  const min = Math.min(0, ...nums);
  const span = max - min || 1;
  const x = (i: number) => (i / (values.length - 1)) * width;
  const y = (v: number) => height - 1 - ((v - min) / span) * (height - 2);

  let d = "";
  let pen = false;
  values.forEach((v, i) => {
    if (v === null) {
      pen = false;
      return;
    }
    d += `${pen ? "L" : "M"}${x(i).toFixed(1)} ${y(v).toFixed(1)} `;
    pen = true;
  });
  const color = tone === "alarm" ? "var(--alarm)" : "var(--ink-soft)";
  return (
    <svg className="spark" width={width} height={height} viewBox={`0 0 ${width} ${height}`} role={label ? "img" : undefined} aria-label={label} aria-hidden={label ? undefined : true}>
      <line x1="0" x2={width} y1={height - 0.5} y2={height - 0.5} stroke="var(--rule)" strokeWidth="1" />
      <path d={d} fill="none" stroke={color} strokeWidth="1.5" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}
