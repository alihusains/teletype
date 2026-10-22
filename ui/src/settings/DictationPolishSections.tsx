// Transcription, Live Preview, and AI Polish settings sections, ported from
// Envious Wispr's SpeechEngineSettingsView, LivePreviewSettingsView, and
// AIPolishSettingsView. The grouping follows EW's mental model: what gets
// recorded, what you see while you speak, and what happens to your words
// afterwards. The parent screen owns the Settings object and the save path;
// these components only patch fields through onChange.

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";
import { navigateTo } from "../lib/useTauriEvent";
import { confirmDialog } from "../lib/dialogs";
import { errorMessage } from "../lib/errors";
import type { RuntimeStatus } from "../lib/runtimeStatus";
import {
  Chip,
  Explainer,
  FrozenNotice,
  Row,
  Section,
  SecondaryButton,
  Segmented,
  Select,
  Slider,
  TextInput,
  Toggle,
} from "./primitives";
import type { Settings } from "../screens/SettingsScreen";

// Must stay a superset of the SettingsScreen interface for the fields this
// file touches: the contract test parses SettingsScreen's Settings for the
// UI-visible field list, and these patch keys must remain valid there.
interface SectionProps {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
  runtimeStatus?: RuntimeStatus;
}

// Runtime state to chip tone. "ready" (downloaded, not resident) is a working
// state, so it reads as success rather than pending.
const STATE_TONE: Record<string, "neutral" | "success" | "warning" | "danger"> = {
  loaded: "success",
  ready: "success",
  loading: "neutral",
  missing: "warning",
  none: "neutral",
  error: "danger",
};

const STATE_LABEL: Record<string, string> = {
  loaded: "Loaded",
  ready: "Ready",
  loading: "Loading",
  missing: "Not installed",
  none: "Not selected",
  error: "Error",
};

function providerLabel(provider: string): string {
  if (provider === "local-server") return "Local model";
  if (provider === "openai-compat") return "OpenAI-compatible API";
  return "None selected";
}

// The speech model display name, resolved against the catalog the Models
// screen uses, so the chip says "Parakeet V2" instead of an id.
function useSpeechModelName(id: string): string {
  const [name, setName] = useState("");
  useEffect(() => {
    let cancelled = false;
    invoke<{ id: string; name: string }[]>("list_speech_models")
      .then((models) => {
        if (cancelled) return;
        setName(models.find((m) => m.id === id)?.name ?? id);
      })
      .catch(() => {
        if (!cancelled) setName(id);
      });
    return () => {
      cancelled = true;
    };
  }, [id]);
  return name;
}

