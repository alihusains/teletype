import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder, { displayHotkey } from "../components/HotkeyRecorder";
import WebviewHotkeyRecorder, { usesWebviewCapture } from "../components/WebviewHotkeyRecorder";
import { Icon, type IconName } from "../components/Icon";
import { Explainer, Row, Toggle } from "../settings/primitives";
import { WriteFailed } from "../components/StateRows";
import { useWrites } from "../lib/writeGuard";
import type { Settings } from "./SettingsScreen";
import { errorMessage } from "../lib/errors";

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
  saving,
  writeError,
  onDismissError,
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
  saving?: boolean;
  writeError?: string | null;
  onDismissError?: () => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 14px",
        borderTop: first ? "none" : "1px solid var(--border-subtle)",
      }}
    >
      <span
        style={{
          width: 32,
          height: 32,
          borderRadius: 8,
          flex: "0 0 32px",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--surface-2)",
          boxShadow: "var(--shadow-1)",
        }}
      >
        <Icon name={icon} size={16} color="var(--text-secondary)" />
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span
            style={{
              fontSize: "var(--text-sm)",
              fontWeight: 600,
              letterSpacing: "var(--tracking-tight)",
            }}
          >
            {title}
          </span>
          <Explainer text={help} />
        </div>
        <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 2 }}>
          {short}
        </div>
        {status ? (
          <div
            style={{
              display: "inline-block",
              marginTop: 4,
              padding: "1px 8px",
              borderRadius: "var(--radius-pill, 999px)",
              fontSize: "var(--text-xs)",
              fontWeight: 600,
              color: "var(--danger)",
              background: "var(--danger-soft)",
            }}
          >
            {status}
          </div>
        ) : null}
        {/* The error belongs on the row the user just pressed, not at the foot
            of a scrollable screen where it can never be seen. */}
        {writeError ? (
          <div style={{ marginTop: 6 }}>
            <WriteFailed
              message={writeError}
              label={title}
              onDismiss={onDismissError}
            />
          </div>
        ) : null}
      </div>
      {/* While the save is in flight the recorder is inert: a second combo
          committed mid-write starts a competing read-modify-write on the same
          field. Guarded in `save` as well, so this only has to be visible. */}
      <div
        style={{
          pointerEvents: saving ? "none" : "auto",
          opacity: saving ? 0.5 : 1,
        }}
        aria-disabled={saving ? true : undefined}
      >
        {usesWebviewCapture() ? (
          <WebviewHotkeyRecorder value={value} onSave={onChange} />
        ) : (
          <HotkeyRecorder value={value} onSave={onChange} hint={null} />
        )}
      </div>
      {saving ? (
        <span
          role="status"
          style={{
            fontSize: "var(--text-xs)",
            fontWeight: 600,
            color: "var(--text-secondary)",
            whiteSpace: "nowrap",
          }}
        >
          Saving…
        </span>
      ) : null}
      {clearable && value ? (
        <button
          onClick={onClear}
          disabled={saving}
          style={{
            fontSize: "var(--text-xs)",
            fontWeight: 500,
            padding: "4px 10px",
            borderRadius: "var(--radius-sm)",
            border: "1px solid var(--border)",
            background: "transparent",
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
    <section style={{ marginBottom: 20 }}>
      <div
        className="tt-eyebrow"
        style={{
          marginBottom: 8,
        }}
      >
        {title}
      </div>
      <div className="tt-card" style={{ overflow: "hidden" }}>
        {children}
      </div>
    </section>
  );
}

export default function KeybindsScreen() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [transforms, setTransforms] = useState<Transform[]>([]);
  const [error, setError] = useState<string | null>(null);
  const writes = useWrites();

  const refresh = () => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
    invoke<Transform[]>("list_transforms").then(setTransforms).catch(console.error);
  };
  useEffect(refresh, []);

  // One write guard per field: while a hotkey save is in flight the recorder
  // must not commit a second binding, or two saves racing the read-modify-write
  // backend can drop one of them (A-3).
  const save = async (updated: Settings, prev: Settings, key: string) => {
    if (writes.busy(key)) return;
    setSettings(updated);
    const res = await writes.run(key, () => invoke("save_settings", { settings: updated }));
    if (!res.ok) {
      setSettings(prev);
      console.error("save_settings failed; restored previous hotkey");
    }
  };
  const patch = (p: Partial<Settings>, key: string) => {
    if (settings) save({ ...settings, ...p }, settings, key);
  };

  const allBindings = useMemo(() => {
    if (!settings) return [];
    const out: { label: string; key: string }[] = [
      { label: "Record", key: settings.hotkey },
      { label: "Command", key: settings.commandHotkey ?? "" },
      { label: "Paste Last", key: settings.pasteLastHotkey ?? "" },
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

  if (!settings)
    return (
      <div style={{ width: "100%", maxWidth: 860 }}>
        <div
          style={{
            height: 24,
            width: 140,
            borderRadius: 6,
            background: "var(--surface-2)",
            marginBottom: 12,
          }}
        />
        <div
          style={{
            height: 40,
            width: "100%",
            borderRadius: "var(--radius)",
            background: "var(--surface-2)",
          }}
        />
      </div>
    );

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
        setError(errorMessage(e));
        setTransforms((prev) =>
          prev.map((x) => (x.id === id ? { ...x, shortcut: t.shortcut } : x)),
        );
      });
  };

  const saveBinding = (
    field:
      | "hotkey"
      | "cancelHotkey"
      | "quickAddHotkey"
      | "commandHotkey"
      | "pasteLastHotkey",
    next: string,
  ) => {
    const nextTrimmed = next.trim();
    if (field === "hotkey" && !nextTrimmed) {
      setError("The record hotkey cannot be empty. Teletype needs one to start listening.");
      return;
    }
    setError(null);
    // Each field is its own write lane, so saving one hotkey never blocks (or
    // double-fires) another. The guard inside `save` drops a second commit that
    // lands while the first is still in flight, and the row goes inert for the
    // duration so the user can see why the keystroke did nothing.
    patch({ [field]: nextTrimmed } as Partial<Settings>, field);
  };

  // The write state for one field, spread into its row: a rejected save names
  // the row it failed on instead of a banner at the bottom of the scroll.
  const fieldWrite = (field: string) => ({
    saving: writes.busy(field),
    writeError: writes.error(field),
    onDismissError: () => writes.clear(field),
  });

  const enabledTransforms = transforms.filter((t) => t.enabled || t.shortcut);

  // The backend's real reason, shown once for whichever un-rowed lane failed.
  const failedLane = (["reset", "commandWakePhrase"] as const).find((k) => writes.error(k) !== null);

  const laneError = failedLane ? (
    <WriteFailed
      message={writes.error(failedLane) as string}
      label={failedLane === "reset" ? "Reset to defaults" : "Wake phrase"}
      onDismiss={() => writes.clear(failedLane)}
    />
  ) : null;

  return (
    <div style={{ width: "100%", maxWidth: 860 }}>
      <div style={{ marginBottom: 24 }}>
        <h2 className="tt-screen-title">Keybinds</h2>
        <p
          style={{
            color: "var(--text-secondary)",
            fontSize: "var(--text-sm)",
            marginTop: 4,
            marginBottom: 0,
          }}
        >
          Every shortcut Teletype uses, in one place. Tap a key to record a new combination; press{" "}
          <kbd>Esc</kbd> while recording to cancel without changing it.
        </p>
      </div>

      {conflicts.length > 0 && (
        <div
          role="alert"
          style={{
            padding: "10px 14px",
            borderRadius: "var(--radius-sm)",
            background: "var(--danger-soft)",
            color: "var(--danger)",
            fontSize: "var(--text-sm)",
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
          {...fieldWrite("hotkey")}
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
                  {...fieldWrite("cancelHotkey")}
        />
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "10px 14px",
            borderTop: "1px solid var(--border-subtle)",
          }}
        >
          <span
            style={{
              width: 32,
              height: 32,
              borderRadius: 8,
              flex: "0 0 32px",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "var(--surface-2)",
              boxShadow: "var(--shadow-1)",
            }}
          >
            <Icon name="refresh" size={16} color="var(--text-secondary)" />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span
                style={{
                  fontSize: "var(--text-sm)",
                  fontWeight: 600,
                  letterSpacing: "var(--tracking-tight)",
                }}
              >
                Reset to defaults
              </span>
              <Explainer text={`Restores record to ${DEFAULT_RECORD} and cancel to ${DEFAULT_CANCEL}.`} />
            </div>
            <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 2 }}>
              Back to the out-of-the-box shortcuts.
            </div>
          </div>
          <button
            onClick={() => {
              patch(
                { hotkey: DEFAULT_RECORD, cancelHotkey: DEFAULT_CANCEL } as Partial<Settings>,
                "reset",
              );
            }}
            style={{
              fontSize: "var(--text-xs)",
              fontWeight: 500,
              padding: "4px 10px",
              borderRadius: "var(--radius-sm)",
              border: "1px solid var(--border)",
              background: "transparent",
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
                  {...fieldWrite("quickAddHotkey")}
        />
      </GroupCard>

      <GroupCard title="Command mode">
        <KeybindRow
          first
          icon="terminal"
          title="Run a voice command"
          short="Press, then say what to do — Teletype runs it in the app you are in."
          help="Starts a command take instead of dictation: say “open Safari”, “scroll down”, or “search for the nearest coffee shop”. When text is selected in the focused app, the command is applied to that selection instead."
          value={settings.commandHotkey ?? ""}
          onChange={(h) => saveBinding("commandHotkey", h)}
          onClear={() => saveBinding("commandHotkey", "")}
          clearable
          status={conflictFor(settings.commandHotkey ?? "")}
                  {...fieldWrite("commandHotkey")}
        />
        <Row
          label="Wake phrase"
          hint="Start a dictation with “teletype” or “hey teletype” and the rest runs as a command instead. Near-misses (one letter off) count."
          icon={<Icon name="mic" size={16} color="var(--text-secondary)" />}
        >
          <Toggle
            label="Wake phrase"
            checked={settings.commandWakePhrase ?? true}
            onChange={(v) => patch({ commandWakePhrase: v }, "commandWakePhrase")}
          />
        </Row>
      </GroupCard>
      <GroupCard title="Paste Last">
        <KeybindRow
          first
          icon="copy"
          title="Paste Last Dictation"
          short="Re-paste the last dictated text, even after a delivery failure."
          help="Sends the most recent final dictation to the focused app through the same delivery path as a normal paste. If every delivery route fails (rare), the text is held and this brings it back. When empty, the shortcut is disabled."
          value={settings.pasteLastHotkey ?? ""}
          onChange={(h) => saveBinding("pasteLastHotkey", h)}
          onClear={() => saveBinding("pasteLastHotkey", "")}
          clearable
          status={conflictFor(settings.pasteLastHotkey ?? "")}
                  {...fieldWrite("pasteLastHotkey")}
        />
      </GroupCard>

      <GroupCard title="AI Polish shortcuts">
        {enabledTransforms.length === 0 && (
          <div style={{ padding: "12px 14px", fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>
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
          <div style={{ padding: "12px 14px", fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>
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
              padding: "8px 14px",
              borderTop: i === 0 ? "none" : "1px solid var(--border-subtle)",
            }}
          >
            <span style={{ flex: 1, fontSize: "var(--text-sm)" }}>{b.label}</span>
            <code
              style={{
                fontSize: "var(--text-xs)",
                fontWeight: 600,
                fontVariantNumeric: "tabular-nums",
                padding: "3px 10px",
                borderRadius: 6,
                background: "var(--surface-2)",
                boxShadow: "var(--shadow-1)",
                color: "var(--text-secondary)",
              }}
            >
              {displayHotkey(b.key)}
            </code>
          </div>
        ))}
      </GroupCard>

      {/* The five hotkey rows each carry their own error; this is the fallback
          for the two write lanes that have no row of their own. */}
      {laneError}

      {error ? (
        <div
          role="alert"
          style={{
            padding: "10px 14px",
            borderRadius: "var(--radius-sm)",
            background: "var(--danger-soft)",
            color: "var(--danger)",
            fontSize: "var(--text-sm)",
            marginTop: 8,
          }}
        >
          {error}
        </div>
      ) : null}
    </div>
  );
}
