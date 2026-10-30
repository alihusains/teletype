import { useCallback, useEffect, useReducer, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "./lib/useTauriEvent";

/** Number of bars kept in the recording waveform level history. */
export const WAVE_BARS = 34;

/** How long a transform skip/fallback message stays visible (ms). */
export const SKIP_MESSAGE_TTL_MS = 6000;

/** How long the post-insertion polish card stays up before collapsing (ms). */
export const POLISH_DIFF_TTL_MS = 8000;

/** The backend's post-insertion summary (event `polish-diff`). */
export interface PolishDiffPayload {
  raw: string;
  polished: string;
  ms: number;
}

// --- Wire payloads (exactly what the Rust backend emits) --------------------
//
// `pill-state` carries teletype_core::state::PillState (serde tag = "phase",
// rename_all = "camelCase"): idle | recording{startedAtMs} | warming |
// processing{message}. `startedAtMs` is required in Rust but treated as
// optional here: older payloads may omit it, and the reducer falls back to
// the receipt time exactly like the previous handler did.
export type PillWireState =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs?: number; command: boolean }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

// `dictation-state` carries teletype_core::state::UiState to *all* windows
// (app.emit broadcast), so the pill receives it too: idle |
// listening{startedAtMs} | transcribing | transforming | inserting |
// message{text}. It is emitted immediately before the matching `pill-state`
// at every call site (broadcast-then-show), so `pill-state` always has the
// last word and this mapping can only refine, never contradict, the pill.
export interface DictationStatePayload {
  phase: string;
  text?: string;
  startedAtMs?: number;
  started_at_ms?: number;
}

// --- Machine phases ----------------------------------------------------------
//
// The four wire phases plus derived refinements. `transcribing`/`streaming`
// are derived from the processing message ("Transcribing…" / "Transforming…")
// and from `transform-token` arrivals; `done`/`error` are derived from
// terminal flash messages ("Cancelled", "*failed*") and `dictation-state`
// message/error payloads. Every non-idle/non-recording/non-warming phase
// renders through the same processing UI, so the refinement changes labels
// for tests and future features, never pixels.
export type PillMachinePhase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number; command?: boolean }
  | { phase: "warming" }
  | { phase: "processing"; message: string }
  | { phase: "transcribing"; message: string }
  | { phase: "streaming"; message: string }
  | { phase: "done"; message: string }
  | { phase: "error"; message: string }
  | { phase: "result"; raw: string; polished: string; ms: number };

export type PillPhaseName = PillMachinePhase["phase"];

/** True for the phases that render through the processing UI. */
export function isBusyPhase(phase: PillPhaseName): boolean {
  return (
    phase === "processing" ||
    phase === "transcribing" ||
    phase === "streaming" ||
    phase === "done" ||
    phase === "error"
  );
}

/** The status message of a busy phase, or "" for idle/recording/warming. */
export function pillMessage(pill: PillMachinePhase): string {
  return "message" in pill ? pill.message : "";
}

export interface PillSettings {
  recordingMode: string;
  language: string;
  inputDevice: string;
  pillStyle: string;
}

export interface PillInputDevice {
  id: string;
  name: string;
  is_default: boolean;
}

export interface PillMachineState {
  pill: PillMachinePhase;
  settings: PillSettings | null;
  deviceName: string;
  devices: PillInputDevice[];
  hovered: boolean;
  cancelArmed: boolean;
  skipMessage: string | null;
  /** Concatenated `transform-token` deltas for the live LLM stream. */
  streamText: string;
  /** Highest accepted `transform-token` seq; stale/duplicate deltas drop. */
  lastTokenSeq: number;
  /** Latest partial ASR transcript while recording. */
  interimText: string;
  languageChip: { code: string; detected: boolean } | null;
  levels: number[];
  /** Receipt time of the current warming phase (warming clock). */
  warmingSinceMs: number;
  /** Fallback recording clock when the event omits startedAtMs. */
  recordingFallbackMs: number;
  /** The last take captured no audio (backend `mic-failure` event). */
  micFailure: boolean;
}

export function initialPillMachineState(): PillMachineState {
  return {
    pill: { phase: "idle" },
    settings: null,
    deviceName: "",
    devices: [],
    hovered: false,
    cancelArmed: false,
    skipMessage: null,
    streamText: "",
    lastTokenSeq: 0,
    interimText: "",
    languageChip: null,
    levels: new Array(WAVE_BARS).fill(0),
    warmingSinceMs: 0,
    recordingFallbackMs: 0,
    micFailure: false,
  };
}

// --- Actions (every transition is an explicit, testable value) ---------------

