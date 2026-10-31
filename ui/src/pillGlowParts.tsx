// The "glow" pill style is adapted from OpenWhispr's VoicePill
// (github.com/OpenWhispr/openwhispr, src/components/dictation/VoicePill.tsx,
// PillWaveform.tsx, waveformMath.ts, src/helpers/voicePillPresentation.js,
// src/styles/dictation-panel.css). Copyright (c) 2024 OpenWhispr Team.
// MIT License. Modified: driven by Teletype's pill phases and the scalar
// `pill-level` stream, transform-based bars, glow confined inside the pill
// box, and a text note state that OpenWhispr does not have.
//
// The visible glow views: the recording capsule / working circle shell, the
// note capsule and the warming capsule. The frame that measures and locks
// them is GlowPill in ui/src/pillGlow.tsx.

import { useEffect, useLayoutEffect, useRef, type CSSProperties } from "react";
import {
  CancelButton,
  MicFailureAction,
  RecordingDot,
  SettingsAction,
  warmingStatus,
} from "./pill";
import { errorHasSettingsAction, pillMessage, type PillMachineState } from "./pillMachine";
import {
  GLOW_BARS,
  GLOW_BAR_MAX_PX,
  GLOW_SAMPLE_MS,
  barScale,
  glowTone,
  pushSample,
  type GlowTone,
} from "./lib/glowModel";
import {
  CAPSULE_W,
  CIRCLE_W,
  FADE_MS,
  GLOW,
  HEIGHT,
  LANE_DIM,
  MORPH_MS,
  RADIUS,
  RING_GRADIENT,
  RING_MASK,
  usePrefersMoreContrast,
  type Surface,
} from "./lib/glowTokens";

// --- Recording lane ----------------------------------------------------------

/** Eleven bars driven by direct transform writes from one rAF loop, so a
 * level update never re-renders React. Each take starts flat (4px). */
function GlowMeter({ levels, active, takeKey, reduced }: { levels: number[]; active: boolean; takeKey: number; reduced: boolean }) {
  const bars = useRef<Array<HTMLSpanElement | null>>([]);
  const history = useRef<number[]>(new Array(GLOW_BARS).fill(0));
  const peak = useRef(0);
  const fresh = useRef(false);
  const latest = levels.length > 0 ? levels[levels.length - 1] : 0;
  const latestRef = useRef(latest);
  const latestAt = useRef(0);

  const paint = () => {
    history.current.forEach((h, i) => {
      const el = bars.current[i];
      if (el) el.style.transform = `scaleY(${barScale(h)})`;
    });
  };

  useLayoutEffect(() => {
    history.current = new Array(GLOW_BARS).fill(0);
    peak.current = 0;
    fresh.current = false;
    paint();
  }, [takeKey]);

  // Peak-hold: a transient between two samples is kept, not dropped.
  useEffect(() => {
    latestRef.current = latest;
    latestAt.current = performance.now();
    if (!active) return;
    peak.current = fresh.current ? Math.max(peak.current, latest) : latest;
    fresh.current = true;
  }, [levels, active, latest]);

  useEffect(() => {
    if (!active || typeof requestAnimationFrame !== "function") return;
    let raf = 0;
    let last: number | null = null;
    let acc = 0;
    const tick = (t: number) => {
      if (last !== null && !document.hidden) {
        acc += t - last;
        if (acc >= GLOW_SAMPLE_MS) {
          // Never replay a backlog after a stall: one sample per tick.
          acc = acc >= GLOW_SAMPLE_MS * 2 ? 0 : acc - GLOW_SAMPLE_MS;
          // No level since the last sample: a slow stream holds its latest
          // value for a moment rather than drawing a false gap, but a stalled
          // one drops to the flat line instead of freezing a wave.
          const stale = t - latestAt.current > GLOW_SAMPLE_MS * 2;
          const sample = fresh.current ? peak.current : stale ? 0 : latestRef.current;
          history.current = pushSample(history.current, sample);
          peak.current = 0;
          fresh.current = false;
          paint();
        }
      }
      last = t;
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [active]);

  return (
    <span
      aria-hidden="true"
      data-glow-lane=""
      style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 3, width: 60, height: GLOW_BAR_MAX_PX }}
    >
      {Array.from({ length: GLOW_BARS }, (_, i) => (
        <span
          key={i}
          aria-hidden="true"
          data-glow-bar=""
          ref={(el) => {
            bars.current[i] = el;
          }}
          style={{
            width: 2,
            height: GLOW_BAR_MAX_PX,
            flexShrink: 0,
            borderRadius: 1,
            background: GLOW.bar,
            transformOrigin: "50% 50%",
            transition: reduced ? undefined : "transform 75ms linear",
          }}
        />
      ))}
    </span>
  );
}

// --- Working circle layers ---------------------------------------------------

