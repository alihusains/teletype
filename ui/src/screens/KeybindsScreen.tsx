import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder, { displayHotkey } from "../components/HotkeyRecorder";
import { Icon, type IconName } from "../components/Icon";
import { Explainer } from "../settings/primitives";
import type { Settings } from "./SettingsScreen";

interface Transform {
  id: string;
  name: string;
  shortcut: string;
  enabled: boolean;
  builtIn: boolean;
}

const DEFAULT_RECORD = "Cmd+Shift+Space";
const DEFAULT_CANCEL = "Escape";

/// One keybind row in the EnviousWispr style: icon tile + title + short line
/// + a "?" that carries the full explanation, with the key field and a
/// Change pill on the right.
function KeybindRow({
  icon,
  title,
  short,
  help,
  value,
  onChange,
  onClear,
  clearable,
  first,
  status,
}: {
  icon: IconName;
  title: string;
  short: string;
  help: string;
  value: string;
  onChange: (v: string) => void;
  onClear?: () => void;
  clearable?: boolean;
  first?: boolean;
  status?: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "12px 14px",
        borderTop: first ? "none" : "1px solid var(--border-subtle)",
      }}
    >
      <span
        style={{
          width: 32,
          height: 32,
          borderRadius: 9,
          flex: "0 0 32px",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--accent-soft)",
        }}
      >
        <Icon name={icon} size={16} color="var(--accent)" />
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ fontSize: "var(--text-sm)", fontWeight: 600 }}>{title}</span>
          <Explainer text={help} />
        </div>
        <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 2 }}>
          {short}
        </div>
        {status ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--accent)", marginTop: 3 }}>{status}</div>
        ) : null}
      </div>
      <HotkeyRecorder value={value} onSave={onChange} />
      {clearable && value ? (
        <button
          onClick={onClear}
          style={{
            fontSize: 12,
            fontWeight: 600,
            padding: "5px 12px",
            borderRadius: 999,
            border: "1px solid var(--border)",
            background: "var(--surface-2)",
            color: "var(--text-secondary)",
            cursor: "pointer",
            whiteSpace: "nowrap",
          }}
        >
          Clear
        </button>
      ) : null}
    </div>
  );
}

function GroupCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section style={{ marginBottom: 24 }}>
      <div
        style={{
          fontSize: 11,
          fontWeight: 700,
          letterSpacing: "0.08em",
          textTransform: "uppercase",
          color: "var(--text-tertiary)",
          marginBottom: 8,
        }}
      >
        {title}
      </div>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {children}
      </div>
    </section>
  );
}

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

  const allBindings = useMemo(() => {
    if (!settings) return [];
    const out: { label: string; key: string }[] = [
      { label: "Record", key: settings.hotkey },
      { label: "Cancel dictation", key: settings.cancelHotkey },
      { label: "Quick Add", key: settings.quickAddHotkey ?? "" },
    ];
    for (const t of transforms) {
      if (t.enabled && t.shortcut) out.push({ label: t.name, key: t.shortcut });
    }
    return out.filter((b) => (b.key ?? "").trim() !== "");
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

  const conflictFor = (key: string) => {
    const k = (key ?? "").trim().toLowerCase();
    const hit = conflicts.find(([ck]) => ck === k);
    if (!hit) return undefined;
    const others = hit[1].filter((l) => l !== "me");
    return `Shares keys with: ${others.join(", ")}`;
  };

  const updateTransformShortcut = (id: string, shortcut: string) => {
    const t = transforms.find((x) => x.id === id);
    if (!t) return;
    setTransforms((prev) => prev.map((x) => (x.id === id ? { ...x, shortcut } : x)));
    invoke("update_transform", {
      transform: { ...t, shortcut, updatedAt: Date.now() },
    })
      .then(refresh)
      .catch((e) => {
        console.error(e);
        setError(String(e));
        setTransforms((prev) =>
          prev.map((x) => (x.id === id ? { ...x, shortcut: t.shortcut } : x)),
        );
      });
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

  const enabledTransforms = transforms.filter((t) => t.enabled || t.shortcut);

  return (
    <div style={{ width: "100%", maxWidth: 860 }}>
      <h2 style={{ fontSize: 18, fontWeight: 600, margin: 0 }}>Keybinds</h2>
      <p style={{ color: "var(--text-secondary)", marginTop: 4, marginBottom: 20, fontSize: 13 }}>
        Every shortcut Teletype uses, in one place. Tap a key to record a new combination; press{" "}
        <kbd>Esc</kbd> while recording to cancel without changing it.
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
          {conflicts.map(([key, labels]) => (
            <div key={key} style={{ marginTop: 4 }}>
              <strong>{displayHotkey(key)}</strong> → {labels.join(", ")}
            </div>
          ))}
        </div>
      )}

      <GroupCard title="Recording">
        <KeybindRow
          first
          icon="mic"
          title="Start / stop recording"
          short="Hold to talk, or press once to start and again to stop."
          help="This keybind starts and stops dictation. In tap mode one press starts and the next stops; in push-to-talk mode holding records and releasing stops."
          value={settings.hotkey}
          onChange={(h) => saveBinding("hotkey", h)}
          status={conflictFor(settings.hotkey)}
        />
        <KeybindRow
          icon="alert"
          title="Cancel dictation"
          short="Abandons the take in progress and inserts nothing."
          help="Press to throw away the current take without inserting any text. The way out when a sentence goes wrong. Escape Recovery keeps cancelled takes in History for 24 hours."
          value={settings.cancelHotkey}
          onChange={(h) => saveBinding("cancelHotkey", h)}
          onClear={() => saveBinding("cancelHotkey", "")}
          clearable
          status={conflictFor(settings.cancelHotkey)}
        />
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "12px 14px",
            borderTop: "1px solid var(--border-subtle)",
          }}
        >
          <span
            style={{
              width: 32,
              height: 32,
              borderRadius: 9,
              flex: "0 0 32px",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "var(--accent-soft)",
            }}
          >
            <Icon name="refresh" size={16} color="var(--accent)" />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span style={{ fontSize: "var(--text-sm)", fontWeight: 600 }}>Reset to defaults</span>
              <Explainer text={`Restores record to ${DEFAULT_RECORD} and cancel to ${DEFAULT_CANCEL}.`} />
            </div>
            <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 2 }}>
              Back to the out-of-the-box shortcuts.
            </div>
          </div>
          <button
            onClick={() => {
              saveBinding("hotkey", DEFAULT_RECORD);
              saveBinding("cancelHotkey", DEFAULT_CANCEL);
            }}
            style={{
              fontSize: 12,
              fontWeight: 600,
              padding: "5px 12px",
              borderRadius: 999,
              border: "1px solid var(--border)",
              background: "var(--surface-2)",
              color: "var(--text)",
              cursor: "pointer",
              whiteSpace: "nowrap",
            }}
          >
            Reset
          </button>
        </div>
      </GroupCard>

      <GroupCard title="Quick Add">
        <KeybindRow
          first
          icon="zap"
          title="Add selected word"
          short="Highlight any word in any app, then press this to add it to your dictionary."
          help="The fix for a name or term Teletype keeps getting wrong. Select the misheard word anywhere and press the shortcut. Terminal windows do not share their selection, so it will not work there. When empty, Quick Add is disabled."
          value={settings.quickAddHotkey ?? ""}
          onChange={(h) => saveBinding("quickAddHotkey", h)}
          onClear={() => saveBinding("quickAddHotkey", "")}
          clearable
          status={conflictFor(settings.quickAddHotkey ?? "")}
        />
      </GroupCard>

      <GroupCard title="AI Polish shortcuts">
        {enabledTransforms.length === 0 && (
          <div style={{ padding: "12px 14px", fontSize: 13, color: "var(--text-secondary)" }}>
            Create a preset in AI Polish to give it a shortcut.
          </div>
        )}
        {enabledTransforms.map((t, i) => (
          <KeybindRow
            key={t.id}
            first={i === 0}
            icon="wand"
            title={t.name}
            short={t.builtIn ? "Built-in preset" : "Your preset"}
            help="Select text anywhere, press the shortcut, and this preset rewrites it in place. Two-key combos like Alt+1 work."
            value={t.shortcut}
            onChange={(h) => updateTransformShortcut(t.id, h)}
            onClear={() => updateTransformShortcut(t.id, "")}
            clearable
            status={conflictFor(t.shortcut)}
          />
        ))}
      </GroupCard>

      <GroupCard title="Currently registered">
        {allBindings.length === 0 && (
          <div style={{ padding: "12px 14px", fontSize: 13, color: "var(--text-secondary)" }}>
            Nothing registered. Set a keybind above.
          </div>
        )}
        {allBindings.map((b, i) => (
          <div
            key={`${b.label}-${b.key}`}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "10px 14px",
              borderTop: i === 0 ? "none" : "1px solid var(--border-subtle)",
            }}
          >
            <span style={{ flex: 1, fontSize: "var(--text-sm)" }}>{b.label}</span>
            <code
              style={{
                fontSize: 12.5,
                padding: "3px 10px",
                borderRadius: 6,
                background: "var(--surface-2)",
                border: "1px solid var(--border)",
                color: "var(--text-secondary)",
              }}
            >
              {displayHotkey(b.key)}
            </code>
          </div>
        ))}
      </GroupCard>

      {error ? (
        <div
          role="alert"
          style={{
            padding: "10px 14px",
            borderRadius: "var(--radius-sm)",
            border: "1px solid var(--danger)",
            background: "color-mix(in srgb, var(--danger) 8%, transparent)",
            color: "var(--danger)",
            fontSize: 13,
            marginTop: 8,
          }}
        >
          {error}
        </div>
      ) : null}
    </div>
  );
}
