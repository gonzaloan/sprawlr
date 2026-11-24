import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  LEVEL_LABEL,
  TOP,
  fmtCount,
  isStale,
  logErrorsOf,
  logsReadNote,
  monthlyCost,
  readAgo,
  statusOf,
  type Priority,
  type Snapshot,
  type Target,
} from "../../domain";
import { useBackend, type RefreshState } from "../../platform";
import { LevelMark } from "./LevelMark";
import { TraySettings } from "./TraySettings";

const PRIORITY: Record<Priority, string> = { high: "Act now", medium: "Plan for it", low: "Good to know" };
const IDLE: RefreshState = { state: "idle", next_at: null, paused: false };

/** Every focusable control in the popover, in tab order. */
const focusables = (root: HTMLElement) =>
  Array.from(root.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), [tabindex='0']"));

/**
 * The tray popover: the account in one look, and a door into the full app. Reads the stored
 * snapshot's status (computed in Rust) and never recomputes rules.
 */
export function TrayPopover() {
  const backend = useBackend();
  const root = useRef<HTMLDivElement>(null);
  const [snap, setSnap] = useState<Snapshot | null | undefined>(undefined);
  const [refresh, setRefresh] = useState<RefreshState>(IDLE);
  const [now, setNow] = useState(() => new Date());
  const [view, setView] = useState<"status" | "settings">("status");
  const [profile, setProfile] = useState<string | null>(null);

  const focusFirst = useCallback(() => {
    requestAnimationFrame(() => {
      const r = root.current;
      if (!r) return;
      (r.querySelector<HTMLElement>("[data-autofocus]") ?? focusables(r)[0])?.focus();
    });
  }, []);

  useEffect(() => {
    backend.lastSnapshot().then(setSnap).catch(() => setSnap(null));
    backend.refreshState().then(setRefresh).catch(() => {});
    backend.settings().then((s) => setProfile(s?.profile ?? null)).catch(() => {});
    const stops = [
      backend.onSnapshotChanged((s) => setSnap(s)),
      backend.onRefreshState(setRefresh),
      backend.onPopoverShown(() => {
        setNow(new Date());
        setView("status");
        backend.settings().then((s) => setProfile(s?.profile ?? null)).catch(() => {});
        focusFirst();
      }),
    ];
    const tick = setInterval(() => setNow(new Date()), 60_000);
    return () => {
      clearInterval(tick);
      stops.forEach((p) => p.then((stop) => stop()));
    };
  }, [backend, focusFirst]);

  useEffect(focusFirst, [view, snap === undefined, focusFirst]);

  const open = (target?: Target) => backend.openMain(target);

  // Esc hides; Tab wraps; arrows move within a list.
  const onKeyDown = (e: React.KeyboardEvent) => {
    const r = root.current;
    if (!r) return;
    if (e.key === "Escape") {
      e.preventDefault();
      if (view === "settings") setView("status");
      else backend.hidePopover();
      return;
    }
    const all = focusables(r);
    const i = all.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Tab" && all.length) {
      if (e.shiftKey && i <= 0) {
        e.preventDefault();
        all[all.length - 1].focus();
      } else if (!e.shiftKey && i === all.length - 1) {
        e.preventDefault();
        all[0].focus();
      }
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const list = (document.activeElement as HTMLElement | null)?.closest("[data-list]");
      if (!list) return;
      const items = Array.from(list.querySelectorAll<HTMLElement>("button"));
      const j = items.indexOf(document.activeElement as HTMLElement);
      const next = items[j + (e.key === "ArrowDown" ? 1 : -1)];
      if (next) {
        e.preventDefault();
        next.focus();
      }
    }
  };

  const status = snap ? statusOf(snap) : null;
  const stale = snap ? isStale(snap.taken_at, now) : false;
  const logsNote = useMemo(() => {
    if (!snap) return null;
    const reads = snap.nodes.map((n) => logErrorsOf(n)?.read_at).filter((t): t is string => !!t);
    return reads.length ? logsReadNote(reads.sort()[0], snap.taken_at) : null;
  }, [snap]);
  const readsLogs = useMemo(() => !!snap?.nodes.some((n) => logErrorsOf(n)), [snap]);

  const running = refresh.state === "running";
  const doRefresh = () => backend.refreshStored().catch(() => {});

  let body: React.ReactNode;
  if (view === "settings") {
    body = <TraySettings onBack={() => setView("status")} />;
  } else if (snap === undefined) {
    body = null;
  } else if (!snap || !status) {
    body = (
      <div className="tray__empty">
        <LevelMark level="stale" size={40} />
        <p>No account read yet. Scan an account in sprawlr to see its status here.</p>
        <button className="btn" onClick={() => open()} data-autofocus>
          Open sprawlr
        </button>
      </div>
    );
  } else {
    const level = stale ? "stale" : status.level;
    const top = status.items.slice(0, TOP);
    const flows = status.flows_with_errors;
    body = (
      <>
        <section className={`tray__status tray__status--${level}`} aria-live="polite">
          <LevelMark level={level} size={30} />
          <div>
            <h2>{stale ? "No current status" : LEVEL_LABEL[status.level]}</h2>
            <p>
              {stale
                ? `Last read ${readAgo(snap.taken_at, now)}. Refresh health to see the account now.`
                : status.headline}
            </p>
          </div>
        </section>

        <div className="tray__scroll">
          {top.length > 0 && (
            <section aria-label="What needs attention">
              <ul className="tray__list" data-list>
                {top.map((it) => (
                  <li key={it.key}>
                    <button className={`tray__item tray__item--${it.priority}`} onClick={() => open(it.open)}>
                      <span className="tray__prio">{PRIORITY[it.priority]}</span>
                      <span className="tray__text">{it.title}</span>
                    </button>
                  </li>
                ))}
              </ul>
              {status.items.length > TOP && (
                <button className="btn--link small tray__more" onClick={() => open()}>
                  See all {status.items.length} in sprawlr
                </button>
              )}
            </section>
          )}

          {flows.length > 0 && (
            <section>
              <h3 className="tray__h">Flows with errors, last 24 h</h3>
              <ul className="tray__list" data-list>
                {flows.slice(0, 3).map((f) => (
                  <li key={f.flow}>
                    <button className="tray__row" onClick={() => open({ type: "flow", id: f.flow })}>
                      <span className="tray__text">{f.name}</span>
                      <strong>{fmtCount(f.errors)}</strong>
                    </button>
                  </li>
                ))}
              </ul>
              {flows.length > 3 && <p className="muted small">and {flows.length - 3} more</p>}
              {logsNote && <p className="muted small">{logsNote}</p>}
            </section>
          )}

          {status.cleanup.resources > 0 && (
            <section>
              <h3 className="tray__h">Probably not needed</h3>
              <ul className="tray__list" data-list>
                <li>
                  <button className="tray__row" onClick={() => open({ type: "cleanup" })}>
                    <span className="tray__text">
                      {status.cleanup.resources} resources
                      {status.cleanup.monthly_cost > 0 ? `, about ${monthlyCost(status.cleanup.monthly_cost)} a month` : ""}
                    </span>
                  </button>
                </li>
              </ul>
            </section>
          )}
        </div>

        <footer className="tray__foot">
          <RefreshLine refresh={refresh} />
          <div className="tray__actions">
            <button className="btn btn--quiet" onClick={doRefresh} disabled={running || !backend.can.tray}>
              {running ? "Refreshing…" : refresh.state === "idle" ? "Refresh health" : "Try again"}
            </button>
            <button className="btn" onClick={() => open()}>
              Open sprawlr
            </button>
          </div>
          <p className="muted small">
            {!backend.can.tray
              ? "Refreshing needs the desktop app."
              : readsLogs
                ? "A few cents of CloudWatch, plus Logs Insights per GB scanned."
                : "A few cents of CloudWatch per refresh."}
          </p>
        </footer>
      </>
    );
  }

  return (
    <div className="tray" role="dialog" aria-label="sprawlr status" ref={root} onKeyDown={onKeyDown}>
      {view === "status" && (
        <header className="tray__bar">
          <div>
            <span className="wordmark tray__brand">sprawlr</span>
            {snap && (
              <p className="muted small">
                {backend.can.tray ? `${statusOf(snap)?.account ?? "Unknown account"}, ${profile ?? "default credentials"}` : "Demo account"}, read{" "}
                {readAgo(snap.taken_at, now)}
              </p>
            )}
          </div>
          {backend.can.tray && (
            <button className="btn btn--small btn--quiet" onClick={() => setView("settings")}>
              Settings
            </button>
          )}
        </header>
      )}
      {body}
    </div>
  );
}

/** What the last or current refresh is doing, in words. */
function RefreshLine({ refresh }: { refresh: RefreshState }) {
  switch (refresh.state) {
    case "running":
      return (
        <p className="tray__refresh small" role="status">
          {refresh.step}…
        </p>
      );
    case "failed":
      return (
        <p className="tray__refresh tray__refresh--bad small" role="status">
          Refresh failed: {refresh.message}
          {refresh.next_at ? ` Next try at ${new Date(refresh.next_at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}.` : ""}
        </p>
      );
    case "sign_in_needed":
      return (
        <p className="tray__refresh tray__refresh--bad small" role="status">
          Can't sign in with {refresh.profile ?? "the default credentials"}. Run <code>{refresh.command}</code>, then try again.
          {refresh.paused ? " Background refresh is paused until a refresh works." : ""}
        </p>
      );
    default:
      return null;
  }
}
