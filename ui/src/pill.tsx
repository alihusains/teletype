import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNow } from "./lib/useNow";
import { isBusyPhase, pillMessage, usePillMachine, errorHasSettingsAction, type PillInputDevice, type PillSettings } from "./pillMachine";

import { SIZES } from "./lib/rainbow";
import { changeCount, diffWords, formatPolishMs } from "./lib/wordDiff";
import {
  busyPillSize,
  interimTail,
  planLegacyReport,
  PROCESSING_MAX_WIDTH,
  SHRINK_REPORT_FALLBACK_MS,
  DEVICE_LIST_GAP,
  DEVICE_ROW_HEIGHT,
  deviceListHeight,
  pillAnchor,
} from "./lib/pillLayout";
import {
  DOT_COLS,
  DOT_ROWS,
  DOT_STEP_MS,
  sequenceActive,
  sequenceAnimates,
  speakingIntensity,
  staticFrame,
  type DotGridState,
} from "./lib/dotGrid";

const WAVE_HEIGHT = 30;

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

// The OS window is sized to the pill's border box (overlay.rs resize_to), so
// anything painted outside it is clipped flat. Every surface shadow is
// therefore inset, and the Command ring is drawn inside each style's own root
// so it follows that element's radius.
const TOP_HIGHLIGHT = "inset 0 1px 0 rgba(255,255,255,0.08)";
const COMMAND_RING = "inset 0 0 0 2px rgba(245,197,24,0.55)";
const COMMAND_AMBER = "#f5c518";

function surfaceShadow(command: boolean): string {
  return command ? `${TOP_HIGHLIGHT}, ${COMMAND_RING}` : TOP_HIGHLIGHT;
}

// P2.8: the compact capsules have no LISTENING/COMMAND badge, so a Command
// take also gets a small amber label beside the clock. The inset ring alone
// is too quiet to tell the two take kinds apart at a glance.
function CommandCue() {
  return (
    <span
      title="Command take"
      style={{
        flexShrink: 0,
        fontSize: 9,
        fontWeight: 700,
        letterSpacing: 0.5,
        lineHeight: "14px",
        color: COMMAND_AMBER,
        background: "rgba(245,197,24,0.16)",
        borderRadius: 999,
        padding: "0 5px",
      }}
    >
      CMD
    </span>
  );
}

export { rainbowColor };

