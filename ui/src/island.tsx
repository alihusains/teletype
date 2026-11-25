// The meeting island: a compact always-on-top pill (Dynamic-Island style)
// that shows the live state of a meeting — recording / finalizing / report
// ready — so the user can check or stop a meeting from anywhere, without
// navigating to the Meeting screen.
//
// The island polls `meeting_status` once a second (the same seam the
// Meeting screen uses) and hides itself when there is nothing to show.
// Rust shows the window on meeting start (crate::island::show) and hides
// it on cancel/failure; the "report ready" moment ends with an
// island-side auto-hide after DONE_HIDE_MS (paused while hovered).
//
// Interactions:
// - Drag the pill body to move it. `startDragging()` is unreliable on this
//   `focusable: false` window (tao's ReleaseCapture fails and the error is
//   swallowed), so the window is moved manually with setPosition, tracking
//   the pointer via setPointerCapture. The drop position persists across
//   meetings; a real drag never triggers the pill's click (navigate/expand).
// - Minimize (−) collapses to a compact dot+timer pill with its own Stop;
//   clicking the compact pill expands. The window resizes to the pill so
//   the transparent area never blocks clicks meant for other windows.
// - The minimize state resets for every new meeting.
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";

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

/** Window sizes (logical points) — the full pill and the compact pill
 * (same height; only the width changes). */
const FULL_W = 440;
const FULL_H = 92;
const COMPACT_W = 134;

/** Pointer travel (px) before a press counts as a drag, not a click. */
const DRAG_CLICK_SLOP = 5;

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

/** Resizes the island window to the current pill so the transparent
 * (click-blocking) area shrinks with the visible content. */