function GlowWorkingLayers({ reduced }: { reduced: boolean }) {
  return (
    <span aria-hidden="true" data-glow-working="" style={{ position: "absolute", inset: 0, animation: "fade-in 220ms ease-out" }}>
      <span
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: "50%",
          background: "radial-gradient(circle, rgba(65,105,225,0.35) 0%, rgba(65,105,225,0) 70%)",
          opacity: reduced ? 0.6 : undefined,
          animation: reduced ? undefined : "glow-breathe 1.9s ease-in-out infinite",
        }}
      />
      <span
        data-glow-ring=""
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: "50%",
          pointerEvents: "none",
          background: RING_GRADIENT,
          mask: RING_MASK,
          WebkitMask: RING_MASK,
          opacity: reduced ? 0.8 : undefined,
          animation: reduced ? undefined : "glow-spin 1.6s linear infinite",
        }}
      />
      <span
        style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", gap: 2 }}
      >
        {[8, 12, 8].map((h, i) => (
          <span key={i} style={{ width: 2.5, height: h, borderRadius: 999, background: GLOW.mark }} />
        ))}
      </span>
    </span>
  );
}

// --- The shell: recording capsule and working circle -------------------------

export type Stage = "dot" | "wide" | "lane";

/** One element for both fixed footprints, so the capsule-to-circle change is
 * a real CSS width morph rather than a remount. */
export function GlowShell({
  machine,
  view,
  stage,
  reduced,
  surface,
  setCancelArmed,
  onMorphEnd,
  morphing,
}: {
  machine: PillMachineState;
  view: "recording" | "working";
  stage: Stage;
  /** The shrink lock still holds the frame: keep the frozen lane fading out. */
  morphing: boolean;
  reduced: boolean;
  surface: Surface;
  setCancelArmed: (armed: boolean) => void;
  onMorphEnd: () => void;
}) {
  const recording = view === "recording" && machine.pill.phase === "recording";
  const command = machine.pill.phase === "recording" && !!machine.pill.command;
  const takeKey = machine.pill.phase === "recording" ? machine.pill.startedAtMs : 0;
  const width = view === "working" || stage === "dot" ? CIRCLE_W : CAPSULE_W;
  const laneVisible = recording && stage === "lane" && !machine.hovered;
  const laneOpacity = laneVisible ? (machine.levelArrived ? 1 : LANE_DIM) : 0;
  return (
    <div
      data-glow-shell=""
      onTransitionEnd={(e) => {
        if (e.target === e.currentTarget && e.propertyName === "width") onMorphEnd();
      }}
      style={{
        position: "relative",
        boxSizing: "border-box",
        width,
        height: HEIGHT,
        flexShrink: 0,
        borderRadius: RADIUS,
        overflow: "hidden",
        background: surface.background,
        boxShadow: [surface.border, GLOW.highlight, command ? GLOW.commandRing : null].filter(Boolean).join(", "),
        transition: reduced
          ? undefined
          : `width ${MORPH_MS}ms ${GLOW.ease}, border-radius ${MORPH_MS}ms ${GLOW.ease}`,
      }}
    >
      {(recording || morphing) && (
        <div
          aria-hidden={recording ? undefined : "true"}
          style={{
            position: "absolute",
            top: 0,
            left: 0,
            display: "flex",
            alignItems: "center",
            gap: 6,
            width: CAPSULE_W,
            height: "100%",
            paddingLeft: 6,
            boxSizing: "border-box",
            opacity: recording ? 1 : 0,
            visibility: recording ? "visible" : "hidden",
            transition: `opacity ${FADE_MS}ms ease-out, visibility 0s linear ${recording ? 0 : FADE_MS}ms`,
          }}
        >
          <RecordingDot command={command} />
          <span style={{ position: "relative", display: "flex", alignItems: "center", justifyContent: "center", width: 60, height: HEIGHT }}>
            <span
              data-glow-lane-wrap=""
              style={{
                display: "flex",
                opacity: laneOpacity,
                transition: `opacity ${machine.hovered ? 120 : FADE_MS}ms ease-out`,
              }}
            >
              <GlowMeter
                levels={machine.levels}
                active={recording && machine.levelArrived}
                takeKey={takeKey}
                reduced={reduced}
              />
            </span>
            {recording && machine.hovered && (
              <span style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", animation: "fade-in 120ms ease-out" }}>
                <CancelButton armed={machine.cancelArmed} setArmed={setCancelArmed} style={{ minHeight: 24, padding: "5px 10px", whiteSpace: "nowrap" }} />
              </span>
            )}
          </span>
        </div>
      )}
      {view === "working" && <GlowWorkingLayers reduced={reduced} />}
    </div>
  );
}

// --- Notes -------------------------------------------------------------------

function NoteGlyph({ tone }: { tone: GlowTone }) {
  if (tone === "neutral") return null;
  const color = tone === "success" ? GLOW.success : tone === "warn" ? GLOW.warn : GLOW.error;
  return (
    <svg aria-hidden="true" width={14} height={14} viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
      {tone === "success" && <path d="M2.5 7.5l3 3 6-7" />}
      {tone !== "success" && <circle cx="7" cy="7" r="5.6" />}
      {tone === "warn" && (
        <>
          <path d="M7 4v3.6" />
          <path d="M7 10h0" />
        </>
      )}
      {tone === "error" && <path d="M5 5l4 4M9 5l-4 4" />}
    </svg>
  );
}

