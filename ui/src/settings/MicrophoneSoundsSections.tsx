// Microphone and Sounds settings sections, ported from Envious Wispr's
// AudioSettingsView and RecordingSoundsSettingsView. The parent screen owns
// the Settings object and the save path; these components only patch fields
// through onChange.

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Explainer,
  FrozenNotice,
  Row,
  SecondaryButton,
  Section,
  Segmented,
  Select,
  Toggle,
} from "./primitives";
import type { Settings } from "../screens/SettingsScreen";

// Must stay a superset of the SettingsScreen interface for the fields this
// section touches: the contract test parses SettingsScreen's Settings for the
// UI-visible field list, and these patch keys must remain valid there.
export interface InputDevice {
  id: string;
  name: string;
  is_default: boolean;
}


const WARM_ENGINE_POLICIES: { value: string; label: string; title: string }[] = [
  { value: "off", label: "Off", title: "Release the engine right after each take" },
  { value: "10", label: "10s", title: "Keep the engine warm for 10 seconds" },
  { value: "30", label: "30s", title: "Keep the engine warm for 30 seconds" },
  { value: "60", label: "60s", title: "Keep the engine warm for 60 seconds" },
  { value: "always", label: "Always", title: "Keep the engine warm at all times" },
];

// The macOS system sounds this app knows how to play. The list is fixed, but
// an older config may name a sound that is no longer offered, so the current
// value is always kept in the options.
const RECORDING_SOUNDS = ["Submarine", "Tink", "Pop", "Ping", "Glass", "Hero", "Funk", "Blow"];

function soundOptions(current: string): { value: string; label: string }[] {
  const names = RECORDING_SOUNDS.includes(current) ? RECORDING_SOUNDS : [...RECORDING_SOUNDS, current];
  return names.map((n) => ({ value: n, label: n }));
}

export function MicrophoneSection(props: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
  devices: InputDevice[];
}) {
  const { settings, onChange, devices } = props;
  const current = devices.find((d) => d.id === settings.inputDevice);
  const usingName =
    settings.inputDevice === ""
      ? devices.find((d) => d.is_default)?.name ?? "default"
      : current?.name ?? "selected device";

  return (
    <Section title="Microphone">
      <FrozenNotice>
        Changing the microphone mid-recording cannot affect the take in progress; the new
        device is used from the next recording.
      </FrozenNotice>
      <Row label="Input device" first>
        <Explainer text="Which microphone records your voice. Switching the device applies to the next recording, not the one in progress." />
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", whiteSpace: "nowrap" }}>
          Using {usingName}
        </span>
        <Select
          label="Input device"
          value={settings.inputDevice}
          onChange={(v) => onChange({ inputDevice: v })}
          options={[
            { value: "", label: "System default" },
            ...devices.map((d) => ({ value: d.id, label: d.name + (d.is_default ? " (default)" : "") })),
          ]}
        />
      </Row>
      <Row label="Pre-warm the engine">
        <Explainer text="Keeping the speech engine warm means the first word lands sooner, at the cost of memory and battery while it stays loaded." />
        <Segmented
          label="Pre-warm the engine"
          value={settings.warmEnginePolicy}
          options={WARM_ENGINE_POLICIES}
          onChange={(v) => onChange({ warmEnginePolicy: v })}
        />
      </Row>
    </Section>
  );
}

export function SoundsSection(props: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const { settings, onChange } = props;
  const [previewError, setPreviewError] = useState<string | null>(null);

  const preview = async () => {
    setPreviewError(null);
    await invoke("preview_recording_sound", { name: settings.recordingSound }).catch((e) => {
      setPreviewError(String(e));
    });
  };

  return (
    <Section title="Sounds">
      <Row label="Play a sound when dictation starts and stops" first>
        <Explainer text="Short system sounds that ship with macOS, so nothing is downloaded." />
        <Toggle
          label="Play a sound when dictation starts and stops"
          checked={settings.playRecordingSounds}
          onChange={(v) => onChange({ playRecordingSounds: v })}
        />
      </Row>
      <Row label="Sound" disabled={!settings.playRecordingSounds}>
        <Select
          label="Recording sound"
          value={settings.recordingSound}
          onChange={(v) => onChange({ recordingSound: v })}
          options={soundOptions(settings.recordingSound)}
          disabled={!settings.playRecordingSounds}
        />
        <SecondaryButton onClick={preview}>Preview</SecondaryButton>
      </Row>
      {previewError && (
        <div style={{ padding: "9px 14px", fontSize: "var(--text-xs)", color: "var(--danger)" }} role="alert">
          The sound did not play: {previewError}
        </div>
      )}
    </Section>
  );
}
