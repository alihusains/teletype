import { StrictMode, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "./lib/useTauriEvent";
import { useNow } from "./lib/useNow";

const WAVE_BARS = 34;
const WAVE_HEIGHT = 22;

const SIZES = {
  idle: { width: 58, height: 24 },
  warming: { width: 320, height: 44 },
  processing: { width: 220, height: 44 },
  processingSkip: { width: 420, height: 44 },
  recording: { width: 250, height: 44 },
  recordingExpanded: { width: 460, height: 44 },
} as const;

// Max width the processing pill will grow to before clipping (EW uses
// content-sized width; we cap to keep the pill on-screen).
const PROCESSING_MAX_WIDTH = 480;

type PillPhase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

interface Settings {
  recordingMode: string;
  language: string;
  inputDevice: string;
  pillStyle: string;
}

// --- Pill styles (ported from the reference implementation's designs) ------
//
// Ported from LiveKit's Agents UI (github.com/livekit/components-js,
// packages/shadcn), Apache-2.0 licensed; modified (see DotGridMatrix). The
// original drives a dot grid from live agent state and multiband track
// volume; we keep the same sequence geometry (ring sweep, center pulse, row
// scan, volume rows) but drive it from Teletype's pill phases and the scalar
// `pill-level` stream, in inline styles with no Tailwind or livekit deps.
// "classic" and "levelRail" are their two compact capsule designs; "well" is
// their "Reading Well" panel. Live preview words in the well arrive with
// streaming ASR (P2-29); until then the well shows the listening header only.
// "dotGrid" is the Dot Matrix style from LiveKit's Agents UI (ported, above).

const RAINBOW = [
  "#ff2a40", "#ff8c00", "#ffd700", "#adff2f", "#00fa9a",
  "#00ffff", "#1e90ff", "#4169e1", "#8a2be2",
] as const;

// Dark translucent capsule surface shared by the ported designs.
const DARK_SURFACE = "rgba(20,20,28,0.82)";
const DARK_BORDER = "rgba(255,255,255,0.1)";
const DARK_BORDER_W = 0.5; // EW uses 0.5pt border, not 1px
const WELL_SURFACE = "rgba(17,15,24,0.90)"; // EW reading well is more opaque

// Interpolates across the 9-color brand spectrum; t in 0..1.
export function rainbowColor(t: number): string {
  const clamped = Math.min(1, Math.max(0, t));
  const scaled = clamped * (RAINBOW.length - 1);
  const i = Math.min(RAINBOW.length - 2, Math.floor(scaled));
  const f = scaled - i;
  const a = RAINBOW[i].match(/\w\w/g)!.map((h) => parseInt(h, 16));
  const b = RAINBOW[i + 1].match(/\w\w/g)!.map((h) => parseInt(h, 16));
  const rgb = a.map((v, k) => Math.round(v + (b[k] - v) * f));
  return `rgb(${rgb[0]},${rgb[1]},${rgb[2]})`;
}

// The breathing rainbow hairline along the capsule's bottom edge.
// EW fades the gradient to clear at both ends.
function RainbowHairline({ steady = false }: { steady?: boolean }) {
  return (
    <div
      style={{
        position: "absolute",
        left: 20,
        right: 20,
        bottom: 1,
        height: 1,
        borderRadius: 1,
        background: `linear-gradient(90deg, transparent, ${RAINBOW.join(",")}, transparent)`,
        opacity: steady ? 0.5 : undefined,
        animation: steady ? undefined : "hairline-breathe 2s ease-in-out infinite",
        pointerEvents: "none",
      }}
    />
  );
}

// 18 vertical bars (9 upper + 9 lower) that scale with the audio level,
// colored across the rainbow spectrum.
export function RainbowLips({ level }: { level: number }) {
  const bars = 18;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 1.5, height: 24 }}>
      {Array.from({ length: bars }, (_, i) => {
        const centerWeight = 1 - Math.abs(i - (bars - 1) / 2) / ((bars - 1) / 2) * 0.3;
        const scale = 0.55 + (0.9 - 0.55) * level * centerWeight;
        return (
          <span
            key={i}
            style={{
              width: 2.5,
              height: 20 * scale,
              borderRadius: 2,
              background: rainbowColor(i / (bars - 1)),
              transition: "height 90ms ease-out",
            }}
          />
        );
      })}
    </div>
  );
}

// 24 scrolling history bars, each colored by its position in the spectrum.
export function RainbowMeter({ levels, height = 16, barWidth = 2 }: { levels: number[]; height?: number; barWidth?: number }) {
  const n = 24;
  const hist = levels.slice(-n);
  while (hist.length < n) hist.unshift(0);
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 1.5, height }}>
      {hist.map((level, i) => (
        <span
          key={i}
          style={{
            width: barWidth,
            flexShrink: 0,
            height: Math.max(height * 0.14, level * height),
            borderRadius: barWidth / 2,
            background: rainbowColor(i / (n - 1)),
            transition: "height 80ms ease-out",
          }}
        />
      ))}
    </div>
  );
}

// --- Inline SVG icons (no external deps) -----------------------------------