export function TranscriptionSection(props: SectionProps) {
  const { settings, onChange } = props;
  const speechLanguages = useSpeechLanguages();
  const speechModelName = useSpeechModelName(settings.selectedSpeechModel);

  return (
    <Section title="Transcription">
      <FrozenNotice>
        These settings freeze when a recording starts, so a change here never
        affects the take already in progress.
      </FrozenNotice>
      <Row label="Recording mode" first>
        <Explainer text="Hold: press and hold to talk, release to insert. Toggle: press once to start, again to finish." />
        <Segmented
          label="Recording mode"
          value={settings.recordingMode === "toggle" ? "toggle" : "hold"}
          options={[
            { value: "hold", label: "Hold", title: "Press and hold to talk, release to insert" },
            { value: "toggle", label: "Toggle", title: "Press once to start, again to finish" },
          ]}
          onChange={(v) => onChange({ recordingMode: v })}
        />
      </Row>
      <Row label="Language">
        <Explainer text="Auto detects the language per app: the frontmost app's override wins, otherwise Whisper detects the language you actually speak. Pick a specific language to lock it." />
        <Select
          label="Language"
          value={settings.language}
          onChange={(v) => onChange({ language: v })}
          options={[
            { value: "auto", label: `Auto-detect (${speechLanguages.length} languages)` },
            ...speechLanguages.map((l) => ({ value: l.code, label: l.name })),
          ]}
        />
      </Row>
      {/* The model itself is chosen on the Models screen; the row jumps there
          instead of leaving the user to find it. */}
      <Row
        label="Speech model"
        hint="Selected in the Models screen."
        onClick={() => navigateTo("models")}
      >
        <Chip tone={speechModelName ? "neutral" : "warning"}>
          {speechModelName || "Not selected"}
        </Chip>
      </Row>
      <Row label="Stop automatically after a pause">
        <Explainer text="Trades a pause in your speech for not having to press the key again: once you fall silent for the set duration, the take ends on its own." />
        <Toggle
          label="Stop automatically after a pause"
          checked={settings.vadAutoStop}
          onChange={(v) => onChange({ vadAutoStop: v })}
        />
      </Row>
      <Row label={`Stop after ${settings.vadSilenceMs} ms of silence`} disabled={!settings.vadAutoStop}>
        <Slider
          label="Silence duration in milliseconds"
          value={settings.vadSilenceMs}
          min={300}
          max={2000}
          step={100}
          disabled={!settings.vadAutoStop}
          onChange={(ms) => onChange({ vadSilenceMs: ms })}
        />
      </Row>
      <Row label="Noise reduction">
        <Explainer text="Uses RNNoise to suppress background noise (fans, keyboards, room hum) before speech recognition. Adds negligible latency." />
        <Toggle
          label="Noise reduction"
          checked={settings.noiseReductionEnabled}
          onChange={(v) => onChange({ noiseReductionEnabled: v })}
        />
      </Row>
      <Row label="Lower other audio while dictating">
        <Explainer text="Lowers the output volume to about 20% while you record, so music or video playing on this device stops bleeding into the microphone. The previous volume is restored when the take ends." />
        <Toggle
          label="Lower other audio while dictating"
          checked={settings.duckOtherAudio}
          onChange={(v) => onChange({ duckOtherAudio: v })}
        />
      </Row>
      <Row label="Pause media while dictating">
        <Explainer text="Sends the system play/pause key when a take starts and stops. Off by default." />
        <Toggle
          label="Pause media while dictating"
          checked={settings.pauseMediaWhileDictating}
          onChange={(v) => onChange({ pauseMediaWhileDictating: v })}
        />
      </Row>
      <Row label="Remove filler words">
        <Explainer text="Strips filler words like um, uh, and er from the transcript before it is inserted." />
        <Toggle
          label="Remove filler words"
          checked={settings.removeFillerWords}
          onChange={(v) => onChange({ removeFillerWords: v })}
        />
      </Row>
      {settings.removeFillerWords && <FillerWordEditor settings={settings} onChange={onChange} />}
      <Row label="Restore emoji" hint="Undoes spoken-emoji cleanup after the transform.">
        <Toggle
          label="Restore emoji"
          checked={settings.restoreEmoji}
          onChange={(v) => onChange({ restoreEmoji: v })}
        />
      </Row>
      <Row label="Spoken emoji" hint={"Converts phrases like “thumbs up emoji” to glyphs."}>
        <Toggle
          label="Spoken emoji"
          checked={settings.spokenEmoji}
          onChange={(v) => onChange({ spokenEmoji: v })}
        />
      </Row>
      <Row label="Spoken punctuation" hint={"Converts “comma” and “period” to symbols."}>
        <Toggle
          label="Spoken punctuation"
          checked={settings.spokenPunctuation}
          onChange={(v) => onChange({ spokenPunctuation: v })}
        />
      </Row>
    </Section>
  );
}

