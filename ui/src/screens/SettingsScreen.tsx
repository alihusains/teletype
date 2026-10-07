import type { CSSProperties } from "react";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  GeneralSection,
  LivePreviewSection,
  TranscriptionSection,
} from "../settings/DictationPolishSections";
import { useRuntimeStatus } from "../lib/runtimeStatus";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";
import { RainbowLips, RainbowMeter, rainbowColor } from "../pill";

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
  removeFillerWords: boolean;
  fillerWords: string[];
  pillPosition: string;
  alwaysShowPill: boolean;
  livePreviewEnabled: boolean;
  cancelHotkey: string;
  smartInsertion: boolean;
  playRecordingSounds: boolean;
  recordingSound: string;
  warmEnginePolicy: string;
  matchStrictness: string;
  pillStyle: string;
  appIcon: string;
  transcriptsDir: string;
  vadAutoStop: boolean;
  vadSilenceMs: number;
  enableDeveloperTab: boolean;
  polishGateEnabled: boolean;
  polishGateThresholdWords: number;
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
}

const APP_ICON_CHOICES: { id: string; label: string; src: string }[] = [
  { id: "white", label: "Light", src: "/teletype-app-icon-white.png" },
  { id: "blue", label: "Blue", src: "/teletype-app-icon-blue.png" },
];

const PILL_STYLES: { value: string; label: string; hint: string }[] = [
  { value: "default", label: "Teletype", hint: "The original design: red dot, waveform, and expandable details." },
  { value: "classic", label: "Classic Capsule", hint: "Rainbow audio-reactive bars with a timer. Compact." },
  { value: "levelRail", label: "Level Rail", hint: "A 24-bar rainbow level meter with a timer." },
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

const PILL_PREVIEW_SURFACE = "rgba(20,20,28,0.95)";
const PILL_PREVIEW_BORDER = "rgba(255,255,255,0.12)";
const PILL_PREVIEW_SHADOW = "inset 0 1px 0 rgba(255,255,255,0.08), 0 6px 14px rgba(0,0,0,0.4)";
const PILL_PREVIEW_CLOCK = "0:07";

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

function previewClockStyle(): CSSProperties {
  return {
    fontVariantNumeric: "tabular-nums",
    fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace",
    color: "rgba(255,255,255,0.95)",
  };
}

function previewCapsule(width: number): CSSProperties {
  return {
    width,
    height: 44,
    borderRadius: 22,
    background: PILL_PREVIEW_SURFACE,
    border: `1px solid ${PILL_PREVIEW_BORDER}`,
    boxShadow: PILL_PREVIEW_SHADOW,
    display: "flex",
    alignItems: "center",
    gap: 12,
    padding: "0 18px",
    flexShrink: 0,
  };
}

function TeletypePreview() {
  return (
    <div style={previewCapsule(150)}>
      <span style={{ width: 10, height: 10, borderRadius: "50%", background: "#ff453d", flexShrink: 0 }} />
      <div style={{ display: "flex", alignItems: "center", gap: 2, height: 22, flex: 1, overflow: "hidden" }}>
        {[6, 14, 22, 16, 20, 12, 8].map((h, i) => (
          <span key={i} style={{ width: 2.5, height: h, borderRadius: 999, background: "rgba(255,255,255,0.9)" }} />
        ))}
      </div>
    </div>
  );
}

function ClassicPreview({ levels }: { levels: number[] }) {
  const recent = levels.slice(-8);
  const level = recent.length ? recent.reduce((a, b) => a + b, 0) / recent.length : 0;
  return (
    <div style={previewCapsule(185)}>
      <div style={{ height: 24 }}>
        <RainbowLips level={level} />
      </div>
      <span style={{ ...previewClockStyle(), fontSize: 15, fontWeight: 700, color: "white", letterSpacing: 1 }}>{PILL_PREVIEW_CLOCK}</span>
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
              width: 3,
              flexShrink: 0,
              height: Math.max(28 * 0.14, lv * 28),
              borderRadius: 1.5,
              background: rainbowColor(i / 23),
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
        width: 300,
        height: 96,
        borderRadius: 14,
        background: PILL_PREVIEW_SURFACE,
        border: `1px solid ${PILL_PREVIEW_BORDER}`,
        boxShadow: PILL_PREVIEW_SHADOW,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        flexShrink: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px 8px 16px" }}>
        <span style={{ ...previewClockStyle(), fontSize: 14, fontWeight: 700 }}>{PILL_PREVIEW_CLOCK}</span>
        <RainbowMeter levels={levels} height={16} />
        <span style={{ flex: 1 }} />
        <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: 1.5, color: "rgba(255,255,255,0.88)", padding: "2px 9px", borderRadius: 999, background: "rgba(255,255,255,0.08)", display: "inline-flex", alignItems: "center", gap: 5 }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: "rgba(255,255,255,0.9)" }} />
          LISTENING
        </span>
      </div>
      <div style={{ borderTop: "1px solid rgba(255,255,255,0.08)" }} />
      <div style={{ flex: 1, padding: "10px 16px 10px 16px", background: "rgba(0,0,0,0.28)" }}>
        <p style={{ fontSize: 13, lineHeight: 1.5, color: "rgba(255,255,255,0.92)", margin: 0 }}>Listening…</p>
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
      <div style={{ display: "grid", gridTemplateColumns: `repeat(${cols}, ${dot}px)`, gap, pointerEvents: "none" }}>
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
                width: dot,
                height: dot,
                borderRadius: "50%",
                background: on ? rainbowColor(x / (cols - 1)) : "rgba(255,255,255,0.12)",
                transition: "background-color 90ms linear",
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
  return <TeletypePreview />;
}

