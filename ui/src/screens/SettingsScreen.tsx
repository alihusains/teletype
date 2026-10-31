import type { CSSProperties } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WriteFailed } from "../components/StateRows";
import { errorMessage } from "../lib/errors";
import {
  ClipboardSection,
  KeybindsSection,
} from "../settings/KeybindsClipboardSections";
import {
  MicrophoneSection,
  SoundsSection,
} from "../settings/MicrophoneSoundsSections";
import {
  LicenseSection,
  PermissionsSection,
  UpdatesSection,
} from "../settings/PermissionsUpdatesLicenseSections";
import {
  AIPolishSection,
  AudioSection,
  GeneralSection,
  LearningSection,
  LivePreviewSection,
  TranscriptionSection,
} from "../settings/DictationPolishSections";
import {
  SecondaryButton,
  Segmented,
  Select,
  Toggle,
} from "../settings/primitives";
import { useRuntimeStatus } from "../lib/runtimeStatus";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";
import { RainbowLips, RainbowMeter } from "../pill";
import { search } from "../settings/settingsSearch";

// The single source of truth for the settings shape on this side of the IPC
// boundary. It lives here rather than in ui/src/settings/types.ts because
// crates/teletype-core/tests/ipc_contract.rs parses THIS file to assert that
// every persisted Rust field has a control somewhere. Move it and that guard
// silently stops guarding. Every settings section imports this type.
export interface Settings {
  hotkey: string;
  recordingMode: string;
  selectedSpeechModel: string;
  language: string;
  inputDevice: string;
  restoreClipboard: boolean;
  keepTextOnClipboard: boolean;
  autoApplyTransform: boolean;
  showTrayIcon: boolean;
  hasCompletedOnboarding: boolean;
  selectedLlmModel: string;
  typingAutotextEnabled: boolean;
  autotextTiming: string;
  removeFillerWords: boolean;
  fillerWords: string[];
  fillerStrength: string;
  pillPosition: string;
  alwaysShowPill: boolean;
  livePreviewEnabled: boolean;
  cancelHotkey: string;
  smartInsertion: boolean;
  playRecordingSounds: boolean;
  recordingSound: string;
  warmEnginePolicy: string;
  matchStrictness: string;
  dictionaryEnabled: boolean;
  pillStyle: string;
  appIcon: string;
  transcriptsDir: string;
  vadAutoStop: boolean;
  vadSilenceMs: number;
  noiseReductionEnabled: boolean;
  enableDeveloperTab: boolean;
  polishGateEnabled: boolean;
  polishGateThresholdWords: number;
  /// Ceiling on how much of a polished take the model may replace, as a
  /// percentage of words. 100 disables the ceiling.
  polishMaxChangePercent: number;
  restoreEmoji: boolean;
  spokenEmoji: boolean;
  spokenPunctuation: boolean;
  modelUnloadDelaySecs: number;
  activeStyleProfile: string;
  appLanguageOverrides: Record<string, string>;
  enabledPacks: string[];
  selectedLlmProvider: string;
  theme: string;
  reduceMotion: boolean;
  polishRules: Record<string, boolean>;
  listStyle: string;
  quickAddHotkey: string;
  commandHotkey: string;
  pasteLastHotkey?: string;
  commandWakePhrase: boolean;
  pendingNotice: string | null;
  polishWholeTextAtEnd: boolean;
  polishSkipWhenClean: boolean;
  instantStart: string;
  preferBuiltinMicWithBt: boolean;
  audioConditioning: boolean;
  learnFromEdits: boolean;
  learnNamesFromScreen: boolean;
  keepAudioForSelfTests: boolean;
  duckOtherAudio: boolean;
  duckLevel: number;
  pauseMediaWhileDictating: boolean;
  /// Kept under its original name: the backend field is still
  /// `scratchpad_enabled` after the Notetaker rename.
  scratchpadEnabled: boolean;
  speechEngine: string;
  openaiBaseUrl: string;
  openaiModel: string;
  commandModeEnabled: boolean;
  /** T20: show the strong-language coach card on the Insights screen.
      On by default; the card is hidden entirely when off. */
  showLanguageCoach?: boolean;
}

const APP_ICON_CHOICES: { id: string; label: string; src: string }[] = [
  { id: "white", label: "Light", src: "/teletype-app-icon-white.png" },
  { id: "purple", label: "Purple", src: "/teletype-app-icon-purple.png" },
];