export type PillAction =
  | { type: "pill-state"; payload: PillWireState; now: number }
  | { type: "dictation-state"; payload: DictationStatePayload; now: number }
  | { type: "level"; level: number }
  | { type: "skip"; message: string }
  | { type: "skip-clear" }
  | { type: "language"; code: string; detected: boolean }
  | { type: "language-clear" }
  | { type: "token"; seq: number; tok: string }
  | { type: "interim"; text: string }
  | { type: "hover"; hovered: boolean }
  | { type: "cancel-armed"; armed: boolean }
  | { type: "settings-loaded"; settings: PillSettings; devices: PillInputDevice[]; deviceName: string }
  | { type: "settings-patched"; settings: PillSettings }
  | { type: "device-name"; name: string }
  | { type: "device-selected"; name: string; settings: PillSettings }
  | { type: "mic-failure" }
  | { type: "polish-diff"; payload: PolishDiffPayload }
  | { type: "result-dismiss" }
  | { type: "error"; message: string };

/** Refine a processing message into its machine phase. The only progress
 * labels the backend emits are "Transcribing…" and "Transforming…";
 * everything else is a 2s terminal flash (cancel/failure notices shown
 * before idle). Unknown labels stay `processing` (the safe default). */
function refineBusyPhase(message: string): PillMachinePhase {
  if (/^transcrib/i.test(message)) return { phase: "transcribing", message };
  if (/^transform/i.test(message)) return { phase: "streaming", message };
  if (/fail|error/i.test(message)) return { phase: "error", message };
  if (/cancell?ed/i.test(message)) return { phase: "done", message };
  return { phase: "processing", message };
}

/** True when the failure is fixable from the app's Settings screen, so the
 * error body can offer a recovery action. The mic case is handled separately
 * (it opens the OS permission pane, not the app). */
export function errorHasSettingsAction(message: string): boolean {
  return /settings|model|device|permission/i.test(message);
}

function applyPillState(s: PillMachineState, payload: PillWireState, now: number): PillMachineState {
  const prev = s.pill.phase;
  // A new session begins: drop leftovers exactly like the previous handler.
  if (payload.phase === "recording") {
    const startedAtMs =
      typeof payload.startedAtMs === "number" ? payload.startedAtMs : now;
    return {
      ...s,
      pill: { phase: "recording", startedAtMs, ...(payload.phase === "recording" && payload.command ? { command: true } : {}) },
      skipMessage: null,
      languageChip: null,
      streamText: "",
      lastTokenSeq: 0,
      interimText: "",
      micFailure: false,
      // Reset the waveform only on entry, not on re-emits.
      levels: prev === "recording" ? s.levels : new Array(WAVE_BARS).fill(0),
      // Fallback clock when the event omits startedAtMs (no entry guard,
      // matching the previous handler).
      recordingFallbackMs:
        typeof payload.startedAtMs === "number" ? s.recordingFallbackMs : now,
      hovered: s.hovered,
    };
  }
  // Leaving recording disarms hover + cancel, like the previous handler.
  const next: PillMachineState = {
    ...s,
    hovered: false,
    cancelArmed: false,
  };
  if (payload.phase === "warming") {
    if (prev !== "warming") next.warmingSinceMs = now;
    next.pill = { phase: "warming" };
    next.micFailure = false;
    return next;
  }
  if (payload.phase === "processing") {
    next.pill = refineBusyPhase(payload.message);
    return next;
  }
  next.pill = { phase: "idle" };
  next.micFailure = false;
  return next;
}

function applyDictationState(
  s: PillMachineState,
  payload: DictationStatePayload,
  now: number,
): PillMachineState {
  // `pill-state` owns the recording exit: while recording, only a fresh
  // listening broadcast is even plausible, and it carries no new information.
  // Ignoring everything here keeps out-of-order broadcasts from flickering
  // the recording UI.
  if (s.pill.phase === "recording") return s;
  const phase = payload.phase.toLowerCase();
  const startedAtMs =
    typeof payload.startedAtMs === "number"
      ? payload.startedAtMs
      : typeof payload.started_at_ms === "number"
        ? payload.started_at_ms
        : now;
  switch (phase) {
    case "listening":
      return {
        ...s,
        pill: { phase: "recording", startedAtMs },
        skipMessage: null,
        languageChip: null,
        streamText: "",
        lastTokenSeq: 0,
        interimText: "",
        levels: new Array(WAVE_BARS).fill(0),
        recordingFallbackMs: startedAtMs,
        micFailure: false,
      };
    case "transcribing":
      return { ...s, pill: { phase: "transcribing", message: "Transcribing…" } };
    case "transforming":
      return {
        ...s,
        pill: {
          phase: "streaming",
          message: s.pill.phase === "streaming" ? pillMessage(s.pill) || "Transforming…" : "Transforming…",
        },
      };
    case "inserting":
      // Final step before idle.
      return { ...s, pill: { phase: "done", message: "Inserting…" }, micFailure: false };
    case "message":
      return {
        ...s,
        pill: refineBusyPhase(payload.text ?? ""),
        hovered: false,
        cancelArmed: false,
      };
    case "error":
      return {
        ...s,
        pill: { phase: "error", message: payload.text ?? "Something went wrong" },
        hovered: false,
        cancelArmed: false,
        micFailure: false,
      };
    case "idle":
    case "completed":
    case "cancelled":
      return { ...s, pill: { phase: "idle" }, hovered: false, cancelArmed: false, micFailure: false };
    default:
      return s;
  }
}