function MicIcon({ size = 11 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="2" width="6" height="12" rx="3" />
      <path d="M5 10a7 7 0 0 0 14 0" />
      <line x1="12" y1="17" x2="12" y2="21" />
    </svg>
  );
}

function HandIcon({ size = 11 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M18 11V6a2 2 0 0 0-4 0v5" />
      <path d="M14 10V4a2 2 0 0 0-4 0v6" />
      <path d="M10 10.5V6a2 2 0 0 0-4 0v8" />
      <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
    </svg>
  );
}

function RepeatIcon({ size = 11 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="m17 2 4 4-4 4" />
      <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
      <path d="m7 22-4-4 4-4" />
      <path d="M21 13v1a4 4 0 0 1-4 4H3" />
    </svg>
  );
}

function GlobeIcon({ size = 11 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" />
      <path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" />
      <path d="M2 12h20" />
    </svg>
  );
}

function ChevronsIcon({ size = 10 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="m7 15 5 5 5-5" />
      <path d="m7 9 5-5 5 5" />
    </svg>
  );
}

// --- Components -------------------------------------------------------------

function IdleBars() {
  return (
    <div style={{ display: "flex", width: "100%", alignItems: "center", justifyContent: "center", gap: 3 }}>
      {[0.5, 0.85, 1, 0.85, 0.5].map((scale, i) => (
        <span
          key={i}
          style={{
            width: 2.5,
            height: 12 * scale,
            borderRadius: 999,
            background: "rgba(255,255,255,0.85)",
          }}
        />
      ))}
    </div>
  );
}