function PillStylePicker({ value, onSelect, active }: { value: string; onSelect: (v: string) => void; active: boolean }) {
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
              borderRadius: 12,
              cursor: "pointer",
              background: "var(--surface)",
              border: selected ? "2px solid var(--accent)" : "1px solid var(--border)",
            }}
          >
            <div style={{ height: 100, display: "flex", alignItems: "center", justifyContent: "flex-start", overflow: "hidden", marginBottom: 10 }}>
              <PillStylePreview value={style.value} levels={levels} />
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span style={{ fontSize: 13, fontWeight: 600, color: selected ? "var(--accent)" : "var(--text)" }}>
                {style.label}
              </span>
              {selected && <span style={{ color: "var(--accent)", fontSize: 12 }}>✓</span>}
            </div>
            <div style={{ fontSize: 11.5, color: "var(--text-secondary)", lineHeight: 1.4, marginTop: 2 }}>{style.hint}</div>
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
    display: "flex", alignItems: "center", gap: 8, fontSize: 13, marginBottom: 6,
  };

  return (
    <>
      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        Per-app overrides
      </h3>
      <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
        Dictate in another language, or with another style, in specific apps. App keys are
        lowercased bundle ids (e.g. com.google.gmail).
      </div>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>Dictation language</div>
      {Object.entries(langMap).map(([appKey, lang]) => (
        <div key={appKey} style={rowStyle}>
          <code style={{ flex: 1 }}>{appKey}</code>
          <span>{speechLanguages.find((l) => l.code === lang)?.name ?? lang}</span>
          <button
            onClick={async () => {
              await invoke("set_app_language_override", { appKey, lang: "" }).catch(console.error);
              refresh();
            }}
          >
            Remove
          </button>
        </div>
      ))}
      <div style={{ ...rowStyle, marginTop: 4 }}>
        <input
          placeholder="App key, e.g. com.google.gmail"
          value={newApp}
          onChange={(e) => setNewApp(e.target.value)}
          style={{ flex: 1 }}
        />
        <select value={newLang} onChange={(e) => setNewLang(e.target.value)}>
          {speechLanguages.map((l) => (
            <option key={l.code} value={l.code}>
              {l.name}
            </option>
          ))}
        </select>
        <button
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
        </button>
      </div>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6, marginTop: 14 }}>Writing style</div>
      {Object.entries(styleMap).map(([appKey, styleId]) => (
        <div key={appKey} style={rowStyle}>
          <code style={{ flex: 1 }}>{appKey}</code>
          <span>{styles.find((s) => s.id === styleId)?.name ?? styleId}</span>
          <button
            onClick={async () => {
              await invoke("set_app_style_override", { appKey, styleId: "" }).catch(console.error);
              refresh();
            }}
          >
            Remove
          </button>
        </div>
      ))}
      <div style={{ ...rowStyle, marginTop: 4 }}>
        <input
          placeholder="App key, e.g. com.slackmac.Slack"
          value={newStyleApp}
          onChange={(e) => setNewStyleApp(e.target.value)}
          style={{ flex: 1 }}
        />
        <select value={newStyle} onChange={(e) => setNewStyle(e.target.value)}>
          {styles.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <button
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
        </button>
      </div>
    </>
  );
}