function sizeWindow(width: number, height: number): void {
  void invoke("island_set_size", { width, height }).catch(() => {
    // Not in a Tauri context; the window keeps its declared size.
  });
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

const compactStopBtn: React.CSSProperties = {
  background: "#ffffff",
  color: "#070709",
  border: "none",
  borderRadius: 100,
  width: 26,
  height: 26,
  padding: 0,
  fontSize: 10,
  cursor: "pointer",
  flexShrink: 0,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
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

export function Island() {
  const [status, setStatus] = useState<IslandStatus | null>(null);
  const [minimized, setMinimized] = useState(false);
  const doneSinceRef = useRef<number | null>(null);
  // True right after a drag ends; the click event that follows a drag
  // must not navigate (full) or expand (compact).
  const swallowedClickRef = useRef(false);
  // While the user hovers the "report ready" state, pause the auto-hide.
  const hoverRef = useRef(false);
  const prevPhaseRef = useRef<string>("idle");

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
          } else if (!hoverRef.current && Date.now() - doneSinceRef.current > DONE_HIDE_MS) {
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

  // A new meeting starts un-minimized (the state is per meeting, not per
  // window lifetime — the webview stays alive across meetings).
  useEffect(() => {
    if (phase === "recording" && prevPhaseRef.current !== "recording") {
      setMinimized(false);
    }
    prevPhaseRef.current = phase;
  }, [phase]);

  const compact = minimized && phase === "recording";

  // The window tracks the pill: compact while minimized-recording, full
  // otherwise (finalizing/done always show the full pill).
  useEffect(() => {
    sizeWindow(compact ? COMPACT_W : FULL_W, FULL_H);
  }, [compact]);

  const handleStop = () => {
    void invoke("meeting_stop").catch(() => {});
  };
  const handleReport = () => {
    // While recording this opens the LIVE (self-refreshing) report;
    // after stop it opens the finished report.
    void invoke("meeting_open_report").catch(() => {});
  };

  // Drag: pointerdown on the pill body (never on a button) starts a manual
  // window drag. We move the window ourselves with setPosition (throttled to
  // one update per animation frame) instead of startDragging, which is a
  // silent no-op on this focusable:false window. setPointerCapture keeps the
  // move events flowing even when the cursor outruns the pill. A drag that
  // travels more than DRAG_CLICK_SLOP swallows the following click so it
  // doesn't also navigate/expand.
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    winX: number;
    winY: number;
    moved: boolean;
  } | null>(null);
  const dragFrameRef = useRef<number | null>(null);
  const pendingDragRef = useRef<{ x: number; y: number } | null>(null);

  const finishDrag = async () => {
    if (dragFrameRef.current !== null) {
      cancelAnimationFrame(dragFrameRef.current);
      dragFrameRef.current = null;
    }
    pendingDragRef.current = null;
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag || !drag.moved) return;
    swallowedClickRef.current = true;
    try {
      const win = getCurrentWindow();
      const pos = await win.outerPosition();
      const sf = await win.scaleFactor();
      await invoke("island_save_position", { x: pos.x / sf, y: pos.y / sf });
    } catch {
      // Not in a Tauri context; nothing to persist.
    }
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    // Capture the element + coords synchronously: React nulls e.currentTarget
    // once the handler returns, so the async continuation must not read it.
    const el = e.currentTarget as HTMLElement;
    const pointerId = e.pointerId;
    const startX = e.screenX;
    const startY = e.screenY;
    const win = getCurrentWindow();
    void (async () => {
      try {
        const pos = await win.outerPosition();
        const scale = await win.scaleFactor();
        dragRef.current = {
          pointerId,
          startX,
          startY,
          winX: pos.x / scale,
          winY: pos.y / scale,
          moved: false,
        };
        el.setPointerCapture(pointerId);
      } catch {
        // Not in a Tauri context (plain browser dev); nothing to drag.
      }
    })();
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const drag = dragRef.current;
    if (!drag || e.pointerId !== drag.pointerId) return;
    const dx = e.screenX - drag.startX;
    const dy = e.screenY - drag.startY;
    if (!drag.moved && Math.hypot(dx, dy) < DRAG_CLICK_SLOP) return;
    drag.moved = true;
    pendingDragRef.current = { x: drag.winX + dx, y: drag.winY + dy };
    if (dragFrameRef.current !== null) return;
    dragFrameRef.current = requestAnimationFrame(() => {
      dragFrameRef.current = null;
      const target = pendingDragRef.current;
      if (!target) return;
      void getCurrentWindow()
        .setPosition(new LogicalPosition(target.x, target.y))
        .catch(() => {
          // Not in a Tauri context; nothing to move.
        });
    });
  };

  const onPointerUp = (e: React.PointerEvent) => {
    if (dragRef.current?.pointerId !== e.pointerId) return;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      // Capture may already be released.
    }
    void finishDrag();
  };

  const handlePillClick = () => {
    if (swallowedClickRef.current) {
      swallowedClickRef.current = false;
      return;
    }
    if (compact) {
      setMinimized(false);
    } else {
      void goToMeeting();
    }
  };

  // Buttons: never start a drag, never trigger the pill's click.
  const noDrag = (e: React.PointerEvent) => e.stopPropagation();
  const btnProps = {
    onPointerDown: noDrag,
    onClick: (e: React.MouseEvent) => e.stopPropagation(),
  };

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        padding: 8,
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
      }}
    >
      <div
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onClick={handlePillClick}
        onMouseEnter={() => {
          hoverRef.current = true;
        }}
        onMouseLeave={() => {
          hoverRef.current = false;
        }}
        title={compact ? "Click to expand" : "Drag to move"}
        style={{
          width: compact ? 118 : "100%",
          height: "100%",
          display: "flex",
          alignItems: "center",
          gap: compact ? 7 : 10,
          padding: compact ? "0 12px" : "0 14px",
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
                fontSize: 14,
                fontWeight: 650,
                fontVariantNumeric: "tabular-nums",
                letterSpacing: "-0.01em",
                whiteSpace: "nowrap",
              }}
            >
              {formatClock(elapsed)}
            </span>
            <button
              {...btnProps}
              onClick={handleStop}
              style={compactStopBtn}
              title="Stop the meeting and generate the report"
            >
              ■
            </button>
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
            <span style={{ fontSize: 12, color: "#a1a1aa", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
              {words > 0 ? `${words} words` : "listening…"}
            </span>
            <span style={{ flex: 1 }} />
            <button
              {...btnProps}
              onClick={(e) => {
                e.stopPropagation();
                setMinimized(true);
              }}
              style={minBtn}
              title="Minimize — click the pill to expand"
            >
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
