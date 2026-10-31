// The "glow" pill style is adapted from OpenWhispr's VoicePill
// (github.com/OpenWhispr/openwhispr, src/components/dictation/VoicePill.tsx,
// PillWaveform.tsx, waveformMath.ts, src/helpers/voicePillPresentation.js,
// src/styles/dictation-panel.css). Copyright (c) 2024 OpenWhispr Team.
// MIT License. Modified: driven by Teletype's pill phases and the scalar
// `pill-level` stream, transform-based bars, glow confined inside the pill
// box, and a text note state that OpenWhispr does not have.
//
// Behaviour-only grafts, re-implemented here (no code copied): the dim lane
// until the first real level (FluidVoice, Handy), scaleY writes from one rAF
// loop (vocs), peak-hold between samples (EnviousWispr), the 320ms minimum
// working hold (whisperstream), the wash timings (talkdat) and the fixed
// Cancel slot (Handy).
//
// Layout contract (research/pills/PILL_SPEC.md section 2): the OS window is
// sized to the measured frame, so nothing paints outside the pill box (every
// ring and glow is inset), hover never changes size, and a shrink happens
// only after the inner morph has finished.

import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { useReducedMotion } from "./pill";
import { pillMessage, type PillMachineState } from "./pillMachine";
import { glowFrameTitle, glowSentence, glowView, type GlowView } from "./lib/glowModel";
import {
  CAPSULE_W,
  CIRCLE_W,
  ENTRANCE_LANE_AT_MS,
  ENTRANCE_WIDEN_AT_MS,
  FADE_MS,
  GLOW,
  HEIGHT,
  MORPH_FALLBACK_MS,
  VISUALLY_HIDDEN,
  WORKING_HOLD_MS,
  useGlowSurface,
} from "./lib/glowTokens";
import { GlowNote, GlowShell, GlowWarming, type LevelFeed, type Stage } from "./pillGlowParts";

export { GLOW, usePrefersMoreContrast, usePrefersReducedTransparency } from "./lib/glowTokens";

// --- The frame -----------------------------------------------------------------

interface Size {
  w: number;
  h: number;
}

/** The fixed footprints: the recording capsule and the working circle. */
function footprintOf(view: GlowView): Size | null {
  if (view === "recording") return { w: CAPSULE_W, h: HEIGHT };
  if (view === "working") return { w: CIRCLE_W, h: HEIGHT };
  return null;
}

function viewKeyOf(view: GlowView, machine: PillMachineState): string {
  if (view !== "note") return view;
  return `note|${pillMessage(machine.pill)}|${machine.skipMessage ?? ""}|${machine.micFailure}`;
}

/**
 * The glow pill for every phase except idle and result. The measured element
 * is the outer frame (the ResizeObserver in pill.tsx reports its box), and the
 * visible pill is an inner element centred in it. A footprint change grows the
 * frame at once, but a shrink holds the previous size as a floor until the
 * inner morph or cross-fade is done, so the window shrinks once, after the
 * animation, and never clips it.
 */