/**
 * Perceptual loudness law for the meters (sqrt ≈ -6 dB at half scale).
 * The backend level is dB-linear with a noise gate; normal speech through
 * a low macOS input gain lands at 0.2..0.5, which renders as a flat line
 * on a linear bar. Display-only: VAD and transcripts are unaffected.
 */
export function displayLevel(level: number): number {
  if (!(level > 0)) return 0;
  return Math.sqrt(Math.min(level, 1));
}

/** Pure reducer: every pill transition, unit-testable with no Tauri mock. */
export function pillReducer(s: PillMachineState, a: PillAction): PillMachineState {
  switch (a.type) {
    case "pill-state":
      return applyPillState(s, a.payload, a.now);
    case "dictation-state":
      return applyDictationState(s, a.payload, a.now);
    case "level":
      return { ...s, levels: [...s.levels.slice(1), displayLevel(a.level)] };
    case "skip":
      return { ...s, skipMessage: a.message };
    case "skip-clear":
      return { ...s, skipMessage: null };
    case "language":
      return { ...s, languageChip: { code: a.code, detected: a.detected } };
    case "language-clear":
      return { ...s, languageChip: null };
    case "token": {
      // The backend emits only the delta per token; the pill concatenates.
      // Stale or duplicate seqs (replayed after a session reset) are dropped.
      if (a.seq <= s.lastTokenSeq) return s;
      const streamText = s.streamText + a.tok;
      const streaming =
        s.pill.phase === "processing" || s.pill.phase === "transcribing"
          ? { phase: "streaming" as const, message: pillMessage(s.pill) }
          : s.pill;
      return { ...s, streamText, lastTokenSeq: a.seq, pill: streaming };
    }
    case "interim":
      return { ...s, interimText: a.text };
    case "hover":
      return { ...s, hovered: a.hovered };
    case "cancel-armed":
      return { ...s, cancelArmed: a.armed };
    case "settings-loaded":
      return { ...s, settings: a.settings, devices: a.devices, deviceName: a.deviceName };
    case "settings-patched":
      return { ...s, settings: a.settings };
    case "device-name":
      return { ...s, deviceName: a.name };
    case "device-selected":
      return { ...s, deviceName: a.name, settings: a.settings };
    case "mic-failure":
      return { ...s, micFailure: true };
    case "polish-diff":
      // A card with nothing to show would be noise: the backend already
      // filters most of these, but a whitespace-only difference can still
      // slip through smart insertion.
      if (
        !a.payload.raw.trim() ||
        !a.payload.polished.trim() ||
        a.payload.raw.trim() === a.payload.polished.trim()
      ) {
        return s;
      }
      return {
        ...s,
        pill: { phase: "result", raw: a.payload.raw, polished: a.payload.polished, ms: a.payload.ms },
        skipMessage: null,
        micFailure: false,
        hovered: false,
        cancelArmed: false,
      };
    case "result-dismiss":
      // Only the card is dismissible: a late timer must not reset a newer
      // take that has already taken the pill over.
      return s.pill.phase === "result" ? { ...s, pill: { phase: "idle" } } : s;
    case "error":
      return {
        ...s,
        pill: { phase: "error", message: a.message },
        hovered: false,
        cancelArmed: false,
      };
  }
}

// --- Hook: wires the reducer to the backend, preserving every invoke --------

export interface PillMachine {
  machine: PillMachineState;
  dispatch: React.Dispatch<PillAction>;
  selectDevice: (id: string, name: string) => Promise<void>;
  lockLanguage: (code: string) => Promise<void>;
  toggleRecordingMode: () => Promise<void>;
}

