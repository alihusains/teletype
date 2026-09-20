import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "./lib/useTauriEvent";
import { Icon, type IconName } from "./components/Icon";
import HomeScreen from "./screens/HomeScreen";
import DictationScreen from "./screens/DictationScreen";
import DashboardScreen from "./screens/DashboardScreen";
import InsightsScreen from "./screens/InsightsScreen";
import DictionaryScreen from "./screens/DictionaryScreen";
import StylesScreen from "./screens/StylesScreen";
import ScratchpadScreen from "./screens/ScratchpadScreen";
import TransformsScreen from "./screens/TransformsScreen";
import AutoTextScreen from "./screens/AutoTextScreen";
import PersonalizationScreen from "./screens/PersonalizationScreen";
import ModelsScreen from "./screens/ModelsScreen";
import SettingsScreen from "./screens/SettingsScreen";
import OnboardingScreen from "./screens/OnboardingScreen";

type Screen =
  | "home"
  | "dictation"
  | "dashboard"
  | "insights"
  | "transforms"
  | "autotext"
  | "dictionary"
  | "style"
  | "scratchpad"
  | "personalization"
  | "models"
  | "settings";

const NAV: { id: Screen; label: string; icon: IconName }[] = [
  { id: "home", label: "Home", icon: "home" },
  { id: "dictation", label: "Dictation", icon: "dictation" },
  { id: "dashboard", label: "Dashboard", icon: "dashboard" },
  { id: "insights", label: "Insights", icon: "insights" },
  { id: "transforms", label: "Transforms", icon: "transforms" },
  { id: "autotext", label: "AutoText", icon: "autotext" },
  { id: "dictionary", label: "Dictionary", icon: "dictionary" },
  { id: "style", label: "Style", icon: "style" },
  { id: "scratchpad", label: "Scratchpad", icon: "scratchpad" },
  { id: "personalization", label: "Personalization", icon: "personalization" },
  { id: "models", label: "Models", icon: "models" },
  { id: "settings", label: "Settings", icon: "settings" },
];

export default function App() {
  const [screen, setScreen] = useState<Screen>("home");
  const [dictationState, setDictationState] = useState<string>("idle");
  const [onboardingDone, setOnboardingDone] = useState<boolean>(true);

  useTauriEvent<{ phase: string }>("dictation-state", ({ payload }) => {
    setDictationState(payload.phase);
  });

  useEffect(() => {
    invoke<{ hasCompletedOnboarding: boolean }>("get_settings")
      .then((s) => {
        console.log("[teletype] get_settings resolved:", JSON.stringify(s));
        setOnboardingDone(s.hasCompletedOnboarding ?? true);
      })
      .catch((e) => {
        console.error("[teletype] get_settings failed:", e);
        // Default to showing the app if settings can't be loaded.
      });
  }, []);

  if (!onboardingDone) {
    return <OnboardingScreen onCompleted={() => setOnboardingDone(true)} />;
  }

  const listening = dictationState !== "idle";

  return (
    <div style={{ display: "flex", height: "100vh" }}>
      <nav
        style={{
          width: 220,
          background: "var(--surface)",
          borderRight: "1px solid var(--border)",
          padding: "20px 12px",
          display: "flex",
          flexDirection: "column",
          gap: 2,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "0 10px 20px" }}>
          <img
            src="/teletype-icon.png"
            alt=""
            width={30}
            height={30}
            style={{ borderRadius: 8 }}
          />
          <span style={{ fontWeight: 700, fontSize: 17, letterSpacing: -0.3 }}>Teletype</span>
        </div>
        {NAV.map((item) => {
          const active = screen === item.id;
          return (
            <button
              key={item.id}
              onClick={() => setScreen(item.id)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                textAlign: "left",
                padding: "9px 12px",
                borderRadius: 8,
                background: active ? "var(--accent-soft)" : "transparent",
                color: active ? "var(--accent)" : "var(--text-secondary)",
                fontWeight: active ? 600 : 500,
                border: "none",
              }}
            >
              <Icon name={item.icon} size={19} />
              {item.label}
            </button>
          );
        })}
        <div style={{ flex: 1 }} />
        <div
          style={{
            padding: "10px 12px",
            fontSize: 12,
            color: listening ? "var(--success)" : "var(--text-secondary)",
          }}
        >
          {listening ? `● ${dictationState}` : "Ready"}
        </div>
      </nav>
      <main style={{ flex: 1, overflow: "auto", padding: 28 }}>
        {screen === "home" && <HomeScreen onNavigate={setScreen} listening={listening} />}
        {screen === "dictation" && <DictationScreen />}
        {screen === "dashboard" && <DashboardScreen />}
        {screen === "insights" && <InsightsScreen />}
        {screen === "transforms" && <TransformsScreen />}
        {screen === "dictionary" && <DictionaryScreen />}
        {screen === "style" && <StylesScreen />}
        {screen === "scratchpad" && <ScratchpadScreen />}
        {screen === "autotext" && <AutoTextScreen />}
        {screen === "personalization" && <PersonalizationScreen />}
        {screen === "models" && <ModelsScreen />}
        {screen === "settings" && <SettingsScreen />}
      </main>
    </div>
  );
}

