import { listen } from "@tauri-apps/api/event";

const dot = document.getElementById("dot") as HTMLElement;
const label = document.getElementById("label") as HTMLElement;

listen<{ phase: string }>("pill-state", (e) => {
  const { visible, text } = e.payload as unknown as { visible: boolean; text: string };
  const pill = document.getElementById("pill")!;
  pill.style.display = visible ? "flex" : "none";
  if (text) label.textContent = text;
  dot.className = "dot" + (text?.includes("Listening") ? " recording" : "");
});
