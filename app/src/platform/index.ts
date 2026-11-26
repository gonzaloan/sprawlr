// Picks the backend for where the UI runs and hands it to features through React context,
// so tests and stories can pass their own.
import { createContext, useContext } from "react";
import type { Snapshot } from "../domain";
import demo from "../demo/demo-snapshot.json";
import { freshen } from "../demo/freshen";
import type { Backend } from "./backend";
import { tauriBackend } from "./tauri";
import { webBackend } from "./web";

export type { Backend, Every, RefreshState, ScanRequest, Settings, SettingsInput } from "./backend";
export { download } from "./web";

/** The fictional demo account every build ships with, read "a couple of minutes ago". */
export const DEMO = freshen(demo as unknown as Snapshot, new Date());

export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const defaultBackend = (): Backend => (inTauri() ? tauriBackend : webBackend(DEMO));

export const BackendContext = createContext<Backend>(defaultBackend());

export const useBackend = () => useContext(BackendContext);
