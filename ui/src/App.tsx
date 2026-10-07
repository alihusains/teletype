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
import { useRuntimeStatus } from "./lib/runtimeStatus";

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

// Grouped sidebar (item 11): items are arranged into product sections so the
// user can find a feature by what it does, not by scrolling a flat list.
// The group label is hidden in the narrow (icon-only) layout.
type NavItem = { id: Screen; label: string; icon: IconName };
const NAV_GROUPS_BASE: { label: string; items: NavItem[] }[] = [
  {
    label: "App",
    items: [
      { id: "home", label: "Home", icon: "home" },
      { id: "insights", label: "Insights", icon: "insights" },
    ],
  },
  {
    label: "Record",
    items: [
      { id: "dictation", label: "Dictation", icon: "dictation" },
      { id: "scratchpad", label: "Scratchpad", icon: "scratchpad" },
    ],
  },
  {
    label: "Process",
    items: [
      { id: "transforms", label: "AI Polish", icon: "sparkles" },
      { id: "autotext", label: "AutoText", icon: "autotext" },
      { id: "dictionary", label: "Dictionary", icon: "dictionary" },
      { id: "style", label: "Style", icon: "style" },
    ],
  },
  {
    label: "You",
    items: [
      { id: "personalization", label: "Personalization", icon: "personalization" },
    ],
  },
  {
    label: "System",
    items: [
      { id: "models", label: "Models", icon: "models" },
      { id: "settings", label: "Settings", icon: "settings" },
    ],
  },
];

