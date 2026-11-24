import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/barlow/400.css";
import "@fontsource/barlow/500.css";
import "@fontsource/barlow/600.css";
import "@fontsource/barlow-condensed/500.css";
import "@fontsource/barlow-condensed/600.css";
import "@xyflow/react/dist/style.css";
import "./styles.css";
import App from "./app/App";
import { TrayPopover } from "./features/tray/TrayPopover";
import { defaultBackend } from "./platform";

// One bundle, two windows: the full app, or the tray popover (window "tray", or ?view=tray
// in a browser).
defaultBackend()
  .view()
  .then((view) => {
    if (view === "tray") document.body.classList.add("tray-body");
    createRoot(document.getElementById("root")!).render(<StrictMode>{view === "tray" ? <TrayPopover /> : <App />}</StrictMode>);
  });
