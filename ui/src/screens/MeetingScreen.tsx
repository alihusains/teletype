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

export interface RubricCriterion {
  id: string;
  name: string;
  description: string;
}

export interface Rubric {
  id: string;
  name: string;
  description: string;
  criteria: RubricCriterion[];
}

export interface CriterionScore {
  criterion: string;
  score: number;
  note: string;
}

export interface RubricScore {
  overall: number;
  scores: CriterionScore[];
  strengths: string[];
  improvements: string[];
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
  /** The rubric selected at start; null when none was picked. */
  rubric: Rubric | null;
  /** The LLM's score against the rubric; null when unscored. */
  rubricScores: RubricScore | null;
  segments: ReportSegmentRow[];
}

function scoreColor(score: number): string {
  if (score >= 7) return "#0b6e4f";
  if (score >= 4) return "#b45309";
  return "#b3261e";
}

function formatScore(score: number): string {
  const s = Math.round(Math.min(10, Math.max(0, score)) * 10) / 10;
  return Number.isInteger(s) ? String(s) : s.toFixed(1);
}

// Speaker palette — mirrors the report's SPEAKER_COLORS so a speaker keeps
// the same color in the app and in the HTML report.
const SPEAKER_COLORS = [
  "#0b6e4f",
  "#8a3ffc",
  "#b3261e",
  "#1256a3",
  "#8a6d00",
  "#a3326b",
  "#0f766e",
  "#57534e",
] as const;
const SPEAKER_TINTS = [
  "#e6f2ec",
  "#f1e9fd",
  "#f9e9e8",
  "#e8eff8",
  "#f5f0dd",
  "#f8e9f1",
  "#e6f2f1",
  "#efedeb",
] as const;

function speakerColor(name: string, speakers: string[]): string {
  const idx = Math.max(0, speakers.indexOf(name));
  return SPEAKER_COLORS[idx % SPEAKER_COLORS.length];
}

function speakerTint(name: string, speakers: string[]): string {
  const idx = Math.max(0, speakers.indexOf(name));
  return SPEAKER_TINTS[idx % SPEAKER_TINTS.length];
}

