// Browser adapter for the public demo: no AWS, no file system, no tray. Documentation for
// the demo account is precomputed by the CLI (`sprawlr docs --json`). `?view=tray` previews
// the tray popover on the demo account.
import type { DocFile, Snapshot } from "../domain";
import demoDocs from "../demo/demo-docs.json";
import type { Backend, RefreshState } from "./backend";

const desktopOnly = (what: string) => () => Promise.reject(new Error(`${what} needs the desktop app.`));
const never = async () => () => {};
const IDLE: RefreshState = { state: "idle", next_at: null, paused: false };

export function webBackend(demo: Snapshot): Backend {
  return {
    can: { scan: false, saveDocsToFolder: false, tray: false },
    view: async () => (new URLSearchParams(window.location.search).get("view") === "tray" ? "tray" : "main"),
    listProfiles: async () => [],
    scan: desktopOnly("Scanning an account"),
    refreshHealth: desktopOnly("Refreshing"),
    // The tray preview reads the demo as if it were the stored scan.
    lastSnapshot: async () => (new URLSearchParams(window.location.search).get("view") === "tray" ? demo : null),
    renderDocs: async (snapshot) => (snapshot === demo ? (demoDocs as DocFile[]) : null),
    saveDocs: async () => null,
    onProgress: never,
    refreshStored: desktopOnly("Refreshing"),
    refreshState: async () => IDLE,
    onRefreshState: never,
    onSnapshotChanged: never,
    onOpenTarget: never,
    onPopoverShown: never,
    openMain: async () => {},
    hidePopover: async () => {},
    settings: async () => null,
    saveSettings: desktopOnly("Settings"),
  };
}

/** Browser download of one page (the web demo's way of saving). */
export function download(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/markdown" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