// The filler-word list editor, shown under the "Remove filler words" toggle.
// Previously this lived in a separate legacy "Filler Words" section; it is
// colocated here so the toggle and its word list are one control, not two.
function FillerWordEditor({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const [newWord, setNewWord] = useState("");
  const add = () => {
    const word = newWord.trim().toLowerCase();
    if (!word || settings.fillerWords.includes(word)) return;
    onChange({ fillerWords: [...settings.fillerWords, word] });
    setNewWord("");
  };
  const remove = (word: string) =>
    onChange({ fillerWords: settings.fillerWords.filter((w) => w !== word) });

  return (
    <div
      style={{
        padding: "12px 14px",
        borderTop: "1px solid var(--border-subtle)",
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
        {settings.fillerWords.map((word) => (
          <span
            key={word}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "2px 8px",
              background: "transparent",
              borderRadius: 999,
              fontSize: "var(--text-xs)",
              fontWeight: 500,
              border: "1px solid var(--border-subtle)",
              color: "var(--text)",
            }}
          >
            {word}
            <button
              onClick={() => remove(word)}
              aria-label={`Remove ${word}`}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                color: "var(--text-secondary)",
                fontSize: 14,
                padding: 0,
                lineHeight: 1,
              }}
            >
              ×
            </button>
          </span>
        ))}
        {settings.fillerWords.length === 0 && (
          <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>No filler words</span>
        )}
      </div>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <TextInput
          label="Add a filler word"
          placeholder="Add a word…"
          value={newWord}
          onChange={setNewWord}
          width={200}
        />
        <SecondaryButton onClick={add} disabled={!newWord.trim()}>
          Add
        </SecondaryButton>
      </div>
    </div>
  );
}

export function LivePreviewSection(props: SectionProps) {
  const { settings, onChange, runtimeStatus } = props;
  const speech = runtimeStatus?.speech;
  const tone = speech ? STATE_TONE[speech.state] ?? "neutral" : "neutral";

  return (
    <Section title="Live Preview">
      <Row label="Show live transcript while recording" first>
        <Explainer text="Live preview re-decodes the audio you are still speaking, so every interim update costs extra work on the speech engine. Turning it off leaves the recording animation with no text under it." />
        <Toggle
          label="Show live transcript while recording"
          checked={settings.livePreviewEnabled}
          onChange={(v) => onChange({ livePreviewEnabled: v })}
        />
      </Row>
      <Row
        label={`Keep the model in memory for ${settings.modelUnloadDelaySecs}s after a take`}
        hint={settings.modelUnloadDelaySecs === 0 ? "The model unloads as soon as possible." : undefined}
      >
        <Explainer text="How long the speech model stays resident after a take. Longer keeps the next dictation fast but holds memory; 0 unloads it as soon as possible and the next take pays a reload delay." />
        <TextInput
          label="Model unload delay in seconds"
          value={String(settings.modelUnloadDelaySecs)}
          onChange={(v) => {
            const secs = Math.round(Number(v));
            if (Number.isFinite(secs) && secs >= 0) onChange({ modelUnloadDelaySecs: secs });
          }}
          width={90}
        />
      </Row>
      <Row label="Speech engine" hint="Read from the shared runtime status; the Models screen is where it is prepared.">
        <Chip tone={tone}>
          {speech
            ? speech.state === "error"
              ? speech.detail
              : `${STATE_LABEL[speech.state] ?? speech.state} · ${speech.label}`
            : "Checking…"}
        </Chip>
      </Row>
    </Section>
  );
}

