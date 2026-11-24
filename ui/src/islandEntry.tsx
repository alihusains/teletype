// Entry point for the meeting island webview (ui/island.html).
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Island } from "./island";

const root = createRoot(document.getElementById("root")!);
root.render(
  <StrictMode>
    <Island />
  </StrictMode>
);
