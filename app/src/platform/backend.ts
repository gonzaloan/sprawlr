// The port between the UI and whatever runs sprawlr: the desktop app (Rust, through Tauri)
// or the browser demo. Features depend on this interface, never on Tauri directly.
import type { DocFile, ScanResult, Snapshot } from "../domain";

export interface ScanRequest {
  profile: string | null;
  regions: string[];
  /** Alarms, metrics and traces. */
  health: boolean;
  /** Error lines from logs (Logs Insights). */
  errorLogs: boolean;
}

export interface Backend {
  /** What this backend can do; the UI hides what it cannot. */
  readonly can: { scan: boolean; saveDocsToFolder: boolean };
  listProfiles(): Promise<string[]>;
  scan(req: ScanRequest): Promise<ScanResult>;
  refreshHealth(profile: string | null, snapshot: Snapshot): Promise<ScanResult>;
  /** The snapshot to open on start, if one was kept. */
  lastSnapshot(): Promise<Snapshot | null>;
  /** Documentation pages for a snapshot; null when this backend cannot render them. */
  renderDocs(snapshot: Snapshot): Promise<DocFile[] | null>;
  /** Asks for a folder and writes every page there. Resolves to the folder, or null if cancelled. */
  saveDocs(snapshot: Snapshot): Promise<string | null>;
  /** Progress messages while a scan runs. Returns a function that stops listening. */
  onProgress(listener: (message: string) => void): Promise<() => void>;
}
