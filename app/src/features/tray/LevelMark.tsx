import type { Level } from "../../domain";

/** The level as a shape, matching the tray icon: triangle, ring, dot, dashed ring. */
export function LevelMark({ level, size = 22 }: { level: Level | "stale"; size?: number }) {
  const common = { width: size, height: size, viewBox: "0 0 24 24", "aria-hidden": true } as const;
  switch (level) {
    case "act_now":
      return (
        <svg {...common} className="lvl lvl--act">
          <path d="M12 2.5 23 21.5H1Z" fill="currentColor" />
          <rect x="10.75" y="8.5" width="2.5" height="7" rx="1.25" fill="var(--paper)" />
          <circle cx="12" cy="18.3" r="1.35" fill="var(--paper)" />
        </svg>
      );
    case "plan_for_it":
      return (
        <svg {...common} className="lvl lvl--plan">
          <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" strokeWidth="3.2" />
        </svg>
      );
    case "all_clear":
      return (
        <svg {...common} className="lvl lvl--clear">
          <circle cx="12" cy="12" r="7" fill="currentColor" />
        </svg>
      );
    default:
      return (
        <svg {...common} className="lvl lvl--none">
          <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" strokeWidth="2.4" strokeDasharray="4 3.2" />
        </svg>
      );
  }
}
