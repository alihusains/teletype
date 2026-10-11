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
  | "scratchpad"
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
      { id: "keybinds", label: "Keybinds", icon: "keyboard" },
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
    try {
      await invoke("undo_learned", { ids: learnedOffer.ids });
    } catch (e) {
      // Surface the failure instead of silently swallowing it — a dead Undo
      // button that looks working is worse than an honest error.
      console.error("[teletype] undo_learned failed:", e);
      alert(`Could not undo the learned preference: ${e}`);
    }
    setLearnedOffer(null);
  };

  // Polish vocabulary candidates: the backend publishes up to three
  // "Teletype noticed…" suggestions after a Polish run. The user explicitly
  // adds one to the dictionary or dismisses it; nothing is written
  // automatically. The panel is dismissed only after the backend confirms.
  interface PolishCandidate {
    id: string;
    heard: string;
    word: string;
    reason: string;
  }
  const [polishCandidates, setPolishCandidates] = useState<PolishCandidate[]>([]);
  const [candidateBusy, setCandidateBusy] = useState<string | null>(null);

  const loadPolishCandidates = () => {
    invoke<PolishCandidate[]>("list_polish_candidates")
      .then(setPolishCandidates)
      .catch(console.error);
  };
  useEffect(loadPolishCandidates, []);
  // New candidates are published from the dictation worker after a Polish
  // run; the event carries the newly-added rows so the panel appears
  // without waiting for a manual refresh.
  useTauriEvent<PolishCandidate[]>("polish-candidates", ({ payload }) => {
    setPolishCandidates((prev) => {
      const seen = new Set(prev.map((c) => c.id));
      const added = payload.filter((c) => !seen.has(c.id));
      return added.length ? [...prev, ...added].slice(-3) : prev;
    });
  });
  useTauriEvent<{ id: string; resolution: string }>("polish-candidate-resolved", ({ payload }) => {
    setPolishCandidates((prev) => prev.filter((c) => c.id !== payload.id));
  });

  const acceptCandidate = async (c: PolishCandidate) => {
    setCandidateBusy(c.id);
    try {
      await invoke("accept_polish_candidate", { id: c.id });
      setPolishCandidates((prev) => prev.filter((x) => x.id !== c.id));
    } catch (e) {
      // Keep the candidate visible on failure — an honest error, not a
      // silently-dropped suggestion.
      console.error("[teletype] accept_polish_candidate failed:", e);
      alert(`Could not add \u201c${c.word}\u201d to your dictionary: ${e}`);
    } finally {
      setCandidateBusy(null);
    }
  };

  const dismissCandidate = async (c: PolishCandidate) => {
    setCandidateBusy(c.id);
    try {
      await invoke("dismiss_polish_candidate", { id: c.id });
      setPolishCandidates((prev) => prev.filter((x) => x.id !== c.id));
    } catch (e) {
      console.error("[teletype] dismiss_polish_candidate failed:", e);
    } finally {
      setCandidateBusy(null);
    }
  };

  // Quick Add confirmation: the global shortcut captures the selection
  // synchronously (so it can't be corrupted) and emits `quick-add-requested`.
  // Nothing is written to the dictionary until the user confirms here — the
  // word is editable, and a busy dictation is refused with a visible message.
  const [quickAdd, setQuickAdd] = useState<{ selection: string | null } | null>(null);
  const [quickAddWord, setQuickAddWord] = useState("");
  const [quickAddHeard, setQuickAddHeard] = useState("");
  const [quickAddBusy, setQuickAddBusy] = useState(false);
  const [quickAddError, setQuickAddError] = useState("");

  useTauriEvent<{ selection: string | null }>("quick-add-requested", ({ payload }) => {
    const sel = payload.selection?.trim() ?? "";
    const first = sel.split(/\s+/).filter(Boolean)[0] ?? "";
    setQuickAddError("");
    setQuickAddWord(first);
    setQuickAddHeard(sel);
    setQuickAdd({ selection: sel || null });
  });

  const closeQuickAdd = () => {
    setQuickAdd(null);
    setQuickAddError("");
  };

  const confirmQuickAdd = async () => {
    const w = quickAddWord.trim();
    if (!w) {
      setQuickAddError("Type the word to add.");
      return;
    }
    setQuickAddBusy(true);
    setQuickAddError("");
    try {
      await invoke("quick_add_word", { word: w, heard: quickAddHeard.trim() || null });
      closeQuickAdd();
    } catch (e) {
      // Surface the refusal (busy dictation, invalid word, …) instead of
      // failing silently.
      setQuickAddError(String(e));
    } finally {
      setQuickAddBusy(false);
    }
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
      {polishCandidates.length > 0 && (
        <div
          role="region"
          aria-label="Words Teletype noticed"
          style={{
            position: "fixed",
            bottom: 24,
            right: 24,
            width: 320,
            background: "var(--surface)",
            border: "1px solid var(--border)",
            borderRadius: 10,
            boxShadow: "0 8px 28px rgba(0,0,0,0.28)",
            zIndex: 1000,
            animation: "fade-in 0.18s ease-out",
            overflow: "hidden",
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: "10px 14px",
              borderBottom: "1px solid var(--border-subtle)",
              fontSize: 12.5,
              fontWeight: 700,
              color: "var(--text)",
            }}
          >
            <Icon name="sparkles" size={14} color="var(--accent)" />
            Teletype noticed
            <span style={{ marginLeft: "auto", fontSize: 11, fontWeight: 500, color: "var(--text-tertiary)" }}>
              local only
            </span>
          </div>
          <div style={{ display: "flex", flexDirection: "column", padding: "6px 8px 8px" }}>
            {polishCandidates.map((c) => (
              <div key={c.id} style={{ padding: "8px 6px" }}>
                <div style={{ fontSize: 13.5, fontWeight: 600 }}>{c.word}</div>
                {c.heard && c.heard.toLowerCase() !== c.word.toLowerCase() && (
                  <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 1 }}>
                    heard as “{c.heard}”
                  </div>
                )}
                <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                  <button
                    className="primary"
                    disabled={candidateBusy === c.id}
                    onClick={() => acceptCandidate(c)}
                    aria-label={`Add ${c.word} to dictionary`}
                    style={{ flex: 1, fontSize: 12.5 }}
                  >
                    {candidateBusy === c.id ? "Adding…" : "Add to dictionary"}
                  </button>
                  <button
                    disabled={candidateBusy === c.id}
                    onClick={() => dismissCandidate(c)}
                    aria-label={`Dismiss ${c.word}`}
                    style={{ fontSize: 12.5 }}
                  >
                    Dismiss
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
      {quickAdd && (
        <div
          role="dialog"
          aria-label="Quick Add word"
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.35)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1100,
          }}
          onClick={closeQuickAdd}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              width: 360,
              maxWidth: "90vw",
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: 12,
              boxShadow: "0 12px 40px rgba(0,0,0,0.35)",
              padding: "18px 20px",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 9, marginBottom: 4 }}>
              <Icon name="zap" size={16} color="var(--accent)" />
              <span style={{ fontSize: 15, fontWeight: 700 }}>Add word to dictionary</span>
            </div>
            {!quickAdd.selection ? (
              <p style={{ fontSize: 13, color: "var(--text-secondary)", margin: "8px 0 0" }}>
                No text was selected in the focused app. Highlight a word and press Quick Add
                again, or type the word below.
              </p>
            ) : (
              <p style={{ fontSize: 12.5, color: "var(--text-tertiary)", margin: "6px 0 0" }}>
                Selected: <span style={{ color: "var(--text-secondary)" }}>“{quickAdd.selection}”</span>
              </p>
            )}
            <div style={{ marginTop: 14 }}>
              <label
                htmlFor="quick-add-word"
                style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}
              >
                Word
              </label>
              <input
                id="quick-add-word"
                autoFocus
                value={quickAddWord}
                onChange={(e) => setQuickAddWord(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") confirmQuickAdd();
                  if (e.key === "Escape") closeQuickAdd();
                }}
                placeholder="e.g. Teletype"
                style={{
                  width: "100%",
                  marginTop: 4,
                  padding: "9px 12px",
                  fontSize: 14,
                  borderRadius: 8,
                  border: "1px solid var(--border)",
                  background: "var(--surface-2)",
                  color: "var(--text)",
                  boxSizing: "border-box",
                }}
              />
            </div>
            {quickAddError && (
              <div
                role="alert"
                style={{
                  marginTop: 10,
                  fontSize: 12.5,
                  color: "var(--danger)",
                  background: "color-mix(in srgb, var(--danger) 8%, transparent)",
                  border: "1px solid var(--danger)",
                  borderRadius: 8,
                  padding: "8px 10px",
                }}
              >
                {quickAddError}
              </div>
            )}
            <div style={{ display: "flex", gap: 8, marginTop: 16, justifyContent: "flex-end" }}>
              <button onClick={closeQuickAdd} disabled={quickAddBusy}>
                Cancel
              </button>
              <button className="primary" onClick={confirmQuickAdd} disabled={quickAddBusy}>
                {quickAddBusy ? "Adding…" : "Add"}
              </button>
            </div>
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
