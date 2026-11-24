// The meeting island: a compact always-on-top pill (Dynamic-Island style)
// that shows the live state of a meeting — recording / finalizing / report
// ready — so the user can check or stop a meeting from anywhere, without
// navigating to the Meeting screen.
//
// The island polls `meeting_status` once a second (the same seam the
// Meeting screen uses) and hides itself when there is nothing to show.
// Rust shows the window on meeting start (crate::island::show) and hides
// it on cancel/failure; the "report ready" moment ends with an
// island-side auto-hide after DONE_HIDE_MS.
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface IslandReport {
  title: string;
  reportPath: string;
}

interface IslandStatus {
  phase: "idle" | "recording" | "finalizing" | "done";
  elapsedMs: number | null;
  liveText: string | null;
  liveWordCount: number | null;
  liveSegmentCount: number | null;
  report: IslandReport | null;
  error: string | null;
}

/** How long the "report ready" state stays on screen before auto-hiding. */
const DONE_HIDE_MS = 8000;

function formatClock(ms: number): string {
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

/** Hides this window from the island side (done/idle states). */
function hideSelf(): void {
  void getCurrentWindow()
    .hide()
    .catch(() => {
      // Not in a Tauri context (plain browser dev); nothing to hide.
    });
}

/** Asks the main window to come forward on the Meeting screen.
 * The main window listens for the event, shows/focuses itself, and
 * navigates — the island never reaches across windows. */
async function goToMeeting(): Promise<void> {
  try {
    await emit("meeting-island:go-to-meeting");
  } catch {
    // Not in a Tauri context; nothing to reach.
  }
}

const stopBtn: React.CSSProperties = {
  background: "#ffffff",
  color: "#070709",
  border: "none",
  borderRadius: 100,
  padding: "7px 14px",
  fontSize: 12.5,
  fontWeight: 650,
  cursor: "pointer",
  flexShrink: 0,
};

const ghostBtn: React.CSSProperties = {
  background: "transparent",
  color: "#ffffff",
  border: "1px solid rgba(255,255,255,.28)",
  borderRadius: 100,
  padding: "6px 13px",
  fontSize: 12.5,
  fontWeight: 600,
  cursor: "pointer",
  flexShrink: 0,
};

const minBtn: React.CSSProperties = {
  background: "transparent",
  color: "#a1a1aa",
  border: "1px solid rgba(255,255,255,.18)",
  borderRadius: 100,
  width: 24,
  height: 24,
  padding: 0,
  fontSize: 13,
  fontWeight: 600,
  lineHeight: 1,
  cursor: "pointer",
  flexShrink: 0,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
};

/** True for mousedown events that should start a window drag. */
function isDragStart(e: React.MouseEvent): boolean {
  return e.button === 0;
}

export function Island() {
  const [status, setStatus] = useState<IslandStatus | null>(null);
  const [minimized, setMinimized] = useState(false);
  const doneSinceRef = useRef<number | null>(null);

  // One poll per second: drives the state, the timer, and the auto-hide.
  useEffect(() => {
    let alive = true;
    const poll = async () => {
      try {
        const s: IslandStatus = await invoke("meeting_status");
        if (!alive) return;
        setStatus(s);
        if (s.phase === "done") {
          if (doneSinceRef.current === null) {
            doneSinceRef.current = Date.now();
          } else if (Date.now() - doneSinceRef.current > DONE_HIDE_MS) {
            hideSelf();
          }
        } else if (s.phase === "idle") {
          doneSinceRef.current = null;
          hideSelf();
        } else {
          doneSinceRef.current = null;
        }
      } catch {
        // Status fetch failed; keep polling.
      }
    };
    void poll();
    const iv = window.setInterval(() => void poll(), 1000);
    return () => {
      alive = false;
      window.clearInterval(iv);
    };
  }, []);

  const phase = status?.phase ?? "idle";
  const elapsed = status?.elapsedMs ?? 0;
  const words = status?.liveWordCount ?? 0;

  const handleStop = () => {
    void invoke("meeting_stop").catch(() => {});
  };
  const handleReport = () => {
    // While recording this opens the LIVE (self-refreshing) report;
    // after stop it opens the finished report.
    void invoke("meeting_open_report").catch(() => {});
  };

  // Drag: mousedown on the pill body (never on a button) starts a native
  // window drag; when it ends we persist the drop position so the island
  // comes back where the user left it.
  const handleDragStart = async (e: React.MouseEvent) => {
    if (!isDragStart(e)) return;
    e.stopPropagation();
    const win = getCurrentWindow();
    try {
      await win.startDragging();
      const pos = await win.outerPosition();
      const sf = await win.scaleFactor();
      await invoke("island_save_position", { x: pos.x / sf, y: pos.y / sf });
    } catch {
      // Not in a Tauri context (plain browser dev); nothing to drag.
    }
  };

  // Buttons: never start a drag, never trigger the pill's click.
  const noDrag = (e: React.MouseEvent) => e.stopPropagation();
  const btnProps = { onMouseDown: noDrag, onClick: (e: React.MouseEvent) => e.stopPropagation() };

  const compact = minimized && phase === "recording";

  return (
    <div style={{ width: "100%", height: "100%", padding: 8, display: "flex", justifyContent: "center", alignItems: "center" }}>
      <div
        onMouseDown={handleDragStart}
        onClick={() => {
          if (compact) {
            setMinimized(false);
          } else {
            void goToMeeting();
          }
        }}
        title={compact ? "Click to expand" : "Drag to move"}
        style={{
          width: compact ? 118 : "100%",
          height: "100%",
          display: "flex",
          alignItems: "center",
          gap: compact ? 8 : 10,
          padding: compact ? "0 16px" : "0 14px",
          background: "#070709",
          borderRadius: 24,
          boxShadow: "0 10px 30px rgba(0,0,0,.28), 0 2px 8px rgba(0,0,0,.2)",
          color: "#fff",
          cursor: compact ? "pointer" : "grab",
          userSelect: "none",
        }}
      >
        {compact && (
          <>
            <span
              style={{
                width: 9,
                height: 9,
                borderRadius: "50%",
                background: "#ff453a",
                animation: "island-pulse 1.2s ease-in-out infinite",
                flexShrink: 0,
              }}
            />
            <span
              style={{
                fontSize: 15,
                fontWeight: 650,
                fontVariantNumeric: "tabular-nums",
                letterSpacing: "-0.01em",
              }}
            >
              {formatClock(elapsed)}
            </span>
          </>
        )}

        {!compact && phase === "recording" && (
          <>
            <span
              style={{
                width: 9,
                height: 9,
                borderRadius: "50%",
                background: "#ff453a",
                animation: "island-pulse 1.2s ease-in-out infinite",
                flexShrink: 0,
              }}
            />
            <span
              style={{
                fontSize: 11,
                fontWeight: 700,
                letterSpacing: "0.1em",
                color: "#ff6b61",
                flexShrink: 0,
              }}
            >
              REC
            </span>
            <span
              style={{
                fontSize: 17,
                fontWeight: 650,
                fontVariantNumeric: "tabular-nums",
                letterSpacing: "-0.01em",
              }}
            >
              {formatClock(elapsed)}
            </span>
            <span style={{ fontSize: 12, color: "#a1a1aa", whiteSpace: "nowrap" }}>
              {words > 0 ? `${words} words` : "listening…"}
            </span>
            <span style={{ flex: 1 }} />
            <button {...btnProps} onClick={(e) => { e.stopPropagation(); setMinimized(true); }} style={minBtn} title="Minimize — click the pill to expand">
              −
            </button>
            <button {...btnProps} onClick={handleReport} style={ghostBtn} title="Open the live report in a new tab">
              Report
            </button>
            <button {...btnProps} onClick={handleStop} style={stopBtn} title="Stop the meeting and generate the report">
              Stop
            </button>
          </>
        )}

        {phase === "finalizing" && (
          <>
            {minimized && (
              <span
                style={{
                  width: 14,
                  height: 14,
                  border: "2px solid rgba(255,255,255,.25)",
                  borderTopColor: "#fff",
                  borderRadius: "50%",
                  animation: "island-spin 0.8s linear infinite",
                  flexShrink: 0,
                }}
              />
            )}
            {!minimized && (
              <>
                <span
                  style={{
                    width: 14,
                    height: 14,
                    border: "2px solid rgba(255,255,255,.25)",
                    borderTopColor: "#fff",
                    borderRadius: "50%",
                    animation: "island-spin 0.8s linear infinite",
                    flexShrink: 0,
                  }}
                />
                <span style={{ fontSize: 14, fontWeight: 550 }}>Building your report…</span>
                <span style={{ flex: 1 }} />
              </>
            )}
          </>
        )}

        {phase === "done" && (
          <>
            <span
              style={{
                width: 18,
                height: 18,
                borderRadius: "50%",
                background: "#16a34a",
                color: "#fff",
                fontSize: 12,
                fontWeight: 700,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                flexShrink: 0,
              }}
            >
              ✓
            </span>
            <span
              style={{
                fontSize: 14,
                fontWeight: 550,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              Report ready{status?.report ? ` — ${status.report.title}` : ""}
            </span>
            <span style={{ flex: 1 }} />
            <button {...btnProps} onClick={handleReport} style={stopBtn} title="Open the report in a new tab">
              Open
            </button>
          </>
        )}

        {/* idle: the island hides itself on the next poll; render nothing. */}
        <style>{`
          @keyframes island-pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.3; }
          }
          @keyframes island-spin {
            to { transform: rotate(360deg); }
          }
        `}</style>
      </div>
    </div>
  );
}