// The OS reduced-motion preference. The pill is a separate webview, so the
// app's index.html media rule does not reach it; the JS loop here (the dot
// grid) needs its own hook. Reads the live preference and reacts to changes.
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() =>
    typeof window !== "undefined" && "matchMedia" in window
      ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
      : false,
  );
  useEffect(() => {
    if (typeof window === "undefined" || !("matchMedia" in window)) return;
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = (e: MediaQueryListEvent) => setReduced(e.matches);
    setReduced(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return reduced;
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

function warmingStatus(progress: number | null): string {
  // T10c: show the download fraction while it is known; the backend
  // clamps it to 0..1, so the pill never claims more than it downloaded.
  if (progress === null) return "Getting model ready…";
  return `Downloading model ${Math.round(progress * 100)}%`;
}

function Warming({ seconds, progress }: { seconds: number; progress: number | null }) {
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
          {warmingStatus(progress)}
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
      // The expanded capsule's chips must never crush the meter to a sliver.
      minWidth: 72,
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

function Chip({ icon, label, badge, onClick }: { icon: React.ReactNode; label: string; badge?: React.ReactNode; onClick?: () => void }) {
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
      {badge}
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
// where precise clicking is hard. Long-press (500ms) opens the full list,
// which the parent renders inline (see DeviceList), so `open` is lifted.
function DevicePicker({
  deviceName,
  devices,
  onSelectDevice,
  open,
  onOpenChange,
  anchorRef,
}: {
  deviceName: string;
  devices: InputDevice[];
  onSelectDevice: (id: string, name: string) => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  anchorRef: React.RefObject<HTMLDivElement | null>;
}) {
  const pressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // The click that ends a long-press must not also cycle or close the list.
  const longPressed = useRef(false);

  const cycleDevice = () => {
    if (devices.length === 0) return;
    const currentIdx = devices.findIndex((d) => d.name.split(" ")[0] === deviceName);
    const next = devices[(currentIdx + 1) % devices.length];
    onSelectDevice(next.id, next.name.split(" ")[0]);
  };

  const onPointerDown = () => {
    longPressed.current = false;
    pressTimer.current = setTimeout(() => {
      if (devices.length === 0) return;
      longPressed.current = true;
      onOpenChange(true);
    }, 500);
  };
  const onPointerUp = () => {
    if (pressTimer.current) {
      clearTimeout(pressTimer.current);
      pressTimer.current = null;
    }
  };
  const onClick = () => {
    if (longPressed.current) {
      longPressed.current = false;
      return;
    }
    // A tap on the chip while the list is open closes it; otherwise it cycles.
    if (open) onOpenChange(false);
    else cycleDevice();
  };

  return (
    <div ref={anchorRef} style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
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
    </div>
  );
}

// Closes an open list on a pointerdown outside it and its trigger. Replaces
// the old full-window click shield, which swallowed the next click (usually
// the one on Cancel); an outside press now closes the list and still reaches
// its own target.
function useDismissOnOutsidePointer(
  refs: Array<React.RefObject<HTMLElement | null>>,
  onClose: () => void,
) {
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const refsRef = useRef(refs);
  refsRef.current = refs;
  useEffect(() => {
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node | null;
      if (target && refsRef.current.some((r) => r.current?.contains(target))) return;
      onCloseRef.current();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, []);
}

// The input-device list behind a mic chip. It renders inline, inside the
// pill box, never above it: the OS window is sized to the pill, so a menu
// positioned outside the box was clipped away and never seen. In the
// measured styles the ResizeObserver grows the window around it; the legacy
// default capsule adds `deviceListHeight` to its reported size.
function DeviceList({
  deviceName,
  devices,
  onSelectDevice,
  onClose,
  triggerRef,
  width = "100%",
}: {
  deviceName: string;
  devices: InputDevice[];
  onSelectDevice: (id: string, name: string) => void;
  onClose: () => void;
  triggerRef: React.RefObject<HTMLElement | null>;
  width?: number | string;
}) {
  const listRef = useRef<HTMLDivElement | null>(null);
  useDismissOnOutsidePointer([listRef, triggerRef], onClose);
  return (
    <div
      ref={listRef}
      style={{
        width,
        height: deviceListHeight(devices.length),
        flexShrink: 0,
        overflowY: "auto",
        background: "rgba(24,24,32,0.96)",
        border: "0.5px solid rgba(255,255,255,0.12)",
        borderRadius: 8,
        padding: "4px 0",
        animation: "fade-in 100ms ease-out",
      }}
    >
      {devices.map((d) => {
        const current = d.name.split(" ")[0] === deviceName;
        return (
          <button
            key={d.id}
            onClick={(e) => {
              e.stopPropagation();
              onSelectDevice(d.id, d.name.split(" ")[0]);
            }}
            style={{
              display: "flex",
              alignItems: "center",
              width: "100%",
              height: DEVICE_ROW_HEIGHT,
              border: "none",
              cursor: "pointer",
              padding: "0 10px",
              fontSize: 11,
              fontFamily: "inherit",
              textAlign: "left",
              color: current ? "white" : "rgba(255,255,255,0.7)",
              background: current ? "rgba(255,255,255,0.08)" : "transparent",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
            onMouseEnter={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.08)")}
            onMouseLeave={(e) => (e.currentTarget.style.background = current ? "rgba(255,255,255,0.08)" : "transparent")}
          >
            <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{d.name}</span>
            {d.is_default && <span style={{ marginLeft: 6, fontSize: 9, color: "rgba(255,255,255,0.4)", flexShrink: 0 }}>default</span>}
          </button>
        );
      })}
    </div>
  );
}

// The panel styles (well, dotGrid) hold the device list open state, so the
// list can render as a row inside the panel below the header.
function usePanelDeviceList(props: RecordingStyleProps) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const picker = (
    <DevicePicker
      deviceName={props.deviceName}
      devices={props.devices}
      onSelectDevice={props.onSelectDevice}
      open={open}
      onOpenChange={setOpen}
      anchorRef={anchorRef}
    />
  );
  const list = open ? (
    <div style={{ padding: "0 12px 8px", flexShrink: 0 }}>
      <DeviceList
        deviceName={props.deviceName}
        devices={props.devices}
        onSelectDevice={(id, name) => {
          props.onSelectDevice(id, name);
          setOpen(false);
        }}
        onClose={() => setOpen(false)}
        triggerRef={anchorRef}
      />
    </div>
  ) : null;
  return { picker, list };
}

// "Classic": dark capsule, audio-reactive rainbow lips + clock.
// No trailing text — the reference shows only the mark and the timer,
// so the 185 px box has room for both without clipping. Live interim text
// is intentionally omitted here: the 185 px row has no room beside the
// lips + clock, so the live preview is delivered by levelRail/dotGrid/well.
function ClassicPill({ clock, levels, hovered, cancelArmed, setCancelArmed, command }: RecordingStyleProps) {
  return (
    <div
      style={{
        position: "relative",
        width: 185,
        height: 52,
        borderRadius: 22,
        background: DARK_SURFACE,
        border: `${DARK_BORDER_W}px solid ${DARK_BORDER}`,
        boxShadow: surfaceShadow(command),
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
      {command && <CommandCue />}
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
// With live text the meter drops to 16 bars (78px) so the text keeps a
// usable slot; hovered, the text gives way to Cancel.
const RAIL_BARS = 24;
const RAIL_BARS_WITH_TEXT = 16;

function LevelRailPill({ clock, levels, hovered, cancelArmed, setCancelArmed, interimText, command }: RecordingStyleProps) {
  const showText = !!interimText && !hovered;
  const bars = showText ? RAIL_BARS_WITH_TEXT : RAIL_BARS;
  return (
    <div
      style={{
        position: "relative",
        width: 288,
        height: 52,
        borderRadius: 22,
        background: DARK_SURFACE,
        border: `${DARK_BORDER_W}px solid ${DARK_BORDER}`,
        boxShadow: surfaceShadow(command),
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
      {command && <CommandCue />}
      <div
        data-rail-meter
        style={{
          display: "flex",
          alignItems: "center",
          gap: 2,
          height: 28,
          // Fixed beside text, so the bars can never run under it.
          flex: showText ? "0 0 auto" : "1 1 auto",
          minWidth: 0,
        }}
      >
        {Array.from({ length: bars }, (_, i) => {
          const hist = levels.slice(-bars);
          const level = hist[i + Math.max(0, hist.length - bars)] ?? 0;
          return (
            <span
              key={i}
              style={{
                width: 3,
                flexShrink: 0,
                height: Math.max(28 * 0.14, level * 28),
                borderRadius: 1.5,
                background: rainbowColor(i / (bars - 1)),
                transition: "height 80ms ease-out",
              }}
            />
          );
        })}
      </div>
      {showText && (
        // The newest words matter, so the slot is end-aligned: what does not
        // fit overflows off the left edge under a short fade, never the end.
        <span
          title={interimText}
          style={{
            flex: "1 1 0",
            minWidth: 0,
            display: "flex",
            justifyContent: "flex-end",
            overflow: "hidden",
            maskImage: "linear-gradient(to right, transparent, #000 12px)",
            WebkitMaskImage: "linear-gradient(to right, transparent, #000 12px)",
          }}
        >
          <span style={{ fontSize: 12, color: "rgba(255,255,255,0.6)", whiteSpace: "nowrap" }}>
            {interimTail(interimText)}
          </span>
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

function ReadingWellPill(props: RecordingStyleProps) {
  const { clock, levels, hovered, cancelArmed, setCancelArmed, interimText, command } = props;
  const deviceList = usePanelDeviceList(props);
  const text = interimText || (command ? "Speak your command…" : "Listening…");
  const maxWellHeight = WELL_MAX_LINES * WELL_LINE_HEIGHT;

  return (
    <div
      style={{
        width: 400,
        borderRadius: 16,
        background: WELL_SURFACE,
        border: `${DARK_BORDER_W}px solid rgba(255,255,255,0.13)`,
        boxShadow: surfaceShadow(command),
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
        {deviceList.picker}
        <span style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.5, color: command ? "#f5c518" : "rgba(255,255,255,0.88)", padding: "3px 9px", borderRadius: 999, background: command ? "rgba(245,197,24,0.16)" : "rgba(255,255,255,0.08)", display: "inline-flex", alignItems: "center", gap: 5 }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: command ? "#f5c518" : "rgba(255,255,255,0.9)" }} />
          {command ? "COMMAND" : "LISTENING"}
        </span>
      </div>
      {deviceList.list}
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

// Dot matrix grid: 3px dots, 2px gaps, single accent color. The frame math
// (sequences, still frames, speaking glow) lives in ./lib/dotGrid.
const DOT_SIZE = 3;
const DOT_GAP = 2;
// Single accent color (LiveKit uses #1FD5F9).
const DOT_COLOR = "#1FD5F9";

function DotGridMatrix({ state, levels, reducedMotion = false }: { state: DotGridState; levels: number[]; reducedMotion?: boolean }) {
  // Sequence states (ring sweep, centre pulse, row scan) advance one step per
  // DOT_STEP_MS on a timer; nothing runs per frame for them.
  const [step, setStep] = useState(0);
  useEffect(() => {
    if (reducedMotion || !sequenceAnimates(state)) return;
    setStep(0);
    const id = setInterval(() => setStep((s) => s + 1), DOT_STEP_MS);
    return () => clearInterval(id);
  }, [state, reducedMotion]);

  // Speaking keeps a smoothed copy of the levels that eases toward the
  // latest samples in a rAF loop, so the glow tracks the voice without
  // stepping. Only speaking reads it, so only speaking runs the loop; other
  // states draw every dot dim or a sequence, and reset it to silence.
  const [smoothed, setSmoothed] = useState<number[]>(levels);
  const levelsRef = useRef(levels);
  levelsRef.current = levels;

  useEffect(() => {
    if (reducedMotion) return;
    if (state !== "speaking") {
      setSmoothed((prev) => (prev.every((v) => v === 0) ? prev : prev.map(() => 0)));
      return;
    }
    let raf: number;
    const tick = () => {
      // Exponential smoothing: factor 0.4 per frame at 60fps is a ~55ms
      // time constant. A converged frame keeps the same array, so React
      // skips the render while the level holds still.
      setSmoothed((prev) => {
        const target = levelsRef.current;
        let moved = prev.length !== target.length;
        const next = target.map((t, i) => {
          const v = prev[i] ?? 0;
          const n = v + (t - v) * 0.4;
          if (Math.abs(n - v) > 0.001) moved = true;
          return n;
        });
        return moved ? next : prev;
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [state, reducedMotion]);

  // Reduced motion: speaking follows each `pill-level` event directly (no
  // easing), and sequence states show a legible still frame.
  const intensity = state === "speaking" ? speakingIntensity(reducedMotion ? levels : smoothed) : null;
  const boolActive =
    intensity || state === "idle" ? null : reducedMotion ? staticFrame(state) : sequenceActive(state, step);

  return (
    <div
      data-dot-grid={state}
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
// `multiline` (the processing variant) lets the capsule hold an error or a
// long note: content-sized up to 480px, then the text wraps to two clamped
// lines and the capsule grows taller instead of wider. The warming variant
// keeps the fixed one-line capsule.
function DotGridCapsule({ children, multiline = false }: { children: React.ReactNode; multiline?: boolean }) {
  return (
    <div
      style={{
        position: "relative",
        ...(multiline
          ? { minHeight: 44, width: "max-content", maxWidth: PROCESSING_MAX_WIDTH, padding: "8px 14px" }
          : { height: 44, padding: "0 14px" }),
        borderRadius: 22,
        background: DARK_SURFACE,
        border: `${DARK_BORDER_W}px solid ${DARK_BORDER}`,
        boxShadow: TOP_HIGHLIGHT,
        display: "flex",
        alignItems: "center",
        gap: 12,
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
function DotGridRecordingPill(props: RecordingStyleProps) {
  const { clock, levels, hovered, cancelArmed, setCancelArmed, interimText, command } = props;
  const deviceList = usePanelDeviceList(props);
  const reducedMotion = useReducedMotion();
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
        boxShadow: surfaceShadow(command),
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
        <DotGridMatrix state={speaking ? "speaking" : "idle"} levels={levels} reducedMotion={reducedMotion} />
        <span style={{ flex: 1 }} />
        {deviceList.picker}
        <span style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.5, color: command ? "#f5c518" : "rgba(255,255,255,0.88)", padding: "3px 9px", borderRadius: 999, background: command ? "rgba(245,197,24,0.16)" : "rgba(255,255,255,0.08)", display: "inline-flex", alignItems: "center", gap: 5, flexShrink: 0 }}>
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: command ? "#f5c518" : "rgba(255,255,255,0.9)" }} />
          {command ? "COMMAND" : "LISTENING"}
        </span>
      </div>
      {deviceList.list}
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
function DotGridWarmingPill({ seconds, progress }: { seconds: number; progress: number | null }) {
  const reducedMotion = useReducedMotion();
  return (
    <DotGridCapsule>
      <DotGridMatrix state="connecting" levels={[]} reducedMotion={reducedMotion} />
      <div style={{ minWidth: 0, flex: 1, lineHeight: 1.25 }}>
        <p style={{ fontSize: 13, fontWeight: 500, color: "rgba(255,255,255,0.9)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {warmingStatus(progress).replace("…", "...")}
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
// A take that captured no audio gets an actionable escape hatch: the pill is
// already clickable, so the shortcut opens the OS microphone permission pane.
function MicFailureAction() {
  return (
    <button
      type="button"
      onClick={() => {
        invoke("open_permission_settings", { kind: "microphone" }).catch(() => {});
      }}
      style={{
        flexShrink: 0,
        fontSize: 12,
        fontWeight: 600,
        color: "rgba(255,255,255,0.92)",
        background: "rgba(255,255,255,0.14)",
        border: "1px solid rgba(255,255,255,0.24)",
        borderRadius: 999,
        padding: "5px 12px",
        cursor: "pointer",
      }}
    >
      Open Settings
    </button>
  );
}

// Recovery action for a settings-fixable error (model/device not found):
// opens the app's Settings screen. Same shape as MicFailureAction, which
// opens the OS permission pane instead.
function SettingsAction() {
  return (
    <button
      type="button"
      onClick={() => {
        invoke("open_main_window", { route: "settings" }).catch(() => {});
      }}
      style={{
        flexShrink: 0,
        fontSize: 12,
        fontWeight: 600,
        color: "rgba(255,255,255,0.92)",
        background: "rgba(255,255,255,0.14)",
        border: "1px solid rgba(255,255,255,0.24)",
        borderRadius: 999,
        padding: "5px 12px",
        cursor: "pointer",
      }}
    >
      Open Settings
    </button>
  );
}

function DotGridProcessingPill({
  message,
  skipMessage,
  micFailure,
  error,
}: {
  message: string;
  skipMessage: string | null;
  micFailure: boolean;
  error: boolean;
}) {
  const reducedMotion = useReducedMotion();
  return (
    <DotGridCapsule multiline>
      <DotGridMatrix state="thinking" levels={[]} reducedMotion={reducedMotion} />
      <div style={{ minWidth: 0, flex: 1, display: "flex", flexDirection: "column", gap: 2 }}>
        <p
          role={error ? "alert" : "status"}
          aria-live={error ? "assertive" : "polite"}
          title={message}
          style={{
            fontSize: 13,
            fontWeight: 500,
            lineHeight: "18px",
            color: error ? "#ff6369" : "rgba(255,255,255,0.9)",
            display: "-webkit-box",
            WebkitLineClamp: 2,
            WebkitBoxOrient: "vertical",
            whiteSpace: "normal",
            overflowWrap: "anywhere",
            overflow: "hidden",
            margin: 0,
          }}
        >
          {message}
        </p>
        {skipMessage && (
          <p title={skipMessage} style={{ fontSize: 12, fontWeight: 500, color: "#f5c518", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", margin: 0 }}>
            {skipMessage}
          </p>
        )}
      </div>
      {micFailure && <MicFailureAction />}
      {error && !micFailure && errorHasSettingsAction(message) && <SettingsAction />}
    </DotGridCapsule>
  );
}

// Post-insertion card: what polish changed, shown once the text has landed.
// Removed words stay readable but muted; added words take the accent. The card
// is a summary, not an editor: clicking it opens the full entry in the app.
const DIFF_ADD = "#7db1ff";
const DIFF_ADD_BG = "rgba(79,142,247,0.16)";

function PolishDiffCard({
  raw,
  polished,
  ms,
  onClose,
}: {
  raw: string;
  polished: string;
  ms: number;
  onClose: () => void;
}) {
  const segments = diffWords(raw, polished);
  const changes = changeCount(segments);
  const seconds = formatPolishMs(ms);

  const open = (route: string) => {
    invoke("open_main_window", { route }).catch(() => {});
    onClose();
  };

  return (
    <div
      role="status"
      onClick={() => open("home")}
      style={{
        width: 460,
        padding: "13px 16px 12px",
        borderRadius: 18,
        background: "rgba(13,13,13,0.94)",
        border: "0.5px solid rgba(255,255,255,0.12)",
        boxShadow: TOP_HIGHLIGHT,
        backdropFilter: "blur(24px)",
        color: "white",
        cursor: "pointer",
        animation: "fade-in 0.18s ease-out",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ fontSize: 13, fontWeight: 650 }}>
          {changes} {changes === 1 ? "change" : "changes"}
        </span>
        {seconds && (
          <span
            style={{
              fontSize: 12,
              color: "rgba(255,255,255,0.5)",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            · {seconds}
          </span>
        )}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 10 }}>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              open("transforms");
            }}
            style={{
              fontSize: 12,
              fontWeight: 600,
              color: "#8bbaff",
              background: "transparent",
              border: "none",
              padding: 0,
              cursor: "pointer",
            }}
          >
            Configure Polish
          </button>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={(e) => {
              e.stopPropagation();
              onClose();
            }}
            style={{
              width: 18,
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              borderRadius: 999,
              border: "none",
              background: "transparent",
              color: "rgba(255,255,255,0.45)",
              fontSize: 14,
              lineHeight: 1,
              cursor: "pointer",
              padding: 0,
            }}
          >
            ×
          </button>
        </div>
      </div>
      <div
        style={{
          marginTop: 9,
          fontSize: 13,
          lineHeight: 1.5,
          color: "rgba(255,255,255,0.74)",
          maxHeight: 60,
          overflow: "hidden",
        }}
      >
        {segments.map((seg, i) =>
          seg.kind === "same" ? (
            <span key={i}>{seg.text} </span>
          ) : seg.kind === "del" ? (
            <span
              key={i}
              style={{ color: "rgba(255,255,255,0.34)", textDecoration: "line-through" }}
            >
              {seg.text}{" "}
            </span>
          ) : (
            <span
              key={i}
              style={{ color: DIFF_ADD, background: DIFF_ADD_BG, borderRadius: 3, padding: "0 2px" }}
            >
              {seg.text}{" "}
            </span>
          ),
        )}
      </div>
      <div style={{ marginTop: 8, fontSize: 11, color: "rgba(255,255,255,0.42)" }}>
        Inserted · click to open History
      </div>
    </div>
  );
}

// --- Main component ---------------------------------------------------------
// Legacy default-chrome processing view: status message plus the optional
// transform skip/fallback notice. Extracted verbatim from Pill; every busy
// machine phase (processing/transcribing/streaming/done/error) renders
// through it with identical pixels.
function LegacyProcessingBody({
  message,
  skipMessage,
  micFailure,
  error,
  lines,
}: {
  message: string;
  skipMessage: string | null;
  micFailure: boolean;
  error: boolean;
  lines: 1 | 2;
}) {
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
        role={error ? "alert" : "status"}
        aria-live={error ? "assertive" : "polite"}
        title={message}
        style={{
          // The pill is sized to the text (busyPillSize), so a single line
          // never truncates; past the 480px cap it wraps to two clamped
          // lines and the full message stays in the title.
          ...(lines === 2
            ? {
                display: "-webkit-box",
                WebkitLineClamp: 2,
                WebkitBoxOrient: "vertical" as const,
                whiteSpace: "normal",
                overflowWrap: "anywhere" as const,
                lineHeight: "18px",
                textAlign: "left" as const,
              }
            : { whiteSpace: "nowrap", textOverflow: "ellipsis", textAlign: "center" as const }),
          overflow: "hidden",
          minWidth: 0,
          fontSize: 13,
          fontWeight: 500,
          margin: 0,
          color: error ? "#ff6369" : undefined,
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
      {micFailure && <MicFailureAction />}
      {error && !micFailure && errorHasSettingsAction(message) && <SettingsAction />}
    </div>
  );
}

export function Pill() {
  // All session/phase/timer state lives in the explicit machine
  // (ui/src/pillMachine.ts); this component derives layout and renders.
  const { machine, dispatch, selectDevice, lockLanguage, toggleRecordingMode } = usePillMachine();
  // The input-device list opened from the recording mic chip. The chip is a
  // plain button (not the press-and-hold capsule), so it needs its own open
  // flag; the list itself is the same DeviceList the panel styles use.
  const [micMenuOpen, setMicMenuOpen] = useState(false);
  const micChipRef = useRef<HTMLSpanElement | null>(null);
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
  const errorAction = phase === "error" && !machine.micFailure && errorHasSettingsAction(message);
  const busySize = busyPillSize({
    message,
    skipMessage: machine.skipMessage,
    micFailure: machine.micFailure,
    hasAction: machine.micFailure || errorAction,
  });
  const size = usesPortedStyle
    ? { width: 0, height: 0 }
    : expanded
      ? SIZES.recordingExpanded
      : busy
        ? busySize
        : phase === "recording"
          ? SIZES.recording
          : phase === "warming"
            ? SIZES.warming
            : SIZES.idle;
  // The default capsule reports a computed size, so an open device list adds
  // its exact height to the window (the capsule itself keeps `size`). The
  // list sits on the side away from the anchored edge, so the capsule stays
  // put under the cursor while the window grows.
  const anchor = pillAnchor(settings?.pillPosition);
  const deviceListOpen = !usesPortedStyle && expanded && micMenuOpen && machine.devices.length > 0;
  const windowSize = deviceListOpen
    ? { width: size.width, height: size.height + DEVICE_LIST_GAP + deviceListHeight(machine.devices.length) }
    : size;
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
  // Each style draws it inset on its own root (see COMMAND_RING).
  const commandTake = machine.pill.phase === "recording" && !!machine.pill.command;
  const styleProps: RecordingStyleProps = {
    levels,
    level: levels.length > 0 ? levels[levels.length - 1] : 0,
    clock,
    startedAtMs,
    // P2.8: only a recording take can be a command take.
    command: commandTake,
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
  // capsules 52px, so a fixed window would clip or letterbox them. Every
  // render branch must publish its size: a branch that does not leaves the
  // window at whatever the previous branch measured, which is how a small
  // polish card could shrink the window and then clip the next recording.
  // The legacy pill transitions its own width and height, so its live box is
  // not the target size during that animation; its size is known exactly, and
  // reporting the target keeps the very first frame unclipped. A shrink is
  // the opposite case: the window keeps its size until the capsule's
  // transition ends (planLegacyReport), or the wider pill is cut square in
  // the smaller window while it animates. The backend
  // also waits for this report before showing a new take. Skip the report
  // when the computed size equals the last reported one: stream tokens
  // (machine.streamText) re-render every frame during polish, and re-invoking
  // set_pill_size with the same size is a wasted IPC round-trip.
  const lastReported = useRef<{ w: number; h: number } | null>(null);
  const legacyRef = useRef<HTMLDivElement | null>(null);
  // The capsule size the legacy branch last rendered (null after a measured
  // branch: a freshly mounted capsule has no transition to wait for).
  const lastCapsule = useRef<{ w: number; h: number } | null>(null);
  // A shrink waiting for the capsule's transitionend: its final size and the
  // teardown for its listener and fallback timer. It outlives effect re-runs
  // (stream tokens, hover) so an unrelated render cannot report it early.
  const pendingShrink = useRef<{ final: { w: number; h: number }; cancel: () => void } | null>(null);
  const cancelPendingShrink = () => {
    pendingShrink.current?.cancel();
    pendingShrink.current = null;
  };
  useEffect(() => cancelPendingShrink, []);
  useLayoutEffect(() => {
    const report = (w: number, h: number) => {
      if (w > 0 && h > 0) {
        const cw = Math.ceil(w);
        const ch = Math.ceil(h);
        if (lastReported.current && lastReported.current.w === cw && lastReported.current.h === ch) {
          return;
        }
        lastReported.current = { w: cw, h: ch };
        invoke("set_pill_size", { width: cw, height: ch }).catch(() => {});
      }
    };
    if (!usesPortedStyle && phase !== "result") {
      const capsule = { w: size.width, h: size.height };
      const prev = lastCapsule.current;
      lastCapsule.current = capsule;
      const plan = planLegacyReport({
        reported: lastReported.current,
        target: { w: windowSize.width, h: windowSize.height },
        capsuleShrinking: !!prev && (capsule.w < prev.w || capsule.h < prev.h),
        inFlight: pendingShrink.current !== null,
      });
      report(plan.now.w, plan.now.h);
      if (!plan.deferred) {
        cancelPendingShrink();
        return;
      }
      const final = { w: windowSize.width, h: windowSize.height };
      if (pendingShrink.current) {
        pendingShrink.current.final = final;
        return;
      }
      const node = legacyRef.current;
      const finish = () => {
        const pending = pendingShrink.current;
        if (!pending) return;
        cancelPendingShrink();
        report(pending.final.w, pending.final.h);
      };
      const onEnd = (e: Event) => {
        const prop = (e as TransitionEvent).propertyName;
        if (e.target !== node || (prop && prop !== "width" && prop !== "height")) return;
        finish();
      };
      node?.addEventListener("transitionend", onEnd);
      const timer = setTimeout(finish, SHRINK_REPORT_FALLBACK_MS);
      pendingShrink.current = {
        final,
        cancel: () => {
          node?.removeEventListener("transitionend", onEnd);
          clearTimeout(timer);
        },
      };
      return;
    }
    cancelPendingShrink();
    lastCapsule.current = null;
    const node = pillRef.current;
    if (!node) return;
    const measure = () => {
      const r = node.getBoundingClientRect();
      report(r.width, r.height);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [
    phase,
    pillStyle,
    usesPortedStyle,
    windowSize.width,
    windowSize.height,
    size.width,
    size.height,
    machine.hovered,
    expanded,
    machine.skipMessage,
    machine.interimText,
    machine.micFailure,
  ]);

  // The mic overlay only exists while the chips are expanded, so collapse it
  // with them: a later recording must not resume with a stale menu open.
  useEffect(() => {
    if (!expanded || !deviceName) setMicMenuOpen(false);
  }, [expanded, deviceName]);

  // The polish card is its own surface: it keeps the same measured-size
  // contract (the ResizeObserver above resizes the OS window to fit it) and is
  // independent of the pill style the user picked.
  if (machine.pill.phase === "result") {
    return (
      <div style={{ display: "flex", width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
        <div ref={pillRef}>
          <PolishDiffCard
            raw={machine.pill.raw}
            polished={machine.pill.polished}
            ms={machine.pill.ms}
            onClose={() => dispatch({ type: "result-dismiss" })}
          />
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
      {usesPortedStyle ? (
        <div ref={pillRef} onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)} style={{ animation: "fade-in 0.18s ease-out", flexShrink: 0 }}>
          {dotGrid && phase === "recording" && <DotGridRecordingPill {...styleProps} />}
          {dotGrid && phase === "warming" && (
            <DotGridWarmingPill
              seconds={Math.floor((now - machine.warmingSinceMs) / 1000)}
              progress={machine.loadProgress}
            />
          )}
          {dotGrid && busy && (
            <DotGridProcessingPill
              message={message}
              skipMessage={machine.skipMessage}
              micFailure={machine.micFailure}
              error={phase === "error"}
            />
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
          display: "flex",
          flexDirection: anchor.v === "bottom" ? "column-reverse" : "column",
          alignItems: "flex-end",
          gap: DEVICE_LIST_GAP,
          flexShrink: 0,
        }}
      >
      <div
        ref={legacyRef}
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
          boxShadow: commandTake ? COMMAND_RING : undefined,
          border: `${DARK_BORDER_W}px solid rgba(255,255,255,0.1)`,
          transition: "width 450ms cubic-bezier(0.2,0.9,0.3,1), height 450ms cubic-bezier(0.2,0.9,0.3,1), border-radius 450ms cubic-bezier(0.2,0.9,0.3,1)",
        }}
      >
        {phase === "idle" && <IdleBars />}
        {phase === "warming" && (
          <Warming
            seconds={Math.floor((now - machine.warmingSinceMs) / 1000)}
            progress={machine.loadProgress}
          />
        )}
        {busy && (
          <LegacyProcessingBody
            message={message}
            skipMessage={machine.skipMessage}
            micFailure={machine.micFailure}
            error={phase === "error"}
            lines={busySize.lines}
          />
        )}
        {phase === "recording" && (
          <div style={{ display: "flex", width: "100%", animation: "fade-in 0.18s ease-out", alignItems: "center", gap: 10, padding: "0 14px" }}>
            <RecordingDot />
            <Waveform levels={levels} />
            {commandTake && <CommandCue />}
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
                  <span ref={micChipRef} style={{ display: "inline-flex" }}>
                    <Chip icon={<MicIcon />} label={deviceName} onClick={() => setMicMenuOpen((open) => !open)} />
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
                      ? languageChip.code.toUpperCase()
                      : settings.language === "auto" || settings.language === "" ? "Auto" : settings.language.toUpperCase()
                  }
                  badge={
                    languageChip?.detected ? (
                      // A detected language is a small dot inside the chip, not
                      // a " · detected" suffix that pushed Cancel out of the capsule.
                      <span
                        role="img"
                        aria-label="detected"
                        title="Detected language. Click to lock it."
                        style={{ flexShrink: 0, width: 5, height: 5, borderRadius: "50%", background: "#7db1ff" }}
                      />
                    ) : undefined
                  }
                  onClick={languageChip ? () => lockLanguage(languageChip.code) : undefined}
                />
                <CancelButton armed={cancelArmed} setArmed={setCancelArmed} />
              </div>
            )}
          </div>
        )}
      </div>
      {deviceListOpen && (
        <DeviceList
          deviceName={deviceName}
          devices={machine.devices}
          onSelectDevice={(id, name) => {
            selectDevice(id, name);
            setMicMenuOpen(false);
          }}
          onClose={() => setMicMenuOpen(false)}
          triggerRef={micChipRef}
          width={220}
        />
      )}
      </div>
      )}
    </div>
  );
}