export function GlowPill({
  machine,
  warmingSeconds,
  setCancelArmed,
}: {
  machine: PillMachineState;
  warmingSeconds: number;
  setCancelArmed: (armed: boolean) => void;
}) {
  const reduced = useReducedMotion();
  const surface = useGlowSurface();
  const target: GlowView = glowView(machine) ?? "working";

  // Minimum working hold: once the circle is up, a note waits until it has
  // been visible for WORKING_HOLD_MS, so a fast take does not flash.
  const [workingSince, setWorkingSince] = useState<number | null>(null);
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  const holdLeft =
    target === "note" && workingSince !== null ? WORKING_HOLD_MS - (Date.now() - workingSince) : 0;
  const view: GlowView = holdLeft > 0 ? "working" : target;
  useEffect(() => {
    if (view === "working") {
      if (workingSince === null) setWorkingSince(Date.now());
    } else if (workingSince !== null) {
      setWorkingSince(null);
    }
  }, [view, workingSince]);
  useEffect(() => {
    if (!(target === "note" && view === "working" && workingSince !== null)) return;
    const left = WORKING_HOLD_MS - (Date.now() - workingSince);
    const t = setTimeout(rerender, Math.max(0, left));
    return () => clearTimeout(t);
  }, [target, view, workingSince]);

  // The live sentence; while a note is held back, keep announcing the work.
  const heldSentence = useRef("");
  const sentence = view === target ? glowSentence(machine) : heldSentence.current;
  if (view === "working" && view === target) heldSentence.current = sentence;

  // Shrink lock: on a view change, the previous frame size becomes a floor.
  const frameRef = useRef<HTMLDivElement | null>(null);
  const lastSize = useRef<Size | null>(null);
  const viewKey = viewKeyOf(view, machine);
  const [prevKey, setPrevKey] = useState(viewKey);
  const [lock, setLock] = useState<(Size & { morph: boolean }) | null>(null);
  if (viewKey !== prevKey) {
    setPrevKey(viewKey);
    const prev = lastSize.current;
    const shell = (k: string) => k === "recording" || k === "working";
    setLock(
      !reduced && prev && prev.w > 0 && prev.h > 0
        ? { ...prev, morph: shell(prevKey) && shell(viewKey) }
        : null,
    );
  }
  const minW = Math.max(lock?.w ?? 0, footprintOf(view)?.w ?? 0);
  const minH = Math.max(lock?.h ?? 0, footprintOf(view)?.h ?? 0);
  // Measured only when the frame's size can change (a new view, a lock, the
  // warming text), never on the 20 Hz level renders: a measurement forces a
  // synchronous layout.
  const progress = machine.loadProgress;
  useLayoutEffect(() => {
    const r = frameRef.current?.getBoundingClientRect();
    if (r) lastSize.current = { w: r.width, h: r.height };
  }, [viewKey, minW, minH, warmingSeconds, progress]);
  useEffect(() => {
    if (!lock) return;
    const t = setTimeout(() => setLock(null), lock.morph ? MORPH_FALLBACK_MS : FADE_MS);
    return () => clearTimeout(t);
  }, [lock]);

  // Staged entrance, keyed on the take so a re-emit does not restart it.
  const entryKey = view === "recording" && machine.pill.phase === "recording" ? String(machine.pill.startedAtMs) : null;
  const [stageKey, setStageKey] = useState(entryKey);
  const [stage, setStage] = useState<Stage>(reduced ? "lane" : "dot");
  if (entryKey !== stageKey) {
    setStageKey(entryKey);
    if (entryKey !== null) setStage(reduced ? "lane" : "dot");
  }
  useEffect(() => {
    if (entryKey === null) return;
    if (reduced) {
      setStage("lane");
      return;
    }
    const widen = setTimeout(() => setStage("wide"), ENTRANCE_WIDEN_AT_MS);
    const lane = setTimeout(() => setStage("lane"), ENTRANCE_LANE_AT_MS);
    return () => {
      clearTimeout(widen);
      clearTimeout(lane);
    };
  }, [entryKey, reduced]);

  // The level stream reaches the bars through this feed, not props, so a
  // level event re-renders neither the memoised shell nor its meter.
  // Peak-hold: a transient between two bar samples is kept, not dropped.
  const feed = useRef<LevelFeed>({ latest: 0, at: 0, peak: 0, fresh: false });
  const levels = machine.levels;
  const latest = levels.length > 0 ? levels[levels.length - 1] : 0;
  const recording = view === "recording" && machine.pill.phase === "recording";
  const meterActive = recording && machine.levelArrived;
  useEffect(() => {
    const f = feed.current;
    f.latest = latest;
    f.at = performance.now();
    if (!meterActive) return;
    f.peak = f.fresh ? Math.max(f.peak, latest) : latest;
    f.fresh = true;
  }, [levels, meterActive, latest]);
  const onMorphEnd = useCallback(() => setLock((l) => (l && l.morph ? null : l)), []);

  const isError = machine.pill.phase === "error" && view === "note";
  const command = machine.pill.phase === "recording" && !!machine.pill.command;
  const takeKey = machine.pill.phase === "recording" ? machine.pill.startedAtMs : 0;

  return (
    <div
      ref={frameRef}
      data-glow-frame=""
      title={glowFrameTitle(view, machine.pill.phase, sentence)}
      style={{
        position: "relative",
        // A block-level flex box, not inline-flex: an inline frame sits on a
        // text baseline and the line box adds a few px of descender space
        // under the textless shell, which the window would then report.
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
        flexShrink: 0,
        width: "max-content",
        height: "max-content",
        minWidth: minW || undefined,
        minHeight: minH || undefined,
        color: GLOW.text,
      }}
    >
      <span role="status" aria-live="polite" aria-atomic="true" style={VISUALLY_HIDDEN}>
        {isError ? "" : sentence}
      </span>
      {(view === "recording" || view === "working") && (
        <GlowShell
          view={view}
          recording={recording}
          command={command}
          takeKey={takeKey}
          hovered={machine.hovered}
          cancelArmed={machine.cancelArmed}
          levelArrived={machine.levelArrived}
          feed={feed}
          stage={stage}
          reduced={reduced}
          surface={surface}
          setCancelArmed={setCancelArmed}
          morphing={!!lock?.morph}
          onMorphEnd={onMorphEnd}
        />
      )}
      {view === "note" && <GlowNote machine={machine} reduced={reduced} surface={surface} />}
      {view === "warming" && (
        <GlowWarming seconds={warmingSeconds} progress={machine.loadProgress} reduced={reduced} surface={surface} />
      )}
    </div>
  );
}
