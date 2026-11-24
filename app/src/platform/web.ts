// Browser adapter for the public demo: no AWS, no file system. Documentation for the demo
// account is precomputed by the CLI (`sprawlr docs --json`).
import type { DocFile, Snapshot } from "../domain";
import demoDocs from "../demo/demo-docs.json";
import type { Backend } from "./backend";

const desktopOnly = () => Promise.reject(new Error("Scanning an account needs the desktop app."));

export function webBackend(demo: Snapshot): Backend {
  return {
    can: { scan: false, saveDocsToFolder: false },
    listProfiles: async () => [],
    scan: desktopOnly,
    refreshHealth: desktopOnly,
    lastSnapshot: async () => null,
    renderDocs: async (snapshot) => (snapshot === demo ? (demoDocs as DocFile[]) : null),
    saveDocs: async () => null,
    onProgress: async () => () => {},
  };
}

/** Browser download of one page (the web demo's way of saving). */
export function download(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/markdown" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