const PILL_STYLES: { value: string; label: string; hint: string }[] = [
  { value: "default", label: "Teletype", hint: "The Teletype capsule: red dot, waveform, and expandable details." },
  { value: "classic", label: "Classic Capsule", hint: "The first-release design and current default: audio-reactive bars with a timer. Compact." },
  { value: "levelRail", label: "Level Rail", hint: "A 24-bar level meter with a timer." },
  { value: "well", label: "Reading Well", hint: "A wider panel with a header and a live-preview well." },
  { value: "dotGrid", label: "Dot Matrix", hint: "A retro dot-matrix grid that reacts to your voice, with sweep and scan animations. Ported from LiveKit Agents UI." },
];

const PILL_POSITIONS: { value: string; label: string }[] = [
  { value: "topLeft", label: "Top Left" },
  { value: "topCenter", label: "Top Center" },
  { value: "topRight", label: "Top Right" },
  { value: "centerLeft", label: "Middle Left" },
  { value: "center", label: "Middle Center" },
  { value: "centerRight", label: "Middle Right" },
  { value: "bottomLeft", label: "Bottom Left" },
  { value: "bottomCenter", label: "Bottom Center" },
  { value: "bottomRight", label: "Bottom Right" },
];

const PILL_PREVIEW_CLOCK = "0:07";
// The default pill's live waveform collapses to baseline dots between
// syllables. At preview scale that reads as "not animating", so keep the
// original seven-bar shape as a floor and let the live level stream grow it.
const PILL_PREVIEW_WAVE = [6, 14, 22, 16, 20, 12, 8];