export default function SettingsScreen({ active }: { active: boolean }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<{ id: string; name: string; is_default: boolean }[]>([]);
  // Coarse 5s poll, paused when the window is hidden: the readiness chips must
  // not wake the engines or the render loop while the window is in the background.
  const runtimeStatus = useRuntimeStatus();

  useEffect(() => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
    invoke<{ id: string; name: string; is_default: boolean }[]>("list_input_devices").then(setDevices).catch(console.error);
  }, []);

  const save = async (updated: Settings) => {
    const prev = settings;
    setSettings(updated);
    try {
      await invoke("save_settings", { settings: updated });
    } catch (e) {
      setSettings(prev);
      console.error(e);
    }
  };

  // Merges a partial update into the current settings and persists the whole
  // object. Declared after `save` so it always reads the latest `settings`.
  const patch = (p: Partial<Settings>) => {
    if (!settings) return;
    save({ ...settings, ...p });
  };

  if (!settings) return <p>Loading…</p>;

  return (
    <div>
      <h2 style={{ fontSize: 18, fontWeight: 600, marginBottom: 16 }}>Settings</h2>

      {/* Ported sections. Each is its own component so the nine clusters can
          stay consistent instead of re-deriving row styling inline here.
          `patch` merges into the whole Settings object because the backend
          serialises read-modify-write under a lock: two screens saving
          separate fields must not revert each other. */}
      {/* Transcription, Live Preview and AI Polish live here rather than on
          the Dictation tab: that tab is the transcript history, and these are
          settings. They are grouped here so the dictation-side decisions sit
          together instead of scattered through the screen. */}
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
      <PermissionsSection settings={settings} onChange={patch} />
      <KeybindsSection settings={settings} onChange={patch} />
      <ClipboardSection settings={settings} onChange={patch} />
      <MicrophoneSection settings={settings} onChange={patch} devices={devices} />
      <SoundsSection settings={settings} onChange={patch} />
      <UpdatesSection />
      <LicenseSection />

      <GeneralSection settings={settings} onChange={patch} />

      <PerAppOverrides />

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        Floating Pill
      </h3>
      <div style={{ display: "grid", gap: 10 }}>
        <div>
          <span style={{ fontSize: 13 }}>Pill style</span>
          <PillStylePicker
            value={settings.pillStyle || "default"}
            onSelect={(v) => save({ ...settings, pillStyle: v })}
            active={active}
          />
        </div>
        <label>
          Pill position
          <select
            style={{ width: "100%", marginTop: 4 }}
            value={settings.pillPosition}
            onChange={(e) => save({ ...settings, pillPosition: e.target.value })}
          >
            {PILL_POSITIONS.map((p) => (
              <option key={p.value} value={p.value}>{p.label}</option>
            ))}
          </select>
        </label>
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <input
            type="checkbox"
            checked={settings.alwaysShowPill}
            onChange={(e) => save({ ...settings, alwaysShowPill: e.target.checked })}
          />
          <span style={{ fontSize: 13 }}>Always show the pill (even when idle)</span>
        </label>
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        App Icon
      </h3>
      <p style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 12 }}>
        Choose the icon shown in the sidebar, window, and menu bar.
      </p>
      <div style={{ display: "flex", gap: 16 }}>
        {APP_ICON_CHOICES.map((choice) => {
          const selected = settings.appIcon === choice.id;
          return (
            <button
              key={choice.id}
              onClick={() => invoke("set_app_icon", { id: choice.id }).catch(console.error)}
              style={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                gap: 8,
                padding: 12,
                borderRadius: 12,
                cursor: "pointer",
                background: "var(--surface)",
                border: selected ? "2px solid var(--accent)" : "1px solid var(--border)",
              }}
            >
              <img
                src={choice.src}
                alt={choice.label}
                width={64}
                height={64}
                style={{ borderRadius: 14 }}
              />
              <span style={{ fontSize: 13, fontWeight: selected ? 600 : 400, color: selected ? "var(--accent)" : "var(--text)" }}>
                {choice.label}
                {selected ? " ✓" : ""}
              </span>
            </button>
          );
        })}
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        Appearance
      </h3>
      <p style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 12 }}>
        Theme and motion for the main window. The floating pill stays dark in every theme.
      </p>
      <div style={{ display: "grid", gap: 10 }}>
        <label style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 13, minWidth: 90 }}>Theme</span>
          <select
            value={settings.theme || "system"}
            onChange={(e) => invoke("set_theme", { theme: e.target.value }).catch(console.error)}
            style={{ minWidth: 140 }}
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <input
            type="checkbox"
            checked={!!settings.reduceMotion}
            onChange={(e) => invoke("set_reduce_motion", { enabled: e.target.checked }).catch(console.error)}
          />
          <span style={{ fontSize: 13 }}>Reduce Motion (follows the OS setting when off)</span>
        </label>
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        Transcripts
      </h3>
      <p style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 12 }}>
        Every dictation is also saved as a plain-text file, grouped by day, in this folder.
      </p>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <input
          value={settings.transcriptsDir || "(default)"}
          placeholder="Leave blank for the default folder"
          onChange={(e) => save({ ...settings, transcriptsDir: e.target.value })}
          style={{
            flex: "1 1 320px",
            minWidth: 240,
            padding: "8px 10px",
            borderRadius: 8,
            border: "1px solid var(--border)",
            background: "var(--surface)",
            fontSize: 13,
            color: "var(--text)",
          }}
        />
        <button
          onClick={() => invoke("reveal_transcripts_dir").catch(console.error)}
          style={{
            padding: "8px 14px",
            borderRadius: 8,
            border: "1px solid var(--border)",
            background: "var(--surface)",
            fontSize: 13,
            fontWeight: 600,
            color: "var(--text)",
            cursor: "pointer",
            whiteSpace: "nowrap",
          }}
        >
          Open folder
        </button>
      </div>
    </div>
  );
}
