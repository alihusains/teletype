import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNow } from "./lib/useNow";
import {
  isBusyPhase,
  pillMessage,
  usePillMachine,
  type PillInputDevice,
  type PillSettings,
} from "./pillMachine";

import { SIZES } from "./lib/rainbow";

const WAVE_HEIGHT = 30;

// Max width the processing pill will grow to before clipping (EW uses
// content-sized width; we cap to keep the pill on-screen).
const PROCESSING_MAX_WIDTH = 480;

// Settings/device shapes live with the pill state machine (ui/src/pillMachine.ts).
type Settings = PillSettings;
type InputDevice = PillInputDevice;

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

import { RAINBOW, rainbowColor } from "./lib/rainbow";

// Dark translucent capsule surface shared by the ported designs.
const DARK_SURFACE = "rgba(20,20,28,0.82)";
const DARK_BORDER = "rgba(255,255,255,0.1)";
const DARK_BORDER_W = 0.5; // EW uses 0.5pt border, not 1px
const WELL_SURFACE = "rgba(17,15,24,0.90)"; // EW reading well is more opaque

export { rainbowColor };

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

// Vertical bars coloured across the rainbow spectrum. Each bar takes its own
// sample from the level history, so the capsule shows a moving waveform
// instead of N copies of one averaged level. A center-weighted envelope keeps
// the classic "lips" silhouette when the signal is steady. The bar row fills
// its container, so the capsule has no dead space between the waveform and the
// clock.
export function RainbowLips({ levels, bars = 18 }: { levels: number[]; bars?: number }) {
  const hist = levels.slice(-bars);
  while (hist.length < bars) hist.unshift(0);
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 1.5, width: "100%", height: 30 }}>
      {hist.map((level, i) => {
        const centerWeight = 1 - Math.abs(i - (bars - 1) / 2) / ((bars - 1) / 2) * 0.35;
        const sample = Math.max(0, Math.min(1, level));
        const height = 3 + (26 - 3) * centerWeight * (0.12 + 0.88 * sample);
        return (
          <span
            key={i}
            style={{
              width: 2.5,
              flexShrink: 0,
              height,
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
  /// P2.8: true when this take is a Command take — styles render a
  /// distinct badge/ring so "Transcribe my words" and "Run what I say"
  /// are never ambiguous.
  command: boolean;
  settings: Settings;
  deviceName: string;
  hovered: boolean;
  cancelArmed: boolean;
  setCancelArmed: (v: boolean) => void;
  interimText: string;
  devices: InputDevice[];
  onSelectDevice: (id: string, name: string) => void;
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

// Compact device chip: shows the active mic name; clicking cycles to the next
// available input device. Simpler than a dropdown for a small overlay window
// where precise clicking is hard. Long-press (500ms) opens the full list.
function DevicePicker({
  deviceName,
  devices,
  onSelectDevice,
}: {
  deviceName: string;
  devices: InputDevice[];
  onSelectDevice: (id: string, name: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const pressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cycleDevice = () => {
    if (devices.length === 0) return;
    const currentIdx = devices.findIndex((d) => d.name.split(" ")[0] === deviceName);
    const next = devices[(currentIdx + 1) % devices.length];
    onSelectDevice(next.id, next.name.split(" ")[0]);
  };

  const onPointerDown = () => {
    pressTimer.current = setTimeout(() => setOpen(true), 500);
  };
  const onPointerUp = () => {
    if (pressTimer.current) {
      clearTimeout(pressTimer.current);
      pressTimer.current = null;
    }
  };
  const onClick = () => {
    // Only cycle if it was a quick tap (not a long-press that opened the list).
    if (!open) cycleDevice();
  };

  return (
    <div style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
      <button
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
        onPointerLeave={onPointerUp}
        onClick={onClick}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 4,
          border: "none",
          cursor: "pointer",
          borderRadius: 999,
          padding: "3px 9px",
          fontSize: 11,
          fontWeight: 600,
          letterSpacing: 0.5,
          fontFamily: "inherit",
          color: "rgba(255,255,255,0.88)",
          background: "rgba(255,255,255,0.08)",
          transition: "background 120ms ease",
          maxWidth: 130,
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.14)")}
        onMouseLeave={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.08)")}
      >
        <MicIcon size={11} />
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {deviceName || "Select mic"}
        </span>
      </button>
      {open && (
        <DeviceMenu
          deviceName={deviceName}
          devices={devices}
          onSelectDevice={(id, name) => {
            onSelectDevice(id, name);
            setOpen(false);
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
}

// The input-device list behind a mic chip: the click-outside shield plus the
// panel itself. Extracted from DevicePicker so the recording chips (which are
// plain `Chip` buttons, not a press-and-hold capsule) open exactly the same
// overlay the long-press does, instead of inventing a second one.
function DeviceMenu({
  deviceName,
  devices,
  onSelectDevice,
  onClose,
}: {
  deviceName: string;
  devices: InputDevice[];
  onSelectDevice: (id: string, name: string) => void;
  onClose: () => void;
}) {
  return (
    <>
      <div style={{ position: "fixed", inset: 0, zIndex: 998 }} onClick={onClose} />
      <div
        style={{
          position: "absolute",
          bottom: "calc(100% + 4px)",
          right: 0,
          zIndex: 999,
          minWidth: 160,
          maxWidth: 220,
          maxHeight: 140,
          overflowY: "auto",
          background: "rgba(24,24,32,0.96)",
          border: "0.5px solid rgba(255,255,255,0.12)",
          borderRadius: 8,
          boxShadow: "0 8px 24px rgba(0,0,0,0.5)",
          padding: "4px 0",
          animation: "fade-in 100ms ease-out",
        }}
      >
        {devices.map((d) => (
          <button
            key={d.id}
            onClick={(e) => {
              e.stopPropagation();
              onSelectDevice(d.id, d.name.split(" ")[0]);
            }}
            style={{
              display: "block",
              width: "100%",
              border: "none",
              cursor: "pointer",
              padding: "5px 10px",
              fontSize: 11,
              fontFamily: "inherit",
              textAlign: "left",
              color: d.name.split(" ")[0] === deviceName ? "white" : "rgba(255,255,255,0.7)",
              background: d.name.split(" ")[0] === deviceName ? "rgba(255,255,255,0.08)" : "transparent",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
            onMouseEnter={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.08)")}
            onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
          >
            {d.name}
            {d.is_default && <span style={{ marginLeft: 6, fontSize: 9, color: "rgba(255,255,255,0.4)" }}>default</span>}
          </button>
        ))}
      </div>
    </>
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
        height: 52,
        borderRadius: 22,
        background: DARK_SURFACE,
        border: `${DARK_BORDER_W}px solid ${DARK_BORDER}`,
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 26px rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "0 12px 0 14px",
        overflow: "hidden",
      }}
    >
      <div style={{ flex: 1, minWidth: 0, height: 30, display: "flex", alignItems: "center" }}>
        <RainbowLips levels={levels} bars={26} />
      </div>
      {!hovered && (
        <span style={{ flexShrink: 0, fontSize: 13, fontWeight: 600, color: "white", fontVariantNumeric: "tabular-nums", fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace" }}>
          {formatClock(clock)}
        </span>
      )}
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
        height: 52,
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
      <div style={{ display: "flex", alignItems: "center", gap: 2, height: 28, flex: 1, minWidth: 0 }}>
        {Array.from({ length: 24 }, (_, i) => {
          const hist = levels.slice(-24);
          const level = hist[i + Math.max(0, hist.length - 24)] ?? 0;
          return (
            <span
              key={i}
              style={{
                width: 3,
                flexShrink: 0,
                height: Math.max(28 * 0.14, level * 28),
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

function ReadingWellPill({ clock, levels, hovered, cancelArmed, setCancelArmed, interimText, deviceName, devices, onSelectDevice, command }: RecordingStyleProps) {
  const text = interimText || (command ? "Speak your command…" : "Listening…");
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
      <div style={{ display: "flex", alignItems: "center", gap: 10, height: 40, padding: "0 16px", flexShrink: 0 }}>
        <span style={{ fontSize: 13, fontWeight: 600, color: "white", fontVariantNumeric: "tabular-nums", fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace" }}>
          {formatClock(clock)}
        </span>
        <RainbowMeter levels={levels} height={24} barWidth={2} />
        <span style={{ flex: 1 }} />
        <DevicePicker deviceName={deviceName} devices={devices} onSelectDevice={onSelectDevice} />
        <span style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.5, color: command ? "#f5c518" : "rgba(255,255,255,0.88)", padding: "3px 9px", borderRadius: 999, background: command ? "rgba(245,197,24,0.16)" : "rgba(255,255,255,0.08)", display: "inline-flex", alignItems: "center", gap: 5 }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: command ? "#f5c518" : "rgba(255,255,255,0.9)" }} />
          {command ? "COMMAND" : "LISTENING"}
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

// Dot matrix grid: 25×7, single accent color, 3px dots, 2px gaps.
const DOT_COLS = 25;
const DOT_ROWS = 7;
const DOT_SIZE = 3;
const DOT_GAP = 2;
// Single accent color (LiveKit uses #1FD5F9).
const DOT_COLOR = "#1FD5F9";

// The speaking state: audio-reactive dot-matrix waveform. The grid is
// stationary; individual dots change opacity based on per-column amplitude.
//
// Each column gets one amplitude value (from the level history). For each
// column, the center row activates first; higher amplitude activates more
// rows above and below. This creates vertical expansion/contraction.
function speakingIntensity(levels: number[], _step: number): number[][] {
  // 2D center-bright falloff: the grid glows from the center outward,
  // with the audio level controlling the radius of the bright area.
  // Matches the reference: a soft rectangular glow, bright center,
  // fading to dim at the edges.
  const hist = levels.slice(-DOT_COLS);
  while (hist.length < DOT_COLS) hist.unshift(0);
  const currentLevel = hist[hist.length - 1] ?? 0;
  const colMid = (DOT_COLS - 1) / 2;
  const rowMid = (DOT_ROWS - 1) / 2;
  const maxColDist = colMid;
  const maxRowDist = rowMid;

  // The "radius" of the bright zone scales with the audio level.
  // Higher base + multiplier so the glow covers the full 25×7 grid,
  // including the top and bottom rows, at moderate-to-loud speech.
  const radius = 0.45 + currentLevel * 1.1;

  return Array.from({ length: DOT_ROWS }, (_, y) => {
    const rowNorm = Math.abs(y - rowMid) / maxRowDist; // 0 at center, 1 at edge
    return Array.from({ length: DOT_COLS }, (_, x) => {
      const colNorm = Math.abs(x - colMid) / maxColDist; // 0 at center, 1 at edge
      // Circular-ish distance from center. With a 25×7 grid the row axis
      // is short, so no vertical compression — the glow reaches the top
      // and bottom rows at moderate speech levels.
      const dist = Math.sqrt(colNorm * colNorm + rowNorm * rowNorm);
      // Falloff: bright inside the radius, dim outside.
      if (dist > radius) {
        // Outside the glow: very dim (still slightly visible)
        return 0.15;
      }
      // Inside: full brightness with a soft edge
      const edge = radius * 0.8;
      if (dist < edge) return 1;
      // Soft falloff in the edge band
      return 1 - ((dist - edge) / (radius - edge)) * 0.85;
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
  // Use requestAnimationFrame for 60fps smooth animation. We keep a
  // smoothed copy of the levels that interpolates toward the target,
  // so the grid responds in real-time to voice changes.
  const [smoothed, setSmoothed] = useState<number[]>(levels);
  const levelsRef = useRef(levels);
  levelsRef.current = levels;

  useEffect(() => {
    if (state === "idle") {
      // Fade out: lerp toward zero so the glow shrinks smoothly.
      let raf: number;
      const tick = () => {
        setSmoothed((prev) => {
          const next = prev.map((v) => v * 0.8);
          if (next.every((v) => v < 0.005)) return next.map(() => 0);
          raf = requestAnimationFrame(tick);
          return next;
        });
      };
      raf = requestAnimationFrame(tick);
      return () => cancelAnimationFrame(raf);
    }
    let raf: number;
    const tick = () => {
      // Exponential smoothing: lerp toward the latest audio levels.
      // Factor 0.4 per frame at 60fps ≈ ~55ms time constant — real-time feel.
      setSmoothed((prev) => {
        const target = levelsRef.current;
        return prev.map((v, i) => v + (target[i] - v) * 0.4);
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [state]);

  // For speaking: per-dot intensity from smoothed levels. For idle: all dim.
  const intensity = state === "speaking" ? speakingIntensity(smoothed, 0) : null;
  const boolActive = intensity ? null : state === "idle" ? null : sequenceActive(state, 0);

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
      {Array.from({ length: DOT_ROWS }, (_, y) =>
        Array.from({ length: DOT_COLS }, (_, x) => {
          // Speaking: fractional opacity (0..1) for the 2D glow falloff.
          // Idle: all dim. Other states: boolean sequence.
          const opacity = intensity
            ? intensity[y][x]
            : boolActive
              ? boolActive[y][x] ? 1 : 0.1
              : 0.1;
          return (
            <span
              key={`${x}-${y}`}
              style={{
                width: DOT_SIZE,
                height: DOT_SIZE,
                borderRadius: "50%",
                background: DOT_COLOR,
                opacity,
              }}
            />
          );
        }),
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
function DotGridRecordingPill({ clock, levels, hovered, cancelArmed, setCancelArmed, interimText, deviceName, devices, onSelectDevice, command }: RecordingStyleProps) {
  // Use a smoothed level with a small hysteresis to avoid the grid
  // blinking on/off at the speech boundary.
  const rawLevel = recordingLevel(levels);
  const [speaking, setSpeaking] = useState(false);
  useEffect(() => {
    if (rawLevel > 0.04) setSpeaking(true);
    else if (rawLevel < 0.01) setSpeaking(false);
    // Between 0.01 and 0.04: keep the current state (hysteresis band).
  }, [rawLevel]);
  const text = interimText || (command ? "Speak your command…" : "Listening…");
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
      {/* Header: clock + dot grid + device picker + listening badge. */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 16px", flexShrink: 0 }}>
        <span style={{ fontSize: 13, fontWeight: 600, color: "white", fontVariantNumeric: "tabular-nums", fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace", flexShrink: 0 }}>
          {formatClock(clock)}
        </span>
        <DotGridMatrix state={speaking ? "speaking" : "idle"} levels={levels} />
        <span style={{ flex: 1 }} />
        <DevicePicker deviceName={deviceName} devices={devices} onSelectDevice={onSelectDevice} />
        <span style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.5, color: command ? "#f5c518" : "rgba(255,255,255,0.88)", padding: "3px 9px", borderRadius: 999, background: command ? "rgba(245,197,24,0.16)" : "rgba(255,255,255,0.08)", display: "inline-flex", alignItems: "center", gap: 5, flexShrink: 0 }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: command ? "#f5c518" : "rgba(255,255,255,0.9)" }} />
          {command ? "COMMAND" : "LISTENING"}
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

// Processing: row scan (the original's thinking state) over the status
// message. The streamed transform text is intentionally NOT shown here —
// the user already saw the interim text during recording, so re-showing it
// after transform adds latency without value. The text goes straight to
// the target app.
function DotGridProcessingPill({ message, skipMessage }: { message: string; skipMessage: string | null }) {
  return (
    <DotGridCapsule>
      <DotGridMatrix state="thinking" levels={[]} />
      <div style={{ minWidth: 0, flex: 1, display: "flex", flexDirection: "column", gap: 2 }}>
        <p style={{ fontSize: 13, fontWeight: 500, color: "rgba(255,255,255,0.9)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", margin: 0 }}>
          {message}
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

// Legacy default-chrome processing view: status message plus the optional
// transform skip/fallback notice. Extracted verbatim from Pill; every busy
// machine phase (processing/transcribing/streaming/done/error) renders
// through it with identical pixels.
function LegacyProcessingBody({ message, skipMessage }: { message: string; skipMessage: string | null }) {
  return (
    <div
      key={`${message}|${skipMessage ?? ""}`}
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
        {message}
      </p>
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
  );
}

export function Pill() {
  // All session/phase/timer state lives in the explicit machine
  // (ui/src/pillMachine.ts); this component derives layout and renders.
  const { machine, dispatch, selectDevice, lockLanguage, toggleRecordingMode } = usePillMachine();
  // The input-device list opened from the recording mic chip. The chip is a
  // plain button (not the press-and-hold capsule), so it needs its own open
  // flag; the overlay itself is shared with DevicePicker.
  const [micMenuOpen, setMicMenuOpen] = useState(false);
  // The rendered pill element. We measure its natural size and report it to
  // Rust so the OS window can be resized to fit the active style (the "well"
  // is 120px tall, the capsules 52px); a fixed window would clip or letterbox.
  const pillRef = useRef<HTMLDivElement | null>(null);
  const phase = machine.pill.phase;
  const busy = isBusyPhase(phase);
  const message = pillMessage(machine.pill);
  const now = useNow(phase === "warming" ? 1000 : 500, phase === "recording" || phase === "warming");
  const setHovered = (hovered: boolean) => dispatch({ type: "hover", hovered });
  const setCancelArmed = (armed: boolean) => dispatch({ type: "cancel-armed", armed });
  const settings = machine.settings;
  const deviceName = machine.deviceName;
  const hovered = machine.hovered;
  const languageChip = machine.languageChip;
  const cancelArmed = machine.cancelArmed;

  const expanded = phase === "recording" && machine.hovered;
  // Ported styles (classic/levelRail/well) render their own fixed-size chrome
  // and ignore the legacy size/expand logic.
  const pillStyle = settings?.pillStyle ?? "classic";
  // Dot Matrix replaces the chrome for every active phase (recording rows,
  // warming ring sweep, processing scan); idle keeps the small Teletype bars.
  const dotGrid = pillStyle === "dotGrid" && phase !== "idle";
  const usesPortedStyle =
    dotGrid || (phase === "recording" && pillStyle !== "default");
  const size = usesPortedStyle
    ? { width: 0, height: 0 }
    : expanded
      ? SIZES.recordingExpanded
      : busy
        ? {
            width: Math.min(
              PROCESSING_MAX_WIDTH,
              machine.skipMessage ? SIZES.processingSkip.width : SIZES.processing.width,
            ),
            height: SIZES.processing.height,
          }
        : phase === "recording"
          ? SIZES.recording
          : phase === "warming"
            ? SIZES.warming
            : SIZES.idle;
  const active = phase !== "idle";
  // The ported capsules sit on a translucent surface; the busy pill (which
  // renders through the legacy chrome for every style) must reuse it, or the
  // background visibly darkens the instant recording ends. The legacy
  // default style keeps its opaque surface.
  const activeSurface = pillStyle === "default" ? "#0d0d0d" : DARK_SURFACE;
  const startedAtMs =
    machine.pill.phase === "recording" && typeof machine.pill.startedAtMs === "number"
      ? machine.pill.startedAtMs
      : machine.recordingFallbackMs;
  const clock = machine.pill.phase === "recording" ? (Date.now() - startedAtMs) / 1000 : 0;
  const levels = machine.levels;
  // P2.8: a Command take wears an amber ring in every pill style, so the
  // state is unambiguous even when the take starts before any word lands.
  const commandRing =
    machine.pill.phase === "recording" && machine.pill.command
      ? "0 0 0 2px rgba(245,197,24,0.55)"
      : undefined;
  const styleProps: RecordingStyleProps = {
    levels,
    level: levels.length > 0 ? levels[levels.length - 1] : 0,
    clock,
    startedAtMs,
    // P2.8: only a recording take can be a command take.
    command: machine.pill.phase === "recording" && !!machine.pill.command,
    settings: settings ?? { recordingMode: "hold", language: "en", inputDevice: "", pillStyle: "classic" },
    deviceName,
    hovered,
    cancelArmed,
    setCancelArmed,
    interimText: machine.interimText,
    devices: machine.devices,
    onSelectDevice: selectDevice,
  };

  // Report the pill's natural size to Rust whenever it changes, so the OS
  // window resizes to fit the active style. The "well" is 120px tall and the
  // capsules 52px, so a fixed window would clip or letterbox them. ResizeObserver
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
  }, [phase, pillStyle, machine.hovered, expanded, machine.skipMessage, machine.streamText, machine.interimText]);

  // The mic overlay only exists while the chips are expanded, so collapse it
  // with them: a later recording must not resume with a stale menu open.
  useEffect(() => {
    if (!expanded || !deviceName) setMicMenuOpen(false);
  }, [expanded, deviceName]);

  return (
    <div style={{ display: "flex", width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
      {usesPortedStyle ? (
        <div ref={pillRef} onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)} style={{ animation: "fade-in 0.18s ease-out", borderRadius: 24, boxShadow: commandRing }}>
          {dotGrid && phase === "recording" && <DotGridRecordingPill {...styleProps} />}
          {dotGrid && phase === "warming" && (
            <DotGridWarmingPill seconds={Math.floor((now - machine.warmingSinceMs) / 1000)} />
          )}
          {dotGrid && busy && (
            <DotGridProcessingPill message={message} skipMessage={machine.skipMessage} />
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
          background: active ? activeSurface : "rgba(13,13,13,0.85)",
          backdropFilter: active ? undefined : "blur(24px)",
          boxShadow: [
            commandRing,
            "0 1px 2px rgba(0,0,0,0.04), 0 10px 30px rgba(0,0,0,0.1)",
          ]
            .filter(Boolean)
            .join(", "),
          border: `${DARK_BORDER_W}px solid rgba(255,255,255,0.1)`,
          transition: "width 450ms cubic-bezier(0.2,0.9,0.3,1), height 450ms cubic-bezier(0.2,0.9,0.3,1), border-radius 450ms cubic-bezier(0.2,0.9,0.3,1)",
        }}
      >
        {phase === "idle" && <IdleBars />}
        {phase === "warming" && (
          <Warming seconds={Math.floor((now - machine.warmingSinceMs) / 1000)} />
        )}
        {busy && (
          <LegacyProcessingBody message={message} skipMessage={machine.skipMessage} />
        )}
        {phase === "recording" && (
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
              {formatClock(clock)}
            </span>
            {expanded && settings && (
              <div style={{ display: "flex", flexShrink: 0, animation: "pop-in 0.2s ease-out", alignItems: "center", gap: 6, paddingLeft: 4 }}>
                {deviceName && (
                  <span style={{ position: "relative", display: "inline-flex" }}>
                    <Chip icon={<MicIcon />} label={deviceName} onClick={() => setMicMenuOpen(true)} />
                    {micMenuOpen && (
                      <DeviceMenu
                        deviceName={deviceName}
                        devices={machine.devices}
                        onSelectDevice={(id, name) => {
                          selectDevice(id, name);
                          setMicMenuOpen(false);
                        }}
                        onClose={() => setMicMenuOpen(false)}
                      />
                    )}
                  </span>
                )}
                <Chip
                  icon={settings.recordingMode === "hold" ? <HandIcon /> : <RepeatIcon />}
                  label={settings.recordingMode === "hold" ? "Hold" : "Toggle"}
                  onClick={toggleRecordingMode}
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