// Feeds one shared 33 ms level stream to every preview in the picker, so all
// cards animate together exactly like the live pill. The timer only runs
// while the Settings tab is actually on screen and the window is visible:
// every screen stays mounted (display: none), so an unthrottled 30 FPS timer
// would burn CPU and battery in the background forever.
function usePillPreviewLevels(active: boolean): number[] {
  const [levels, setLevels] = useState<number[]>(() => new Array(34).fill(0));
  const startRef = useRef(Date.now());
  useEffect(() => {
    if (!active) return;
    let visible = document.visibilityState === "visible";
    const onVisibility = () => {
      visible = document.visibilityState === "visible";
    };
    document.addEventListener("visibilitychange", onVisibility);
    const start = startRef.current;
    const id = window.setInterval(() => {
      if (!visible) return;
      const t = (Date.now() - start) / 1000;
      const v = Math.max(0, Math.min(1, 0.55 * Math.sin(t * 2.1) + 0.3 * Math.sin(t * 7.7) + Math.abs(Math.sin(t * 0.3)) * 0.6));
      setLevels((prev) => [...prev.slice(1), v]);
    }, 33);
    return () => {
      window.clearInterval(id);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [active]);
  return levels;
}

// The previews are miniature copies of the live pill, so they use the same
// tokens as CAPSULE/CLOCK in pill.tsx: --surface, --shadow-3, --text. That
// also makes them follow the app theme — a light pill in light mode and a
// dark pill in dark mode, exactly like the real thing.
function previewClockStyle(): CSSProperties {
  return {
    fontVariantNumeric: "tabular-nums",
    color: "var(--text)",
  };
}

function previewCapsule(width: number): CSSProperties {
  return {
    // Cap the width so the widest preview (Level Rail, 24 bars) fits inside
    // its card at the smallest grid column (280px - 24px padding = 256px).
    // `flexShrink: 1` lets a narrower column squeeze it further without the
    // pill spilling past the card edge (item 12).
    maxWidth: Math.min(width, 256),
    width: "100%",
    height: 44,
    borderRadius: 22,
    background: "var(--surface)",
    boxShadow: "var(--shadow-3)",
    display: "flex",
    alignItems: "center",
    gap: 12,
    padding: "0 18px",
    flexShrink: 1,
  };
}

function TeletypePreview({ levels }: { levels: number[] }) {
  // Animate the compact 7-bar miniature from the same live level stream as the
  // other previews. The full live pill renders all 34 bars; seven is the right
  // density for this 150px capsule.
  const recent = levels.slice(-7);
  return (
    <div style={previewCapsule(150)}>
      <span
        style={{
          position: "relative",
          width: 18,
          height: 18,
          borderRadius: "50%",
          background: "var(--danger-soft)",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        <span
          style={{
            position: "absolute",
            width: 8,
            height: 8,
            borderRadius: "50%",
            background: "var(--danger)",
            opacity: 0.35,
            animation: "tt-ping 1.4s cubic-bezier(0,0,0.2,1) infinite",
          }}
        />
        <span style={{ position: "relative", width: 8, height: 8, borderRadius: "50%", background: "var(--danger)" }} />
      </span>
      <div style={{ display: "flex", alignItems: "center", gap: 2, height: 22, flex: 1, overflow: "hidden" }}>
        {recent.map((level, i) => {
          const v = Math.max(0, Math.min(1, level));
          return (
            <span
              key={i}
              style={{
                width: 2.5,
                height: Math.max(4, Math.min(22, PILL_PREVIEW_WAVE[i] * (0.5 + 0.7 * v) + 4 * v)),
                borderRadius: 999,
                background: "var(--accent)",
                transition: "height 90ms ease-out",
              }}
            />
          );
        })}
      </div>
    </div>
  );
}

function ClassicPreview({ levels }: { levels: number[] }) {
  return (
    <div style={previewCapsule(185)}>
      <div style={{ flex: 1, minWidth: 0, height: 30 }}>
        <RainbowLips levels={levels} bars={26} />
      </div>
      <span style={{ flexShrink: 0, ...previewClockStyle(), fontSize: 15, fontWeight: 700, letterSpacing: 1 }}>{PILL_PREVIEW_CLOCK}</span>
    </div>
  );
}

function LevelRailPreview({ levels }: { levels: number[] }) {
  const hist = [...levels.slice(-24)];
  while (hist.length < 24) hist.unshift(0);
  return (
    <div style={{ ...previewCapsule(288), gap: 14 }}>
      <span style={{ ...previewClockStyle(), fontSize: 14, fontWeight: 700 }}>{PILL_PREVIEW_CLOCK}</span>
      <div style={{ display: "flex", alignItems: "center", gap: 2, height: 28, flex: 1 }}>
        {hist.map((lv, i) => (
          <span
            key={i}
            style={{
              // Flexible bars: the 24-bar meter scales to whatever width the
              // card gives it instead of overflowing past the pill edge.
              flex: "1 1 3px",
              maxWidth: 4,
              height: Math.max(28 * 0.14, lv * 28),
              borderRadius: 1.5,
              background: "var(--accent)",
              transition: "height 80ms ease-out",
            }}
          />
        ))}
      </div>
    </div>
  );
}

function ReadingWellPreview({ levels }: { levels: number[] }) {
  return (
    <div
      style={{
        width: "100%",
        maxWidth: 256,
        height: 96,
        borderRadius: "var(--radius)",
        background: "var(--surface)",
        boxShadow: "var(--shadow-3)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        flexShrink: 1,
      }}
    >
      {/* Compact header at preview scale: the meter takes the remaining room,
          so the LISTENING badge never overflows the 256px capsule. */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 12px 8px 12px" }}>
        <span style={{ ...previewClockStyle(), fontSize: 14, fontWeight: 700, flexShrink: 0 }}>{PILL_PREVIEW_CLOCK}</span>
        <div style={{ display: "flex", flex: "1 1 0", minWidth: 0, overflow: "hidden" }}>
          <RainbowMeter levels={levels} height={16} />
        </div>
        <span style={{ fontSize: 9.5, fontWeight: 700, letterSpacing: 1, color: "var(--text-secondary)", padding: "2px 8px", borderRadius: 999, background: "var(--surface-2)", display: "inline-flex", alignItems: "center", gap: 5, flexShrink: 0, whiteSpace: "nowrap" }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--accent)" }} />
          LISTENING
        </span>
      </div>
      <div style={{ borderTop: "1px solid var(--border-subtle)" }} />
      <div style={{ flex: 1, padding: "10px 16px 10px 16px", background: "var(--surface-2)" }}>
        <p style={{ fontSize: 13, lineHeight: 1.5, color: "var(--text)", margin: 0 }}>Listening…</p>
      </div>
    </div>
  );
}

function DotGridPreview({ levels }: { levels: number[] }) {
  const [step, setStep] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setStep((s) => s + 1), 100);
    return () => window.clearInterval(id);
  }, []);
  // Same geometry as the live pill (24 x 3) at half dot size.
  const cols = 15;
  const rows = 3;
  const dot = 4;
  const gap = 2;
  const hist = [...levels.slice(-cols)];
  while (hist.length < cols) hist.unshift(0);
  const speaking = levels.length > 0 && levels[levels.length - 1] > 0.05;
  const mid = Math.floor(rows / 2);
  const center = { x: Math.floor(cols / 2), y: mid };
  return (
    <div style={previewCapsule(150)}>
      <span style={{ ...previewClockStyle(), fontSize: 14, fontWeight: 700, flexShrink: 0 }}>{PILL_PREVIEW_CLOCK}</span>
      <div
        style={{
          display: "grid",
          // Dots scale to the available width (min 3px, max 4px) so the
          // matrix never overflows the pill at narrow card widths (item 12).
          gridTemplateColumns: `repeat(${cols}, minmax(3px, 4px))`,
          gap,
          pointerEvents: "none",
          overflow: "hidden",
        }}
      >
        {Array.from({ length: rows * cols }, (_, k) => {
          const y = Math.floor(k / cols);
          const x = k % cols;
          let on = false;
          if (speaking) {
            const chunk = 1 / (mid + 1);
            on = (hist[x] ?? 0) >= Math.abs(mid - y) * chunk + 0.02;
          } else {
            on = x === center.x && y === center.y && step % 9 !== 0;
          }
          return (
            <span
              key={k}
              style={{
                width: "100%",
                height: dot,
                borderRadius: "50%",
                background: "var(--accent)",
                opacity: on ? 1 : 0.12,
                transition: "opacity 90ms linear",
              }}
            />
          );
        })}
      </div>
    </div>
  );
}

function PillStylePreview({ value, levels }: { value: string; levels: number[] }) {
  if (value === "classic") return <ClassicPreview levels={levels} />;
  if (value === "levelRail") return <LevelRailPreview levels={levels} />;
  if (value === "well") return <ReadingWellPreview levels={levels} />;
  if (value === "dotGrid") return <DotGridPreview levels={levels} />;
  return <TeletypePreview levels={levels} />;
}

function PillStylePicker({
  value,
  onSelect,
  active,
  previewTheme,
}: {
  value: string;
  onSelect: (v: string) => void;
  active: boolean;
  previewTheme: "app" | "light" | "dark";
}) {
  const levels = usePillPreviewLevels(active);
  return (
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))", gap: 8, marginTop: 6 }}>
      {PILL_STYLES.map((style) => {
        const selected = value === style.value;
        return (
          <button
            key={style.value}
            onClick={() => onSelect(style.value)}
            style={{
              textAlign: "left",
              padding: 12,
              borderRadius: "var(--radius-sm)",
              cursor: "pointer",
              background: "var(--surface-2)",
              border: selected ? "2px solid var(--accent-strong)" : "1px solid var(--border-subtle)",
              boxShadow: selected ? "var(--shadow-1)" : "none",
            }}
          >
            <div
              className="tt-pill-stage"
              data-preview-theme={previewTheme === "app" ? undefined : previewTheme}
              style={{ marginBottom: 10 }}
            >
              <PillStylePreview value={style.value} levels={levels} />
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span style={{ fontSize: "var(--text-sm)", fontWeight: 600, color: selected ? "var(--accent-ink)" : "var(--text)" }}>
                {style.label}
              </span>
              {selected && <span style={{ color: "var(--accent-ink)", fontSize: 12 }}>✓</span>}
            </div>
            <div style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", lineHeight: 1.4, marginTop: 2 }}>{style.hint}</div>
          </button>
        );
      })}
    </div>
  );
}


