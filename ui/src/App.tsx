import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "./lib/useTauriEvent";
import { Icon, type IconName } from "./components/Icon";
import { startDownloadStore } from "./lib/downloadStore";
import HomeScreen from "./screens/HomeScreen";
import DictationScreen from "./screens/DictationScreen";
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
import DeveloperScreen from "./screens/DeveloperScreen";

type Screen =
  | "home"
  | "dictation"
  | "insights"
  | "transforms"
  | "autotext"
  | "dictionary"
  | "style"
  | "scratchpad"
  | "personalization"
  | "models"
  | "settings"
  | "developer";

const NAV_BASE: { id: Screen; label: string; icon: IconName }[] = [
  { id: "home", label: "Home", icon: "home" },
  { id: "dictation", label: "Dictation", icon: "dictation" },
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
  const [appIcon, setAppIcon] = useState<string>("white");
  const [developerTabEnabled, setDeveloperTabEnabled] = useState<boolean>(false);
  const [theme, setTheme] = useState<string>("system");
  const [reduceMotion, setReduceMotion] = useState<boolean>(false);
  // A transform skip/fallback toast shown at dictation time (P1-16 T7b).
  const [skipToast, setSkipToast] = useState<string | null>(null);

  // Apply the theme and motion preferences to <html>. "system" (the default)
  // removes the data-theme attribute so the OS media query decides; "light"/
  // "dark" force the palette. Reduce Motion, when on, sets data-motion so the
  // CSS near-instant rule applies; when off the OS preference still applies.
  useEffect(() => {
    const root = document.documentElement;
    if (theme === "system") root.removeAttribute("data-theme");
    else root.dataset.theme = theme;
    if (reduceMotion) root.dataset.motion = "reduced";
    else root.removeAttribute("data-motion");
  }, [theme, reduceMotion]);

  // Keep model-download-progress alive across screen unmounts.
  useEffect(() => {
    startDownloadStore();
  }, []);

  useTauriEvent<{ phase: string; text?: string }>("dictation-state", ({ payload }) => {
    setDictationState(payload.phase);
    // Surface a transform skip/fallback message as a toast. The backend sends
    // UiState::Message with the skip text; we show it for a few seconds.
    if (payload.phase === "message" && payload.text) {
      setSkipToast(payload.text);
      setTimeout(() => setSkipToast(null), 6000);
    }
  });

  useEffect(() => {
    invoke<{ hasCompletedOnboarding: boolean; appIcon: string; enableDeveloperTab?: boolean; theme?: string; reduceMotion?: boolean }>(
      "get_settings"
    )
      .then((s) => {
        setOnboardingDone(s.hasCompletedOnboarding ?? true);
        if (s.appIcon) setAppIcon(s.appIcon);
        setDeveloperTabEnabled(!!s.enableDeveloperTab);
        setTheme(s.theme || "system");
        setReduceMotion(!!s.reduceMotion);
      })
      .catch((e) => {
        console.error("[teletype] get_settings failed:", e);
        // Default to showing the app if settings can't be loaded.
      });
  }, []);

  // Re-fetch settings when the backend emits settings-changed (e.g. after
  // toggling "Show Developer tab" in Settings). Without this, the NAV array
  // is computed once on mount and never updates.
  useTauriEvent<void>("settings-changed", () => {
    invoke<{ hasCompletedOnboarding: boolean; appIcon: string; enableDeveloperTab?: boolean; theme?: string; reduceMotion?: boolean }>(
      "get_settings"
    )
      .then((s) => {
        setOnboardingDone(s.hasCompletedOnboarding ?? true);
        if (s.appIcon) setAppIcon(s.appIcon);
        setDeveloperTabEnabled(!!s.enableDeveloperTab);
        setTheme(s.theme || "system");
        setReduceMotion(!!s.reduceMotion);
      })
      .catch((e) => {
        console.error("[teletype] settings-changed refetch failed:", e);
      });
  });


  if (!onboardingDone) {
    return <OnboardingScreen onCompleted={() => setOnboardingDone(true)} />;
  }

  const listening = dictationState !== "idle";
  const NAV = developerTabEnabled
    ? [...NAV_BASE, { id: "developer" as Screen, label: "Developer", icon: "terminal" as IconName }]
    : NAV_BASE;

  return (
    <div style={{ display: "flex", height: "100vh", position: "relative" }}>
      {skipToast && (
        <div
          style={{
            position: "fixed",
            bottom: 24,
            left: "50%",
            transform: "translateX(-50%)",
            background: "rgba(30,30,30,0.92)",
            color: "#f5c518",
            fontSize: 13,
            fontWeight: 500,
            padding: "10px 18px",
            borderRadius: 8,
            boxShadow: "0 4px 16px rgba(0,0,0,0.25)",
            zIndex: 1000,
            animation: "fade-in 0.18s ease-out",
            maxWidth: 480,
            textAlign: "center",
          }}
        >
          {skipToast}
        </div>
      )}
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
            src={appIcon === "blue" ? "/teletype-app-icon-blue.png" : "/teletype-app-icon-white.png"}
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
          {/* Keep Models mounted so in-flight download UI state is not lost on tab switch. */}
          <div style={{ display: screen === "models" ? "block" : "none" }}>
            <ModelsScreen />
          </div>
          {screen === "home" && <HomeScreen listening={listening} />}
          {screen === "dictation" && <DictationScreen />}
          {screen === "insights" && <InsightsScreen />}
          {screen === "transforms" && <TransformsScreen />}
          {screen === "dictionary" && <DictionaryScreen />}
          {screen === "style" && <StylesScreen />}
          {screen === "scratchpad" && <ScratchpadScreen />}
          {screen === "autotext" && <AutoTextScreen />}
          {screen === "personalization" && <PersonalizationScreen />}
          {screen === "settings" && <SettingsScreen />}
          {screen === "developer" && <DeveloperScreen />}
        </main>
    </div>
  );
}

