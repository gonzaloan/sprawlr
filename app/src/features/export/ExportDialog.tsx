import { useEffect, useRef, useState } from "react";
import { exportError, type ExportScope, type Snapshot } from "../../domain";
import { useBackend } from "../../platform";
import type { ExportResult } from "../../platform/backend";
import { ExportOutcome } from "./ExportOutcome";

const EXAMPLE = "https://github.com/gonzaloan/sprawlr/tree/main/examples/demo-export";

interface Props {
  scope: ExportScope;
  snapshot: Snapshot;
  /** The demo account: its configuration comes with the app, not from AWS. */
  demo: boolean;
  onClose: () => void;
}

/** Export the account or an app as Terraform and CloudFormation, with the documentation. */
export function ExportDialog({ scope, snapshot, demo, onClose }: Props) {
  const backend = useBackend();
  const desktop = backend.can.exportCode;
  const dialog = useRef<HTMLDialogElement>(null);
  const [profiles, setProfiles] = useState<string[]>([]);
  const [profile, setProfile] = useState("");
  const [busy, setBusy] = useState(false);
  const [step, setStep] = useState<string | null>(null);
  const [result, setResult] = useState<ExportResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const d = dialog.current;
    if (d && !d.open) d.showModal();
  }, []);

  // The profile of the last scan, so the export reads the account on screen.
  useEffect(() => {
    if (!desktop || demo) return;
    Promise.all([backend.listProfiles(), backend.settings()])
      .then(([list, settings]) => {
        setProfiles(list);
        setProfile(settings?.profile ?? (list.includes("default") ? "default" : list[0] ?? ""));
      })
      .catch((e) => setError(String(e)));
  }, [backend, desktop, demo]);

  useEffect(() => {
    if (!busy) return;
    let stop: (() => void) | undefined;
    let done = false;
    backend.onExportProgress(setStep).then((u) => (done ? u() : (stop = u)));
    return () => {
      done = true;
      stop?.();
    };
  }, [backend, busy]);

  const run = async () => {
    setBusy(true);
    setError(null);
    setStep(null);
    try {
      const r = await backend.exportCode({ snapshot, app: scope.app, profile: demo ? null : profile || null, demo });
      if (r) setResult(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setStep(null);
    }
  };

  const close = () => dialog.current?.close();

  return (
    <dialog ref={dialog} className="export" aria-labelledby="export-title" onClose={onClose} onCancel={(e) => busy && e.preventDefault()}>
      <h2 id="export-title">Export {scope.title} as code</h2>
      {result ? (
        <ExportOutcome result={result} />
      ) : desktop ? (
        <>
          <p>
            Writes a folder with the documentation, a Terraform project (awscc provider) and CloudFormation templates for {scope.title}, with an import for each
            resource, so you can check the code against the account before you use it.
          </p>
          <p>
            {demo
              ? "Reads the demo account's configuration."
              : "Reads each resource's configuration again, through Cloud Control. Cloud Control adds no charge; Secrets Manager and KMS bill their own reads by the request, a fraction of a cent for an account."}{" "}
            It covers every environment, whichever one the map shows.
          </p>
          <p className="muted">
            Values that could be secrets are never written: environment variable values, passwords, tokens and user data become inputs you fill in. Policy
            documents are written as AWS returns them, so keep the folder where you keep code.
          </p>
          {!demo && (
            <label className="export__field">
              AWS profile
              <select value={profile} onChange={(e) => setProfile(e.target.value)} disabled={busy}>
                <option value="">Default credentials</option>
                {profiles.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </label>
          )}
          {busy && (
            <p className="scan__progress" aria-live="polite">
              {step ?? "Starting"}
            </p>
          )}
          {error && (
            <p className="scan__msg" role="alert">
              {exportError(error, demo)}
            </p>
          )}
        </>
      ) : (
        <>
          <p>
            Export runs in the desktop app: it reads each resource's configuration from your account and writes Terraform, CloudFormation and the documentation
            to a folder. Values that could be secrets are never written.
          </p>
          <p>
            <a href={EXAMPLE} target="_blank" rel="noreferrer">
              See what it writes for this demo account
            </a>
            .
          </p>
        </>
      )}
      <div className="export__actions">
        {desktop && !result && (
          <button className="btn" onClick={run} disabled={busy}>
            {busy ? "Exporting" : "Choose a folder and export"}
          </button>
        )}
        <button className="btn btn--quiet" onClick={close} disabled={busy}>
          {result || !desktop ? "Done" : "Cancel"}
        </button>
      </div>
    </dialog>
  );
}
