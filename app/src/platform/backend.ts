// The port between the UI and whatever runs sprawlr: the desktop app (Rust, through Tauri)
// or the browser demo. Features depend on this interface, never on Tauri directly.
import type { DocFile, ExportSummary, ScanResult, Snapshot, Target } from "../domain";

export interface ScanRequest {
  profile: string | null;
  regions: string[];
  /** Alarms, metrics and traces. */
  health: boolean;
  /** Error lines from logs (Logs Insights). */
  errorLogs: boolean;
}

/** What the last or current health refresh is doing (desktop). */
export type RefreshState = (
  | { state: "idle" }
  | { state: "running"; step: string }
  | { state: "failed"; message: string }
  | { state: "sign_in_needed"; profile: string | null; hint: string }
) & {
  /** When the next background refresh is due (ISO time), if one is scheduled. */
  next_at: string | null;
  /** Background refresh is on. */
  background: boolean;
  /** Background refresh waits for a read that works, after a sign-in failure. */
  paused: boolean;
  /** A refresh of the stored scan reads error logs again (Logs Insights, billed per GB). */
  reads_error_logs: boolean;
  /** Parts of the last read that could not be read. */
  warnings: number;
};

export type Every = "15m" | "1h" | "4h";

export interface Settings {
  background: Every | null;
  notifications: boolean;
  launch_at_login: boolean;
  close_to_tray: boolean;
  /** The profile of the last scan or refresh (null: default credentials). */
  profile: string | null;
  /** "menu bar" or "system tray". */
  place: string;
  /** What the person should know about the last change (a permission the OS refused). */
  notice: string | null;
}

export interface ExportRequest {
  snapshot: Snapshot;
  /** null: the whole account. */
  app: string | null;
  profile: string | null;
  /** Read the demo account's configuration instead of AWS. */
  demo: boolean;
}

export interface ExportResult {
  dir: string;
  summary: ExportSummary;
}

export type SettingsInput = Pick<Settings, "background" | "notifications" | "launch_at_login" | "close_to_tray">;

type Unlisten = () => void;

export interface Backend {
  /** What this backend can do; the UI hides what it cannot. */
  readonly can: {
    scan: boolean;
    saveDocsToFolder: boolean;
    /** Export as code: reads each resource's configuration again and writes a folder. */
    exportCode: boolean;
    tray: boolean;
    /** Back and Forward from the keyboard and mouse must be handled by the UI (a browser does it itself). */
    historyKeys: boolean;
  };
  /** Which window this UI is: the full app, or the tray popover. */
  view(): Promise<"main" | "tray">;
  listProfiles(): Promise<string[]>;
  scan(req: ScanRequest): Promise<ScanResult>;
  refreshHealth(profile: string | null, snapshot: Snapshot): Promise<ScanResult>;
  /** The snapshot to open on start, if one was kept. */
  lastSnapshot(): Promise<Snapshot | null>;
  /** Documentation pages for a snapshot; null when this backend cannot render them. */
  renderDocs(snapshot: Snapshot): Promise<DocFile[] | null>;
  /** Asks for a folder and writes every page there. Resolves to the folder, or null if cancelled. */
  saveDocs(snapshot: Snapshot): Promise<string | null>;
  /** Asks for a folder, reads each resource's configuration and writes the code there.
   *  Resolves to the folder and what was written, or null if cancelled. */
  exportCode(req: ExportRequest): Promise<ExportResult | null>;
  onExportProgress(listener: (message: string) => void): Promise<Unlisten>;
  /** Progress messages while a scan runs. Returns a function that stops listening. */
  onProgress(listener: (message: string) => void): Promise<Unlisten>;

  // ---- tray (desktop)
  /** Refresh health for the stored scan with the last profile used. */
  refreshStored(): Promise<void>;
  refreshState(): Promise<RefreshState>;
  onRefreshState(listener: (s: RefreshState) => void): Promise<Unlisten>;
  /** A scan or refresh saved a new snapshot (from any window, or in the background). */
  onSnapshotChanged(listener: (s: Snapshot) => void): Promise<Unlisten>;
  /** The popover asked the main window to open an item. */
  onOpenTarget(listener: (t: Target) => void): Promise<Unlisten>;
  /** The popover was shown (refocus and re-read time). */
  onPopoverShown(listener: () => void): Promise<Unlisten>;
  openMain(target?: Target): Promise<void>;
  hidePopover(): Promise<void>;
  settings(): Promise<Settings | null>;
  saveSettings(s: SettingsInput): Promise<Settings>;
}