export default function App() {
  const [screen, setScreen] = useState<Screen>("home");
  const [dictationState, setDictationState] = useState<string>("idle");
  const [onboardingDone, setOnboardingDone] = useState<boolean>(true);
  const [appIcon, setAppIcon] = useState<string>("white");
  const [developerTabEnabled, setDeveloperTabEnabled] = useState<boolean>(false);
  const [theme, setTheme] = useState<string>("system");
  const [reduceMotion, setReduceMotion] = useState<boolean>(false);
  // Responsive: collapse the nav to icons-only when the window is narrow.
  const [narrow, setNarrow] = useState<boolean>(false);
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 760px)");
    setNarrow(mq.matches);
    const handler = (e: MediaQueryListEvent) => setNarrow(e.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  // A transform skip/fallback toast shown at dictation time (P1-16 T7b).
  const [skipToast, setSkipToast] = useState<string | null>(null);
  // Escape-recovery offer: a crashed run left a spool behind. The backend
  // emits "recovery-available" at startup, but it fires before this UI
  // mounts, so mount also polls recovery_status to catch it.
  const [recoverySeconds, setRecoverySeconds] = useState<number | null>(null);
  const [recoveredText, setRecoveredText] = useState<string | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  // Live model state for the nav footer. Polled coarsely and paused when the
  // window is hidden; see lib/runtimeStatus.tsx for why this is not an event.
  const runtime = useRuntimeStatus();

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
  useTauriEvent<{ path: string; seconds: number }>("recovery-available", ({ payload }) => {
    setRecoverySeconds(payload.seconds);
  });

  useEffect(() => {
    // Catch a spool the startup event fired before this UI subscribed.
    invoke<{ seconds: number; truncated: boolean } | null>("recovery_status")
      .then((s) => {
        if (s) setRecoverySeconds(s.seconds);
      })
      .catch(() => {});
  }, []);

  const recoverSpool = async () => {
    setRecoveryBusy(true);
    try {
      const text = await invoke<string>("recover_last_dictation");
      setRecoveredText(text);
    } catch (e) {
      setRecoveredText(`Recovery failed: ${String(e)}`);
    } finally {
      setRecoveryBusy(false);
    }
  };

  const discardSpool = async () => {
    await invoke("discard_recovery").catch(() => {});
    setRecoverySeconds(null);
    setRecoveredText(null);
  };

  // Auto-learn undo offer: the backend emits this when personalization
  // learns from an edit. The tray menu carries the same offer for when this
  // window is closed; the toast is the visible half.
  const [learnedOffer, setLearnedOffer] = useState<{ message: string; ids: string[] } | null>(null);
  useTauriEvent<{ message: string; ids: string[] }>("learned-preference", ({ payload }) => {
    setLearnedOffer(payload);
  });
  const undoLearned = async () => {
    if (!learnedOffer) return;
    await invoke("undo_learned", { ids: learnedOffer.ids }).catch(() => {});
    setLearnedOffer(null);
  };

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
  // Append the Developer group only when the tab is enabled, so the sidebar
  // stays honest about what is reachable.
  const navGroups: { label: string; items: NavItem[] }[] = developerTabEnabled
    ? [
        ...NAV_GROUPS_BASE,
        { label: "Debug", items: [{ id: "developer", label: "Developer", icon: "terminal" as IconName }] },
      ]
    : NAV_GROUPS_BASE;

  // The footer dot is the one piece of state visible from every screen, so it
  // has to be true at all times rather than only when it looks good. Dictation
  // phase wins when it is anything but idle, because that is what the user is
  // doing right now; otherwise the answer comes from the runtime.
  const navStatus = listening
    ? {
        label: dictationState,
        color: "var(--success)",
        title: `Dictation: ${dictationState}`,
      }
    : !runtime
      ? { label: "Checking…", color: "var(--text-tertiary)", title: "Checking what is loaded" }
      : runtime.speech.state === "missing" || runtime.speech.state === "none"
        ? {
            label: "Model needed",
            color: "var(--warning)",
            title: runtime.speech.detail,
          }
        : runtime.speech.state === "loading"
          ? {
              label: "Loading model…",
              color: "var(--accent)",
              title: runtime.speech.detail,
            }
          : {
              label: "Ready",
              color: "var(--success)",
              title: `${runtime.speech.label} is available. ${runtime.speech.detail}`,
            };

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
      {(recoverySeconds !== null || recoveredText !== null) && (        <div
          style={{
            position: "fixed",
            bottom: 24,
            left: "50%",
            transform: "translateX(-50%)",
            background: "rgba(30,30,30,0.92)",
            color: "#f5f5f5",
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
          role="alert"
        >
          {recoveredText === null ? (
            <>
              <div>
                Unfinished dictation (~{recoverySeconds}s) from a previous run. Recover it?
              </div>
              <div style={{ marginTop: 8, display: "flex", gap: 8, justifyContent: "center" }}>
                <button onClick={recoverSpool} disabled={recoveryBusy}>
                  {recoveryBusy ? "Recovering…" : "Recover"}
                </button>
                <button onClick={discardSpool} disabled={recoveryBusy}>
                  Discard
                </button>
              </div>
            </>
          ) : (
            <>
              <div style={{ maxHeight: 120, overflow: "auto", textAlign: "left" }}>{recoveredText}</div>
              <div style={{ marginTop: 4, fontSize: 12, color: "var(--text-secondary)" }}>
                Saved to History.
              </div>
              <div style={{ marginTop: 8 }}>
                <button onClick={discardSpool}>Dismiss</button>
              </div>
            </>
          )}
        </div>
      )}
      {learnedOffer && (
        <div
          style={{
            position: "fixed",
            bottom: 24,
            left: "50%",
            transform: "translateX(-50%)",
            background: "rgba(30,30,30,0.92)",
            color: "#f5f5f5",
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
          role="alert"
        >
          <div>{learnedOffer.message}</div>
          <div style={{ marginTop: 8, display: "flex", gap: 8, justifyContent: "center" }}>
            <button onClick={undoLearned}>Undo</button>
            <button onClick={() => setLearnedOffer(null)}>Keep</button>
          </div>
        </div>
      )}
      <nav
        style={{
          width: narrow ? 60 : 220,
          background: "var(--surface)",
          borderRight: "1px solid var(--border)",
          padding: narrow ? "16px 8px" : "20px 12px",
          display: "flex",
          flexDirection: "column",
          gap: 2,
          transition: "width 0.15s ease",
          overflow: "hidden",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: narrow ? "0 0 16px" : "0 10px 20px", justifyContent: narrow ? "center" : "flex-start" }}>
          <img
            src={appIcon === "blue" ? "/teletype-app-icon-blue.png" : "/teletype-app-icon-white.png"}
            alt=""
            width={30}
            height={30}
            style={{ borderRadius: 8 }}
          />
          {!narrow && <span style={{ fontWeight: 700, fontSize: 17, letterSpacing: -0.3, whiteSpace: "nowrap" }}>Teletype</span>}
        </div>
        {navGroups.map((group, gi) => (
          <div key={group.label} style={{ display: "flex", flexDirection: "column", gap: 2, marginTop: gi === 0 ? 0 : narrow ? 10 : 14 }}>
            {!narrow && (
              <div
                style={{
                  padding: "0 12px",
                  marginBottom: 4,
                  fontSize: 10.5,
                  fontWeight: 700,
                  letterSpacing: 0.8,
                  textTransform: "uppercase",
                  color: "var(--text-tertiary)",
                }}
              >
                {group.label}
              </div>
            )}
            {group.items.map((item) => {
              const active = screen === item.id;
              return (
                <button
                  key={item.id}
                  onClick={() => setScreen(item.id)}
                  title={item.label}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 12,
                    textAlign: "left",
                    padding: narrow ? "9px 0" : "9px 12px",
                    justifyContent: narrow ? "center" : "flex-start",
                    borderRadius: 8,
                    background: active ? "var(--accent-soft)" : "transparent",
                    color: active ? "var(--accent)" : "var(--text-secondary)",
                    fontWeight: active ? 600 : 500,
                    border: "none",
                    whiteSpace: "nowrap",
                  }}
                >
                  <Icon name={item.icon} size={19} />
                  {!narrow && item.label}
                </button>
              );
            })}
          </div>
        ))}
        <div style={{ flex: 1 }} />
        {/* Footer status. This used to read "Ready" whenever dictation was
            idle, which is a claim about the whole app and was true only when a
            model happened to be both selected and present. It now distinguishes
            the three states a user can act on: a model is needed, one is
            loading, or dictation is genuinely ready. */}
        <div
          style={{
            padding: narrow ? "8px 0" : "10px 12px",
            fontSize: 12,
            textAlign: narrow ? "center" : "left",
            color: navStatus.color,
            overflow: "hidden",
            textOverflow: "ellipsis",
            display: "flex",
            alignItems: "center",
            gap: 6,
            justifyContent: narrow ? "center" : "flex-start",
          }}
          title={navStatus.title}
        >
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: 999,
              flex: "0 0 auto",
              background: "currentColor",
            }}
          />
          {!narrow && navStatus.label}
        </div>
      </nav>
        <main style={{ flex: 1, overflow: "auto", padding: narrow ? 16 : 28 }}>
          {/* All screens stay mounted; visibility toggles via display. This
              preserves component state (fetched data, scroll position, form
              inputs) across tab switches — no re-fetch, no flash. */}
          <div style={{ display: screen === "home" ? "block" : "none" }}>
            <HomeScreen onNavigate={(s) => setScreen(s as Screen)} />
          </div>
          <div style={{ display: screen === "dictation" ? "block" : "none" }}>
            <DictationScreen />
          </div>
          <div style={{ display: screen === "insights" ? "block" : "none" }}>
            <InsightsScreen />
          </div>
          <div style={{ display: screen === "transforms" ? "block" : "none" }}>
            <TransformsScreen />
          </div>
          <div style={{ display: screen === "dictionary" ? "block" : "none" }}>
            <DictionaryScreen />
          </div>
          <div style={{ display: screen === "style" ? "block" : "none" }}>
            <StylesScreen />
          </div>
          <div style={{ display: screen === "scratchpad" ? "block" : "none" }}>
            <ScratchpadScreen />
          </div>
          <div style={{ display: screen === "autotext" ? "block" : "none" }}>
            <AutoTextScreen />
          </div>
          <div style={{ display: screen === "personalization" ? "block" : "none" }}>
            <PersonalizationScreen />
          </div>
          <div style={{ display: screen === "models" ? "block" : "none" }}>
            <ModelsScreen />
          </div>
          <div style={{ display: screen === "settings" ? "block" : "none" }}>
            <SettingsScreen active={screen === "settings"} />
          </div>
          <div style={{ display: screen === "developer" ? "block" : "none" }}>
            <DeveloperScreen />
          </div>
        </main>
    </div>
  );
}
