import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { confirmDialog } from "../lib/dialogs";
import { errorMessage } from "../lib/errors";

/** Wire shape of `ReportRef` (commands/meeting.rs, camelCase). */
export interface MeetingReport {
  id: string;
  title: string;
  reportPath: string;
  audioPath: string;
  durationMs: number;
  wordCount: number;
  segmentCount: number;
  speakers: string[];
  hasSummary: boolean;
}

/** Wire shape of `ReportData` — the finished meeting's full report content. */
export interface ReportSegmentRow {
  startMs: number;
  endMs: number;
  speaker: string | null;
  text: string;
}

export interface ReportData {
  id: string;
  title: string;
  startedAt: string;
  reportPath: string;
  audioPath: string;
  durationMs: number;
  wordCount: number;
  wpm: number;
  segmentCount: number;
  speakers: string[];
  /** LLM overview (what the meeting was about); null when no model ran. */
  summary: string | null;
  actionItems: string[];
  segments: ReportSegmentRow[];
}

export interface MeetingStatus {
  phase: "idle" | "recording" | "finalizing" | "done";
  elapsedMs: number | null;
  /** Real-time transcript so far (committed segments), while recording. */
  liveText: string | null;
  /** Live stats while recording (committed so far). */
  liveWordCount: number | null;
  liveSegmentCount: number | null;
  report: MeetingReport | null;
  error: string | null;
}

