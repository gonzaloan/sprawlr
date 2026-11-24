// Desktop adapter: every call goes to a Rust command (app/src-tauri/src/main.rs).
import type { DocFile, ScanResult, Snapshot } from "../domain";
import type { Backend, ScanRequest } from "./backend";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const tauriBackend: Backend = {
  can: { scan: true, saveDocsToFolder: true },
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
  async onProgress(listener) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<string>("scan-progress", (e) => listener(e.payload));
  },
};
