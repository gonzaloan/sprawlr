// Desktop adapter: every call goes to a Rust command (app/src-tauri/src/commands.rs).
import type { DocFile, ExportSummary, ScanResult, Snapshot, Target } from "../domain";
import type { Backend, ExportRequest, RefreshState, ScanRequest, Settings, SettingsInput } from "./backend";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function on<T>(event: string, listener: (payload: T) => void) {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => listener(e.payload));
}

export const tauriBackend: Backend = {
  can: { scan: true, saveDocsToFolder: true, exportCode: true, tray: true, historyKeys: true },
  async view() {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return getCurrentWindow().label === "tray" ? "tray" : "main";
  },
  listProfiles: () => call<string[]>("list_profiles"),
  scan: (r: ScanRequest) => call<ScanResult>("scan", { profile: r.profile, regions: r.regions, telemetry: r.health, logs: r.errorLogs }),
  refreshHealth: (profile, snapshot) => call<ScanResult>("refresh_health", { profile, snapshot }),
  lastSnapshot: () => call<Snapshot | null>("last_snapshot"),
  renderDocs: (snapshot) => call<DocFile[]>("render_docs", { snapshot }),
  async saveDocs(snapshot) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, multiple: false, title: "Save documentation to" });
    if (typeof dir !== "string") return null;
    await call<number>("save_docs", { snapshot, dir });
    return dir;
  },
  async exportCode(r: ExportRequest) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, multiple: false, title: "Export as code to" });
    if (typeof dir !== "string") return null;
    const summary = await call<ExportSummary>("export_code", { snapshot: r.snapshot, appName: r.app, profile: r.profile, demo: r.demo, dir });
    return { dir, summary };
  },
  onExportProgress: (listener) => on<string>("export-progress", listener),
  onProgress: (listener) => on<string>("scan-progress", listener),

  refreshStored: () => call<void>("refresh_stored"),
  refreshState: () => call<RefreshState>("refresh_state"),
  onRefreshState: (listener) => on<RefreshState>("refresh-state", listener),
  onSnapshotChanged: (listener) => on<Snapshot>("snapshot-changed", listener),
  onOpenTarget: (listener) => on<Target>("open-target", listener),
  onPopoverShown: (listener) => on<null>("popover-shown", () => listener()),
  openMain: (target) => call<void>("open_main", { target: target ?? null }),
  hidePopover: () => call<void>("hide_popover"),
  settings: () => call<Settings>("get_settings"),
  saveSettings: (input: SettingsInput) => call<Settings>("save_settings", { input }),
};
