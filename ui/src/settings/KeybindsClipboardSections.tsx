// Clipboard settings, ported from Envious Wispr's interaction model onto
// Teletype's tokens. Keyboard bindings now live in the single canonical
// Shortcuts section (ui/src/screens/KeybindsScreen.tsx) inside Settings.

import type { Settings } from "../screens/SettingsScreen";
import { Chip, Explainer, Row, Section, Toggle } from "./primitives";

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
