// Transcription, Live Preview, and AI Polish settings sections, ported from
// Envious Wispr's SpeechEngineSettingsView, LivePreviewSettingsView, and
// AIPolishSettingsView. The grouping follows EW's mental model: what gets
// recorded, what you see while you speak, and what happens to your words
// afterwards. The parent screen owns the Settings object and the save path;
// these components only patch fields through onChange.

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";
import type { RuntimeStatus } from "../lib/runtimeStatus";
import {
  Chip,
  Explainer,
  FrozenNotice,
  Row,
  Section,
  Segmented,
  Select,
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
      <Row label="Speech model" hint="Selected in the Models screen.">
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
        <TextInput
          label="Silence duration in milliseconds"
          value={String(settings.vadSilenceMs)}
          onChange={(v) => {
            const ms = Math.round(Number(v));
            if (Number.isFinite(ms) && ms >= 100) onChange({ vadSilenceMs: ms });
          }}
          width={90}
        />
      </Row>
      <Row label="Remove filler words">
        <Explainer text="Strips filler words like um, uh, and er from the transcript before it is inserted. The word list lives in Settings." />
        <Toggle
          label="Remove filler words"
          checked={settings.removeFillerWords}
          onChange={(v) => onChange({ removeFillerWords: v })}
        />
      </Row>
      <Row label="Restore emoji" hint="Undoes spoken-emoji cleanup after the transform.">
        <Toggle
          label="Restore emoji"
          checked={settings.restoreEmoji}
          onChange={(v) => onChange({ restoreEmoji: v })}
        />
      </Row>
      <Row label="Spoken emoji" hint="Converts phrases like \u201cthumbs up emoji\u201d to glyphs.">
        <Toggle
          label="Spoken emoji"
          checked={settings.spokenEmoji}
          onChange={(v) => onChange({ spokenEmoji: v })}
        />
      </Row>
      <Row label="Spoken punctuation" hint="Converts \u201ccomma\u201d and \u201cperiod\u201d to symbols.">
        <Toggle
          label="Spoken punctuation"
          checked={settings.spokenPunctuation}
          onChange={(v) => onChange({ spokenPunctuation: v })}
        />
      </Row>
    </Section>
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
      <Row label="Provider" first hint="Selected in the Models screen.">
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