function formatClock(ms: number): string {
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

const card: React.CSSProperties = {
  background: "var(--surface)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius)",
  padding: 20,
  display: "flex",
  flexDirection: "column",
  gap: 12,
};

const primaryBtn: React.CSSProperties = {
  background: "var(--accent)",
  color: "white",
  border: "none",
  borderRadius: 8,
  padding: "10px 18px",
  fontSize: 14,
  fontWeight: 600,
  cursor: "pointer",
};

const ghostBtn: React.CSSProperties = {
  background: "transparent",
  color: "var(--text-secondary)",
  border: "1px solid var(--border)",
  borderRadius: 8,
  padding: "10px 18px",
  fontSize: 14,
  cursor: "pointer",
};

export default function MeetingScreen() {
  const [status, setStatus] = useState<MeetingStatus>({
    phase: "idle",
    elapsedMs: null,
    liveText: null,
    liveWordCount: null,
    liveSegmentCount: null,
    report: null,
    error: null,
  });
  const [reportData, setReportData] = useState<ReportData | null>(null);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const pollRef = useRef<number | null>(null);
  // The backend only reports elapsedMs while we poll (i.e. after stop), so
  // during recording we tick a local clock from when we entered the phase.
  const [, setTick] = useState(0);
  const recordingSinceRef = useRef<number | null>(null);

  useEffect(() => {
    if (status.phase !== "recording") {
      recordingSinceRef.current = null;
      return;
    }
    if (recordingSinceRef.current === null) {
      recordingSinceRef.current = Date.now();
    }
    const iv = window.setInterval(() => setTick((t) => t + 1), 1000);
    return () => window.clearInterval(iv);
  }, [status.phase]);

  const stopPolling = useCallback(() => {
    if (pollRef.current !== null) {
      window.clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  const startPolling = useCallback(() => {
    stopPolling();
    pollRef.current = window.setInterval(async () => {
      try {
        const s: MeetingStatus = await invoke("meeting_status");
        setStatus(s);
        // Keep polling while recording (live transcript) and finalizing;
        // stop once the meeting settles (done or failed → idle).
        if (s.phase === "done" || (s.phase === "idle" && s.error)) {
          stopPolling();
        }
      } catch {
        // Status fetch failed; keep polling.
      }
    }, 1000);
  }, [stopPolling]);

  // On mount: adopt the current phase (a meeting may already be running).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const s: MeetingStatus = await invoke("meeting_status");
        if (!cancelled) {
          setStatus(s);
          if (s.phase === "recording" || s.phase === "finalizing") startPolling();
        }
      } catch {
        // Not in a Tauri context (plain browser dev); stay idle.
      }
    })();
    return () => {
      cancelled = true;
      stopPolling();
    };
  }, [startPolling, stopPolling]);

  // When the meeting settles into `done`, fetch the full report content
  // once so it renders on this same page (overview + details + transcript).
  useEffect(() => {
    if (status.phase !== "done" || !status.report) {
      setReportData(null);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const data: ReportData = await invoke("meeting_report");
        if (!cancelled) setReportData(data);
      } catch {
        // The report file still exists; the buttons keep working.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [status.phase, status.report?.id]);

  const handleStart = useCallback(async () => {
    setBusy(true);
    try {
      const s: MeetingStatus = await invoke("meeting_start", {
        title: title.trim() || null,
        deviceId: null,
      });
      setStatus(s);
      // Poll for the live transcript while recording.
      startPolling();
    } catch (e) {
      alert(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [title, startPolling]);

  const handleStop = useCallback(async () => {
    setBusy(true);
    try {
      const s: MeetingStatus = await invoke("meeting_stop");
      setStatus(s);
      startPolling();
    } catch (e) {
      alert(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [startPolling]);

  const handleCancel = useCallback(async () => {
    const ok = await confirmDialog(
      "Stop recording and discard the audio? This cannot be undone.",
      { title: "Cancel meeting", kind: "error" }
    );
    if (!ok) return;
    setBusy(true);
    try {
      const s: MeetingStatus = await invoke("meeting_cancel");
      setStatus(s);
    } catch (e) {
      alert(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const handleReset = useCallback(async () => {
    try {
      const s: MeetingStatus = await invoke("meeting_reset");
      setStatus(s);
      setReportData(null);
      setTitle("");
    } catch (e) {
      alert(errorMessage(e));
    }
  }, []);

  const handleOpenReport = useCallback(async () => {
    try {
      await invoke("meeting_open_report");
    } catch (e) {
      alert(errorMessage(e));
    }
  }, []);

  const { phase } = status;
  const elapsed =
    status.elapsedMs ??
    (recordingSinceRef.current !== null
      ? Date.now() - recordingSinceRef.current
      : 0);

  return (
    <div style={{ maxWidth: 640, margin: "0 auto", display: "flex", flexDirection: "column", gap: 16 }}>
      <div>
        <h2 style={{ margin: 0, fontSize: 20 }}>Meeting</h2>
        <p style={{ margin: "6px 0 0", color: "var(--text-secondary)", fontSize: 14 }}>
          Record a meeting in real time — the transcript appears as people speak — then stop to
          generate a detailed report: overview, action items, stats, and the full transcript.
        </p>
      </div>

      {/* Idle */}
      {phase === "idle" && (
        <div style={card}>
          {status.error && (
            <div
              style={{
                background: "rgba(200, 60, 60, 0.12)",
                border: "1px solid rgba(200, 60, 60, 0.4)",
                borderRadius: 8,
                padding: "10px 12px",
                fontSize: 13,
                color: "var(--text)",
              }}
            >
              {status.error}
            </div>
          )}
          <label style={{ display: "flex", flexDirection: "column", gap: 6, fontSize: 13, color: "var(--text-secondary)" }}>
            Title
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Product sync"
              style={{
                background: "var(--bg)",
                border: "1px solid var(--border)",
                borderRadius: 8,
                padding: "8px 10px",
                fontSize: 14,
                color: "var(--text)",
              }}
            />
          </label>
          <button
            onClick={handleStart}
            disabled={busy}
            style={{ ...primaryBtn, opacity: busy ? 0.6 : 1, display: "flex", alignItems: "center", justifyContent: "center", gap: 8 }}
          >
            <Icon name="mic" size={16} />
            Start recording
          </button>
        </div>
      )}

      {/* Recording */}
      {phase === "recording" && (
        <div
          style={{
            ...card,
            padding: 24,
            alignItems: "center",
            gap: 16,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span
              style={{
                width: 12,
                height: 12,
                borderRadius: "50%",
                background: "#e5484d",
                animation: "meeting-pulse 1.2s ease-in-out infinite",
              }}
            />
            <span style={{ fontSize: 32, fontVariantNumeric: "tabular-nums", fontWeight: 600 }}>
              {formatClock(elapsed)}
            </span>
          </div>
          {/* Live stats: committed words / segments so far. */}
          <div style={{ fontSize: 13, color: "var(--text-secondary)", fontVariantNumeric: "tabular-nums" }}>
            {status.liveWordCount !== null && status.liveWordCount > 0
              ? `${status.liveWordCount} words · ${status.liveSegmentCount ?? 0} segments`
              : "Waiting for speech…"}
          </div>
          <div style={{ display: "flex", gap: 10 }}>
            <button onClick={handleStop} disabled={busy} style={{ ...primaryBtn, opacity: busy ? 0.6 : 1 }}>
              Stop &amp; generate report
            </button>
            <button onClick={handleCancel} disabled={busy} style={{ ...ghostBtn, opacity: busy ? 0.6 : 1 }}>
              Cancel
            </button>
          </div>
          {/* Live transcript: committed segments as they decode. */}
          <div
            style={{
              width: "100%",
              minHeight: 120,
              maxHeight: 320,
              overflowY: "auto",
              background: "var(--bg)",
              border: "1px solid var(--border)",
              borderRadius: 8,
              padding: "12px 14px",
              fontSize: 14,
              lineHeight: 1.6,
              color: "var(--text)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
            }}
          >
            {status.liveText ? (
              status.liveText
            ) : (
              <span style={{ color: "var(--text-secondary)", fontStyle: "italic" }}>
                Listening… transcribed words will appear here as people speak.
              </span>
            )}
          </div>
        </div>
      )}

      {/* Finalizing */}
      {phase === "finalizing" && (
        <div style={{ ...card, padding: 24, alignItems: "center", gap: 12 }}>
          <div
            style={{
              width: 28,
              height: 28,
              border: "3px solid var(--border)",
              borderTopColor: "var(--accent)",
              borderRadius: "50%",
              animation: "spin 0.8s linear infinite",
            }}
          />
          <div style={{ fontSize: 14, color: "var(--text-secondary)" }}>
            Building your report — summary, details, and transcript…
          </div>
        </div>
      )}

      {/* Done: the report rendered on this same page. */}
      {phase === "done" && status.report && (
        <div style={{ ...card, padding: 24, gap: 16 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <Icon name="calendar-check" size={20} />
            <div>
              <div style={{ fontSize: 16, fontWeight: 600 }}>{status.report.title}</div>
              <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
                {reportData ? reportData.startedAt : ""}
              </div>
            </div>
          </div>

          {reportData && (
            <>
              {/* Overview: what the meeting was about. */}
              <div
                style={{
                  background: "var(--bg)",
                  border: "1px solid var(--border)",
                  borderRadius: 8,
                  padding: "12px 14px",
                  fontSize: 14,
                  lineHeight: 1.6,
                }}
              >
                <div
                  style={{
                    fontSize: 12,
                    textTransform: "uppercase",
                    letterSpacing: "0.06em",
                    color: "var(--text-secondary)",
                    marginBottom: 6,
                    fontWeight: 600,
                  }}
                >
                  Overview
                </div>
                {reportData.summary ? (
                  <div>{reportData.summary}</div>
                ) : (
                  <div style={{ color: "var(--text-secondary)", fontStyle: "italic" }}>
                    No AI summary — no local language model is loaded. The transcript and stats
                    below are complete.
                  </div>
                )}
                {reportData.actionItems.length > 0 && (
                  <div style={{ marginTop: 10 }}>
                    <div
                      style={{
                        fontSize: 12,
                        textTransform: "uppercase",
                        letterSpacing: "0.06em",
                        color: "var(--text-secondary)",
                        marginBottom: 6,
                        fontWeight: 600,
                      }}
                    >
                      Action items
                    </div>
                    <ul style={{ margin: 0, paddingLeft: 18 }}>
                      {reportData.actionItems.map((item, i) => (
                        <li key={i} style={{ marginBottom: 4 }}>
                          {item}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>

              {/* Details: the meeting's measurable facts. */}
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(4, 1fr)",
                  gap: 10,
                }}
              >
                {[
                  ["Duration", formatClock(reportData.durationMs)],
                  ["Words", String(reportData.wordCount)],
                  ["WPM", String(reportData.wpm)],
                  ["Segments", String(reportData.segmentCount)],
                ].map(([label, value]) => (
                  <div
                    key={label}
                    style={{
                      background: "var(--bg)",
                      border: "1px solid var(--border)",
                      borderRadius: 8,
                      padding: "10px 12px",
                      textAlign: "center",
                    }}
                  >
                    <div
                      style={{
                        fontSize: 11,
                        textTransform: "uppercase",
                        letterSpacing: "0.06em",
                        color: "var(--text-secondary)",
                      }}
                    >
                      {label}
                    </div>
                    <div style={{ fontSize: 18, fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>
                      {value}
                    </div>
                  </div>
                ))}
              </div>
              {reportData.speakers.length > 0 && (
                <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
                  Speakers: {reportData.speakers.join(", ")}
                </div>
              )}

              {/* Full transcript with speaker labels + timestamps. */}
              <div
                style={{
                  maxHeight: 360,
                  overflowY: "auto",
                  background: "var(--bg)",
                  border: "1px solid var(--border)",
                  borderRadius: 8,
                  padding: "10px 14px",
                }}
              >
                {reportData.segments.length === 0 ? (
                  <div style={{ color: "var(--text-secondary)", fontStyle: "italic", fontSize: 14 }}>
                    No speech was captured.
                  </div>
                ) : (
                  reportData.segments.map((seg, i) => (
                    <div
                      key={i}
                      style={{
                        display: "flex",
                        gap: 12,
                        padding: "7px 0",
                        borderBottom: i < reportData.segments.length - 1 ? "1px solid var(--border)" : "none",
                      }}
                    >
                      <span
                        style={{
                          flex: "0 0 52px",
                          color: "var(--text-secondary)",
                          fontVariantNumeric: "tabular-nums",
                          fontSize: 13,
                          paddingTop: 1,
                        }}
                      >
                        {formatClock(seg.startMs)}
                      </span>
                      {seg.speaker && (
                        <span style={{ flex: "0 0 90px", fontWeight: 600, fontSize: 14 }}>
                          {seg.speaker}
                        </span>
                      )}
                      <span style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", fontSize: 14 }}>
                        {seg.text}
                      </span>
                    </div>
                  ))
                )}
              </div>
            </>
          )}

          <div style={{ display: "flex", gap: 10 }}>
            <button onClick={handleOpenReport} style={primaryBtn}>
              Open in new tab
            </button>
            <button onClick={handleReset} style={ghostBtn}>
              New meeting
            </button>
          </div>
        </div>
      )}

      <style>{`
        @keyframes meeting-pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.35; }
        }
        @keyframes spin {
          to { transform: rotate(360deg); }
        }
      `}</style>
    </div>
  );
}
