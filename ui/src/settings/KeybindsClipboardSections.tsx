// Key binds and Clipboard sections, ported from Envious Wispr's interaction
// model onto Teletype's tokens.
//
// Built here rather than by a pi worker because that task hung; the hotkey
// capture flow is HotkeyRecorder, which already exists and already works, so
// this file is mostly deciding what is worth saying next to it.

import { useState } from "react";
import HotkeyRecorder from "../components/HotkeyRecorder";
import type { Settings } from "../screens/SettingsScreen";
import { Chip, Explainer, Row, Section, SecondaryButton, Toggle } from "./primitives";

const DEFAULT_RECORD = "Cmd+Shift+Space";
const DEFAULT_CANCEL = "Escape";

export function KeybindsSection({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const [error, setError] = useState<string | null>(null);

  // Both bindings have to be mutually exclusive. If they were the same chord,
  // cancelling would fire the record release too, and the take could never be
  // stopped: exactly the failure that makes a "cancel" control worse than none.
  const saveBinding = (field: "hotkey" | "cancelHotkey", next: string) => {
    const other = field === "hotkey" ? settings.cancelHotkey : settings.hotkey;
    const nextTrimmed = next.trim();
    if (!nextTrimmed) {
      // An empty record hotkey leaves no way to start a dictation at all.
      setError(
        field === "hotkey"
          ? "The record hotkey cannot be empty. Teletype needs one to start listening."
          : "",
      );
      if (field === "cancelHotkey") {
        onChange({ cancelHotkey: "" });
        setError(null);
      }
      return;
    }
    if (other && other === nextTrimmed) {
      setError(
        field === "hotkey"
          ? "This is already the cancel hotkey. Recording and cancelling need different keys."
          : "This is already the record hotkey. Recording and cancelling need different keys.",
      );
      return;
    }
    setError(null);
    onChange({ [field]: nextTrimmed } as Partial<Settings>);
  };

  const resetDefaults = () => {
    setError(null);
    onChange({ hotkey: DEFAULT_RECORD, cancelHotkey: DEFAULT_CANCEL });
  };

  const atDefaults =
    settings.hotkey === DEFAULT_RECORD && settings.cancelHotkey === DEFAULT_CANCEL;

  return (
    <Section
      title="Key binds"
      hint="The record hotkey starts and stops dictation. The cancel hotkey throws away the take in progress and inserts nothing, which is the way out when you realise halfway through a sentence that it is wrong."
    >
      <Row
        first
        label="Record hotkey"
        hint="Hold to talk, or switch Dictation to tap-to-start below."
      >
        <HotkeyRecorder value={settings.hotkey} onSave={(h) => saveBinding("hotkey", h)} />
      </Row>

      <Row
        label="Cancel dictation"
        hint="Abandons the take without inserting anything."
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Chip>{settings.cancelHotkey || "Disabled"}</Chip>
          <HotkeyRecorder
            value={settings.cancelHotkey}
            onSave={(h) => saveBinding("cancelHotkey", h)}
          />
          {settings.cancelHotkey ? (
            <SecondaryButton onClick={() => saveBinding("cancelHotkey", "")}>
              Clear
            </SecondaryButton>
          ) : null}
        </div>
      </Row>

      <Row label="Defaults">
        <SecondaryButton onClick={resetDefaults} disabled={atDefaults}>
          Reset to defaults
        </SecondaryButton>
      </Row>

      {error ? (
        <div
          role="alert"
          style={{
            padding: "9px 14px",
            fontSize: "var(--text-xs)",
            color: "var(--danger)",
            borderTop: "1px solid var(--border-subtle)",
          }}
        >
          {error}
        </div>
      ) : null}
    </Section>
  );
}

export function ClipboardSection({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}) {
  return (
    <Section
      title="Clipboard"
      hint="Dictation can restore whatever was on your clipboard, keep a copy of what it inserted, and fit the text into what you already typed."
    >
      <Row
        first
        label="Restore the previous clipboard"
        hint="Puts your clipboard back the way it was after inserting."
      >
        <Toggle
          label="Restore the previous clipboard"
          checked={settings.restoreClipboard}
          onChange={(v) => onChange({ restoreClipboard: v })}
        />
      </Row>

      <Row
        label="Keep dictations on the clipboard"
        hint="Also keeps each dictation in your clipboard history, so a take that landed in the wrong place is still recoverable."
      >
        <Toggle
          label="Keep dictations on the clipboard"
          checked={settings.keepTextOnClipboard}
          onChange={(v) => onChange({ keepTextOnClipboard: v })}
        />
      </Row>

      <Row
        label="Smart insertion"
        hint="Reuses a space that is already there instead of adding a second one, and adds a capital at the start of a sentence."
      >
        <Toggle
          label="Smart insertion"
          checked={settings.smartInsertion}
          onChange={(v) => onChange({ smartInsertion: v })}
        />
      </Row>

      <Row label="What smart insertion will not do">
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Chip>Never lowercases</Chip>
          <Explainer text="Teletype cannot tell you said IBM from you said Ibm, so it will not guess and rewrite a capital into a lowercase letter. It also needs a readable text field: in an app it cannot read, your text is inserted exactly as dictated." />
        </div>
      </Row>
    </Section>
  );
}
