import { useEffect, useState } from "react";
import { useBackend, type Every, type Settings } from "../../platform";

const INTERVALS: { value: Every | null; label: string; perDay?: number }[] = [
  { value: null, label: "Off" },
  { value: "15m", label: "Every 15 minutes", perDay: 96 },
  { value: "1h", label: "Every hour", perDay: 24 },
  { value: "4h", label: "Every 4 hours", perDay: 6 },
];

/** Background refresh, notifications, login and close behaviour. Each change applies at once. */
export function TraySettings({ onBack }: { onBack: () => void }) {
  const backend = useBackend();
  const [s, setS] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    backend
      .settings()
      .then(setS)
      .catch((e) => setError(String(e)));
  }, [backend]);

  const change = async (patch: Partial<Settings>) => {
    if (!s) return;
    const next = { ...s, ...patch };
    setS(next);
    try {
      const saved = await backend.saveSettings({
        background: next.background,
        notifications: next.notifications,
        launch_at_login: next.launch_at_login,
        close_to_tray: next.close_to_tray,
      });
      setS(saved);
      setError(null);
    } catch (e) {
      setError(`Could not save: ${e}`);
    }
  };

  return (
    <div className="tray__settings">
      <header className="tray__bar">
        <button className="btn btn--small btn--quiet" onClick={onBack} data-autofocus>
          Back
        </button>
        <h1 className="tray__title">Settings</h1>
      </header>

      {!s ? (
        <p className="muted small tray__pad">{error ?? (backend.can.tray ? "Loading settings…" : "Settings are available in the desktop app.")}</p>
      ) : (
        <div className="tray__pad">
          <fieldset className="tray__group">
            <legend>Refresh health in the background</legend>
            {INTERVALS.map((o) => (
              <label key={o.label} className="tray__radio">
                <input type="radio" name="every" checked={s.background === o.value} onChange={() => change({ background: o.value })} />
                <span>{o.label}</span>
                {o.perDay && <span className="muted small">about {o.perDay} a day</span>}
              </label>
            ))}
            <p className="muted small">
              Each refresh is billed by CloudWatch. Error logs are never read in the background. Paused while signed out.
            </p>
          </fieldset>

          <label className="check">
            <input type="checkbox" checked={s.notifications} onChange={(e) => change({ notifications: e.target.checked })} />
            <span>Notify me when something needs action now</span>
          </label>
          <label className="check">
            <input type="checkbox" checked={s.launch_at_login} onChange={(e) => change({ launch_at_login: e.target.checked })} />
            <span>Start sprawlr in the {s.place} when I log in</span>
          </label>
          <label className="check">
            <input type="checkbox" checked={s.close_to_tray} onChange={(e) => change({ close_to_tray: e.target.checked })} />
            <span>Keep sprawlr in the {s.place} when its window is closed</span>
          </label>
          {(s.notice || error) && (
            <p className="scan__warn small" role="status">
              {s.notice ?? error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