export function AIPolishSection(props: SectionProps) {
  const { settings, onChange, runtimeStatus } = props;
  const llm = runtimeStatus?.llm;
  const tone = llm ? STATE_TONE[llm.state] ?? "neutral" : "neutral";
  const model = settings.selectedLlmModel || settings.selectedLlmProvider;

  return (
    <Section title="AI Polish">
      {/* The provider is chosen on the Models screen; the row goes there. */}
      <Row
        label="Provider"
        first
        hint="Selected in the Models screen."
        onClick={() => navigateTo("models")}
      >
        <Chip>{providerLabel(settings.selectedLlmProvider)}</Chip>
        {model ? <Chip>{model}</Chip> : null}
      </Row>
      <Row label="Skip polish for short takes">
        <Explainer text="Below the word count, the text is inserted as dictated: cleaning up a two-word message costs more latency than it is worth. The gate only ever skips polish, it never changes the text." />
        <Toggle
          label="Skip polish for short takes"
          checked={settings.polishGateEnabled}
          onChange={(v) => onChange({ polishGateEnabled: v })}
        />
      </Row>
      <Row
        label={`Polish takes with at least ${settings.polishGateThresholdWords} words`}
        disabled={!settings.polishGateEnabled}
      >
        <TextInput
          label="Polish gate word threshold"
          value={String(settings.polishGateThresholdWords)}
          onChange={(v) => {
            const words = Math.round(Number(v));
            if (Number.isFinite(words) && words >= 1) onChange({ polishGateThresholdWords: words });
          }}
          width={90}
        />
      </Row>
      <Row label="Polish whole text at the end">
        <Explainer text="Polish the entire take as one unit at the end instead of per committed segment. Slower, but more consistent output across the whole message." />
        <Toggle
          label="Polish whole text at the end"
          checked={settings.polishWholeTextAtEnd}
          onChange={(v) => onChange({ polishWholeTextAtEnd: v })}
        />
      </Row>
      <Row label="Insert cleaned text as-is">
        <Explainer text="When on, the polished text is inserted without waiting for you to confirm. When off, the dictation lands as dictated and the transform is not applied." />
        <Toggle
          label="Insert cleaned text as-is"
          checked={settings.autoApplyTransform}
          onChange={(v) => onChange({ autoApplyTransform: v })}
        />
      </Row>
      <Row label="Local provider" hint="Read from the shared runtime status; the Models screen is where it is started.">
        <Chip tone={tone}>
          {llm
            ? llm.state === "error"
              ? llm.detail
              : `${STATE_LABEL[llm.state] ?? llm.state} · ${llm.label}`
            : "Checking…"}
        </Chip>
      </Row>
    </Section>
  );
}

// Phase 5/6 audio runtime settings: when recording starts. These are the
// Wave-A runtime behaviors exposed as real settings; the engine consumes them,
// so the UI is the source of truth for the user's intent here.
export function AudioSection({ settings, onChange }: SectionProps) {
  return (
    <Section title="Audio">
      <Row label="Instant start" first>
        <Explainer text="Starts recording ahead of the hotkey press so the first phoneme is never lost. While app active only captures while this app is focused; always captures in the background too." />
        <Select
          label="Instant start"
          value={settings.instantStart || "off"}
          onChange={(v) => onChange({ instantStart: v })}
          options={[
            { value: "off", label: "Off" },
            { value: "while_app_active", label: "While app active" },
            { value: "always", label: "Always" },
          ]}
        />
      </Row>
      {/* `preferBuiltinMicWithBt` and `audioConditioning` had rows here. The
          engine reads neither flag, so the toggles only ever wrote a value
          nothing consumed; both fields stay in the Settings shape because the
          backend still persists them. */}
    </Section>
  );
}

// App-level behavior that is not about a single dictation: the menu bar icon,
// AutoText while typing, and the Developer tab. These were previously a
// legacy "Behavior" block; the duplicated toggles (clipboard, emoji,
// punctuation, polish gate, auto-apply) now live in their proper sections
// above, so only the unique ones remain here.
export function GeneralSection({ settings, onChange }: SectionProps) {
  return (
    <Section title="General">
      <Row label="Show menu bar / tray icon" first>
        <Toggle
          label="Show tray icon"
          checked={settings.showTrayIcon}
          onChange={(v) => onChange({ showTrayIcon: v })}
        />
      </Row>
      <Row label="Expand AutoText while typing" hint="Replaces AutoText snippets as you type, not just after dictation.">
        <Toggle
          label="Expand AutoText while typing"
          checked={settings.typingAutotextEnabled}
          onChange={(v) => onChange({ typingAutotextEnabled: v })}
        />
      </Row>
      <Row label="Show Developer tab" hint="Exposes the live log and runtime diagnostics.">
        <Toggle
          label="Show Developer tab"
          checked={settings.enableDeveloperTab}
          onChange={(v) => onChange({ enableDeveloperTab: v })}
        />
      </Row>
    </Section>
  );
}

// P7.8: the self-learning loop settings. This section records the user's
// choices, offers the destructive reset, and runs the Contacts import.
//
// `learnNamesFromScreen` and `keepAudioForSelfTests` used to have rows here:
// the engine never reads either flag, so they were removed rather than left as
// controls that save a value nothing consumes (see tasks/pi-tasks/
// ui-dead-controls.md item 11). The fields stay in the Settings shape because
// the backend still persists them.

