import { useCallback, useEffect, useRef, useState } from "react";
import { fromHash, popped, pushed, startAt, toHash, type Steps, type Whereabouts } from "../domain";

/**
 * Where the person is, kept in the browser's history: every place, selection and tab is a step,
 * so Back, Forward and the URL work in the desktop app and in the web demo alike.
 *
 * `keep` fits whereabouts to the snapshot on screen (Back and links can name another's).
 * `historyKeys`: the UI handles ⌘[ and ⌘] itself, where no browser does it.
 */
export function useWhereabouts(keep: (w: Whereabouts) => Whereabouts, historyKeys: boolean) {
  const [state, setState] = useState(() => ({ here: keep(fromHash(window.location.hash)), steps: startAt(window.history.state) }));
  // Read by callbacks that outlive a render (a scan finishing, a tray click).
  const latest = useRef(state);
  latest.current = state;
  const keepRef = useRef(keep);
  keepRef.current = keep;

  const show = useCallback((next: Whereabouts, how: "push" | "replace") => {
    const { here, steps } = latest.current;
    const w = keepRef.current(next);
    if (how === "push" && toHash(w) === toHash(here)) return;
    const after = how === "push" ? pushed(steps) : steps;
    window.history[how === "push" ? "pushState" : "replaceState"]({ at: after.at }, "", toHash(w));
    latest.current = { here: w, steps: after };
    setState(latest.current);
  }, []);
  const go = useCallback((next: Whereabouts) => show(next, "push"), [show]);
  const replace = useCallback((next: Whereabouts) => show(next, "replace"), [show]);
  const back = useCallback(() => latest.current.steps.at > 0 && window.history.back(), []);
  const forward = useCallback(() => latest.current.steps.at < latest.current.steps.last && window.history.forward(), []);

  useEffect(() => {
    window.history.replaceState({ at: latest.current.steps.at }, "", toHash(latest.current.here));
    const onPop = (e: PopStateEvent) => {
      const steps: Steps = popped(latest.current.steps, e.state);
      const here = keepRef.current(fromHash(window.location.hash));
      if (e.state === null) window.history.replaceState({ at: steps.at }, "", toHash(here));
      latest.current = { here, steps };
      setState(latest.current);
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  useEffect(() => {
    if (!historyKeys) return;
    const typing = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || typing(e.target) || (e.key !== "[" && e.key !== "]")) return;
      e.preventDefault();
      if (e.key === "[") back();
      else forward();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [historyKeys, back, forward]);

  const { here, steps } = state;
  return { here, go, replace, back, forward, canBack: steps.at > 0, canForward: steps.at < steps.last };
}
