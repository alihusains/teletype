import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../components/HotkeyRecorder";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";

interface Settings {
  hotkey: string;
  recording_mode: string;
  selected_speech_model: string;
  language: string;
  input_device: string;
  restore_clipboard: boolean;
  auto_apply_transform: boolean;
  show_tray_icon: boolean;
  has_completed_onboarding: boolean;
  selected_llm_model: string;
  typing_autotext_enabled: boolean;
  remove_filler_words: boolean;
  filler_words: string[];
  pill_position: string;
  always_show_pill: boolean;
  pill_style: string;
  app_icon: string;
  transcripts_dir: string;
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

export default function SettingsScreen() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [permissions, setPermissions] = useState<Permission[]>([]);
  const [devices, setDevices] = useState<{ id: string; name: string; is_default: boolean }[]>([]);
  const [newWord, setNewWord] = useState("");

  const speechLanguages = useSpeechLanguages();

  useEffect(() => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
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
    if (settings.filler_words.includes(word)) return;
    save({ ...settings, filler_words: [...settings.filler_words, word] });
    setNewWord("");
  };

  const removeFillerWord = (word: string) => {
    if (!settings) return;
    save({ ...settings, filler_words: settings.filler_words.filter((w) => w !== word) });
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
                }
              }}
            />
          </div>
        </div>
        <label>
          Recording Mode
          <select
            style={{ width: "100%", marginTop: 4 }}
            value={settings.recording_mode}
            onChange={(e) => save({ ...settings, recording_mode: e.target.value })}
          >
            <option value="hold">Hold to talk</option>
            <option value="toggle">Push to talk</option>
          </select>
        </label>
        <label>
          Microphone
          <select
            style={{ width: "100%", marginTop: 4 }}
            value={settings.input_device}
            onChange={(e) => save({ ...settings, input_device: e.target.value })}
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
            <option value="auto">Auto-detect (99 languages)</option>
            {speechLanguages.map((lang) => (
              <option key={lang.code} value={lang.code}>{lang.name}</option>
            ))}
          </select>
          <span style={{ fontSize: 12, color: "var(--text-secondary)", display: "block", marginTop: 4 }}>
            Auto: Whisper detects the spoken language. Pick a specific language to lock it.
          </span>
        </label>
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 8 }}>
        Behavior
      </h3>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {([
          ["restore_clipboard", "Restore clipboard after insertion"],
          ["auto_apply_transform", "Auto-apply transform after dictation"],
          ["show_tray_icon", "Show menu bar / tray icon"],
          ["typing_autotext_enabled", "Expand AutoText while typing"],
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
      </div>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginTop: 24, marginBottom: 8 }}>
        Filler Words
      </h3>
      <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", marginBottom: 12 }}>
        <input
          type="checkbox"
          checked={settings.remove_filler_words}
          onChange={(e) => save({ ...settings, remove_filler_words: e.target.checked })}
        />
        <span style={{ fontSize: 13 }}>
          Automatically remove filler words like <em>um</em>, <em>uh</em>, <em>er</em> from transcriptions
        </span>
      </label>
      {settings.remove_filler_words && (
        <div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
            Filler words to remove:
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 10 }}>
            {settings.filler_words.map((word) => (
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
            {settings.filler_words.length === 0 && (
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
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(180px, 1fr))", gap: 8, marginTop: 6 }}>
            {PILL_STYLES.map((style) => {
              const selected = (settings.pill_style || "default") === style.value;
              return (
                <button
                  key={style.value}
                  onClick={() => save({ ...settings, pill_style: style.value })}
                  style={{
                    textAlign: "left",
                    padding: "10px 12px",
                    borderRadius: 10,
                    cursor: "pointer",
                    background: "var(--surface)",
                    border: selected ? "2px solid var(--accent)" : "1px solid var(--border)",
                  }}
                >
                  <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 4 }}>
                    <span style={{ fontSize: 13, fontWeight: 600, color: selected ? "var(--accent)" : "var(--text)" }}>
                      {style.label}
                    </span>
                    {selected && <span style={{ color: "var(--accent)", fontSize: 12 }}>✓</span>}
                  </div>
                  <div style={{ fontSize: 11.5, color: "var(--text-secondary)", lineHeight: 1.4 }}>{style.hint}</div>
                </button>
              );
            })}
          </div>
        </div>
        <label>
          Pill position
          <select
            style={{ width: "100%", marginTop: 4 }}
            value={settings.pill_position}
            onChange={(e) => save({ ...settings, pill_position: e.target.value })}
          >
            {PILL_POSITIONS.map((p) => (
              <option key={p.value} value={p.value}>{p.label}</option>
            ))}
          </select>
        </label>
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <input
            type="checkbox"
            checked={settings.always_show_pill}
            onChange={(e) => save({ ...settings, always_show_pill: e.target.checked })}
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
          const selected = settings.app_icon === choice.id;
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
        Transcripts
      </h3>
      <p style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 12 }}>
        Every dictation is also saved as a plain-text file, grouped by day, in this folder.
      </p>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <input
          value={settings.transcripts_dir || "(default)"}
          placeholder="Leave blank for the default folder"
          onChange={(e) => save({ ...settings, transcripts_dir: e.target.value })}
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
