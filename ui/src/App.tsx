import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "./lib/useTauriEvent";
import { messageDialog } from "./lib/dialogs";
import { errorMessage } from "./lib/errors";
import { Icon, type IconName } from "./components/Icon";
import { startDownloadStore } from "./lib/downloadStore";
import HomeScreen from "./screens/HomeScreen";
import DictationScreen from "./screens/DictationScreen";
import InsightsScreen from "./screens/InsightsScreen";
import DictionaryScreen from "./screens/DictionaryScreen";
import StylesScreen from "./screens/StylesScreen";
import NotetakerScreen from "./screens/NotetakerScreen";
import TransformsScreen from "./screens/TransformsScreen";
import AutoTextScreen from "./screens/AutoTextScreen";
import PersonalizationScreen from "./screens/PersonalizationScreen";
import ModelsScreen from "./screens/ModelsScreen";
import SettingsScreen from "./screens/SettingsScreen";
import KeybindsScreen from "./screens/KeybindsScreen";
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
  | "notetaker"
  | "personalization"
  | "models"
  | "settings"
  | "keybinds"
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
      { id: "notetaker", label: "Notetaker", icon: "notetaker" },
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
      { id: "keybinds", label: "Keybinds", icon: "keyboard" },
      { id: "settings", label: "Settings", icon: "settings" },
    ],
  },
];

// The screen ids a `navigate` event may carry: every sidebar entry, plus the
// Developer tab, which only joins the nav when it is enabled.
const NAVIGABLE_SCREENS: Screen[] = [
  ...NAV_GROUPS_BASE.flatMap((group) => group.items.map((item) => item.id)),
  "developer",
];

// The sidebar shows whichever icon the user picked. The light tile is the default.
const APP_ICON_SRC: Record<string, string> = {
  white: "/teletype-app-icon-white.png",
  purple: "/teletype-app-icon-purple.png",
};

