import type { CSSProperties } from "react";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../components/HotkeyRecorder";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";
import { RainbowLips, RainbowMeter, rainbowColor } from "../pill";

interface Settings {
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
  pillStyle: string;
  appIcon: string;
  transcriptsDir: string;
  vadAutoStop: boolean;
  vadSilenceMs: number;
  enableDeveloperTab: boolean;
  theme: string;
  reduceMotion: boolean;
}

interface Permission {
  kind: string;
  granted: boolean;
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
// cards animate together exactly like the live pill.
function usePillPreviewLevels(): number[] {
  const [levels, setLevels] = useState<number[]>(() => new Array(34).fill(0));
  const startRef = useRef(Date.now());
  useEffect(() => {
    const start = startRef.current;
    const id = window.setInterval(() => {
      const t = (Date.now() - start) / 1000;
      const v = Math.max(0, Math.min(1, 0.55 * Math.sin(t * 2.1) + 0.3 * Math.sin(t * 7.7) + Math.abs(Math.sin(t * 0.3)) * 0.6));
      setLevels((prev) => [...prev.slice(1), v]);
    }, 33);
    return () => window.clearInterval(id);
  }, []);
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

function PillStylePicker({ value, onSelect }: { value: string; onSelect: (v: string) => void }) {
  const levels = usePillPreviewLevels();
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

export default function SettingsScreen() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [permissions, setPermissions] = useState<Permission[]>([]);
  const [devices, setDevices] = useState<{ id: string; name: string; is_default: boolean }[]>([]);
  const [newWord, setNewWord] = useState("");
  const [hotkeyConflict, setHotkeyConflict] = useState<{ binding: string; message: string } | null>(null);

  const speechLanguages = useSpeechLanguages();

  // A launch-time registration failure only logs on the backend; without
  // this the shortcut is silently dead. Refresh after mount and after every
  // hotkey save; the backend only reports a conflict for the current binding.
  const refreshHotkeyConflict = async (binding: string) => {
    try {
      const c = await invoke<{ binding: string; message: string } | null>("hotkey_conflict");
      setHotkeyConflict(c && c.binding === binding ? c : null);
    } catch {
      setHotkeyConflict(null);
    }
  };

  useEffect(() => {
    invoke<Settings>("get_settings").then((s) => {
      setSettings(s);
      refreshHotkeyConflict(s.hotkey);
    }).catch(console.error);
    invoke<Permission[]>("get_permissions").then(setPermissions).catch(console.error);
    invoke<{ id: string; name: string; is_default: boolean }[]>("list_input_devices").then(setDevices).catch(console.error);
  }, []);

  const save = async (updated: Settings) => {
    setSettings(updated);
    await invoke("save_settings", { settings: updated }).catch(console.error);
  };

  const requestPermission = async (kind: string) => {
    await invoke("request_permission", { kind }).catch(console.error);
    setTimeout(() => invoke<Permission[]>("get_permissions").then(setPermissions), 1000);
  };

  const addFillerWord = () => {
    if (!settings || !newWord.trim()) return;
    const word = newWord.trim().toLowerCase();
    if (settings.fillerWords.includes(word)) return;
    save({ ...settings, fillerWords: [...settings.fillerWords, word] });
    setNewWord("");
  };

  const removeFillerWord = (word: string) => {
    if (!settings) return;
    save({ ...settings, fillerWords: settings.fillerWords.filter((w) => w !== word) });
  };

  if (!settings) return <p>Loading…</p>;

  return (
    <div>
      <h2 style={{ fontSize: 18, fontWeight: 600, marginBottom: 16 }}>Settings</h2>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 8 }}>
        Permissions
      </h3>
      <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 24 }}>
        {permissions.map((p) => (
          <div key={p.kind} style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ flex: 1, textTransform: "capitalize" }}>{p.kind}</span>
            {p.granted ? (
              <span style={{ color: "var(--success)", fontSize: 12 }}>✓ Granted</span>
            ) : (
              <button onClick={() => requestPermission(p.kind)}>Grant</button>
            )}
          </div>
        ))}
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 8 }}>
        Dictation
      </h3>
      <div style={{ display: "grid", gap: 10, marginBottom: 24 }}>
        <div>
          <span style={{ fontSize: 13 }}>Hotkey</span>
          <div style={{ marginTop: 6 }}>
            <HotkeyRecorder
              value={settings.hotkey}
              onSave={async (hotkey) => {
                try {
                  await save({ ...settings, hotkey });
                } catch (e) {
                  console.error(e);
                } finally {
                  // A save-time failure surfaces through save(); a failure
                  // from launch (or an older binding) only shows here.
                  refreshHotkeyConflict(hotkey);
                }
              }}
            />
          </div>
          {hotkeyConflict && (
            <div style={{ marginTop: 6, fontSize: 12, color: "var(--warning, #92400e)" }} role="alert">
              {hotkeyConflict.message}
            </div>
          )}
        </div>
        <label>
          Recording Mode
          <select
            style={{ width: "100%", marginTop: 4 }}
            value={settings.recordingMode}
            onChange={(e) => save({ ...settings, recordingMode: e.target.value })}
          >
            <option value="hold">Hold to talk</option>
            <option value="toggle">Push to talk</option>
          </select>
        </label>
        <label>
          Microphone
          <select
            style={{ width: "100%", marginTop: 4 }}
            value={settings.inputDevice}
            onChange={(e) => save({ ...settings, inputDevice: e.target.value })}
          >
            <option value="">System default</option>
            {devices.map((d) => (
              <option key={d.id} value={d.id}>{d.name}{d.is_default ? " (default)" : ""}</option>
            ))}
          </select>
        </label>
        <label>
          Language
          <select
            style={{ width: "100%", marginTop: 4 }}
            value={settings.language}
            onChange={(e) => save({ ...settings, language: e.target.value })}
          >
            {/* BUG-008: count derived from the live list so label and options never drift */}
            <option value="auto">Auto-detect ({speechLanguages.length} languages)</option>
            {speechLanguages.map((lang) => (
              <option key={lang.code} value={lang.code}>{lang.name}</option>
            ))}
          </select>
          <span style={{ fontSize: 12, color: "var(--text-secondary)", display: "block", marginTop: 4 }}>
            Auto: Whisper detects the spoken language. Pick a specific language to lock it.
          </span>
        </label>
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <input
            type="checkbox"
            checked={!!settings.vadAutoStop}
            onChange={(e) => save({ ...settings, vadAutoStop: e.target.checked })}
          />
          Stop automatically after a pause
        </label>
        {settings.vadAutoStop && (
          <label>
            Stop after a pause of {settings.vadSilenceMs} ms
            <input
              type="range"
              min={300}
              max={2000}
              step={100}
              value={settings.vadSilenceMs}
              onChange={(e) => save({ ...settings, vadSilenceMs: Number(e.target.value) })}
              style={{ width: "100%", marginTop: 4 }}
            />
            <span style={{ fontSize: 12, color: "var(--text-secondary)", display: "block", marginTop: 4 }}>
              Applies to hands-free recording (double-tap the hotkey to start).
            </span>
          </label>
        )}
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 8 }}>
        Behavior
      </h3>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {([
          ["restoreClipboard", "Restore clipboard after insertion"],
          [
            "keepTextOnClipboard",
            "Keep dictated text on the clipboard (adds to clipboard history)",
          ],
          ["autoApplyTransform", "Auto-apply transform after dictation"],
          ["showTrayIcon", "Show menu bar / tray icon"],
          ["typingAutotextEnabled", "Expand AutoText while typing"],
        ] as const).map(([key, label]) => (
          <label key={key} style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
            <input
              type="checkbox"
              checked={settings[key] as boolean}
              onChange={(e) => save({ ...settings, [key]: e.target.checked })}
            />
            {label}
          </label>
        ))}
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <input
            type="checkbox"
            checked={settings.enableDeveloperTab}
            onChange={(e) => save({ ...settings, enableDeveloperTab: e.target.checked })}
          />
          Show Developer tab
        </label>
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        Filler Words
      </h3>
      <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", marginBottom: 12 }}>
        <input
          type="checkbox"
          checked={settings.removeFillerWords}
          onChange={(e) => save({ ...settings, removeFillerWords: e.target.checked })}
        />
        <span style={{ fontSize: 13 }}>
          Automatically remove filler words like <em>um</em>, <em>uh</em>, <em>er</em> from transcriptions
        </span>
      </label>
      {settings.removeFillerWords && (
        <div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
            Filler words to remove:
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 10 }}>
            {settings.fillerWords.map((word) => (
              <span
                key={word}
                style={{
                  display: "inline-flex", alignItems: "center", gap: 4,
                  padding: "4px 10px", background: "var(--surface)",
                  borderRadius: 16, fontSize: 13,
                  border: "1px solid var(--border)",
                }}
              >
                {word}
                <button
                  onClick={() => removeFillerWord(word)}
                  style={{
                    background: "none", border: "none", cursor: "pointer",
                    color: "var(--text-secondary)", fontSize: 14, padding: 0,
                    lineHeight: 1,
                  }}
                  title={`Remove ${word}`}
                >
                  ×
                </button>
              </span>
            ))}
            {settings.fillerWords.length === 0 && (
              <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>No filler words</span>
            )}
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              style={{ flex: 1, maxWidth: 200 }}
              placeholder="Add a word…"
              value={newWord}
              onChange={(e) => setNewWord(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && addFillerWord()}
            />
            <button onClick={addFillerWord} disabled={!newWord.trim()}>Add</button>
          </div>
        </div>
      )}

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        Floating Pill
      </h3>
      <div style={{ display: "grid", gap: 10 }}>
        <div>
          <span style={{ fontSize: 13 }}>Pill style</span>
          <PillStylePicker
            value={settings.pillStyle || "default"}
            onSelect={(v) => save({ ...settings, pillStyle: v })}
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
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <input
            type="checkbox"
            checked={settings.livePreviewEnabled}
            onChange={(e) => save({ ...settings, livePreviewEnabled: e.target.checked })}
          />
          <span style={{ fontSize: 13 }}>
            Show live transcript in the pill while recording
            <span style={{ display: "block", fontSize: 12, color: "var(--text-secondary)" }}>
              Words appear in the pill as you speak. Turn off to keep the pill quiet.
            </span>
          </span>
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