// Per-app language and style overrides. The backend has resolved these for
// a while (per-app ASR language, per-app style routing) but nothing could
// configure them: both maps were write-only from the user's side. Keys are
// lowercased bundle ids (or app names where no id is exposed).
function PerAppOverrides() {
  const [langMap, setLangMap] = useState<Record<string, string>>({});
  const [styleMap, setStyleMap] = useState<Record<string, string>>({});
  const [styles, setStyles] = useState<{ id: string; name: string }[]>([]);
  const [newApp, setNewApp] = useState("");
  const [newLang, setNewLang] = useState("en");
  const [newStyleApp, setNewStyleApp] = useState("");
  const [newStyle, setNewStyle] = useState("");
  const speechLanguages = useSpeechLanguages();

  const refresh = async () => {
    try {
      setLangMap(await invoke<Record<string, string>>("get_app_language_overrides"));
    } catch { /* command unavailable; keep empty */ }
    try {
      setStyleMap(await invoke<Record<string, string>>("get_app_style_overrides"));
    } catch { /* command unavailable; keep empty */ }
    try {
      const profiles = await invoke<{ id: string; name: string }[]>("list_style_profiles");
      setStyles(profiles);
      if (profiles.length > 0) setNewStyle((s) => s || profiles[0].id);
    } catch { /* keep empty */ }
  };
  useEffect(() => {
    refresh();
  }, []);

  const rowStyle: CSSProperties = {
    display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-sm)", marginBottom: 6,
  };

  // Add-row field styling: tokenised so the per-app inputs match the rest of
  // Settings instead of falling back to unstyled browser controls.
  const fieldControl: CSSProperties = {
    height: 32,
    padding: "0 10px",
    fontSize: "var(--text-sm)",
    color: "var(--text)",
    background: "var(--surface)",
    border: "1px solid var(--border)",
    borderRadius: "var(--radius-sm)",
  };

  return (
    <>
      <div style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", marginBottom: 10 }}>
        Dictate in another language, or with another style, in specific apps. App keys are
        lowercased bundle ids (e.g. com.google.gmail).
      </div>
      <div className="tt-card" style={{ padding: "14px 16px" }}>
      <div style={{ fontSize: "var(--text-sm)", fontWeight: 600, marginBottom: 6 }}>Dictation language</div>
      {Object.entries(langMap).map(([appKey, lang]) => (
        <div key={appKey} style={rowStyle}>
          <code style={{ flex: 1 }}>{appKey}</code>
          <span>{speechLanguages.find((l) => l.code === lang)?.name ?? lang}</span>
          <SecondaryButton
            onClick={async () => {
              await invoke("set_app_language_override", { appKey, lang: "" }).catch(console.error);
              refresh();
            }}
          >
            Remove
          </SecondaryButton>
        </div>
      ))}
      <div style={{ ...rowStyle, marginTop: 4 }}>
        <input
          aria-label="App key for language override"
          placeholder="App key, e.g. com.google.gmail"
          value={newApp}
          onChange={(e) => setNewApp(e.target.value)}
          style={{ ...fieldControl, flex: 1 }}
        />
        <select
          value={newLang}
          onChange={(e) => setNewLang(e.target.value)}
          aria-label="App language"
          style={{ ...fieldControl, minWidth: 140 }}
        >
          {speechLanguages.map((l) => (
            <option key={l.code} value={l.code}>
              {l.name}
            </option>
          ))}
        </select>
        <SecondaryButton
          disabled={!newApp.trim()}
          onClick={async () => {
            if (!newApp.trim()) return;
            await invoke("set_app_language_override", { appKey: newApp.trim(), lang: newLang }).catch(
              console.error
            );
            setNewApp("");
            refresh();
          }}
        >
          Add
        </SecondaryButton>
      </div>
      <div style={{ fontSize: "var(--text-sm)", fontWeight: 600, marginBottom: 6, marginTop: 14 }}>Writing style</div>
      {Object.entries(styleMap).map(([appKey, styleId]) => (
        <div key={appKey} style={rowStyle}>
          <code style={{ flex: 1 }}>{appKey}</code>
          <span>{styles.find((s) => s.id === styleId)?.name ?? styleId}</span>
          <SecondaryButton
            onClick={async () => {
              await invoke("set_app_style_override", { appKey, styleId: "" }).catch(console.error);
              refresh();
            }}
          >
            Remove
          </SecondaryButton>
        </div>
      ))}
      <div style={{ ...rowStyle, marginTop: 4 }}>
        <input
          aria-label="App key for style override"
          placeholder="App key, e.g. com.slackmac.Slack"
          value={newStyleApp}
          onChange={(e) => setNewStyleApp(e.target.value)}
          style={{ ...fieldControl, flex: 1 }}
        />
        <select
          value={newStyle}
          onChange={(e) => setNewStyle(e.target.value)}
          aria-label="App style"
          disabled={styles.length === 0}
          style={{ ...fieldControl, minWidth: 140, opacity: styles.length === 0 ? 0.55 : 1 }}
        >
          <option value="">{styles.length === 0 ? "No styles yet" : "Select style"}</option>
          {styles.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <SecondaryButton
          disabled={!newStyleApp.trim() || !newStyle}
          onClick={async () => {
            if (!newStyleApp.trim() || !newStyle) return;
            await invoke("set_app_style_override", { appKey: newStyleApp.trim(), styleId: newStyle }).catch(
              console.error
            );
            setNewStyleApp("");
            refresh();
          }}
        >
          Add
        </SecondaryButton>
      </div>
      </div>
    </>
  );
}