export function usePillMachine(): PillMachine {
  const [machine, dispatch] = useReducer(pillReducer, undefined, initialPillMachineState);
  const skipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resultTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refreshSettings = useCallback(async () => {
    try {
      const next = await invoke<PillSettings>("get_settings");
      const devList = await invoke<PillInputDevice[]>("list_input_devices");
      const device = devList.find((d) => d.id === next.inputDevice);
      dispatch({
        type: "settings-loaded",
        settings: next,
        devices: devList,
        deviceName: device
          ? device.name.split(" ")[0]
          : devList[0]
            ? devList[0].name.split(" ")[0]
            : "",
      });
    } catch {
      // Settings not available yet.
    }
  }, []);

  const selectDevice = useCallback(async (id: string, name: string) => {
    dispatch({ type: "device-name", name });
    try {
      // Patch-based save: only inputDevice is written, so a stale local
      // snapshot can no longer clobber other fields another window changed.
      // The registered command is `update_settings_patch` (snake_case, the
      // Tauri name); its `patch` argument carries camelCase struct fields
      // (SettingsPatch is `serde(rename_all = "camelCase")`).
      const saved = await invoke<PillSettings>("update_settings_patch", {
        patch: { inputDevice: id },
      });
      dispatch({ type: "settings-patched", settings: saved });
    } catch {
      // Non-fatal: device selection will be re-read on next refresh.
    }
  }, []);

  // Tap the detected-language chip: lock that language in settings so the
  // next dictation uses it instead of auto-detecting.
  const lockLanguage = useCallback(async (code: string) => {
    try {
      const saved = await invoke<PillSettings>("update_settings_patch", {
        patch: { language: code },
      });
      dispatch({ type: "settings-patched", settings: saved });
    } catch {
      // Non-fatal: the user can lock the language in Settings.
    }
  }, []);

  // Tap the mode chip while recording: flip hold <-> toggle for the next take.
  // Optimistic (the label answers immediately), then confirmed by the settings
  // document the backend actually stored, same as selectDevice.
  const toggleRecordingMode = useCallback(async () => {
    const current = machine.settings?.recordingMode;
    const next = current === "toggle" ? "hold" : "toggle";
    if (machine.settings) {
      dispatch({
        type: "settings-patched",
        settings: { ...machine.settings, recordingMode: next },
      });
    }
    try {
      const saved = await invoke<PillSettings>("update_settings_patch", {
        patch: { recordingMode: next },
      });
      dispatch({ type: "settings-patched", settings: saved });
    } catch {
      // Non-fatal: refreshSettings() on the next `settings-changed` restores
      // the truth, and the label never claims a mode the save did not apply.
      refreshSettings();
    }
  }, [machine.settings, refreshSettings]);

  useEffect(() => {
    refreshSettings();
  }, [refreshSettings]);

  useTauriEvent<{ message: string }>("pill-skip", ({ payload }) => {
    if (skipTimer.current) clearTimeout(skipTimer.current);
    dispatch({ type: "skip", message: payload.message });
    // Clear it once the pill leaves the processing phase, so a stale message
    // never lingers into a later dictation session.
    skipTimer.current = setTimeout(() => dispatch({ type: "skip-clear" }), SKIP_MESSAGE_TTL_MS);
  });

  useTauriEvent<{ code: string; detected: boolean }>("pill-language", ({ payload }) => {
    dispatch({ type: "language", code: payload.code, detected: payload.detected });
  });

  useTauriEvent<PillWireState>("pill-state", ({ payload }) => {
    dispatch({ type: "pill-state", payload, now: Date.now() });
  });

  useTauriEvent<DictationStatePayload>("dictation-state", ({ payload }) => {
    dispatch({ type: "dictation-state", payload, now: Date.now() });
  });

  // Pill-only signal from the backend: the take captured no audio. It rides
  // alongside the "No audio captured" processing flash so the pill can offer
  // the microphone settings shortcut.
  useTauriEvent<void>("mic-failure", () => {
    dispatch({ type: "mic-failure" });
  });

  useTauriEvent<number>("pill-level", ({ payload }) => {
    dispatch({ type: "level", level: payload });
  });

  // Post-insertion polish summary. The backend emits it only after a delivery
  // that actually landed, and only when polish changed words.
  useTauriEvent<PolishDiffPayload>("polish-diff", ({ payload }) => {
    if (resultTimer.current) clearTimeout(resultTimer.current);
    dispatch({ type: "polish-diff", payload });
    resultTimer.current = setTimeout(
      () => dispatch({ type: "result-dismiss" }),
      POLISH_DIFF_TTL_MS,
    );
  });

  useTauriEvent<void>("settings-changed", () => {
    refreshSettings();
  });

  useTauriEvent<{ seq: number; tok: string }>("transform-token", ({ payload }) => {
    dispatch({ type: "token", seq: payload.seq, tok: payload.tok });
  });

  useTauriEvent<string>("interim-transcript", ({ payload }) => {
    // The interim ASR loop emits partial transcripts while recording.
    dispatch({ type: "interim", text: payload });
  });

  // Clear any pending skip-message timer on unmount.
  useEffect(() => {
    return () => {
      if (skipTimer.current) clearTimeout(skipTimer.current);
    };
  }, []);

  return { machine, dispatch, selectDevice, lockLanguage, toggleRecordingMode };
}
