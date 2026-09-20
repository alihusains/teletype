import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../components/HotkeyRecorder";

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
  app_icon: string;
}

interface Permission {
  kind: string;
  granted: boolean;
}

const APP_ICON_CHOICES: { id: string; label: string; src: string }[] = [
  { id: "white", label: "Light", src: "/teletype-app-icon-white.png" },
  { id: "blue", label: "Blue", src: "/teletype-app-icon-blue.png" },
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
            <option value="en">English</option>
            <option value="auto">Auto-detect</option>
          </select>
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
    </div>
  );
}