// Shows the real transcripts folder path (item 13). The backend resolves the
// default location when no custom dir is set, so the user always sees exactly
// where transcripts land instead of an opaque "(default)" placeholder.
function TranscriptsDirField({
  customDir,
  onSave,
}: {
  customDir: string;
  onSave: (dir: string) => void;
}) {
  const [resolved, setResolved] = useState<string>("");
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  useEffect(() => {
    invoke<string>("get_transcripts_dir").then(setResolved).catch(() => {});
  }, [customDir]);

  const startEdit = () => {
    setDraft(customDir);
    setEditing(true);
  };
  const commit = () => {
    setEditing(false);
    onSave(draft.trim());
  };

  if (editing) {
    return (
      <input
        autoFocus
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setEditing(false);
        }}
        placeholder="Leave blank for the default folder"
        style={{
          flex: "1 1 320px",
          minWidth: 240,
          height: 32,
          padding: "0 10px",
          borderRadius: "var(--radius-sm)",
          border: "1px solid var(--border)",
          background: "var(--surface)",
          fontSize: "var(--text-sm)",
          color: "var(--text)",
        }}
      />
    );
  }

  return (
    <button
      onClick={startEdit}
      title="Click to change the transcripts folder"
      style={{
        flex: "1 1 320px",
        minWidth: 240,
        height: 32,
        padding: "0 10px",
        borderRadius: "var(--radius-sm)",
        border: "1px solid var(--border)",
        background: "var(--surface)",
        fontSize: "var(--text-sm)",
        color: "var(--text)",
        textAlign: "left",
        cursor: "text",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      }}
    >
      {resolved || "…"}
    </button>
  );
}

