import { useEffect, useRef, useState } from "react";
import { displayName, kindInfo, search, type Index } from "../model";

/** "Find a resource" box. Press / anywhere to focus it. */
export function Find({ ix, onPick }: { ix: Index; onPick: (id: string) => void }) {
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const results = search(ix, q);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement;
      if (e.key === "/" && !typing) {
        e.preventDefault();
        input.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const pick = (id: string) => {
    onPick(id);
    setQ("");
    input.current?.blur();
  };

  return (
    <div className="find">
      <input
        ref={input}
        value={q}
        placeholder="Find a resource  ( / )"
        aria-label="Find a resource"
        spellCheck={false}
        onChange={(e) => {
          setQ(e.target.value);
          setActive(0);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") setActive((a) => Math.min(a + 1, results.length - 1));
          else if (e.key === "ArrowUp") setActive((a) => Math.max(a - 1, 0));
          else if (e.key === "Enter" && results[active]) pick(results[active].id);
          else if (e.key === "Escape") setQ("");
          else return;
          e.preventDefault();
        }}
      />
      {q && (
        <ul className="find__list" role="listbox" aria-label="Matching resources">
          {results.length === 0 && <li className="find__empty">No resource matches “{q}”.</li>}
          {results.map((n, i) => (
            <li key={n.id} role="option" aria-selected={i === active}>
              <button className={`find__item${i === active ? " is-active" : ""}`} onMouseDown={(e) => e.preventDefault()} onClick={() => pick(n.id)}>
                <span>{displayName(n)}</span>
                <span className="muted">
                  {kindInfo(n).label}
                  {n.app ? ` in ${n.app.value}` : ""}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