function speakerInitial(name: string): string {
  const rest = name.startsWith("Speaker ") ? name.slice(8) : name;
  return (rest.trim()[0] ?? "?").toUpperCase();
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

// Letters design system (ui/index.html tokens): monochrome obsidian actions,
// 18px cards, fully-rounded pills, display font for the timer/headings.
const card: React.CSSProperties = {
  background: "var(--surface)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius)",
  padding: 22,
  display: "flex",
  flexDirection: "column",
  gap: 14,
};

const actionBtn: React.CSSProperties = {
  background: "var(--action)",
  color: "var(--on-action)",
  border: "none",
  borderRadius: 100,
  padding: "11px 22px",
  fontSize: 14,
  fontWeight: 600,
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 8,
};

const ghostBtn: React.CSSProperties = {
  background: "transparent",
  color: "var(--text)",
  border: "1px solid var(--border-strong)",
  borderRadius: 100,
  padding: "10px 20px",
  fontSize: 14,
  fontWeight: 550,
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  gap: 8,
};

const chip: React.CSSProperties = {
  background: "var(--surface-2)",
  border: "1px solid var(--border-subtle)",
  borderRadius: 100,
  padding: "5px 13px",
  fontSize: 12.5,
  fontWeight: 550,
  color: "var(--text-secondary)",
  fontVariantNumeric: "tabular-nums",
  whiteSpace: "nowrap",
};

const sectionLabel: React.CSSProperties = {
  fontSize: 11,
  textTransform: "uppercase",
  letterSpacing: "0.09em",
  color: "var(--text-secondary)",
  fontWeight: 700,
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
  const [rubrics, setRubrics] = useState<Rubric[]>([]);
  const [rubricId, setRubricId] = useState<string>("");
  const [diarInstalled, setDiarInstalled] = useState<boolean | null>(null);
  const [diarProgress, setDiarProgress] = useState<number | null>(null);
  const [diarError, setDiarError] = useState<string | null>(null);
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

  // The built-in scoring rubrics, for the select at start.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r: Rubric[] = await invoke("meeting_rubrics");
        if (!cancelled) setRubrics(r);
      } catch {
        // Not in a Tauri context (plain browser dev); no rubric select.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Diarization models (optional): installed status + download progress.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const s: { installed: boolean } = await invoke("diarization_status");
        if (!cancelled) setDiarInstalled(s.installed);
      } catch {
        // Not in a Tauri context; hide the row.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (diarProgress === null) return;
    let unlisten: (() => void) | undefined;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<{ kind: string; percent: number; status: string; error: string | null }>(
          "model-download-progress",
          (e) => {
            if (e.payload.kind !== "diarization") return;
            if (e.payload.status === "error") {
              setDiarError(e.payload.error ?? "download failed");
              setDiarProgress(null);
            } else if (e.payload.status === "done") {
              setDiarProgress(null);
              setDiarInstalled(true);
              setDiarError(null);
            } else {
              setDiarProgress(e.payload.percent);
            }
          },
        );
      } catch {
        // Not in a Tauri context; progress is best-effort.
      }
    })();
    return () => unlisten?.();
  }, [diarProgress === null]);

  const handleDownloadDiarization = useCallback(async () => {
    setDiarError(null);
    setDiarProgress(0);
    try {
      await invoke("download_diarization_models");
      // "done" progress event flips the installed flag.
    } catch (e) {
      setDiarProgress(null);
      setDiarError(errorMessage(e));
    }
  }, []);

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
        rubricId: rubricId || null,
      });
      setStatus(s);
      // Poll for the live transcript while recording.
      startPolling();
    } catch (e) {
      alert(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [title, rubricId, startPolling]);

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
    <div style={{ maxWidth: 680, margin: "0 auto", display: "flex", flexDirection: "column", gap: 16 }}>
      <div>
        <h2
          style={{
            margin: 0,
            fontSize: 24,
            fontWeight: 650,
            letterSpacing: "-0.02em",
            fontFamily: "var(--font-display)",
          }}
        >
          Meeting
        </h2>
        <p style={{ margin: "6px 0 0", color: "var(--text-secondary)", fontSize: 14 }}>
          Record a meeting in real time — the transcript appears as people speak. A floating
          island keeps you posted from anywhere, and the report can be opened while it's
          still running.
        </p>
      </div>

      {/* Idle */}
      {phase === "idle" && (
        <div style={card}>
          {status.error && (
            <div
              style={{
                background: "var(--danger-soft)",
                border: "1px solid rgba(217, 45, 32, 0.35)",
                borderRadius: 12,
                padding: "10px 14px",
                fontSize: 13,
                color: "var(--text)",
              }}
            >
              {status.error}
            </div>
          )}
          <label
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 7,
              fontSize: 13,
              fontWeight: 550,
              color: "var(--text-secondary)",
            }}
          >
            Title
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Product sync"
              style={{
                background: "var(--bg)",
                border: "1px solid var(--border)",
                borderRadius: 12,
                padding: "10px 12px",
                fontSize: 14,
                color: "var(--text)",
              }}
            />
          </label>
          {rubrics.length > 0 && (
            <label
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 7,
                fontSize: 13,
                fontWeight: 550,
                color: "var(--text-secondary)",
              }}
            >
              Rubric — how the meeting gets scored
              <select
                value={rubricId}
                onChange={(e) => setRubricId(e.target.value)}
                style={{
                  background: "var(--bg)",
                  border: "1px solid var(--border)",
                  borderRadius: 12,
                  padding: "10px 12px",
                  fontSize: 14,
                  color: "var(--text)",
                }}
              >
                <option value="">No rubric</option>
                {rubrics.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name} — {r.description}
                  </option>
                ))}
              </select>
            </label>
          )}
          {diarInstalled !== null && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                flexWrap: "wrap",
                fontSize: 13,
              }}
            >
              <span style={{ color: "var(--text-secondary)", fontWeight: 550 }}>
                Speaker diarization — separate who said what
              </span>
              {diarInstalled ? (
                <span
                  style={{
                    background: "var(--surface-2)",
                    border: "1px solid var(--border-subtle)",
                    borderRadius: 100,
                    padding: "3px 11px",
                    fontSize: 12,
                    fontWeight: 650,
                    color: "var(--text-secondary)",
                  }}
                >
                  Installed
                </span>
              ) : diarProgress !== null ? (
                <span
                  style={{
                    color: "var(--text-secondary)",
                    fontVariantNumeric: "tabular-nums",
                    fontSize: 12.5,
                  }}
                >
                  Downloading… {Math.round(diarProgress)}%
                </span>
              ) : (
                <button
                  type="button"
                  onClick={handleDownloadDiarization}
                  style={{
                    background: "transparent",
                    border: "1px solid var(--border)",
                    borderRadius: 100,
                    padding: "4px 13px",
                    fontSize: 12.5,
                    fontWeight: 600,
                    color: "var(--text)",
                    cursor: "pointer",
                  }}
                >
                  Download (34 MB)
                </button>
              )}
              {diarError && (
                <span style={{ color: "var(--danger)", fontSize: 12.5 }}>{diarError}</span>
              )}
            </div>
          )}
          <button
            onClick={handleStart}
            disabled={busy}
            style={{ ...actionBtn, opacity: busy ? 0.6 : 1, alignSelf: "flex-start" }}
          >
            <Icon name="mic" size={16} />
            Start recording
          </button>
          <p style={{ margin: 0, fontSize: 12.5, color: "var(--text-tertiary)" }}>
            The meeting island appears at the top of your screen while recording — stop or open
            the live report from there, or right here.
          </p>
        </div>
      )}

      {/* Recording */}
      {phase === "recording" && (
        <div style={{ ...card, padding: 24, gap: 18 }}>
          {/* Timer row */}
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <span
              style={{
                width: 10,
                height: 10,
                borderRadius: "50%",
                background: "var(--danger)",
                animation: "meeting-pulse 1.2s ease-in-out infinite",
              }}
            />
            <span
              style={{
                fontSize: 11,
                fontWeight: 700,
                letterSpacing: "0.1em",
                color: "var(--danger)",
              }}
            >
              REC
            </span>
            <span
              style={{
                fontSize: 34,
                fontVariantNumeric: "tabular-nums",
                fontWeight: 650,
                letterSpacing: "-0.02em",
                fontFamily: "var(--font-display)",
              }}
            >
              {formatClock(elapsed)}
            </span>
          </div>

          {/* Live stats chips */}
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <span style={chip}>
              {status.liveWordCount !== null && status.liveWordCount > 0
                ? `${status.liveWordCount} words`
                : "listening…"}
            </span>
            <span style={chip}>{status.liveSegmentCount ?? 0} segments</span>
            <span style={chip}>live report updating</span>
          </div>

          {/* Live transcript — the visual center */}
          <div
            style={{
              width: "100%",
              minHeight: 140,
              maxHeight: 320,
              overflowY: "auto",
              background: "var(--surface-2)",
              border: "1px solid var(--border-subtle)",
              borderRadius: 14,
              padding: "14px 16px",
              fontSize: 14.5,
              lineHeight: 1.65,
              color: "var(--text)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
            }}
          >
            {status.liveText ? (
              status.liveText
            ) : (
              <span style={{ color: "var(--text-tertiary)", fontStyle: "italic" }}>
                Listening… transcribed words will appear here as people speak.
              </span>
            )}
          </div>

          {/* Actions */}
          <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            <button type="button" onClick={handleStop} disabled={busy} style={{ ...actionBtn, opacity: busy ? 0.6 : 1 }}>
              Stop &amp; generate report
            </button>
            <button type="button" onClick={handleOpenReport} style={ghostBtn} title="Open the live report in a new browser tab — it updates while the meeting runs">
              Open live report
            </button>
            <button type="button" onClick={handleCancel} disabled={busy} style={{ ...ghostBtn, color: "var(--text-secondary)", opacity: busy ? 0.6 : 1 }}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Finalizing */}
      {phase === "finalizing" && (
        <div style={{ ...card, padding: 28, alignItems: "center", gap: 14 }}>
          <div
            style={{
              width: 30,
              height: 30,
              border: "3px solid var(--border)",
              borderTopColor: "var(--action)",
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
        <div style={{ ...card, padding: 24, gap: 18 }}>
          <div>
            <div
              style={{
                fontSize: 22,
                fontWeight: 650,
                letterSpacing: "-0.02em",
                fontFamily: "var(--font-display)",
              }}
            >
              {status.report.title}
            </div>
            {reportData && (
              <div style={{ fontSize: 13, color: "var(--text-secondary)", marginTop: 4 }}>
                {reportData.startedAt}
              </div>
            )}
          </div>

          {reportData && (
            <>
              {/* Overview: what the meeting was about. */}
              <div
                style={{
                  background: "var(--surface-2)",
                  border: "1px solid var(--border-subtle)",
                  borderLeft: "3px solid #702fd2",
                  borderRadius: 14,
                  padding: "14px 16px",
                }}
              >
                <div style={{ ...sectionLabel, marginBottom: 8 }}>Overview</div>
                {reportData.summary ? (
                  <div style={{ fontSize: 14.5, lineHeight: 1.6 }}>{reportData.summary}</div>
                ) : (
                  <div style={{ color: "var(--text-secondary)", fontStyle: "italic", fontSize: 14 }}>
                    No AI summary — no local language model is loaded. The transcript and stats
                    below are complete.
                  </div>
                )}
                {reportData.actionItems.length > 0 && (
                  <div style={{ marginTop: 12 }}>
                    <div style={{ ...sectionLabel, marginBottom: 6 }}>Action items</div>
                    <ul style={{ margin: 0, paddingLeft: 18, fontSize: 14 }}>
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
              <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 10 }}>
                {[
                  ["Duration", formatClock(reportData.durationMs)],
                  ["Words", String(reportData.wordCount)],
                  ["WPM", String(reportData.wpm)],
                  ["Segments", String(reportData.segmentCount)],
                ].map(([label, value]) => (
                  <div
                    key={label}
                    style={{
                      background: "var(--surface-2)",
                      border: "1px solid var(--border-subtle)",
                      borderRadius: 14,
                      padding: "12px 14px",
                      textAlign: "center",
                    }}
                  >
                    <div style={{ fontSize: 10.5, textTransform: "uppercase", letterSpacing: "0.08em", color: "var(--text-secondary)", fontWeight: 700 }}>
                      {label}
                    </div>
                    <div
                      style={{
                        fontSize: 20,
                        fontWeight: 650,
                        fontVariantNumeric: "tabular-nums",
                        fontFamily: "var(--font-display)",
                        marginTop: 2,
                      }}
                    >
                      {value}
                    </div>
                  </div>
                ))}
              </div>
              {reportData.speakers.length > 0 && (
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
                  {reportData.speakers.map((sp) => (
                    <span
                      key={sp}
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 7,
                        background: "var(--surface-2)",
                        border: "1px solid var(--border-subtle)",
                        borderRadius: 100,
                        padding: "4px 12px 4px 6px",
                        fontSize: 13,
                        fontWeight: 550,
                      }}
                    >
                      <span
                        style={{
                          width: 20,
                          height: 20,
                          borderRadius: "50%",
                          background: speakerTint(sp, reportData.speakers),
                          color: speakerColor(sp, reportData.speakers),
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          fontSize: 10.5,
                          fontWeight: 700,
                        }}
                      >
                        {speakerInitial(sp)}
                      </span>
                      {sp}
                    </span>
                  ))}
                </div>
              )}

              {/* Score: the meeting judged against the selected rubric. */}
              {reportData.rubric && (
                <div
                  style={{
                    background: "var(--surface-2)",
                    border: "1px solid var(--border-subtle)",
                    borderLeft: "3px solid #b45309",
                    borderRadius: 14,
                    padding: "14px 16px",
                  }}
                >
                  <div style={{ ...sectionLabel, marginBottom: 10 }}>
                    Score — {reportData.rubric.name}
                  </div>
                  {reportData.rubricScores ? (
                    <>
                      <div style={{ display: "flex", alignItems: "baseline", gap: 8, marginBottom: 14 }}>
                        <span
                          style={{
                            fontSize: 36,
                            fontWeight: 700,
                            letterSpacing: "-0.03em",
                            fontVariantNumeric: "tabular-nums",
                            fontFamily: "var(--font-display)",
                            color: scoreColor(reportData.rubricScores.overall),
                          }}
                        >
                          {formatScore(reportData.rubricScores.overall)}
                        </span>
                        <span style={{ fontSize: 12.5, color: "var(--text-secondary)", fontWeight: 550 }}>
                          out of 10
                        </span>
                      </div>
                      {reportData.rubricScores.scores.map((cs) => {
                        const crit = reportData!.rubric!.criteria.find((c) => c.id === cs.criterion);
                        return (
                          <div key={cs.criterion} style={{ marginBottom: 12 }}>
                            <div style={{ display: "flex", justifyContent: "space-between", gap: 10, marginBottom: 4 }}>
                              <span style={{ fontSize: 13.5, fontWeight: 600 }}>
                                {crit?.name ?? cs.criterion}
                              </span>
                              <span
                                style={{
                                  fontSize: 12.5,
                                  fontWeight: 700,
                                  fontVariantNumeric: "tabular-nums",
                                  color: scoreColor(cs.score),
                                }}
                              >
                                {formatScore(cs.score)}/10
                              </span>
                            </div>
                            <div
                              style={{
                                height: 6,
                                borderRadius: 100,
                                background: "var(--border-subtle)",
                                overflow: "hidden",
                              }}
                            >
                              <div
                                style={{
                                  height: "100%",
                                  borderRadius: 100,
                                  width: `${Math.round(Math.min(10, Math.max(0, cs.score)) * 10)}%`,
                                  background: scoreColor(cs.score),
                                }}
                              />
                            </div>
                            {cs.note && (
                              <div style={{ fontSize: 12.5, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
                                {cs.note}
                              </div>
                            )}
                          </div>
                        );
                      })}
                      {reportData.rubricScores.strengths.length > 0 && (
                        <div style={{ marginTop: 10 }}>
                          <div style={{ ...sectionLabel, marginBottom: 6 }}>Strengths</div>
                          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 13.5 }}>
                            {reportData.rubricScores.strengths.map((x, i) => (
                              <li key={i} style={{ marginBottom: 4 }}>{x}</li>
                            ))}
                          </ul>
                        </div>
                      )}
                      {reportData.rubricScores.improvements.length > 0 && (
                        <div style={{ marginTop: 10 }}>
                          <div style={{ ...sectionLabel, marginBottom: 6 }}>Improvements</div>
                          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 13.5 }}>
                            {reportData.rubricScores.improvements.map((x, i) => (
                              <li key={i} style={{ marginBottom: 4 }}>{x}</li>
                            ))}
                          </ul>
                        </div>
                      )}
                    </>
                  ) : (
                    <div style={{ fontSize: 13.5, color: "var(--text-secondary)", lineHeight: 1.6 }}>
                      <em>Not scored — no local language model is loaded.</em>
                      <ul style={{ margin: "8px 0 0", paddingLeft: 18, fontSize: 13 }}>
                        {reportData.rubric.criteria.map((c) => (
                          <li key={c.id} style={{ marginBottom: 4 }}>
                            <b>{c.name}</b> — {c.description}
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </div>
              )}

              {/* Full transcript with speaker labels + timestamps. */}
              <div
                style={{
                  maxHeight: 360,
                  overflowY: "auto",
                  background: "var(--surface-2)",
                  border: "1px solid var(--border-subtle)",
                  borderRadius: 14,
                  padding: "10px 16px",
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
                        padding: "8px 0",
                        borderBottom:
                          i < reportData.segments.length - 1 ? "1px solid var(--border-subtle)" : "none",
                      }}
                    >
                      <span
                        style={{
                          flex: "0 0 52px",
                          color: "var(--text-tertiary)",
                          fontVariantNumeric: "tabular-nums",
                          fontSize: 12.5,
                          paddingTop: 2,
                        }}
                      >
                        {formatClock(seg.startMs)}
                      </span>
                      {seg.speaker && (
                        <>
                          <span
                            style={{
                              flex: "0 0 28px",
                              width: 28,
                              height: 28,
                              borderRadius: "50%",
                              background: speakerTint(seg.speaker, reportData.speakers),
                              color: speakerColor(seg.speaker, reportData.speakers),
                              display: "inline-flex",
                              alignItems: "center",
                              justifyContent: "center",
                              fontSize: 11.5,
                              fontWeight: 700,
                              marginTop: 2,
                            }}
                          >
                            {speakerInitial(seg.speaker)}
                          </span>
                          <span
                            style={{
                              flex: "0 0 92px",
                              fontWeight: 650,
                              fontSize: 14,
                              color: speakerColor(seg.speaker, reportData.speakers),
                            }}
                          >
                            {seg.speaker}
                          </span>
                        </>
                      )}
                      <span
                        style={{
                          flex: 1,
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-word",
                          fontSize: 14,
                          lineHeight: 1.6,
                        }}
                      >
                        {seg.text}
                      </span>
                    </div>
                  ))
                )}
              </div>
            </>
          )}

          <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            <button type="button" onClick={handleOpenReport} style={actionBtn}>
              Open report in new tab
            </button>
            <button type="button" onClick={handleReset} style={ghostBtn}>
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