function Warming({ seconds }: { seconds: number }) {
  return (
    <div style={{ display: "flex", width: "100%", alignItems: "center", gap: 10, padding: "0 16px", animation: "fade-in 0.18s ease-out" }}>
      <span
        style={{
          width: 14,
          height: 14,
          flexShrink: 0,
          borderRadius: "50%",
          background: "conic-gradient(from 0deg, transparent, white 295deg, transparent 296deg)",
          mask: "radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px))",
          WebkitMask: "radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px))",
          animation: "spin 0.85s linear infinite",
        }}
      />
      <div style={{ minWidth: 0, flex: 1, lineHeight: 1.25 }}>
        <p style={{ fontSize: 13, fontWeight: 500, color: "rgba(255,255,255,0.9)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          Getting model ready…
        </p>
        {seconds >= 3 && (
          <p style={{ fontSize: 12, color: "rgba(255,255,255,0.55)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
            {seconds}s · {seconds >= 25 ? "almost there, first load only" : "first load only"}
          </p>
        )}
      </div>
      <span style={{ fontSize: 12, fontWeight: 500, color: "rgba(255,255,255,0.4)" }}>esc</span>
    </div>
  );
}

function RecordingDot() {
  return (
    <button
      type="button"
      title="Recording. Click or press your hotkey to stop."
      onClick={() => invoke("toggle_dictation")}
      style={{
        position: "relative",
        display: "flex",
        width: 24,
        height: 24,
        flexShrink: 0,
        alignItems: "center",
        justifyContent: "center",
        background: "none",
        border: "none",
        cursor: "pointer",
        padding: 0,
      }}
    >
      <span
        style={{
          position: "absolute",
          width: 10,
          height: 10,
          borderRadius: "50%",
          background: "rgba(255,69,61,0.35)",
          animation: "ping 1.4s cubic-bezier(0,0,0.2,1) infinite",
        }}
      />
      <span
        style={{
          position: "relative",
          width: 10,
          height: 10,
          borderRadius: "50%",
          background: "#ff453d",
        }}
      />
    </button>
  );
}

function Waveform({ levels }: { levels: number[] }) {
  return (
    <div style={{
      display: "flex",
      height: WAVE_HEIGHT,
      minWidth: 0,
      flex: 1,
      alignItems: "center",
      justifyContent: "flex-end",
      gap: 2,
      overflow: "hidden",
    }}>
      {levels.map((level, i) => (
        <span
          key={i}
          style={{
            width: 2.5,
            flexShrink: 0,
            borderRadius: 999,
            background: "rgba(255,255,255,0.9)",
            height: Math.max(3, level * WAVE_HEIGHT),
            transition: "height 100ms ease-out",
          }}
        />
      ))}
    </div>
  );
}

function Chip({ icon, label, onClick }: { icon: React.ReactNode; label: string; onClick?: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: "flex",
        height: 24,
        maxWidth: 96,
        alignItems: "center",
        gap: 4,
        borderRadius: 999,
        background: "rgba(255,255,255,0.12)",
        padding: "0 8px",
        fontSize: 12,
        fontWeight: 500,
        color: "rgba(255,255,255,0.9)",
        border: "none",
        cursor: "pointer",
        transition: "background-color 0.15s",
        whiteSpace: "nowrap",
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.2)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.12)")}
    >
      <span style={{ flexShrink: 0, display: "flex", alignItems: "center" }}>{icon}</span>
      <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{label}</span>
      <span style={{ flexShrink: 0, display: "flex", alignItems: "center", color: "rgba(255,255,255,0.45)" }}>
        <ChevronsIcon />
      </span>
    </button>
  );
}

function formatClock(totalSeconds: number) {
  if (!isFinite(totalSeconds) || totalSeconds < 0) return "0:00";
  const s = Math.max(0, Math.floor(totalSeconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// --- Ported recording styles (reference implementation) --------------------

interface RecordingStyleProps {
  levels: number[];
  level: number; // current smoothed level 0..1
  clock: number; // seconds
  startedAtMs: number;
  settings: Settings;
  deviceName: string;
  hovered: boolean;
  cancelArmed: boolean;
  setCancelArmed: (v: boolean) => void;
  interimText: string;
}

function recordingLevel(levels: number[]): number {
  if (levels.length === 0) return 0;
  const recent = levels.slice(-8);
  return recent.reduce((a, b) => a + b, 0) / recent.length;
}

function CancelButton({ armed, setArmed }: { armed: boolean; setArmed: (v: boolean) => void }) {
  return (
    <button
      onClick={() => {
        if (armed) {
          setArmed(false);
          invoke("cancel_dictation").catch(() => {});
        } else {
          setArmed(true);
          setTimeout(() => setArmed(false), 2500);
        }
      }}
      style={{
        border: "none",
        cursor: "pointer",
        borderRadius: 999,
        padding: "3px 10px",
        fontSize: 11,
        fontWeight: 600,
        fontFamily: "inherit",
        color: armed ? "white" : "rgba(255,255,255,0.75)",
        background: armed ? "#e5484d" : "rgba(255,255,255,0.12)",
        transition: "background 120ms ease, color 120ms ease",
      }}
    >
      {armed ? "Sure?" : "Cancel"}
    </button>
  );
}

// "Classic": dark capsule, audio-reactive rainbow lips + clock.
// No trailing text — the reference shows only the mark and the timer,
// so the 185 px box has room for both without clipping. Live interim text
// is intentionally omitted here: the 185 px row has no room beside the
// lips + clock, so the live preview is delivered by levelRail/dotGrid/well.
function ClassicPill({ clock, levels, hovered, cancelArmed, setCancelArmed }: RecordingStyleProps) {
  return (
    <div
      style={{
        position: "relative",
        width: 185,
        height: 44,
        borderRadius: 22,
        background: DARK_SURFACE,
        border: `${DARK_BORDER_W}px solid ${DARK_BORDER}`,
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 26px rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "0 14px",
        overflow: "hidden",
      }}
    >
      <div style={{ height: 24, display: "flex", alignItems: "center" }}>
        <RainbowLips level={recordingLevel(levels)} />
      </div>
      <span style={{ fontSize: 13, fontWeight: 600, color: "white", fontVariantNumeric: "tabular-nums", fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace" }}>
        {formatClock(clock)}
      </span>
      {hovered && (
        <span
          style={{
            position: "absolute",
            right: 8,
            top: "50%",
            transform: "translateY(-50%)",
            animation: "fade-in 150ms ease-out",
            display: "inline-flex",
            // Backdrop so the button is legible over the lips/clock it covers.
            background: DARK_SURFACE,
            borderRadius: 999,
            boxShadow: "0 0 0 3px " + DARK_SURFACE,
          }}
        >
          <CancelButton armed={cancelArmed} setArmed={setCancelArmed} />
        </span>
      )}
      <RainbowHairline />
    </div>
  );
}

// "Level Rail": dark capsule, clock on the left + 24-bar rainbow level meter
// filling the rest of the row. The meter is the subject; EW measured 288 px
// wide so the bars are the pill's whole point, not an ornament.
function LevelRailPill({ clock, levels, hovered, cancelArmed, setCancelArmed, interimText }: RecordingStyleProps) {
  return (
    <div
      style={{
        position: "relative",
        width: 288,
        height: 44,
        borderRadius: 22,
        background: DARK_SURFACE,
        border: `${DARK_BORDER_W}px solid ${DARK_BORDER}`,
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 26px rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "0 14px",
        overflow: "hidden",
      }}
    >
      <span style={{ fontSize: 13, fontWeight: 600, color: "white", fontVariantNumeric: "tabular-nums", fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace" }}>
        {formatClock(clock)}
      </span>
      <div style={{ display: "flex", alignItems: "center", gap: 2, height: 24, flex: 1, minWidth: 0 }}>
        {Array.from({ length: 24 }, (_, i) => {
          const hist = levels.slice(-24);
          const level = hist.length === 24 ? hist[i] : 0;
          return (
            <span
              key={i}
              style={{
                width: 3,
                flexShrink: 0,
                height: Math.max(24 * 0.14, level * 24),
                borderRadius: 1.5,
                background: rainbowColor(i / 23),
                transition: "height 80ms ease-out",
              }}
            />
          );
        })}
      </div>
      {interimText && (
        <span style={{ flex: 1, minWidth: 0, maxWidth: 130, fontSize: 12, color: "rgba(255,255,255,0.6)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {interimText}
        </span>
      )}
      {hovered && <CancelButton armed={cancelArmed} setArmed={setCancelArmed} />}
      <RainbowHairline steady />
    </div>
  );
}

// "Reading Well": wide panel, header (clock + meter + listening badge) over a
// live-preview well. The well shows streamed words once streaming ASR lands;
// until then it shows the listening placeholder.
//
// Content-sized vertically (like EW's `.readingWell`): the pill starts at one
// line of text and grows as words arrive, up to 5 lines. At the cap the text
// is bottom-pinned so the newest words stay visible and the oldest scroll off
// the top. The ResizeObserver in the parent reports the height change to Rust,
// which resizes the OS window.
const WELL_MAX_LINES = 5;
const WELL_LINE_HEIGHT = 14 * 1.4; // font-size * line-height

function ReadingWellPill({ clock, levels, hovered, cancelArmed, setCancelArmed, interimText }: RecordingStyleProps) {
  const text = interimText || "Listening…";
  // Estimate the number of rendered lines to set a max-height cap. The text
  // is 14px at 1.4 line-height in a 368px content width (400 - 2*16 padding).
  // We let the text flow naturally and cap with max-height + bottom alignment.
  const maxWellHeight = WELL_MAX_LINES * WELL_LINE_HEIGHT;

  return (
    <div
      style={{
        width: 400,
        borderRadius: 16,
        background: WELL_SURFACE,
        border: `${DARK_BORDER_W}px solid rgba(255,255,255,0.13)`,
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 26px rgba(0,0,0,0.45)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10, height: 34, padding: "0 16px", flexShrink: 0 }}>
        <span style={{ fontSize: 13, fontWeight: 600, color: "white", fontVariantNumeric: "tabular-nums", fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace" }}>
          {formatClock(clock)}
        </span>
        <RainbowMeter levels={levels} height={16} barWidth={2} />
        <span style={{ flex: 1 }} />
        <span style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.5, color: "rgba(255,255,255,0.88)", padding: "3px 9px", borderRadius: 999, background: "rgba(255,255,255,0.08)", display: "inline-flex", alignItems: "center", gap: 5 }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: "rgba(255,255,255,0.9)" }} />
          LISTENING
        </span>
      </div>
      <div style={{ borderTop: `${DARK_BORDER_W}px solid rgba(255,255,255,0.08)`, flexShrink: 0 }} />
      <div style={{ padding: "12px 16px 15px 16px", background: "rgba(0,0,0,0.28)", boxShadow: "inset 0 1px 3px rgba(0,0,0,0.3)" }}>
        <div style={{ maxHeight: maxWellHeight, overflow: "hidden", display: "flex", flexDirection: "column", justifyContent: "flex-end" }}>
          <p style={{ fontSize: 14, lineHeight: "1.4", color: "rgba(255,255,255,0.92)", margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
            {text}
          </p>
        </div>
        {hovered && (
          <div style={{ marginTop: 8, display: "flex", justifyContent: "flex-end" }}>
            <CancelButton armed={cancelArmed} setArmed={setCancelArmed} />
          </div>
        )}
      </div>
    </div>
  );
}

// --- Dot Matrix pill style --------------------------------------------------
//
// Ported from LiveKit's Agents UI (github.com/livekit/components-js,
// packages/shadcn), Apache-2.0 licensed. The original drives a dot grid from
// live agent state and multiband track volume; we keep the same sequence
// geometry (ring sweep, center pulse, row scan, volume rows) but drive it from
// Teletype's pill phases and the scalar `pill-level` stream instead, in inline
// styles (no Tailwind / livekit deps). Files modified from the original.

type DotGridState = "idle" | "connecting" | "listening" | "speaking" | "thinking";

interface Coordinate {
  x: number;
  y: number;
}

// LiveKit's AgentAudioVisualizerGrid (Apache-2.0) sequences, ported verbatim
// except for formatting: ring sweep for connecting, a center pulse with
// blank beats for listening, a left-right row scan for thinking.
function generateConnectingSequence(rows: number, columns: number, radius: number): Coordinate[] {
  const seq: Coordinate[] = [];
  const centerY = Math.floor(rows / 2);
  const topLeft = { x: Math.max(0, centerY - radius), y: Math.max(0, centerY - radius) };
  const bottomRight = {
    x: columns - 1 - topLeft.x,
    y: Math.min(rows - 1, centerY + radius),
  };
  for (let x = topLeft.x; x <= bottomRight.x; x++) seq.push({ x, y: topLeft.y });
  for (let y = topLeft.y + 1; y <= bottomRight.y; y++) seq.push({ x: bottomRight.x, y });
  for (let x = bottomRight.x - 1; x >= topLeft.x; x--) seq.push({ x, y: bottomRight.y });
  for (let y = bottomRight.y - 1; y > topLeft.y; y--) seq.push({ x: topLeft.x, y });
  return seq;
}

function generateListeningSequence(rows: number, columns: number): Coordinate[] {
  const center = { x: Math.floor(columns / 2), y: Math.floor(rows / 2) };
  const noIndex = { x: -1, y: -1 };
  return [center, noIndex, noIndex, noIndex, noIndex, noIndex, noIndex, noIndex, noIndex];
}

function generateThinkingSequence(rows: number, columns: number): Coordinate[] {
  const seq: Coordinate[] = [];
  const y = Math.floor(rows / 2);
  for (let x = 0; x < columns; x++) seq.push({ x, y });
  for (let x = columns - 1; x >= 0; x--) seq.push({ x, y });
  return seq;
}

function gridSequence(state: DotGridState, rows: number, columns: number, radius: number): Coordinate[] {
  const clamped = Math.min(radius, Math.floor(Math.max(rows, columns) / 2));
  if (state === "thinking") return generateThinkingSequence(rows, columns);
  if (state === "connecting") return generateConnectingSequence(rows, columns, clamped);
  if (state === "listening") return generateListeningSequence(rows, columns);
  return [{ x: Math.floor(columns / 2), y: Math.floor(rows / 2) }];
}

// A wide strip, not the demo's 15x15 square: it fits the 44px-tall capsule.
// 5 rows x 6px + 4 gaps x 3px = 42px, leaving a 1px margin top and bottom.
const DOT_COLS = 24;
const DOT_ROWS = 5;
const DOT_SIZE = 6;
const DOT_GAP = 3;
// The ring sweep / pulse / scan tick at 10 Hz, like the original default.
const DOT_INTERVAL = 100;

// The speaking state: a wave sweeps left-to-right across the grid, with the
// wave's amplitude driven by the audio level. Dots near the wave front light
// up with full rainbow color; dots behind the wave fade out. The level
// history (newest at the right) drives both the wave position and the row
// spread, so louder speech lights more rows and the wave moves faster.
function speakingActive(levels: number[], step: number): boolean[][] {
  const hist = levels.slice(-DOT_COLS);
  while (hist.length < DOT_COLS) hist.unshift(0);
  const currentLevel = recordingLevel(levels);
  const rowMid = Math.floor(DOT_ROWS / 2);

  // Wave position: sweeps across columns over time, speed proportional to level.
  const waveSpeed = 0.3 + currentLevel * 1.5;
  const waveX = (step * waveSpeed) % DOT_COLS;

  return Array.from({ length: DOT_ROWS }, (_, y) => {
    const rowDist = Math.abs(rowMid - y);
    // How many rows light up depends on the current volume.
    const maxRow = Math.floor(currentLevel * (rowMid + 1));
    const rowActive = rowDist <= maxRow;
    return Array.from({ length: DOT_COLS }, (_, x) => {
      if (!rowActive) return false;
      // Distance from the wave front (circular).
      const dist = Math.abs(x - waveX);
      const circularDist = Math.min(dist, DOT_COLS - dist);
      // Dots within 4 of the wave front are lit; intensity fades with distance.
      return circularDist <= 4;
    });
  });
}

function sequenceActive(state: DotGridState, step: number): boolean[][] {
  const seq = gridSequence(state, DOT_ROWS, DOT_COLS, DOT_COLS);
  const dot = seq[step % seq.length] ?? { x: -1, y: -1 };
  return Array.from({ length: DOT_ROWS }, (_, y) =>
    Array.from({ length: DOT_COLS }, (_, x) => x === dot.x && y === dot.y),
  );
}

function DotGridMatrix({ state, levels }: { state: DotGridState; levels: number[] }) {
  const [step, setStep] = useState(0);

  // All animated states (speaking, listening, connecting, thinking) run a
  // fixed-rate clock. Idle freezes on the center dot. The interval only
  // exists while a grid is mounted (active phases), so the hidden pill
  // window burns no CPU on it.
  useEffect(() => {
    if (state === "idle") return;
    const timer = setInterval(() => setStep((s) => s + 1), DOT_INTERVAL);
    return () => clearInterval(timer);
  }, [state]);

  const active = state === "speaking" ? speakingActive(levels, step) : sequenceActive(state, step);

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: `repeat(${DOT_COLS}, ${DOT_SIZE}px)`,
        gap: DOT_GAP,
        flexShrink: 0,
        pointerEvents: "none",
      }}
    >
      {active.map((row, y) =>
        row.map((on, x) => (
          <span
            key={`${x}-${y}`}
            style={{
              width: DOT_SIZE,
              height: DOT_SIZE,
              borderRadius: "50%",
              background: on ? rainbowColor(x / (DOT_COLS - 1)) : "rgba(255,255,255,0.12)",
              transition:
                state === "speaking"
                  ? "background-color 90ms ease-out"
                  : `background-color ${DOT_INTERVAL * 0.9}ms linear`,
            }}
          />
        )),
      )}
    </div>
  );
}

// Shared dark capsule chrome for the Dot Matrix style.
function DotGridCapsule({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        position: "relative",
        height: 44,
        borderRadius: 22,
        background: DARK_SURFACE,
        border: `${DARK_BORDER_W}px solid ${DARK_BORDER}`,
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 26px rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "0 14px",
        overflow: "hidden",
      }}
    >
      {children}
    </div>
  );
}

// Recording: dot grid header on top, interim text below (same vertical layout
// as ReadingWell). The dot grid animates with the audio level; the text grows
// as the user speaks, and the pill window resizes via the parent's
// ResizeObserver.
function DotGridRecordingPill({ clock, levels, hovered, cancelArmed, setCancelArmed, interimText }: RecordingStyleProps) {
  const speaking = recordingLevel(levels) > 0.02;
  const text = interimText || "Listening…";
  const maxWellHeight = WELL_MAX_LINES * WELL_LINE_HEIGHT;

  return (
    <div
      style={{
        width: 400,
        borderRadius: 16,
        background: WELL_SURFACE,
        border: `${DARK_BORDER_W}px solid rgba(255,255,255,0.13)`,
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 26px rgba(0,0,0,0.45)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      {/* Header: clock + dot grid + listening badge */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, height: 34, padding: "0 16px", flexShrink: 0 }}>
        <span style={{ fontSize: 13, fontWeight: 600, color: "white", fontVariantNumeric: "tabular-nums", fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace" }}>
          {formatClock(clock)}
        </span>
        <DotGridMatrix state={speaking ? "speaking" : "listening"} levels={levels} />
        <span style={{ flex: 1 }} />
        <span style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.5, color: "rgba(255,255,255,0.88)", padding: "3px 9px", borderRadius: 999, background: "rgba(255,255,255,0.08)", display: "inline-flex", alignItems: "center", gap: 5 }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: "rgba(255,255,255,0.9)" }} />
          LISTENING
        </span>
      </div>
      <div style={{ borderTop: `${DARK_BORDER_W}px solid rgba(255,255,255,0.08)`, flexShrink: 0 }} />
      {/* Well: interim text grows vertically, bottom-pinned at cap */}
      <div style={{ padding: "12px 16px 15px 16px", background: "rgba(0,0,0,0.28)", boxShadow: "inset 0 1px 3px rgba(0,0,0,0.3)" }}>
        <div style={{ maxHeight: maxWellHeight, overflow: "hidden", display: "flex", flexDirection: "column", justifyContent: "flex-end" }}>
          <p style={{ fontSize: 14, lineHeight: "1.4", color: "rgba(255,255,255,0.92)", margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
            {text}
          </p>
        </div>
        {hovered && (
          <div style={{ marginTop: 8, display: "flex", justifyContent: "flex-end" }}>
            <CancelButton armed={cancelArmed} setArmed={setCancelArmed} />
          </div>
        )}
      </div>
    </div>
  );
}

// Warming: ring sweep (the original's connecting state) + the status text.
function DotGridWarmingPill({ seconds }: { seconds: number }) {
  return (
    <DotGridCapsule>
      <DotGridMatrix state="connecting" levels={[]} />
      <div style={{ minWidth: 0, flex: 1, lineHeight: 1.25 }}>
        <p style={{ fontSize: 13, fontWeight: 500, color: "rgba(255,255,255,0.9)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          Getting model ready...
        </p>
        {seconds >= 3 && (
          <p style={{ fontSize: 12, color: "rgba(255,255,255,0.55)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
            {seconds}s · {seconds >= 25 ? "almost there, first load only" : "first load only"}
          </p>
        )}
      </div>
      <span style={{ fontSize: 12, fontWeight: 500, color: "rgba(255,255,255,0.4)" }}>esc</span>
    </DotGridCapsule>
  );
}

// Processing: row scan (the original's thinking state) over the status or
// streamed text.
function DotGridProcessingPill({ message, streamText, skipMessage }: { message: string; streamText: string; skipMessage: string | null }) {
  return (
    <DotGridCapsule>
      <DotGridMatrix state="thinking" levels={[]} />
      <div style={{ minWidth: 0, flex: 1, display: "flex", flexDirection: "column", gap: 2 }}>
        <p style={{ fontSize: 13, fontWeight: 500, color: streamText ? "#e6e9ef" : "rgba(255,255,255,0.9)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", margin: 0 }}>
          {streamText || message}
        </p>
        {skipMessage && (
          <p style={{ fontSize: 12, fontWeight: 500, color: "#f5c518", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", margin: 0 }}>
            {skipMessage}
          </p>
        )}
      </div>
    </DotGridCapsule>
  );
}

// --- Main component ---------------------------------------------------------

function Pill() {
  const [state, setState] = useState<PillPhase>({ phase: "idle" });
  const [settings, setSettings] = useState<Settings | null>(null);
  const [deviceName, setDeviceName] = useState("Default");
  const [hovered, setHovered] = useState(false);
  const [cancelArmed, setCancelArmed] = useState(false);
  // A transform skip/fallback message shown at dictation time (P1-16 T7b).
  const [skipMessage, setSkipMessage] = useState<string | null>(null);
  // Live streaming text from the LLM transform (T1.1). Updated as
  // tokens arrive; cleared when a new session starts.
  const [streamText, setStreamText] = useState<string>("");
  const [interimText, setInterimText] = useState<string>("");
  // The language Whisper auto-detected this session. Shown as a chip next to
  // the processing message; tapping it locks that language in settings.
  const [languageChip, setLanguageChip] = useState<{ code: string; detected: boolean } | null>(null);
  const skipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // The rendered pill element. We measure its natural size and report it to
  // Rust so the OS window can be resized to fit the active style (the "well"
  // is 120px tall, the capsules 44px); a fixed window would clip or letterbox.
  const pillRef = useRef<HTMLDivElement | null>(null);
  const [levels, setLevels] = useState<number[]>(() => new Array(WAVE_BARS).fill(0));
  const warmingSince = useRef(0);
  const recordingSince = useRef(0);
  const now = useNow(state.phase === "warming" ? 1000 : 500, state.phase === "recording" || state.phase === "warming");

  const refreshSettings = async () => {
    try {
      const next = await invoke<Settings>("get_settings");
      setSettings(next);
      const devices = await invoke<{ id: string; name: string }[]>("list_input_devices");
      const device = devices.find((d) => d.id === next.inputDevice);
      setDeviceName(device ? device.name.split(" ")[0] : "Default");
    } catch {
      // Settings not available yet.
    }
  };

  // Tap the detected-language chip: lock that language in settings so the
  // next dictation uses it instead of auto-detecting.
  const lockLanguage = async (code: string) => {
    try {
      const s = await invoke<Settings>("get_settings");
      await invoke("save_settings", { settings: { ...s, language: code } });
      setSettings({ ...s, language: code });
    } catch {
      // Non-fatal: the user can lock the language in Settings.
    }
  };

  useEffect(() => {
    refreshSettings();
  }, []);

  // Keep a ref to the latest state so event handlers read fresh values.
  const stateRef = useRef(state);
  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  useTauriEvent<{ message: string }>("pill-skip", ({ payload }) => {
    if (skipTimer.current) clearTimeout(skipTimer.current);
    setSkipMessage(payload.message);
    // Clear it once the pill leaves the processing phase, so a stale message
    // never lingers into a later dictation session.
    skipTimer.current = setTimeout(() => setSkipMessage(null), 6000);
  });

  useTauriEvent<{ code: string; detected: boolean }>("pill-language", ({ payload }) => {
    setLanguageChip(payload);
  });

  useTauriEvent<PillPhase>("pill-state", ({ payload }) => {
    // A new session begins: drop any leftover skip message.
    if (payload.phase === "recording") {
      setSkipMessage(null);
      setLanguageChip(null);
      setStreamText("");
      setInterimText("");
    }
    if (payload.phase === "warming" && stateRef.current.phase !== "warming") {
      warmingSince.current = Date.now();
    }
    if (payload.phase === "recording" && stateRef.current.phase !== "recording") {
      setLevels(new Array(WAVE_BARS).fill(0));
      // Fallback start time in case startedAtMs is missing from the event.
      if (typeof payload.startedAtMs !== "number") recordingSince.current = Date.now();
    }
    if (payload.phase !== "recording") setHovered(false);
    // Disarm the cancel button whenever the pill leaves the recording state.
    if (payload.phase !== "recording") setCancelArmed(false);
    setState(payload);
  });
  useTauriEvent<number>("pill-level", ({ payload }) => {
    setLevels((l) => [...l.slice(1), payload]);
  });

  useTauriEvent<void>("settings-changed", () => {
    refreshSettings();
  });
  useTauriEvent<string>("transform-token", ({ payload }) => {
    // The backend emits the accumulated token text as the LLM streams.
    setStreamText(payload);
  });

  useTauriEvent<string>("interim-transcript", ({ payload }) => {
    // The interim ASR loop emits partial transcripts while recording.
    setInterimText(payload);
  });

  // Clear any pending skip-message timer on unmount.
  useEffect(() => {
    return () => {
      if (skipTimer.current) clearTimeout(skipTimer.current);
    };
  }, []);

  const expanded = state.phase === "recording" && hovered;
  // Ported styles (classic/levelRail/well) render their own fixed-size chrome
  // and ignore the legacy size/expand logic.
  const pillStyle = settings?.pillStyle ?? "default";
  // Dot Matrix replaces the chrome for every active phase (recording rows,
  // warming ring sweep, processing scan); idle keeps the small Teletype bars.
  const dotGrid = pillStyle === "dotGrid" && state.phase !== "idle";
  const usesPortedStyle =
    dotGrid || (state.phase === "recording" && pillStyle !== "default");
  const size = usesPortedStyle
    ? { width: 0, height: 0 }
    : expanded
      ? SIZES.recordingExpanded
      : state.phase === "processing"
        ? { width: Math.min(PROCESSING_MAX_WIDTH, 220 + (skipMessage ? 200 : 0)), height: 44 }
        : SIZES[state.phase];
  const active = state.phase !== "idle";
  const startedAtMs =
    state.phase === "recording" && typeof state.startedAtMs === "number"
      ? state.startedAtMs
      : recordingSince.current;
  const clock = state.phase === "recording" ? (Date.now() - startedAtMs) / 1000 : 0;
  const styleProps: RecordingStyleProps = {
    levels,
    level: levels.length > 0 ? levels[levels.length - 1] : 0,
    clock,
    startedAtMs,
    settings: settings ?? { recordingMode: "hold", language: "en", inputDevice: "", pillStyle: "default" },
    deviceName,
    hovered,
    cancelArmed,
    setCancelArmed,
    interimText,
  };

  // Report the pill's natural size to Rust whenever it changes, so the OS
  // window resizes to fit the active style. The "well" is 120px tall and the
  // capsules 44px, so a fixed window would clip or letterbox them. ResizeObserver
  // only fires on real size changes, so this is cheap.
  useEffect(() => {
    const node = pillRef.current;
    if (!node) return;
    const report = () => {
      const r = node.getBoundingClientRect();
      const w = Math.ceil(r.width);
      const h = Math.ceil(r.height);
      if (w > 0 && h > 0) {
        invoke("set_pill_size", { width: w, height: h }).catch(() => {});
      }
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(node);
    return () => observer.disconnect();
  }, [state.phase, pillStyle, hovered, expanded, skipMessage, streamText, interimText]);

  return (
    <div style={{ display: "flex", width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
      {usesPortedStyle ? (
        <div ref={pillRef} onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)} style={{ animation: "fade-in 0.18s ease-out" }}>
          {dotGrid && state.phase === "recording" && <DotGridRecordingPill {...styleProps} />}
          {dotGrid && state.phase === "warming" && (
            <DotGridWarmingPill seconds={Math.floor((now - warmingSince.current) / 1000)} />
          )}
          {dotGrid && state.phase === "processing" && (
            <DotGridProcessingPill message={state.message} streamText={streamText} skipMessage={skipMessage} />
          )}
          {!dotGrid && pillStyle === "classic" && <ClassicPill {...styleProps} />}
          {!dotGrid && pillStyle === "levelRail" && <LevelRailPill {...styleProps} />}
          {!dotGrid && pillStyle === "well" && <ReadingWellPill {...styleProps} />}
        </div>
      ) : (
      <div
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        style={{
          width: size.width,
          height: size.height,
          borderRadius: size.height / 2,
          display: "flex",
          alignItems: "center",
          overflow: "hidden",
          color: "white",
          position: "relative",
          background: active ? "#0d0d0d" : "rgba(13,13,13,0.85)",
          backdropFilter: active ? undefined : "blur(24px)",
          boxShadow: "0 1px 2px rgba(0,0,0,0.04), 0 10px 30px rgba(0,0,0,0.1)",
          border: `${DARK_BORDER_W}px solid rgba(255,255,255,0.1)`,
          transition: "width 450ms cubic-bezier(0.2,0.9,0.3,1), height 450ms cubic-bezier(0.2,0.9,0.3,1), border-radius 450ms cubic-bezier(0.2,0.9,0.3,1)",
        }}
      >
        {state.phase === "idle" && <IdleBars />}
        {state.phase === "warming" && (
          <Warming seconds={Math.floor((now - warmingSince.current) / 1000)} />
        )}
        {state.phase === "processing" && (
          <div
            key={`${state.message}|${skipMessage ?? ""}`}
            style={{
              width: "100%",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 10,
              padding: "0 16px",
              animation: "fade-in 0.18s ease-out",
            }}
          >
            {streamText ? (
              <p
                style={{
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  textAlign: "center",
                  fontSize: 13,
                  fontWeight: 500,
                  margin: 0,
                  color: "#e6e9ef",
                }}
              >
                {streamText}
              </p>
            ) : (
              <p
                style={{
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  textAlign: "center",
                  fontSize: 13,
                  fontWeight: 500,
                  margin: 0,
                }}
              >
                {state.message}
              </p>
            )}
            {skipMessage && (
              <p
                style={{
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  textAlign: "center",
                  fontSize: 12,
                  fontWeight: 500,
                  margin: 0,
                  color: "#f5c518",
                }}
              >
                {skipMessage}
              </p>
            )}
          </div>
        )}
        {state.phase === "recording" && (
          <div style={{ display: "flex", width: "100%", animation: "fade-in 0.18s ease-out", alignItems: "center", gap: 10, padding: "0 14px" }}>
            <RecordingDot />
            <Waveform levels={levels} />
            <span style={{
              flexShrink: 0,
              fontSize: 12,
              fontWeight: 500,
              color: "rgba(255,255,255,0.6)",
              fontVariantNumeric: "tabular-nums",
            }}>
              {state.phase === "recording" && formatClock(clock)}
            </span>
            {expanded && settings && (
              <div style={{ display: "flex", flexShrink: 0, animation: "pop-in 0.2s ease-out", alignItems: "center", gap: 6, paddingLeft: 4 }}>
                <Chip icon={<MicIcon />} label={deviceName} />
                <Chip
                  icon={settings.recordingMode === "hold" ? <HandIcon /> : <RepeatIcon />}
                  label={settings.recordingMode === "hold" ? "Hold" : "Toggle"}
                />
                <Chip
                  icon={<GlobeIcon />}
                  label={
                    languageChip
                      ? `${languageChip.code.toUpperCase()}${languageChip.detected ? " · detected" : ""}`
                      : settings.language === "auto" || settings.language === "" ? "Auto" : settings.language.toUpperCase()
                  }
                  onClick={languageChip ? () => lockLanguage(languageChip.code) : undefined}
                />
                <CancelButton armed={cancelArmed} setArmed={setCancelArmed} />
              </div>
            )}
          </div>
        )}
      </div>
      )}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Pill />
  </StrictMode>,
);