// `list_contacts` / `import_contacts_apply` mirror the Rust serde shapes
// (crates/teletype-core/src/learning/contacts.rs and
// crates/teletype-desktop/src/contacts_cmd.rs), both `rename_all = "camelCase"`.
interface ContactRecord {
  givenName: string;
  familyName: string;
  nickname?: string | null;
  organization?: string | null;
}

interface ContactsImportResult {
  added: number;
  aliasesAdded: number;
  unchanged: number;
  tombstonedSkipped: number;
}

export function LearningSection(props: SectionProps) {
  const { settings, onChange } = props;
  const [resultNote, setResultNote] = useState<{ ok: boolean; text: string } | null>(null);
  // The address-book read and the apply are two round trips, and the first one
  // can raise a macOS Contacts permission prompt. The flag keeps the button
  // from firing a second import while the first is still landing.
  const [importing, setImporting] = useState(false);

  const onDelete = async () => {
    const confirmed = await confirmDialog(
      "Delete all learning data?\n\nThis permanently removes every learned term and the " +
        "recorded edit history. Your dictations and transcripts are not affected.",
      { title: "Delete learning data", kind: "error" },
    );
    if (!confirmed) return;
    try {
      await invoke("delete_learning_data");
      setResultNote({ ok: true, text: "Learning data deleted." });
    } catch (e) {
      setResultNote({ ok: false, text: `Could not delete learning data: ${errorMessage(e)}` });
    }
  };

  const onImportContacts = async () => {
    if (importing) return;
    setImporting(true);
    try {
      const contacts = await invoke<ContactRecord[]>("list_contacts");
      if (contacts.length === 0) {
        setResultNote({ ok: true, text: "No contacts found to import." });
        return;
      }
      const confirmed = await confirmDialog(
        `Import names from ${contacts.length} contact${contacts.length === 1 ? "" : "s"}? ` +
          "Teletype learns them as vocabulary hints you can undo like any other learned term.",
        { title: "Import from Contacts", kind: "info" },
      );
      if (!confirmed) return;
      const result = await invoke<ContactsImportResult>("import_contacts_apply", { contacts });
      const notes: string[] = [];
      if (result.unchanged > 0) notes.push(`${result.unchanged} already learned`);
      if (result.tombstonedSkipped > 0) {
        notes.push(`${result.tombstonedSkipped} you deleted stay deleted`);
      }
      setResultNote({
        ok: true,
        text:
          `Imported ${result.added} term${result.added === 1 ? "" : "s"} and ` +
          `${result.aliasesAdded} alias${result.aliasesAdded === 1 ? "" : "es"} from Contacts` +
          (notes.length > 0 ? ` (${notes.join(", ")})` : "") +
          ".",
      });
    } catch (e) {
      setResultNote({ ok: false, text: `Could not import contacts: ${errorMessage(e)}` });
    } finally {
      setImporting(false);
    }
  };

  return (
    <Section title="Learning">
      <Row
        label="Learn from my edits"
        hint="Improves your personal vocabulary by watching how you fix dictated text."
        first
      >
        <Toggle
          label="Learn from my edits"
          checked={settings.learnFromEdits}
          onChange={(v) => onChange({ learnFromEdits: v })}
        />
      </Row>
      <Row
        label="Import from Contacts…"
        hint="Learns the names in your address book as vocabulary hints."
      >
        <SecondaryButton onClick={onImportContacts} disabled={importing}>
          {importing ? "Checking…" : "Import"}
        </SecondaryButton>
      </Row>
      <Row label="Delete all learning data" hint="Removes every learned term and the recorded edit history permanently.">
        <SecondaryButton onClick={onDelete} danger>
          Delete all learning data
        </SecondaryButton>
      </Row>
      {resultNote ? (
        <div
          style={{
            padding: "10px 14px",
            borderTop: "1px solid var(--border-subtle)",
            fontSize: "var(--text-xs)",
            color: resultNote.ok ? "var(--text-secondary)" : "var(--danger)",
          }}
        >
          {resultNote.text}
        </div>
      ) : null}
    </Section>
  );
}