export default function App() {
  const [screen, setScreen] = useState<Screen>("home");  const [dictationState, setDictationState] = useState<string>("idle");
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
  const runtime = useRuntimeStatus().status;

  // A standing notice the backend set for itself (a migration, a degraded
  // fallback). It lives in settings so it survives a restart and stays until
  // the user acknowledges it — see dismiss_pending_notice.
  const [pendingNotice, setPendingNotice] = useState<string | null>(null);
  const dismissNotice = async () => {
    setPendingNotice(null);
    try {
      // No arguments: the backend clears settings.pendingNotice itself.
      await invoke("dismiss_pending_notice");
    } catch (e) {
      // The banner is informational; a missing or failing command must never
      // trap the user behind it. It stays cleared locally for this session.
      console.warn("[teletype] dismiss_pending_notice failed:", e);
    }
  };

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

  // Screen jumps that start outside this window: the tray menu's "Settings…"
  // emits `navigate` with a screen id (crates/teletype-desktop/src/tray.rs), as
  // does `open_main_window(route)`, and the cross-screen links in Settings emit
  // the same event. An unknown route is ignored rather than blanking the UI.
  useTauriEvent<string>("navigate", ({ payload }) => {
    if (NAVIGABLE_SCREENS.includes(payload as Screen)) setScreen(payload as Screen);
    else console.warn(`[teletype] navigate("${payload}"): unknown screen`);
  });

  useEffect(() => {
    invoke<{ hasCompletedOnboarding: boolean; appIcon: string; enableDeveloperTab?: boolean; theme?: string; reduceMotion?: boolean; pendingNotice?: string | null }>(
      "get_settings"
    )
      .then((s) => {
        setOnboardingDone(s.hasCompletedOnboarding ?? true);
        if (s.appIcon) setAppIcon(s.appIcon);
        setDeveloperTabEnabled(!!s.enableDeveloperTab);
        setTheme(s.theme || "system");
        setReduceMotion(!!s.reduceMotion);
        setPendingNotice(s.pendingNotice ?? null);
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
      setRecoveredText(`Recovery failed: ${errorMessage(e)}`);
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
    try {
      await invoke("undo_learned", { ids: learnedOffer.ids });
    } catch (e) {
      // Surface the failure instead of silently swallowing it — a dead Undo
      // button that looks working is worse than an honest error.
      console.error("[teletype] undo_learned failed:", e);
      void messageDialog(`Could not undo the learned preference: ${e}`, {
        title: "Undo failed",
        kind: "error",
      });
    }
    setLearnedOffer(null);
  };

  useTauriEvent<void>("settings-changed", () => {
    invoke<{ hasCompletedOnboarding: boolean; appIcon: string; enableDeveloperTab?: boolean; theme?: string; reduceMotion?: boolean; pendingNotice?: string | null }>(
      "get_settings"
    )
      .then((s) => {
        setOnboardingDone(s.hasCompletedOnboarding ?? true);
        if (s.appIcon) setAppIcon(s.appIcon);
        setDeveloperTabEnabled(!!s.enableDeveloperTab);
        setTheme(s.theme || "system");
        setReduceMotion(!!s.reduceMotion);
        setPendingNotice(s.pendingNotice ?? null);
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
          width: narrow ? 60 : 232,
          background: "var(--surface)",
          borderRight: "1px solid var(--border)",
          padding: narrow ? "16px 8px" : "18px 12px 14px",
          display: "flex",
          flexDirection: "column",
          transition: "width 0.15s ease",
          overflow: "hidden",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: narrow ? "0 0 16px" : "0 10px 18px", justifyContent: narrow ? "center" : "flex-start" }}>
          <img
            src={APP_ICON_SRC[appIcon] ?? APP_ICON_SRC.white}
            alt=""
            width={30}
            height={30}
            style={{ borderRadius: 8 }}
          />
          {!narrow && <span style={{ fontWeight: 700, fontSize: 18, letterSpacing: -0.3, whiteSpace: "nowrap" }}>Teletype</span>}
        </div>
        {/* The list scrolls and the footer status does not. With 15 destinations
            the last group used to be pushed off the bottom of a normal-height
            window with no way to see that more existed. */}
        <div
          className="tt-scroll-thin"
          style={{
            flex: 1,
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            gap: 2,
            paddingRight: 6,
            marginRight: -2,
          }}
        >
          {navGroups.map((group, gi) => (
            <div key={group.label} style={{ display: "flex", flexDirection: "column", gap: 2, marginTop: gi === 0 ? 0 : narrow ? 10 : 16 }}>
              {!narrow && (
                <div
                  style={{
                    padding: "0 12px",
                    marginBottom: 5,
                    fontSize: 11,
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
                    aria-current={active ? "page" : undefined}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 12,
                      textAlign: "left",
                      padding: narrow ? "10px 0" : "10px 12px",
                      justifyContent: narrow ? "center" : "flex-start",
                      borderRadius: 9,
                      background: active ? "var(--accent-soft)" : "transparent",
                      color: active ? "var(--accent)" : "var(--text-secondary)",
                      fontSize: 15,
                      fontWeight: active ? 600 : 500,
                      border: "none",
                      whiteSpace: "nowrap",
                    }}
                  >
                    <Icon name={item.icon} size={20} />
                    {!narrow && item.label}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
        {/* Footer status. This used to read "Ready" whenever dictation was
            idle, which is a claim about the whole app and was true only when a
            model happened to be both selected and present. It now distinguishes
            the three states a user can act on: a model is needed, one is
            loading, or dictation is genuinely ready. */}
        <div
          style={{
            padding: narrow ? "8px 0" : "12px 12px 2px",
            marginTop: narrow ? 8 : 10,
            borderTop: narrow ? "none" : "1px solid var(--border-subtle)",
            fontSize: 13,
            textAlign: narrow ? "center" : "left",
            color: navStatus.color,
            overflow: "hidden",
            textOverflow: "ellipsis",
            display: "flex",
            alignItems: "center",
            gap: 7,
            flex: "0 0 auto",
            justifyContent: narrow ? "center" : "flex-start",
          }}
          title={navStatus.title}
        >
          <span
            aria-hidden
            style={{
              width: 7,
              height: 7,
              borderRadius: 999,
              flex: "0 0 auto",
              background: "currentColor",
            }}
          />
          {!narrow && navStatus.label}
        </div>
      </nav>
        <main style={{ flex: 1, overflow: "auto", padding: narrow ? 16 : 32 }}>
          {/* A standing notice from the backend (see settings.pendingNotice).
              Dismissing it clears the banner; the command that clears it on the
              backend is allowed to fail without trapping the user here. */}
          {pendingNotice && (
            <div
              role="status"
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 12,
                marginBottom: 18,
                padding: "12px 16px",
                borderRadius: "var(--radius)",
                background: "var(--warning-soft)",
                border: "1px solid color-mix(in srgb, var(--warning) 30%, transparent)",
                fontSize: 13,
                lineHeight: 1.5,
              }}
            >
              <span style={{ color: "var(--warning)", display: "grid", placeItems: "center", flex: "0 0 auto" }}>
                <Icon name="alert" size={16} />
              </span>
              <span style={{ flex: 1, minWidth: 0 }}>{pendingNotice}</span>
              <button
                onClick={dismissNotice}
                style={{
                  flex: "0 0 auto",
                  padding: "4px 10px",
                  fontSize: 12,
                  fontWeight: 600,
                  color: "var(--text)",
                  background: "transparent",
                  border: "1px solid var(--border)",
                  borderRadius: "var(--radius-sm)",
                  cursor: "pointer",
                }}
              >
                Dismiss
              </button>
            </div>
          )}
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
            <TransformsScreen onNavigate={(s) => setScreen(s as Screen)} />
          </div>
          <div style={{ display: screen === "dictionary" ? "block" : "none" }}>
            <DictionaryScreen />
          </div>
          <div style={{ display: screen === "style" ? "block" : "none" }}>
            <StylesScreen />
          </div>
          <div style={{ display: screen === "notetaker" ? "block" : "none" }}>
            <NotetakerScreen />
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
          <div style={{ display: screen === "keybinds" ? "block" : "none" }}>
            <KeybindsScreen />
          </div>
          <div style={{ display: screen === "developer" ? "block" : "none" }}>
            <DeveloperScreen />
          </div>
        </main>
    </div>
  );
}
