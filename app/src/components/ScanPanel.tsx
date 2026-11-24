import { useEffect, useRef, useState } from "react";
import type { ScanResult, Snapshot } from "../types";
import { call, inTauri } from "../platform";

interface Props {
  current: Snapshot;
  fromScan: boolean;
  onLoad: (snap: Snapshot, source: string, fromScan: boolean) => void;
  onDemo: () => void;
}

export function ScanPanel({ current, fromScan, onLoad, onDemo }: Props) {
  const desktop = inTauri();
  const [profiles, setProfiles] = useState<string[]>([]);
  const [profile, setProfile] = useState("");
  const [regions, setRegions] = useState("us-east-1");
  const [telemetry, setTelemetry] = useState(true);
  const [busy, setBusy] = useState<null | "scan" | "health">(null);
  const [step, setStep] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const file = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!desktop) return;
    call<string[]>("list_profiles")
      .then((p) => {
        setProfiles(p);
        setProfile(p.includes("default") ? "default" : p[0] ?? "");
      })
      .catch((e) => setMsg(String(e)));
  }, [desktop]);

  // Progress messages streamed from Rust while a scan runs.
  useEffect(() => {
    if (!desktop || !busy) return;
    let stop: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => listen<string>("scan-progress", (e) => setStep(e.payload)).then((u) => (stop = u)));
    return () => stop?.();
  }, [desktop, busy]);

  const run = async (kind: "scan" | "health") => {
    setBusy(kind);
    setMsg(null);
    setStep(null);
    setWarnings([]);
    try {
      const list = regions.split(/[\s,]+/).filter(Boolean);
      const r =
        kind === "scan"
          ? await call<ScanResult>("scan", { profile: profile || null, regions: list, telemetry })
          : await call<ScanResult>("refresh_health", { profile: profile || null, snapshot: current });
      setWarnings(r.warnings);
      onLoad(r.snapshot, `${profile || "default credentials"}, ${kind === "scan" ? list.join(", ") : "health refreshed"}`, true);
    } catch (e) {
      setMsg(`${kind === "scan" ? "Scan" : "Refresh"} failed: ${e}. Check that the profile is signed in (aws sso login).`);
    } finally {
      setBusy(null);
      setStep(null);
    }
  };

  const open = async (f: File) => {
    try {
      const snap = JSON.parse(await f.text()) as Snapshot;
      if (!Array.isArray(snap.nodes) || !Array.isArray(snap.edges)) throw new Error("not an sprawlr snapshot");
      onLoad(snap, f.name, false);
      setMsg(null);
    } catch (e) {
      setMsg(`Could not open ${f.name}: ${e instanceof Error ? e.message : e}`);
    }
  };

  return (
    <div className="scan">
      {desktop ? (
        <>
          <label className="field">
            <span>AWS profile</span>
            <select value={profile} onChange={(e) => setProfile(e.target.value)} disabled={!!busy}>
              {profiles.length === 0 && <option value="">Default credentials</option>}
              {profiles.map((p) => (
                <option key={p}>{p}</option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>Regions</span>
            <input value={regions} onChange={(e) => setRegions(e.target.value)} spellCheck={false} disabled={!!busy} />
          </label>
          <label className="check">
            <input type="checkbox" checked={telemetry} onChange={(e) => setTelemetry(e.target.checked)} disabled={!!busy} />
            <span>Include alarms, metrics and traces (a few cents of CloudWatch per scan)</span>
          </label>
          <button className="btn" onClick={() => run("scan")} disabled={!!busy}>
            {busy === "scan" ? "Scanning…" : "Scan account"}
          </button>
          {busy && step && (
            <p className="scan__progress" aria-live="polite">
              {step}
            </p>
          )}
          {!busy && <p className="muted small">Read-only. Secret values are never read.</p>}
        </>
      ) : (
        <p className="muted small">You are viewing a fictional demo account. Install the desktop app to map your own.</p>
      )}
      {warnings.length > 0 && (
        <details className="scan__warn">
          <summary>{warnings.length === 1 ? "1 part could not be read" : `${warnings.length} parts could not be read`}</summary>
          <ul>
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </details>
      )}
      <div className="scan__row">
        {desktop && fromScan && (
          <button className="btn btn--link" onClick={() => run("health")} disabled={!!busy}>
            {busy === "health" ? "Refreshing…" : "Refresh health"}
          </button>
        )}
        <button className="btn btn--link" onClick={() => file.current?.click()}>
          Open snapshot
        </button>
        <button className="btn btn--link" onClick={onDemo}>
          Load demo
        </button>
      </div>
      <input ref={file} type="file" accept="application/json,.json" hidden onChange={(e) => e.target.files?.[0] && open(e.target.files[0])} />
      {msg && (
        <p className="scan__msg" role="alert">
          {msg}
        </p>
      )}
    </div>
  );
}
