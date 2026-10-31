// The "glow" pill style is adapted from OpenWhispr's VoicePill
// (github.com/OpenWhispr/openwhispr, src/components/dictation/VoicePill.tsx,
// PillWaveform.tsx, waveformMath.ts, src/helpers/voicePillPresentation.js,
// src/styles/dictation-panel.css). Copyright (c) 2024 OpenWhispr Team.
// MIT License. Modified: driven by Teletype's pill phases and the scalar
// `pill-level` stream, transform-based bars, glow confined inside the pill
// box, and a text note state that OpenWhispr does not have.
//
// Pure model for the glow pill: classification of machine phases into the
// four glow views, the bar math, and the spoken phase sentence. No React.

import type { PillMachineState, PillPhaseName } from "../pillMachine";

/** Bars in the recording lane (OpenWhispr's waveform bar count). */
export const GLOW_BARS = 11;
/** Height of a silent bar, in px. */
export const GLOW_BAR_MIN_PX = 4;
/** Height of a full-scale bar, in px; also the bar element's own height. */
export const GLOW_BAR_MAX_PX = 22;
/** History sampling period: syllable-rate spacing between bars. */
export const GLOW_SAMPLE_MS = 80;

/** Every pill style id this build can render. */
export const KNOWN_PILL_STYLES = ["default", "classic", "levelRail", "well", "dotGrid", "glow"] as const;
export type KnownPillStyle = (typeof KNOWN_PILL_STYLES)[number];

/**
 * The style the pill should render. Settings not loaded yet keep the shipped
 * default (`classic`); an id this build does not know (a newer build's style
 * after a downgrade) falls back to `default`, which renders every phase.
 * Without this, an unknown style measures 0x0 and the recording pill is
 * invisible.
 */
export function knownPillStyle(id: string | null | undefined): KnownPillStyle {
  if (id === null || id === undefined) return "classic";
  return (KNOWN_PILL_STYLES as readonly string[]).includes(id) ? (id as KnownPillStyle) : "default";
}

/** Bar height for a 0..1 display level, as a scaleY factor of the 22px bar. */
export function barScale(v: number): number {
  const s = Math.min(1, Math.max(0, Number.isFinite(v) ? v : 0));
  return (GLOW_BAR_MIN_PX + s * (GLOW_BAR_MAX_PX - GLOW_BAR_MIN_PX)) / GLOW_BAR_MAX_PX;
}

/** Slide the bar history one step: oldest left, newest right. Never mutates. */
export function pushSample(history: readonly number[], sample: number): number[] {
  return [...history.slice(1), sample];
}

const TRAILING_ELLIPSIS = /(…|\.\.\.)\s*$/;

/** A busy message that is still in progress ("Inserting…", "Polishing..."). */
export function isWorkingMessage(phase: PillPhaseName, message: string): boolean {
  if (phase === "transcribing" || phase === "streaming") return true;
  if (phase === "error") return false;
  return TRAILING_ELLIPSIS.test(message);
}

export type GlowTone = "neutral" | "success" | "warn" | "error";

/** The note's tone. "Pasted" and "Too short" arrive as `processing`, so
 * classification is by message, not phase. */
export function glowTone(phase: PillPhaseName, message: string, skip: string | null): GlowTone {
  if (phase === "error") return "error";
  if (/^(pasted|inserted)\b/i.test(message)) return "success";
  if (/too short/i.test(message) || skip) return "warn";
  return "neutral"; // "Cancelled" and anything unknown
}

export type GlowView = "warming" | "recording" | "working" | "note";

/** Which glow view a machine state renders; null for the phases glow leaves
 * to the shared chrome (idle bars, polish result card). */
export function glowView(machine: PillMachineState): GlowView | null {
  const pill = machine.pill;
  switch (pill.phase) {
    case "idle":
    case "result":
      return null;
    case "warming":
      return "warming";
    case "recording":
      return "recording";
    default:
      // A skip line or the mic action needs the note's room.
      if (machine.skipMessage || machine.micFailure) return "note";
      return isWorkingMessage(pill.phase, pill.message) ? "working" : "note";
  }
}

export function stripEllipsis(message: string): string {
  return message.replace(TRAILING_ELLIPSIS, "").trim();
}

/** The phase named in words, for the polite live region. Download progress
 * is announced only at 25% steps so a screen reader is not flooded. */
export function glowSentence(machine: PillMachineState): string {
  const pill = machine.pill;
  switch (pill.phase) {
    case "idle":
    case "result":
      return "";
    case "recording":
      return pill.command ? "Recording a command" : "Recording";
    case "warming":
      if (machine.loadProgress === null) return "Getting model ready";
      return `Downloading model ${Math.floor(Math.min(1, Math.max(0, machine.loadProgress)) * 4) * 25}%`;
    case "transcribing":
      return "Transcribing";
    case "streaming":
      return "Transforming";
    default:
      return isWorkingMessage(pill.phase, pill.message) ? stripEllipsis(pill.message) : pill.message;
  }
}
