import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../components/HotkeyRecorder";
import { Chip, Row, Section, SecondaryButton } from "../settings/primitives";
import type { Settings } from "./SettingsScreen";

interface Transform {
  id: string;
  name: string;
  shortcut: string;
  enabled: boolean;
  builtIn: boolean;
}

// The dedicated Keybinds screen (item 10): every keybinding the user can
// change, in one place, the way the reference apps do it — record / cancel /
// clear per binding, plus the Quick Add binding (item 8) and a conflict
// warning when two bindings fight over the same chord.
const DEFAULT_RECORD = "Cmd+Shift+Space";
const DEFAULT_CANCEL = "Escape";

export default function KeybindsScreen() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [transforms, setTransforms] = useState<Transform[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
    invoke<Transform[]>("list_transforms").then(setTransforms).catch(console.error);
  };
  useEffect(refresh, []);

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
  const patch = (p: Partial<Settings>) => {
    if (settings) save({ ...settings, ...p });
  };

  // Every active binding on the machine, for the conflict check.
  const allBindings = useMemo(() => {
    if (!settings) return [];
    const out: { label: string; key: string }[] = [
      { label: "Record", key: settings.hotkey },
      { label: "Cancel dictation", key: settings.cancelHotkey },
      { label: "Quick Add", key: settings.quickAddHotkey },
    ];
    for (const t of transforms) {
      if (t.enabled && t.shortcut) out.push({ label: t.name, key: t.shortcut });
    }
    return out.filter((b) => b.key.trim() !== "");
  }, [settings, transforms]);

  const conflicts = useMemo(() => {
    const byKey = new Map<string, string[]>();
    for (const b of allBindings) {
      const k = b.key.trim().toLowerCase();
      byKey.set(k, [...(byKey.get(k) ?? []), b.label]);
    }
    return [...byKey.entries()].filter(([, labels]) => labels.length > 1);
  }, [allBindings]);

  if (!settings) return <p>Loading…</p>;

  const updateTransformShortcut = (id: string, shortcut: string) => {
    const t = transforms.find((x) => x.id === id);
    if (!t) return;
    invoke("update_transform", {
      transform: { ...t, shortcut, updatedAt: Date.now() },
    })
      .then(refresh)
      .catch(console.error);
  };

  const saveBinding = (field: "hotkey" | "cancelHotkey" | "quickAddHotkey", next: string) => {
    const nextTrimmed = next.trim();
    if (field === "hotkey" && !nextTrimmed) {
      setError("The record hotkey cannot be empty. Teletype needs one to start listening.");
      return;
    }
    setError(null);
    patch({ [field]: nextTrimmed } as Partial<Settings>);
  };

  return (
    <div style={{ width: "100%", maxWidth: 860 }}>
      <h2 style={{ fontSize: 18, fontWeight: 600, marginBottom: 4 }}>Keybinds</h2>
      <p style={{ color: "var(--text-secondary)", marginTop: 0, marginBottom: 20 }}>
        Every shortcut Teletype uses, in one place. Tap a key to record a new
        combination; press <kbd>Esc</kbd> while recording to cancel without
        changing it.
      </p>

      {conflicts.length > 0 && (
        <div
          role="alert"
          style={{
            padding: "10px 14px",
            borderRadius: "var(--radius-sm, 8px)",
            border: "1px solid var(--danger)",
            background: "color-mix(in srgb, var(--danger) 8%, transparent)",
            color: "var(--danger)",
            fontSize: 13,
            marginBottom: 20,
          }}
        >
          These bindings share the same keys — the last one registered wins:
          {conflicts
            .map(([key, labels]) => (
              <div key={key} style={{ marginTop: 4 }}>
                <strong>{key}</strong> → {labels.join(", ")}
              </div>
            ))}
        </div>
      )}

      <Section
        title="Dictation"
        hint="The record hotkey starts and stops dictation. The cancel hotkey throws away the take in progress and inserts nothing — the way out when a sentence goes wrong."
      >
        <Row
          first
          label="Record"
          hint="Hold to talk, or switch to tap-to-start in Settings."
        >
          <HotkeyRecorder value={settings.hotkey} onSave={(h) => saveBinding("hotkey", h)} />
        </Row>
        <Row label="Cancel dictation" hint="Abandons the take without inserting anything.">
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <HotkeyRecorder value={settings.cancelHotkey} onSave={(h) => saveBinding("cancelHotkey", h)} />
            {settings.cancelHotkey ? (
              <SecondaryButton onClick={() => saveBinding("cancelHotkey", "")}>Clear</SecondaryButton>
            ) : null}
          </div>
        </Row>
        <Row label="Defaults">
          <SecondaryButton
            onClick={() => {
              saveBinding("hotkey", DEFAULT_RECORD);
              saveBinding("cancelHotkey", DEFAULT_CANCEL);
            }}
          >
            Reset record + cancel to defaults
          </SecondaryButton>
        </Row>
      </Section>

      <Section
        title="Quick Add"
        hint="Highlight any word in any app and press this key to add it to your dictionary — the fix for a name or term Teletype keeps getting wrong."
      >
        <Row
          first
          label="Add selected word"
          hint="When empty, Quick Add is disabled."
        >
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <HotkeyRecorder value={settings.quickAddHotkey} onSave={(h) => saveBinding("quickAddHotkey", h)} />
            {settings.quickAddHotkey ? (
              <SecondaryButton onClick={() => saveBinding("quickAddHotkey", "")}>Clear</SecondaryButton>
            ) : null}
          </div>
        </Row>
      </Section>

      <Section
        title="AI Polish shortcuts"
        hint="Select text anywhere, press the shortcut, and the chosen preset polishes it in place. Record a shortcut on any preset from the AI Polish screen; manage them here."
      >
        {transforms.length === 0 && (
          <Row first label="No presets yet">
            <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>
              Create a preset in AI Polish to give it a shortcut.
            </span>
          </Row>
        )}
        {transforms.map((t, i) => (
          <Row key={t.id} first={i === 0} label={t.name} hint={t.builtIn ? "Built-in preset" : "Your preset"}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <HotkeyRecorder value={t.shortcut} onSave={(h) => updateTransformShortcut(t.id, h)} />
              {t.shortcut ? (
                <SecondaryButton onClick={() => updateTransformShortcut(t.id, "")}>Clear</SecondaryButton>
              ) : null}
            </div>
          </Row>
        ))}
      </Section>

      <Section
        title="Currently registered"
        hint="A snapshot of every active global binding, so you can see what the OS is actually listening for."
      >
        {allBindings.length === 0 && (
          <Row first label="Nothing registered">
            <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>Set a keybind above.</span>
          </Row>
        )}
        {allBindings.map((b, i) => (
          <Row key={`${b.label}-${b.key}`} first={i === 0} label={b.label}>
            <Chip>{b.key}</Chip>
          </Row>
        ))}
      </Section>

      {error ? (
        <div
          role="alert"
          style={{
            padding: "10px 14px",
            borderRadius: "var(--radius-sm, 8px)",
            border: "1px solid var(--danger)",
            background: "color-mix(in srgb, var(--danger) 8%, transparent)",
            color: "var(--danger)",
            fontSize: 13,
          }}
        >
          {error}
        </div>
      ) : null}
    </div>
  );
}
