// The name in a sidebar row, cut with "…" when it is too long; the full name shows on hover
// and stays the button's accessible name.
import type { ReactNode } from "react";

export function SideLabel({ name, mark }: { name: string; mark?: ReactNode }) {
  return (
    <span className="side__name">
      {mark}
      <span className="side__text" title={name}>
        {name}
      </span>
    </span>
  );
}
