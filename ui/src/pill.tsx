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
  recording: { width: 250, height: 44 },
  recordingExpanded: { width: 460, height: 44 },
} as const;

type PillPhase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

interface Settings {
  recording_mode: string;
  language: string;
  input_device: string;
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

// --- Main component ---------------------------------------------------------

function Pill() {
  const [state, setState] = useState<PillPhase>({ phase: "idle" });
  const [settings, setSettings] = useState<Settings | null>(null);
  const [deviceName, setDeviceName] = useState("Default");
  const [hovered, setHovered] = useState(false);
  const [levels, setLevels] = useState<number[]>(() => new Array(WAVE_BARS).fill(0));
  const warmingSince = useRef(0);
  const recordingSince = useRef(0);
  const now = useNow(state.phase === "warming" ? 1000 : 500, state.phase === "recording" || state.phase === "warming");

  const refreshSettings = async () => {
    try {
      const next = await invoke<Settings>("get_settings");
      setSettings(next);
      const devices = await invoke<{ id: string; name: string }[]>("list_input_devices");
      const device = devices.find((d) => d.id === next.input_device);
      setDeviceName(device ? device.name.split(" ")[0] : "Default");
    } catch {
      // Settings not available yet.
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

  useTauriEvent<PillPhase>("pill-state", ({ payload }) => {
    if (payload.phase === "warming" && stateRef.current.phase !== "warming") {
      warmingSince.current = Date.now();
    }
    if (payload.phase === "recording" && stateRef.current.phase !== "recording") {
      setLevels(new Array(WAVE_BARS).fill(0));
      // Fallback start time in case startedAtMs is missing from the event.
      if (typeof payload.startedAtMs !== "number") recordingSince.current = Date.now();
    }
    if (payload.phase !== "recording") setHovered(false);
    setState(payload);
  });
  useTauriEvent<number>("pill-level", ({ payload }) => {
    setLevels((l) => [...l.slice(1), payload]);
  });

  const expanded = state.phase === "recording" && hovered;
  const size = expanded ? SIZES.recordingExpanded : SIZES[state.phase];
  const active = state.phase !== "idle";

  return (
    <div style={{ display: "flex", width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
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
          border: "1px solid rgba(255,255,255,0.1)",
          transition: "width 450ms cubic-bezier(0.2,0.9,0.3,1), height 450ms cubic-bezier(0.2,0.9,0.3,1), border-radius 450ms cubic-bezier(0.2,0.9,0.3,1)",
        }}
      >
        {state.phase === "idle" && <IdleBars />}
        {state.phase === "warming" && (
          <Warming seconds={Math.floor((now - warmingSince.current) / 1000)} />
        )}
        {state.phase === "processing" && (
          <p
            key={state.message}
            style={{
              width: "100%",
              animation: "fade-in 0.18s ease-out",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
              padding: "0 16px",
              textAlign: "center",
              fontSize: 13,
              fontWeight: 500,
            }}
          >
            {state.message}
          </p>
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
              {formatClock((Date.now() - (typeof state.startedAtMs === "number" ? state.startedAtMs : recordingSince.current)) / 1000)}
            </span>
            {expanded && settings && (
              <div style={{ display: "flex", flexShrink: 0, animation: "pop-in 0.2s ease-out", alignItems: "center", gap: 6, paddingLeft: 4 }}>
                <Chip icon={<MicIcon />} label={deviceName} />
                <Chip
                  icon={settings.recording_mode === "hold" ? <HandIcon /> : <RepeatIcon />}
                  label={settings.recording_mode === "hold" ? "Hold" : "Toggle"}
                />
                <Chip
                  icon={<GlobeIcon />}
                  label={settings.language === "auto" ? "Auto" : settings.language.toUpperCase()}
                />
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Pill />
  </StrictMode>,
);
