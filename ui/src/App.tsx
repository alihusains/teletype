import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import TransformsScreen from "./screens/TransformsScreen";
import AutoTextScreen from "./screens/AutoTextScreen";
import PersonalizationScreen from "./screens/PersonalizationScreen";
import ModelsScreen from "./screens/ModelsScreen";
import SettingsScreen from "./screens/SettingsScreen";

type Screen = "transforms" | "autotext" | "personalization" | "models" | "settings";

const NAV: { id: Screen; label: string }[] = [
  { id: "transforms", label: "Transforms" },
  { id: "autotext", label: "AutoText" },
  { id: "personalization", label: "Personalization" },
  { id: "models", label: "Models" },
  { id: "settings", label: "Settings" },
];

export default function App() {
  const [screen, setScreen] = useState<Screen>("transforms");
  const [dictationState, setDictationState] = useState<string>("idle");

  useEffect(() => {
    const unlisten = listen<{ phase: string }>("dictation-state", (e) => {
      setDictationState(e.payload.phase);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  return (
    <div style={{ display: "flex", height: "100vh" }}>
      <nav style={{
        width: 200,
        background: "var(--surface)",
        borderRight: "1px solid var(--border)",
        padding: "16px 0",
        display: "flex",
        flexDirection: "column",
      }}>
        <div style={{ padding: "0 16px 16px", fontWeight: 600, fontSize: 15 }}>
          Teletype
        </div>
        {NAV.map((item) => (
          <button
            key={item.id}
            onClick={() => setScreen(item.id)}
            style={{
              textAlign: "left",
              padding: "8px 16px",
              borderRadius: 0,
              background: screen === item.id ? "var(--surface-hover)" : "transparent",
              color: screen === item.id ? "var(--text)" : "var(--text-secondary)",
              border: "none",
            }}
          >
            {item.label}
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <div style={{ padding: "8px 16px", fontSize: 12, color: "var(--text-secondary)" }}>
          {dictationState !== "idle" ? `● ${dictationState}` : "Ready"}
        </div>
      </nav>
      <main style={{ flex: 1, overflow: "auto", padding: 24 }}>
        {screen === "transforms" && <TransformsScreen />}
        {screen === "autotext" && <AutoTextScreen />}
        {screen === "personalization" && <PersonalizationScreen />}
        {screen === "models" && <ModelsScreen />}
        {screen === "settings" && <SettingsScreen />}
      </main>
    </div>
  );
}
