import { useCallback, useEffect, useReducer, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "./lib/useTauriEvent";

/** Number of bars kept in the recording waveform level history. */
export const WAVE_BARS = 34;

/** How long a transform skip/fallback message stays visible (ms). */
export const SKIP_MESSAGE_TTL_MS = 6000;

/** How long the post-insertion polish card stays up before collapsing (ms). */
export const POLISH_DIFF_TTL_MS = 8000;

/** Learned-word pill timings. The backend's offer expiry uses the same
 * learnUndoSecs setting, so the two clocks stay in step; these constants are
 * the independent phases of the pill's own life. */
export const LEARN_UNDO_DEFAULT_SECS = 12;
export const LEARN_UNDO_MIN_SECS = 3;
export const LEARN_UNDO_MAX_SECS = 300;
export const LEARN_PENDING_MS = 3000;
export const LEARN_UNDONE_MS = 1500;
export const LEARN_ERROR_MS = 3000;
export const LEARN_HOVER_RESUME_FLOOR_MS = 2000;
export const LEARN_TICK_MS = 100;

/** The clamp both sides apply to the persisted undo window. */
export function clampLearnUndoSecs(secs: number | undefined): number {
  if (typeof secs !== "number" || !Number.isFinite(secs)) return LEARN_UNDO_DEFAULT_SECS;
  return Math.min(LEARN_UNDO_MAX_SECS, Math.max(LEARN_UNDO_MIN_SECS, Math.round(secs)));
}

/** Remaining dwell at a given elapsed time (never negative). */
export function learnRemainingMs(dwellMs: number, elapsedMs: number): number {
  return Math.max(0, dwellMs - Math.max(0, elapsedMs));
}

/** A hover that leaves with under two seconds left rewinds the rail so the
 * user always gets at least two seconds to reach Undo. */
export function learnResumeElapsedMs(
  dwellMs: number,
  elapsedMs: number,
  floorMs: number = LEARN_HOVER_RESUME_FLOOR_MS,
): number {
  return learnRemainingMs(dwellMs, elapsedMs) >= floorMs
    ? elapsedMs
    : Math.max(0, dwellMs - floorMs);
}

/** Notice delivered by Rust (event `learned-word`) for one learned word. */
export interface LearnedWordPayload {
  card: number;
  offerId: number;
  word: string;
  kind: "added" | "updated";
}

/** Notice delivered by Rust (event `learned-word-error`) on a save failure. */
export interface LearnedWordErrorPayload {
  card: number;
  word: string;
}

/** The backend's post-insertion summary (event `polish-diff`). */
export interface PolishDiffPayload {
  raw: string;
  polished: string;
  ms: number;
  /** The card id Rust handed out (overlay::show_result); the pill passes it
   * back to `dismiss_pill_result` so a late dismiss cannot hide a new take. */
  card?: number;
  /** Polish profile that actually ran (e.g. "structured"). Optional: older
   * payloads and payloads without a profile omit the key entirely. */
  profile?: string;
  /** Gate / fallback reason from the same result. Optional for the same reason. */
  reason?: string;
  ran?: boolean;
  model?: string | null;
  original?: string;
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
  | { phase: "result"; raw: string; polished: string; ms: number; card?: number; profile?: string; reason?: string; ran?: boolean; model?: string | null; original?: string }
  | { phase: "learned"; word: string; kind: "added" | "updated"; offerId: number; card: number;
      status: "live" | "pending" | "undone" | "undoError"; elapsedMs: number }
  | { phase: "learnedError"; word: string; card: number };

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
  /** The 3x3 slot (wire id, e.g. "bottomCenter"); the pill keeps the edge
   * Rust anchors to fixed when content changes size. */
  pillPosition?: string;
  /** Rust keeps the idle pill on screen (otherwise idle hides the window). */
  alwaysShowPill?: boolean;
  /** The app's own Reduce Motion setting, on top of the OS preference. */
  reduceMotion?: boolean;
  /** Undo window in seconds for learned words; both the tray offer and the
   * pill dwell follow it (default 12, clamped to 3..300). */
  learnUndoSecs?: number;
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
  /** Highest notice card id accepted; a stale id from an older card drops. */
  learnedCardSeq: number;
  /** Hover state of the live learned pill (pauses the countdown). */
  learnedHovered: boolean;
  /** Receipt time of the current warming phase (warming clock). */
  warmingSinceMs: number;
  /** Fallback recording clock when the event omits startedAtMs. */
  recordingFallbackMs: number;
  /** The last take captured no audio (backend `mic-failure` event). */
  micFailure: boolean;
  /**
   * Latest model-download fraction (0..1) from the backend's
   * `model-load-progress` event, shown while warming as
   * "Downloading model 42%". Cleared on every phase change.
   */
  loadProgress: number | null;
  /**
   * True once the current take has received its first `pill-level`. The glow
   * pill dims its lane until then, so "the mic is not delivering yet" reads
   * differently from "the room is silent". Reset on every recording entry.
   */
  levelArrived: boolean;
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
    loadProgress: null,
    levelArrived: false,
    learnedCardSeq: 0,
    learnedHovered: false,
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
  | { type: "model-load-progress"; fraction: number }
  | { type: "polish-diff"; payload: PolishDiffPayload }
  | { type: "result-dismiss" }
  | { type: "learned-show"; payload: LearnedWordPayload }
  | { type: "learned-hover"; card: number; hovered: boolean }
  | { type: "learned-tick"; card: number; elapsedMs: number }
  | { type: "learned-undo"; card: number }
  | { type: "learned-undo-result"; card: number; result: "applied" | "stale" | "error" }
  | { type: "learned-expire"; card: number }
  | { type: "learned-error-show"; payload: LearnedWordErrorPayload }
  | { type: "learned-error-expire"; card: number }
  | { type: "learned-dismiss"; card: number }
  | { type: "error"; message: string };

/** Refine a processing message into its machine phase. The only progress
 * labels the backend emits are "Transcribing…" and "Transforming…";
 * everything else is a 2s terminal flash (cancel/failure notices shown
 * before idle). Unknown labels stay `processing` (the safe default). */
function refineBusyPhase(message: string): PillMachinePhase {
  if (/^transcrib/i.test(message)) return { phase: "transcribing", message };
  if (/^transform/i.test(message)) return { phase: "streaming", message };
  if (/fail|error/i.test(message)) return { phase: "error", message };
  // T10a: the model-load failure and the mic-permission-denied message are
  // terminal errors, not progress. The model message carries a reason class
  // (missing file / unsupported / other) that the error body can act on;
  // the mic message is paired with the `mic-failure` event that renders the
  // Open Settings shortcut.
  if (/^couldn't load the speech model/i.test(message)) return { phase: "error", message };
  if (/^microphone access is off/i.test(message)) return { phase: "error", message };
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
  // A learned pill outlives the take's trailing idle push (learning fires
  // seconds after insertion). Only a *new* take replaces it, never idle.
  if (payload.phase === "idle" && (prev === "learned" || prev === "learnedError")) return s;
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
      loadProgress: null,
      // Reset the waveform only on entry, not on re-emits.
      levels: prev === "recording" ? s.levels : new Array(WAVE_BARS).fill(0),
      levelArrived: prev === "recording" ? s.levelArrived : false,
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
    // The download fraction belongs to the warming phase it arrived in; a
    // phase change (warmup done, take end, idle) resets it so a stale
    // percentage never lingers into the next warming.
    loadProgress: null,
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
        levelArrived: false,
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

/** False for a card with nothing to show, which would be noise: the backend
 * already filters most of these, but a whitespace-only difference can still
 * slip through smart insertion (Rust and JS trim different characters). */
export function polishDiffShowable(payload: PolishDiffPayload): boolean {
  const raw = payload.raw.trim();
  const polished = payload.polished.trim();
  // Always showable when there is text, even if polish changed nothing: the
  // card renders "0 changes" so the user can see the take landed and what
  // polish did (or that it did nothing).
  return !!raw && !!polished;
}

/** Pure reducer: every pill transition, unit-testable with no Tauri mock. */
export function pillReducer(s: PillMachineState, a: PillAction): PillMachineState {
  switch (a.type) {
    case "pill-state":
      return applyPillState(s, a.payload, a.now);
    case "dictation-state":
      return applyDictationState(s, a.payload, a.now);
    case "level":
      return { ...s, levels: [...s.levels.slice(1), displayLevel(a.level)], levelArrived: true };
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
    case "model-load-progress":
      // The backend throttles these to 4 Hz and clamps to 0..1; a stale
      // value (phase already moved past warming) is dropped by the render
      // side, which only shows the fraction while warming.
      return { ...s, loadProgress: a.fraction };
    case "polish-diff":
      // A live learned pill owns the slot; the polish card is declined here
      // and its event handler hands the card straight back to Rust.
      if (s.pill.phase === "learned" && s.pill.status === "live") return s;
      if (!polishDiffShowable(a.payload)) return s;
      return {
        ...s,
        pill: {
          phase: "result",
          raw: a.payload.raw,
          polished: a.payload.polished,
          ms: a.payload.ms,
          card: a.payload.card,
          profile: a.payload.profile,
          reason: a.payload.reason,
          ran: a.payload.ran,
          model: a.payload.model,
          original: a.payload.original,
        },
        skipMessage: null,
        micFailure: false,
        hovered: false,
        cancelArmed: false,
      };
    case "result-dismiss":
      // Only the card is dismissible: a late timer must not reset a newer
      // take that has already taken the pill over.
      return s.pill.phase === "result" ? { ...s, pill: { phase: "idle" } } : s;
    case "learned-show": {
      const c = a.payload.card;
      if (!Number.isFinite(c) || c <= s.learnedCardSeq) return s;
      return {
        ...s,
        learnedCardSeq: c,
        learnedHovered: false,
        pill: {
          phase: "learned",
          word: a.payload.word,
          kind: a.payload.kind === "updated" ? "updated" : "added",
          offerId: a.payload.offerId,
          card: c,
          status: "live",
          elapsedMs: 0,
        },
        skipMessage: null,
        languageChip: null,
        micFailure: false,
        cancelArmed: false,
      };
    }
    case "learned-hover":
      if (s.pill.phase !== "learned" || s.pill.card !== a.card || s.pill.status !== "live") return s;
      return { ...s, learnedHovered: a.hovered };
    case "learned-tick":
      if (s.pill.phase !== "learned" || s.pill.card !== a.card || s.pill.status !== "live") return s;
      return { ...s, pill: { ...s.pill, elapsedMs: a.elapsedMs } };
    case "learned-undo":
      if (s.pill.phase !== "learned" || s.pill.card !== a.card || s.pill.status !== "live") return s;
      // Morph before the await; the controls are hidden but their space is
      // reserved so the pill does not resize mid-undo.
      return { ...s, learnedHovered: false, pill: { ...s.pill, status: "pending" } };
    case "learned-undo-result": {
      if (s.pill.phase !== "learned" || s.pill.card !== a.card || s.pill.status !== "pending") return s;
      const status = a.result === "applied" ? "undone" : "undoError";
      return { ...s, pill: { ...s.pill, status } };
    }
    case "learned-expire":
      if (s.pill.phase !== "learned" || s.pill.card !== a.card || s.pill.status !== "live") return s;
      return { ...s, learnedHovered: false, pill: { phase: "idle" } };
    case "learned-error-show": {
      const c = a.payload.card;
      if (!Number.isFinite(c) || c <= s.learnedCardSeq) return s;
      return {
        ...s,
        learnedCardSeq: c,
        learnedHovered: false,
        pill: { phase: "learnedError", word: a.payload.word, card: c },
        skipMessage: null,
        micFailure: false,
        cancelArmed: false,
      };
    }
    case "learned-error-expire":
      if (s.pill.phase !== "learnedError" || s.pill.card !== a.card) return s;
      return { ...s, pill: { phase: "idle" } };
    case "learned-dismiss": {
      const p = s.pill;
      if (p.phase !== "learned" && p.phase !== "learnedError") return s;
      if (p.card !== a.card) return s;
      return { ...s, learnedHovered: false, pill: { phase: "idle" } };
    }
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
  hoverLearnedWord: (card: number, hovered: boolean) => void;
  undoLearnedWord: (card: number, offerId: number) => Promise<void>;
  keepLearnedWord: (card: number) => void;
}

export function usePillMachine(): PillMachine {
  const [machine, dispatch] = useReducer(pillReducer, undefined, initialPillMachineState);
  const skipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resultTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Learned-pill timing lives in refs, not in the rendered phase: an
  // elapsedMs tick re-renders every 100 ms and must not re-arm the deadline.
  // React state remains the single source of what is on screen; these refs
  // only own the wall-clock (performance.now, immune to clock changes).
  interface LearnTimer {
    card: number;
    dwellMs: number;
    elapsedBase: number;
    startedAt: number;
    hovered: boolean;
    schedule: () => void;
    tick: ReturnType<typeof setInterval> | null;
    expiry: ReturnType<typeof setTimeout> | null;
    settle: ReturnType<typeof setTimeout> | null;
  }
  const learnTimer = useRef<LearnTimer | null>(null);
  const learnErrorTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const learnedCardRef = useRef(0);
  const settingsRef = useRef(machine.settings);
  settingsRef.current = machine.settings;
  const learnedRef = useRef(machine.pill);
  learnedRef.current = machine.pill;

  const clearLearnTimers = useCallback(() => {
    const t = learnTimer.current;
    if (t) {
      if (t.tick) clearInterval(t.tick);
      if (t.expiry) clearTimeout(t.expiry);
      if (t.settle) clearTimeout(t.settle);
    }
    learnTimer.current = null;
    if (learnErrorTimer.current) clearTimeout(learnErrorTimer.current);
    learnErrorTimer.current = null;
  }, []);

  const startLearnTimers = useCallback((card: number) => {
    clearLearnTimers();
    const dwellMs = clampLearnUndoSecs(settingsRef.current?.learnUndoSecs) * 1000;
    const t: LearnTimer = {
      card,
      dwellMs,
      elapsedBase: 0,
      startedAt: 0,
      hovered: false,
      tick: null,
      expiry: null,
      settle: null,
      schedule: () => {},
    };
    t.schedule = () => {
      if (t.tick) clearInterval(t.tick);
      if (t.expiry) clearTimeout(t.expiry);
      const remaining = learnRemainingMs(t.dwellMs, t.elapsedBase);
      t.startedAt = performance.now();
      t.tick = setInterval(() => {
        if (t.hovered) return;
        const elapsed = Math.min(t.dwellMs, t.elapsedBase + (performance.now() - t.startedAt));
        dispatch({ type: "learned-tick", card: t.card, elapsedMs: elapsed });
      }, LEARN_TICK_MS);
      t.expiry = setTimeout(() => {
        dispatch({ type: "learned-expire", card: t.card });
        if (learnTimer.current === t) learnTimer.current = null;
      }, remaining);
    };
    learnTimer.current = t;
    t.schedule();
  }, [clearLearnTimers]);

  const hoverLearnedWord = useCallback((card: number, hovered: boolean) => {
    const t = learnTimer.current;
    if (!t || t.card !== card) return;
    const p = learnedRef.current;
    if (p.phase !== "learned" || p.status !== "live") return;
    dispatch({ type: "learned-hover", card, hovered });
    if (hovered === t.hovered) return;
    if (hovered) {
      t.elapsedBase = Math.min(t.dwellMs, t.elapsedBase + (performance.now() - t.startedAt));
      t.hovered = true;
      if (t.tick) clearInterval(t.tick);
      if (t.expiry) clearTimeout(t.expiry);
      t.tick = null;
      t.expiry = null;
    } else {
      t.elapsedBase = learnResumeElapsedMs(t.dwellMs, t.elapsedBase);
      t.hovered = false;
      t.schedule();
    }
  }, []);

  const undoLearnedWord = useCallback(async (card: number, offerId: number) => {
    const t = learnTimer.current;
    if (!t || t.card !== card) return;
    if (t.tick) clearInterval(t.tick);
    if (t.expiry) clearTimeout(t.expiry);
    t.tick = null;
    t.expiry = null;
    dispatch({ type: "learned-undo", card });
    let result: "applied" | "stale" | "error" = "error";
    try {
      const reply = await invoke<string>("undo_learned", { ids: ["offer:" + offerId] });
      result = reply === "applied" ? "applied" : "stale";
    } catch {
      result = "error";
    }
    if (learnTimer.current !== t) return;
    dispatch({ type: "learned-undo-result", card, result });
    const settleMs = result === "applied" ? LEARN_UNDONE_MS : LEARN_ERROR_MS;
    t.settle = setTimeout(() => {
      dispatch({ type: "learned-dismiss", card });
      if (learnTimer.current === t) learnTimer.current = null;
    }, settleMs);
  }, []);

  const keepLearnedWord = useCallback((card: number) => {
    if (!learnTimer.current || learnTimer.current.card !== card) return;
    clearLearnTimers();
    dispatch({ type: "learned-dismiss", card });
  }, [clearLearnTimers]);

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

  // PILL-LISTEN-1: every subscription registers only when the async
  // listen() promise resolves (useTauriEvent), so a take that starts before
  // the resolve is invisible to the event handlers. The backend's current
  // phase is read once with the existing get_dictation_state command
  // (crates/teletype-desktop/src/commands/system.rs, wire shape UiState:
  // tag "phase", rename_all = "camelCase") and applied through the same
  // dictation-state reducer path the events use, so the reconcile cannot
  // create a second state model. The reconciled ref and the latestPillPhase
  // guard keep the two safety properties: at most one apply per mount, and
  // never a phase the events already delivered.
  const reconciled = useRef(false);
  const latestPillPhase = useRef(machine.pill.phase);
  latestPillPhase.current = machine.pill.phase;
  const reconcile = useCallback(async () => {
    if (reconciled.current) return;
    reconciled.current = true;
    if (latestPillPhase.current !== "idle") return;
    try {
      const state = await invoke<DictationStatePayload>("get_dictation_state");
      if (typeof state?.phase !== "string") return;
      if (latestPillPhase.current !== "idle") return;
      dispatch({ type: "dictation-state", payload: state, now: Date.now() });
    } catch {
      // Backend not ready yet; the next event will carry the phase.
    }
  }, [dispatch]);
  useEffect(() => {
    reconcile();
  }, [reconcile]);

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
    // Only a new take (not the trailing idle push) reclaims the slot from a
    // learned pill; learning fires seconds after the take has gone idle.
    if (payload.phase !== "idle") clearLearnTimers();
    dispatch({ type: "pill-state", payload, now: Date.now() });
  });

  // The backend emits these only to the pill window, after allocating the
  // overlay card id. A stale id (an older learn arriving late) must hand the
  // card straight back, or Rust waits forever for its size report.
  useTauriEvent<LearnedWordPayload>("learned-word", ({ payload }) => {
    if (!payload || typeof payload.card !== "number") return;
    if (payload.card <= learnedCardRef.current) {
      invoke("dismiss_pill_result", { card: payload.card }).catch(() => {});
      return;
    }
    learnedCardRef.current = payload.card;
    startLearnTimers(payload.card);
    dispatch({ type: "learned-show", payload });
  });

  useTauriEvent<LearnedWordErrorPayload>("learned-word-error", ({ payload }) => {
    if (!payload || typeof payload.card !== "number") return;
    if (payload.card <= learnedCardRef.current) {
      invoke("dismiss_pill_result", { card: payload.card }).catch(() => {});
      return;
    }
    learnedCardRef.current = payload.card;
    clearLearnTimers();
    dispatch({ type: "learned-error-show", payload });
    learnErrorTimer.current = setTimeout(
      () => dispatch({ type: "learned-error-expire", card: payload.card }),
      LEARN_ERROR_MS,
    );
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

  // T10c: the backend's throttled download fraction for the warming pill
  // ("Downloading model 42%"). Emitted from the model-download progress
  // callback while a speech/LLM model is being fetched on first use.
  useTauriEvent<number>("model-load-progress", ({ payload }) => {
    if (typeof payload === "number" && payload >= 0 && payload <= 1) {
      dispatch({ type: "model-load-progress", fraction: payload });
    }
  });

  useTauriEvent<number>("pill-level", ({ payload }) => {
    dispatch({ type: "level", level: payload });
  });

  // Post-insertion polish summary. The backend emits it only after a delivery
  // that actually landed, and only when polish changed words.
  useTauriEvent<PolishDiffPayload>("polish-diff", ({ payload }) => {
    // A live learned pill has precedence; decline the polish card so it does
    // not displace the Undo the user is about to click.
    const current = learnedRef.current;
    if (current.phase === "learned" && current.status === "live") {
      if (typeof payload.card === "number") {
        invoke("dismiss_pill_result", { card: payload.card }).catch(() => {});
      }
      return;
    }
    // Rust already made the pill clickable and is waiting for this card's
    // size report (overlay::show_result). A card the reducer drops never
    // renders, so hand it straight back, or the pill would stay waiting,
    // clickable, until the next take.
    if (!polishDiffShowable(payload)) {
      if (typeof payload.card === "number") {
        invoke("dismiss_pill_result", { card: payload.card }).catch(() => {});
      }
      return;
    }
    if (resultTimer.current) clearTimeout(resultTimer.current);
    dispatch({ type: "polish-diff", payload });
    resultTimer.current = setTimeout(
      () => dispatch({ type: "result-dismiss" }),
      POLISH_DIFF_TTL_MS,
    );
  });

  // A notice card (polish result or learned word) came back from a hidden
  // window; once it leaves for idle, Rust hides the window again. A take that
  // replaced it owns the window, so no call in that case.
  const shownCard = useRef<number | null>(null);
  useEffect(() => {
    const p = machine.pill;
    if (p.phase === "result" || p.phase === "learned" || p.phase === "learnedError") {
      shownCard.current = p.card ?? null;
      return;
    }
    const card = shownCard.current;
    shownCard.current = null;
    if (card !== null && p.phase === "idle") {
      invoke("dismiss_pill_result", { card }).catch(() => {});
    }
  }, [machine.pill]);

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
      clearLearnTimers();
    };
  }, [clearLearnTimers]);

  return {
    machine,
    dispatch,
    selectDevice,
    lockLanguage,
    toggleRecordingMode,
    hoverLearnedWord,
    undoLearnedWord,
    keepLearnedWord,
  };
}