function washStyle(tone: GlowTone, reduced: boolean, contrast: boolean): CSSProperties | null {
  if (tone === "neutral") return null;
  const [r, g, b, a] = tone === "success" ? GLOW.washSuccess : tone === "warn" ? GLOW.washWarn : GLOW.washError;
  const background = `rgba(${r},${g},${b},${contrast ? a * 2 : a})`;
  if (reduced) return { background, opacity: tone === "success" ? 0.8 : 1 };
  const animation =
    tone === "success"
      ? "fade-in 90ms ease-out, glow-settle 430ms ease-out 90ms forwards"
      : tone === "warn"
        ? "fade-in 120ms ease-out"
        : "fade-in 140ms ease-out, glow-beat 380ms ease-in-out 140ms 2";
  return { background, animation };
}

export function GlowNote({
  machine,
  reduced,
  surface,
}: {
  machine: PillMachineState;
  reduced: boolean;
  surface: Surface;
}) {
  const contrast = usePrefersMoreContrast();
  const phase = machine.pill.phase;
  const message = pillMessage(machine.pill);
  const skip = machine.skipMessage;
  const error = phase === "error";
  const tone = glowTone(phase, message, skip);
  const wash = washStyle(tone, reduced, contrast);
  // The action sits inside a 36px single-line note: the button is 24px tall,
  // and the negative margin keeps it from adding to the row height.
  const actionStyle: CSSProperties = { minHeight: 24, padding: "4px 12px", lineHeight: "14px", margin: "-4px 0" };
  return (
    <div
      data-glow-note={tone}
      style={{
        position: "relative",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        boxSizing: "border-box",
        minHeight: HEIGHT,
        // One line stays 36 (min-height); two lines or a skip line land on
        // exactly 52 (36 of text plus 8px above and below).
        padding: "8px 14px",
        width: "max-content",
        minWidth: 120,
        maxWidth: 480,
        flexShrink: 0,
        borderRadius: RADIUS,
        background: surface.background,
        boxShadow: `${surface.border}, ${GLOW.highlight}`,
        color: GLOW.text,
        animation: `fade-in ${FADE_MS}ms ease-out`,
      }}
    >
      {wash && (
        <span
          aria-hidden="true"
          data-glow-wash=""
          style={{ position: "absolute", inset: 0, borderRadius: "inherit", pointerEvents: "none", ...wash }}
        />
      )}
      <NoteGlyph tone={tone} />
      <span style={{ position: "relative", display: "flex", flexDirection: "column", gap: 2, flex: "1 1 auto", minWidth: 0 }}>
        <p
          role={error ? "alert" : undefined}
          title={message}
          style={{
            margin: 0,
            fontSize: 13,
            lineHeight: "18px",
            fontWeight: 500,
            color: error ? GLOW.error : GLOW.text,
            whiteSpace: "normal",
            overflowWrap: "anywhere",
            display: "-webkit-box",
            WebkitLineClamp: 2,
            WebkitBoxOrient: "vertical",
            overflow: "hidden",
          }}
        >
          {message}
        </p>
        {skip && (
          <p
            title={skip}
            style={{
              margin: 0,
              fontSize: 12,
              lineHeight: "16px",
              fontWeight: 500,
              color: GLOW.warn,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {skip}
          </p>
        )}
      </span>
      {machine.micFailure ? (
        <MicFailureAction style={actionStyle} />
      ) : error && errorHasSettingsAction(message) ? (
        <SettingsAction style={actionStyle} />
      ) : null}
    </div>
  );
}

// --- Warming -------------------------------------------------------------------

export function GlowWarming({ seconds, progress, reduced, surface }: { seconds: number; progress: number | null; reduced: boolean; surface: Surface }) {
  const status = warmingStatus(progress);
  return (
    <div
      data-glow-warming=""
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        boxSizing: "border-box",
        height: HEIGHT,
        padding: "0 16px 0 8px",
        width: "max-content",
        minWidth: 160,
        maxWidth: 480,
        flexShrink: 0,
        borderRadius: RADIUS,
        background: surface.background,
        boxShadow: `${surface.border}, ${GLOW.highlight}`,
        animation: `fade-in ${FADE_MS}ms ease-out`,
      }}
    >
      <span
        aria-hidden="true"
        style={{
          width: 20,
          height: 20,
          flexShrink: 0,
          borderRadius: "50%",
          background: RING_GRADIENT,
          mask: RING_MASK,
          WebkitMask: RING_MASK,
          opacity: reduced ? 0.8 : undefined,
          animation: reduced ? undefined : "glow-breathe 1.9s ease-in-out infinite",
        }}
      />
      <span
        title={status}
        style={{ minWidth: 0, fontSize: 13, lineHeight: "18px", fontWeight: 500, color: GLOW.text, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
      >
        {status}
        {seconds >= 3 && <span style={{ color: GLOW.textSecondary, fontVariantNumeric: "tabular-nums" }}> · {seconds}s</span>}
      </span>
    </div>
  );
}

