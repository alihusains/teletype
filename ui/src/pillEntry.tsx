// Entry point for the floating pill webview (ui/pill.html).
//
// The pill component lives in pill.tsx and is imported by SettingsScreen for
// its rainbow visuals. Mounting belongs here, not in the component module, so
// importing those visuals never creates a second root in the main window.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Pill } from "./pill";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Pill />
  </StrictMode>,
);
