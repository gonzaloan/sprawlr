// The port between the UI and whatever runs sprawlr: the desktop app (Rust, through Tauri)
// or the browser demo. Features depend on this interface, never on Tauri directly.
import type { DocFile, ScanResult, Snapshot, Target } from "../domain";

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
  | { state: "sign_in_needed"; profile: string | null; command: string }
) & {
  /** When the next background refresh is due (ISO time), if one is scheduled. */
  next_at: string | null;
  /** Background refresh waits for a refresh that works after a sign-in failure. */
  paused: boolean;
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

export type SettingsInput = Pick<Settings, "background" | "notifications" | "launch_at_login" | "close_to_tray">;

type Unlisten = () => void;

export interface Backend {
  /** What this backend can do; the UI hides what it cannot. */
  readonly can: { scan: boolean; saveDocsToFolder: boolean; tray: boolean };
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
