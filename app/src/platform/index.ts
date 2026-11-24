// Picks the backend for where the UI runs and hands it to features through React context,
// so tests and stories can pass their own.
import { createContext, useContext } from "react";
import type { Snapshot } from "../domain";
import demo from "../demo/demo-snapshot.json";
import type { Backend } from "./backend";
import { tauriBackend } from "./tauri";
import { webBackend } from "./web";

export type { Backend, ScanRequest } from "./backend";
export { download } from "./web";

/** The fictional demo account every build ships with. */
export const DEMO = demo as unknown as Snapshot;

export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const defaultBackend = (): Backend => (inTauri() ? tauriBackend : webBackend(DEMO));

export const BackendContext = createContext<Backend>(defaultBackend());

export const useBackend = () => useContext(BackendContext);
