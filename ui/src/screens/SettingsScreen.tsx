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
}

interface Permission {
  kind: string;
  granted: boolean;
}

export default function SettingsScreen() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [permissions, setPermissions] = useState<Permission[]>([]);
  const [devices, setDevices] = useState<{ id: string; name: string; is_default: boolean }[]>([]);

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
    </div>
  );
}