export default function SettingsScreen({ active }: { active: boolean }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  // The last rejected save, shown under the header until the next save or a
  // dismissal. Without it a failed write silently reverted the control and
  // the user saw a setting snap back for no stated reason.
  const [saveError, setSaveError] = useState<string | null>(null);
  const [devices, setDevices] = useState<{ id: string; name: string; is_default: boolean }[]>([]);
  // Coarse 5s poll, paused when the window is hidden: the readiness chips must
  // not wake the engines or the render loop while the window is in the background.
  const runtimeStatus = useRuntimeStatus().status;

  // Settings search (P8.7): query state plus the current match position for
  // Enter-to-advance. `active` gates the DOM sweep so searching is skipped
  // while this screen is hidden (every screen stays mounted via display:none).
  const [query, setQuery] = useState("");
  const [matchIndex, setMatchIndex] = useState(0);
  const [pillPreviewTheme, setPillPreviewTheme] = useState<"app" | "light" | "dark">("app");
  const searchInputRef = useRef<HTMLInputElement>(null);

  const matches = useMemo(() => {
    if (query.trim() === "" || !active) return [];
    const rows = Array.from(
      document.querySelectorAll<HTMLElement>("[data-settings-label]")
    );
    return search(rows.map((r) => r.dataset.settingsLabel ?? ""), query)
      .map((m) => rows.find((r) => r.dataset.settingsLabel === m.label) ?? null)
      .filter((r): r is HTMLElement => r !== null);
  }, [query, active, settings]);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
    invoke<{ id: string; name: string; is_default: boolean }[]>("list_input_devices").then(setDevices).catch(console.error);
  }, []);

  // Outline every match and keep the first one in view while the query
  // changes; the outline is drawn on the row itself via CSS variables so the
  // search never restyles anything else on the screen.
  useEffect(() => {
    const marked = matches.map((m) => m.style.outline);
    matches.forEach((m) => {
      m.style.outline = "2px solid var(--accent-strong)";
      m.style.outlineOffset = "-2px";
      m.style.borderRadius = "var(--radius-sm)";
    });
    if (matches.length > 0) matches[0].scrollIntoView({ block: "nearest" });
    return () => {
      matches.forEach((m, i) => {
        m.style.outline = marked[i];
      });
    };
  }, [matches]);

  // Enter advances to the next match and scrolls it into view; the position
  // resets whenever the query (and therefore the match list) changes.
  useEffect(() => {
    setMatchIndex(0);
  }, [query]);
  const advanceMatch = () => {
    if (matches.length === 0) return;
    const next = (matchIndex + 1) % matches.length;
    setMatchIndex(next);
    matches[next].scrollIntoView({ block: "center" });
  };
  // Esc clears the query, which removes all outlines (empty query = the
  // screen looks exactly as it did before searching).
  const clearSearch = () => {
    setQuery("");
    searchInputRef.current?.focus();
  };

  const save = async (updated: Settings) => {
    const prev = settings;
    setSettings(updated);
    setSaveError(null);
    try {
      await invoke("save_settings", { settings: updated });
    } catch (e) {
      setSettings(prev);
      setSaveError(errorMessage(e));
    }
  };

  // Merges a partial update into the current settings and persists the whole
  // object. Declared after `save` so it always reads the latest `settings`.
  const patch = (p: Partial<Settings>) => {
    if (!settings) return;
    save({ ...settings, ...p });
  };

  if (!settings) return <p>Loading…</p>;

  // Group headings (item 14): the settings list is arranged by what the user
  // is trying to change, so navigation is by intent instead of a flat scroll.
  const groupHeading: CSSProperties = {
    fontSize: "var(--text-xs)",
    fontWeight: 600,
    textTransform: "uppercase",
    color: "var(--text-tertiary)",
    marginTop: 28,
    marginBottom: 10,
    letterSpacing: "0.07em",
  };

  return (
    <div className="tt-page" style={{ paddingBottom: 24 }}>
      <div className="tt-page-header" style={{ marginBottom: 24 }}>
        <div>
          <h2 className="tt-screen-title">Settings</h2>
          <p className="tt-page-sub" style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: "4px 0 0" }}>Configure dictation, audio, and app behavior.</p>
        </div>
      </div>

      {saveError && (
        <div style={{ marginBottom: 18 }}>
          <WriteFailed
            label="That setting"
            message={saveError}
            onDismiss={() => setSaveError(null)}
          />
        </div>
      )}

      {/* Settings search: filters the rows below, outlines every match, and
          walks them with Enter. An empty query renders nothing but the
          input itself, so the screen is visually unchanged until you type. */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 24 }}>
        <input
          ref={searchInputRef}
          type="text"
          aria-label="Search settings"
          placeholder="Search settings…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") advanceMatch();
            if (e.key === "Escape") clearSearch();
          }}
          style={{
            flex: 1,
            maxWidth: 360,
            height: 32,
            padding: "0 10px",
            fontSize: "var(--text-sm)",
            color: "var(--text)",
            background: "var(--surface)",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius-sm)",
          }}
        />
        {query.trim() !== "" ? (
          <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", whiteSpace: "nowrap" }}>
            {matches.length} match{matches.length === 1 ? "" : "es"}
            {matches.length > 0 ? ` · ${matchIndex + 1}/${matches.length}` : ""}
          </span>
        ) : null}
      </div>

      {/* Ported sections. Each is its own component so the nine clusters can
          stay consistent instead of re-deriving row styling inline here.
          `patch` merges into the whole Settings object because the backend
          serialises read-modify-write under a lock: two screens saving
          separate fields must not revert each other. */}
      {/* Transcription, Live Preview and AI Polish live here rather than on
          the Dictation tab: that tab is the transcript history, and these are
          settings. They are grouped here so the dictation-side decisions sit
          together instead of scattered through the screen. */}
      <h3 style={groupHeading}>Dictation</h3>
      <TranscriptionSection
        settings={settings}
        onChange={patch}
        runtimeStatus={runtimeStatus ?? undefined}
      />
      <LivePreviewSection
        settings={settings}
        onChange={patch}
        runtimeStatus={runtimeStatus ?? undefined}
      />
      <AIPolishSection
        settings={settings}
        onChange={patch}
        runtimeStatus={runtimeStatus ?? undefined}
      />
      <AudioSection settings={settings} onChange={patch} />

      <h3 style={groupHeading}>Input</h3>
      <PermissionsSection settings={settings} onChange={patch} />
      <KeybindsSection settings={settings} onChange={patch} />
      <ClipboardSection settings={settings} onChange={patch} />
      <MicrophoneSection settings={settings} onChange={patch} devices={devices} />
      <SoundsSection settings={settings} onChange={patch} />

      <h3 style={groupHeading}>General</h3>
      <GeneralSection settings={settings} onChange={patch} />

      <h3 style={groupHeading}>Per-app</h3>
      <PerAppOverrides />

      <h3 style={groupHeading}>Floating Pill</h3>
      <div className="tt-card" style={{ padding: "14px 16px", display: "grid", gap: 16 }}>
        <div>
          <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 12, flexWrap: "wrap", marginBottom: 8 }}>
            <div style={{ fontSize: "var(--text-sm)" }}>Pill style</div>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ fontSize: 11.5, color: "var(--text-tertiary)" }}>Preview</span>
              <Segmented
                label="Preview theme"
                value={pillPreviewTheme}
                options={[
                  { value: "app", label: "App" },
                  { value: "light", label: "Light" },
                  { value: "dark", label: "Dark" },
                ]}
                onChange={setPillPreviewTheme}
              />
            </div>
          </div>
          <PillStylePicker
            value={settings.pillStyle || "classic"}
            onSelect={(v) => save({ ...settings, pillStyle: v })}
            active={active}
            previewTheme={pillPreviewTheme}
          />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ fontSize: "var(--text-sm)", flex: 1 }}>Pill position</span>
          <Select
            label="Pill position"
            value={settings.pillPosition}
            onChange={(v) => save({ ...settings, pillPosition: v })}
            options={PILL_POSITIONS.map((p) => ({ value: p.value, label: p.label }))}
          />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ fontSize: "var(--text-sm)", flex: 1 }}>Always show the pill (even when idle)</span>
          <Toggle
            label="Always show the pill (even when idle)"
            checked={settings.alwaysShowPill}
            onChange={(v) => save({ ...settings, alwaysShowPill: v })}
          />
        </div>
      </div>

      <h3 style={groupHeading}>App Icon</h3>
      <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", marginBottom: 12 }}>
        Choose the icon shown in the sidebar, window, and menu bar.
      </p>
      <div style={{ display: "flex", gap: 16 }}>
        {APP_ICON_CHOICES.map((choice) => {
          const selected = settings.appIcon === choice.id;
          return (
            <button
              key={choice.id}
              onClick={() => {
                // The backend persists the choice and pushes settings-changed,
                // which refreshes the sidebar; move this screen's own checkmark
                // immediately rather than waiting for a remount.
                const prev = settings.appIcon;
                setSettings((s) => (s ? { ...s, appIcon: choice.id } : s));
                invoke("set_app_icon", { id: choice.id }).catch((e) => {
                  setSettings((s) => (s ? { ...s, appIcon: prev } : s));
                  console.error(e);
                });
              }}
              style={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                gap: 8,
                padding: 12,
                borderRadius: "var(--radius)",
                cursor: "pointer",
                background: "var(--surface)",
                border: selected ? "2px solid var(--accent-strong)" : "1px solid var(--border-subtle)",
                boxShadow: selected ? "var(--shadow-1)" : "none",
              }}
            >
              <img
                src={choice.src}
                alt=""
                width={64}
                height={64}
                style={{ borderRadius: 14 }}
              />
              <span style={{ fontSize: "var(--text-sm)", fontWeight: selected ? 600 : 400, color: selected ? "var(--accent-ink)" : "var(--text)" }}>
                {choice.label}
                {selected ? " ✓" : ""}
              </span>
            </button>
          );
        })}
      </div>

      <h3 style={groupHeading}>Appearance</h3>
      <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", marginBottom: 12 }}>
        Theme and motion for the main window. The floating pill follows the system appearance.
      </p>
      <div className="tt-card" style={{ padding: "14px 16px", display: "grid", gap: 14 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ fontSize: "var(--text-sm)", flex: 1 }}>Theme</span>
          <Select
            label="Theme"
            value={settings.theme || "system"}
            onChange={(v) => invoke("set_theme", { theme: v }).catch(console.error)}
            options={[
              { value: "system", label: "System" },
              { value: "light", label: "Light" },
              { value: "dark", label: "Dark" },
            ]}
          />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ fontSize: "var(--text-sm)", flex: 1 }}>Reduce Motion (follows the OS setting when off)</span>
          <Toggle
            label="Reduce Motion (follows the OS setting when off)"
            checked={!!settings.reduceMotion}
            onChange={(v) => invoke("set_reduce_motion", { enabled: v }).catch(console.error)}
          />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ fontSize: "var(--text-sm)", flex: 1 }}>Language coach</span>
          <Toggle
            label="Language coach"
            checked={settings.showLanguageCoach ?? true}
            onChange={(v) => save({ ...settings, showLanguageCoach: v })}
          />
        </div>
      </div>

      <h3 style={groupHeading}>Learning</h3>
      <LearningSection settings={settings} onChange={patch} />

      <h3 style={groupHeading}>Transcripts</h3>
      <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", marginBottom: 12 }}>
        Every dictation is also saved as a plain-text file, grouped by day, in this folder.
      </p>
      <div className="tt-card" style={{ padding: "14px 16px", display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <TranscriptsDirField
          customDir={settings.transcriptsDir}
          onSave={(dir) => save({ ...settings, transcriptsDir: dir })}
        />
        <SecondaryButton onClick={() => invoke("reveal_transcripts_dir").catch(console.error)}>
          Open folder
        </SecondaryButton>
      </div>

      <h3 style={groupHeading}>About</h3>
      <UpdatesSection />
      <LicenseSection />
    </div>
  );
}
