import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "../lib/useTauriEvent";

const POLL_MS = 500;

interface LogEntry {
  level: "Info" | "Success" | "Warn" | "Error";
  message: string;
  durationMs?: number;
}

const MONO = "ui-monospace, SFMono-Regular, Menlo, monospace";

// Status-token palette for the log surface.
const LEVEL_COLORS: Record<LogEntry["level"], string> = {
  Info: "var(--text-secondary)",
  Success: "var(--success)",
  Warn: "var(--warning)",
  Error: "var(--danger)",
};

// Compact badge labels shown before each log line.
const LEVEL_BADGES: Record<LogEntry["level"], string> = {
  Info: "INFO",
  Success: "OK",
  Warn: "WARN",
  Error: "ERR",
};

// Duration token: success (<500ms), warning (500–2000ms), danger (>2000ms).
function durationColor(ms: number): string {
  if (ms < 500) return "var(--success)";
  if (ms <= 2000) return "var(--warning)";
  return "var(--danger)";
}

interface S1Control {
  styling: string;
  structure: string;
  context: string;
}

// The 8 wire tokens the S1-mini model card trains on (mirrors as_wire()
// in teletype-core/src/transforms/prompt.rs). The readout shows which one
// is active per axis so a garbled S1 output can be traced to a stale or
// off-set control value.
const S1_WIRE_TOKENS: { axis: "styling" | "structure" | "context"; label: string; tokens: string[] }[] = [
  { axis: "styling", label: "Styling", tokens: ["casual", "semi-casual", "semi-formal", "formal"] },
  { axis: "structure", label: "Structure", tokens: ["prose", "lists"] },
  { axis: "context", label: "Context", tokens: ["general", "email"] },
];

// The active wire token per axis, from the profile's S1Control (the UI
// stores the wire strings, matching set_s1_control's IPC input).
function s1ValueFor(axis: "styling" | "structure" | "context", s1: S1Control): string {
  return s1[axis];
}

// P0.2 dictation-trace panel: per-stage timing of recent takes, backed by
// the trace JSONL store (`get_dictation_traces`, `export_dictation_traces`).
interface DictationTrace {
  session: number;
  engine: string;
  audio_ms: number;
  t_hotkey_down?: number | null;
  t_first_audio?: number | null;
  t_hotkey_up?: number | null;
  t_asr_start?: number | null;
  t_asr_end?: number | null;
  t_text_pipeline_end?: number | null;
  t_polish_start?: number | null;
  t_polish_first_token?: number | null;
  t_polish_end?: number | null;
  t_paste_sent?: number | null;
  polish_skipped_reason?: string | null;
  committed_segments: number;
  tail_audio_ms: number;
  errors: string[];
}

// Microseconds → milliseconds for display; null when a mark is missing.
function markMs(t: DictationTrace, key: keyof DictationTrace): number | null {
  const v = t[key];
  return typeof v === "number" ? v / 1000 : null;
}

// Duration between two stage marks in ms, or null when either is missing.
function stageMs(
  t: DictationTrace,
  from: keyof DictationTrace,
  to: keyof DictationTrace
): number | null {
  const a = markMs(t, from);
  const b = markMs(t, to);
  if (a == null || b == null || b < a) return null;
  return b - a;
}

function percentile(sorted: number[], p: number): number | null {
  if (!sorted.length) return null;
  const i = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, i)];
}

function median(values: number[]): number | null {
  if (!values.length) return null;
  const s = [...values].sort((a, b) => a - b);
  return percentile(s, 50);
}

function fmtMs(v: number | null): string {
  return v == null ? "—" : `${Math.round(v)} ms`;
}

// Stages shown in the medians row: [label, from-mark, to-mark].
const TRACE_STAGES: { label: string; from: keyof DictationTrace; to: keyof DictationTrace }[] = [
  { label: "capture", from: "t_hotkey_down", to: "t_first_audio" },
  { label: "hold", from: "t_first_audio", to: "t_hotkey_up" },
  { label: "asr", from: "t_asr_start", to: "t_asr_end" },
  { label: "text", from: "t_asr_end", to: "t_text_pipeline_end" },
  { label: "polish", from: "t_polish_start", to: "t_polish_end" },
  { label: "release→paste", from: "t_hotkey_up", to: "t_paste_sent" },
];

// Roadmap A2 waterfall: per-stage spans for the latest take, from the
// in-memory controller trace (`get_dictation_trace`).
interface TraceSpan {
  stage: string;
  elapsedMs: number;
  detail: string;
}

interface LatestTrace {
  session_id: number;
  spans: TraceSpan[];
}

const wellStyle: React.CSSProperties = {
  background: "var(--surface-2)",
  borderRadius: "var(--radius-sm)",
  border: "1px solid var(--border-subtle)",
  fontSize: "var(--text-xs)",
  fontFamily: MONO,
  color: "var(--text-secondary)",
  fontVariantNumeric: "tabular-nums",
};

const wellBtn: React.CSSProperties = {
  fontSize: "var(--text-xs)",
  fontWeight: 500,
  padding: "3px 10px",
  borderRadius: "var(--radius-xs, 6px)",
  border: "1px solid var(--border)",
  background: "var(--surface)",
  color: "var(--text)",
  cursor: "pointer",
  whiteSpace: "nowrap",
};

// Per-stage latency waterfall (roadmap A2/F3): the latest take's trace with
// proportional bars, plus a Copy-JSON button for bug reports.
function TraceWaterfall() {
  const [trace, setTrace] = useState<LatestTrace | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = () => {
    invoke<LatestTrace | null>("get_dictation_trace", { session: null })
      .then((t) => setTrace(t))
      .catch(() => {});
  };

  useEffect(refresh, []);
  useTauriEvent<unknown>("dictation-state", refresh);

  const total = (trace?.spans ?? []).reduce((sum, s) => sum + s.elapsedMs, 0);

  const copyJson = () => {
    if (!trace) return;
    navigator.clipboard
      .writeText(JSON.stringify(trace, null, 2))
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  };

  return (
    <div style={{ ...wellStyle, marginBottom: 12, padding: "8px 12px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: trace ? 8 : 0 }}>
        <span style={{ color: "var(--text-tertiary)" }}>Latest take</span>
        {trace && (
          <span style={{ color: "var(--text)" }}>
            #{trace.session_id} · {total} ms
          </span>
        )}
        <span style={{ flex: 1 }} />
        <button onClick={refresh} style={wellBtn}>
          Refresh
        </button>
        <button onClick={copyJson} disabled={!trace} style={wellBtn}>
          {copied ? "Copied" : "Copy JSON"}
        </button>
      </div>
      {trace ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
          {trace.spans.map((s, i) => (
            <div key={`${s.stage}-${i}`} style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ width: 96, flexShrink: 0, color: "var(--text-tertiary)" }}>{s.stage}</span>
              <div
                style={{
                  flex: 1,
                  minWidth: 40,
                  height: 10,
                  background: "var(--surface-hover)",
                  borderRadius: 5,
                  overflow: "hidden",
                }}
              >
                <div
                  style={{
                    width: total > 0 ? `${Math.max(2, (s.elapsedMs / total) * 100)}%` : "2px",
                    height: "100%",
                    background: durationColor(s.elapsedMs),
                    borderRadius: 5,
                  }}
                />
              </div>
              <span
                style={{
                  width: 190,
                  flexShrink: 0,
                  textAlign: "right",
                  color: durationColor(s.elapsedMs),
                  overflowWrap: "anywhere",
                }}
              >
                {s.elapsedMs} ms · {s.detail}
              </span>
            </div>
          ))}
        </div>
      ) : (
        <div style={{ color: "var(--text-tertiary)" }}>No take traced yet — dictate once, then refresh.</div>
      )}
    </div>
  );
}
export default function DeveloperScreen() {
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [follow, setFollow] = useState(true);
  const [traces, setTraces] = useState<DictationTrace[] | null>(null);
  const [tracesMissing, setTracesMissing] = useState(false);
  const [exportMsg, setExportMsg] = useState<string>("");
  const [s1, setS1] = useState<S1Control | null>(null);
  const [activeModel, setActiveModel] = useState<string>("");
  const [filter, setFilter] = useState<"all" | "info" | "warn" | "error">("all");
  const preRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(true);
  // Display-only timestamps: LogEntry has no time field, so capture the wall
  // clock time when each array index first renders.
  const lineTimesRef = useRef<Map<number, string>>(new Map());
  followRef.current = follow;

  const refreshS1 = () => {
    invoke<{ s1Control: S1Control }>("get_profile")
      .then((p) => setS1(p.s1Control))
      .catch(() => {});
  };

  useEffect(refreshS1, []);

  useEffect(() => {
    invoke<{ selectedLlmModel: string }>("get_settings")
      .then((s) => setActiveModel(s.selectedLlmModel))
      .catch(() => {});
  }, []);

  useTauriEvent<void>("settings-changed", () => {
    invoke<{ selectedLlmModel: string }>("get_settings")
      .then((s) => setActiveModel(s.selectedLlmModel))
      .catch(() => {});
  });

  useEffect(() => {
    let cancelled = false;
    const tick = () => {
      if (cancelled) return;
      invoke<LogEntry[]>("get_logs")
        .then((next) => {
          if (!cancelled) setLines(next);
        })
        .catch(() => {});
    };
    tick();
    const id = setInterval(tick, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (follow && preRef.current) {
      preRef.current.scrollTop = preRef.current.scrollHeight;
    }
  }, [lines, follow]);

  const refreshTraces = () => {
    setExportMsg("");
    invoke<DictationTrace[]>("get_dictation_traces", { limit: 50 })
      .then((next) => {
        setTraces(next);
        setTracesMissing(false);
      })
      .catch(() => {
        // Backend command not wired yet (P0.2 follow-up): show the empty
        // state instead of failing the whole screen.
        setTracesMissing(true);
      });
  };

  useEffect(refreshTraces, []);

  const releaseToPaste = (traces ?? [])
    .map((t) => stageMs(t, "t_hotkey_up", "t_paste_sent"))
    .filter((v): v is number => v != null)
    .sort((a, b) => a - b);
  const p50 = percentile(releaseToPaste, 50);
  const p95 = percentile(releaseToPaste, 95);
  const p99 = percentile(releaseToPaste, 99);

  const clear = () => {
    setFollow(true);
    invoke("clear_logs")
      .then(() => setLines([]))
      .catch(() => {});
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
        <div style={{ flex: 1 }}>
          <h2
            style={{
              fontSize: 22,
              fontWeight: 650,
              letterSpacing: "var(--tracking-tight)",
              margin: 0,
            }}
          >
            Developer
          </h2>
        </div>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: "var(--text-sm)",
            color: "var(--text-secondary)",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            checked={follow}
            onChange={(e) => setFollow(e.target.checked)}
          />
          Follow output
        </label>
        <button onClick={clear}>Clear</button>
        <button
          onClick={() =>
            invoke("devtools_toggle").catch(() => {
              /* not a debug build */
            })
          }
        >
          Toggle DevTools
        </button>
      </div>
      <div
        role="radiogroup"
        aria-label="Log level filter"
        style={{
          display: "inline-flex",
          alignSelf: "flex-start",
          padding: 2,
          gap: 2,
          marginBottom: 12,
          background: "var(--surface-2)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius-sm)",
        }}
      >
        {(["all", "info", "warn", "error"] as const).map((f) => {
          const active = filter === f;
          return (
            <button
              key={f}
              role="radio"
              aria-checked={active}
              onClick={() => setFilter(f)}
              style={{
                padding: "4px 12px",
                borderRadius: "calc(var(--radius-sm) - 1px)",
                fontSize: "var(--text-xs)",
                fontWeight: active ? 600 : 400,
                cursor: "pointer",
                border: "none",
                background: active ? "var(--surface)" : "transparent",
                color: active ? "var(--text)" : "var(--text-tertiary)",
                boxShadow: active ? "var(--shadow-1)" : "none",
              }}
            >
              {f === "all" ? "All" : f[0].toUpperCase() + f.slice(1)}
            </button>
          );
        })}
      </div>
      {s1 && activeModel === "s1-mini" && (
        <div style={{ ...wellStyle, marginBottom: 12, padding: "8px 12px", overflowWrap: "anywhere" }}>
          <span style={{ color: "var(--text-tertiary)" }}>S1 control: </span>
          <span>[Styling: {s1.styling}] [Structure: {s1.structure}] [Context: {s1.context}]</span>
          {" "}
          <span style={{ color: "var(--text-tertiary)" }}>· wire tokens: </span>
          {S1_WIRE_TOKENS.map(({ label, axis, tokens }) => (
            <span key={axis}>
              {label}[{tokens.map((t) => (t === s1ValueFor(axis, s1) ? t : `⟨${t}⟩`)).join("/")}]
              {" "}
            </span>
          ))}
        </div>
      )}
      <TraceWaterfall />
      <div style={{ ...wellStyle, marginBottom: 12, padding: "8px 12px" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8, flexWrap: "wrap" }}>
          <span style={{ color: "var(--text)", fontWeight: 600 }}>Dictation traces</span>
          <span style={{ color: "var(--text-tertiary)" }}>
            release→paste p50 {fmtMs(p50)} · p95 {fmtMs(p95)} · p99 {fmtMs(p99)}
          </span>
          <span style={{ flex: 1 }} />
          <button onClick={refreshTraces} style={wellBtn}>
            Refresh
          </button>
          <button
            onClick={() =>
              invoke<string>("export_dictation_traces")
                .then((path) => setExportMsg(`Exported to ${path}`))
                .catch((e) => setExportMsg(`Export failed: ${String(e)}`))
            }
            style={wellBtn}
          >
            Export traces
          </button>
        </div>
        {exportMsg && (
          <div style={{ color: "var(--text-secondary)", marginBottom: 8, overflowWrap: "anywhere" }}>
            {exportMsg}
          </div>
        )}
        {tracesMissing || traces === null ? (
          <div style={{ color: "var(--text-tertiary)" }}>
            {tracesMissing
              ? "Trace backend not wired yet (get_dictation_traces missing) — no timing data."
              : "Loading traces…"}
          </div>
        ) : traces.length === 0 ? (
          <div style={{ color: "var(--text-tertiary)" }}>No traces yet — dictate something.</div>
        ) : (
          <>
            <div style={{ color: "var(--text-tertiary)", marginBottom: 8, overflowWrap: "anywhere" }}>
              per-stage medians:{" "}
              {TRACE_STAGES.map(({ label, from, to }) => {
                const vals = traces
                  .map((t) => stageMs(t, from, to))
                  .filter((v): v is number => v != null);
                return (
                  <span key={label} style={{ marginRight: 12 }}>
                    {label} {fmtMs(median(vals))}
                  </span>
                );
              })}
            </div>
            <div style={{ overflowX: "auto" }}>
              <table style={{ borderCollapse: "collapse", width: "100%" }}>
                <thead>
                  <tr style={{ color: "var(--text-tertiary)", textAlign: "left" }}>
                    {["session", "engine", "audio", "release→paste", "asr", "polish", "skipped", "errors"].map(
                      (h) => (
                        <th
                          key={h}
                          style={{ padding: "2px 8px 2px 0", fontWeight: 500, fontSize: "var(--text-xs)" }}
                        >
                          {h}
                        </th>
                      )
                    )}
                  </tr>
                </thead>
                <tbody>
                  {traces.slice(-50).reverse().map((t) => (
                    <tr key={t.session}>
                      <td style={{ padding: "2px 8px 2px 0" }}>{t.session}</td>
                      <td style={{ padding: "2px 8px 2px 0" }}>{t.engine || "—"}</td>
                      <td style={{ padding: "2px 8px 2px 0" }}>{t.audio_ms} ms</td>
                      <td style={{ padding: "2px 8px 2px 0" }}>
                        {fmtMs(stageMs(t, "t_hotkey_up", "t_paste_sent"))}
                      </td>
                      <td style={{ padding: "2px 8px 2px 0" }}>
                        {fmtMs(stageMs(t, "t_asr_start", "t_asr_end"))}
                      </td>
                      <td style={{ padding: "2px 8px 2px 0" }}>
                        {fmtMs(stageMs(t, "t_polish_start", "t_polish_end"))}
                      </td>
                      <td
                        style={{
                          padding: "2px 8px 2px 0",
                          maxWidth: 220,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {t.polish_skipped_reason ?? "—"}
                      </td>
                      <td style={{ padding: "2px 8px 2px 0", overflowWrap: "anywhere" }}>
                        {t.errors.length ? t.errors.join("; ") : "—"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
      </div>
      <div
        ref={preRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          if (el.scrollHeight - el.scrollTop - el.clientHeight < 24) {
            setFollow(true);
          } else if (followRef.current) {
            setFollow(false);
          }
        }}
        style={{
          flex: 1,
          minHeight: 320,
          margin: 0,
          padding: 12,
          overflow: "auto",
          background: "var(--surface-2)",
          borderRadius: "var(--radius-sm)",
          border: "1px solid var(--border-subtle)",
          fontSize: "var(--text-xs)",
          lineHeight: 1.6,
          fontFamily: MONO,
          color: "var(--text-secondary)",
        }}
      >
        {lines.length ? (
          lines
            .map((line, i) => ({ line, i }))
            .filter(
              ({ line }) =>
                filter === "all" ||
                (filter === "info" && (line.level === "Info" || line.level === "Success")) ||
                (filter === "warn" && line.level === "Warn") ||
                (filter === "error" && line.level === "Error")
            )
            .map(({ line, i }) => {
            if (!lineTimesRef.current.has(i)) {
              lineTimesRef.current.set(i, new Date().toLocaleTimeString());
            }
            const time = lineTimesRef.current.get(i)!;
            const color = LEVEL_COLORS[line.level] ?? LEVEL_COLORS.Info;
            return (
              <div
                key={i}
                style={{
                  display: "flex",
                  alignItems: "baseline",
                  gap: 8,
                  marginBottom: 2,
                  lineHeight: 1.5,
                  // Slightly larger gap after Error lines for visual separation.
                  ...(line.level === "Error" ? { marginBottom: 6 } : null),
                }}
              >
                <span
                  style={{
                    color: "var(--text-tertiary)",
                    fontSize: "var(--text-xs)",
                    flexShrink: 0,
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  {time}
                </span>
                <span
                  style={{
                    color,
                    fontSize: "var(--text-xs)",
                    fontWeight: 600,
                    letterSpacing: 0.5,
                    flexShrink: 0,
                  }}
                >
                  {LEVEL_BADGES[line.level]}
                </span>
                <span
                  style={{
                    color: LEVEL_COLORS[line.level] ?? LEVEL_COLORS.Info,
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                    flex: 1,
                  }}
                >
                  {line.message}
                </span>
                {line.durationMs != null && (
                  <span
                    style={{
                      color: durationColor(line.durationMs),
                      marginLeft: "auto",
                      flexShrink: 0,
                      fontSize: "var(--text-xs)",
                      fontVariantNumeric: "tabular-nums",
                    }}
                  >
                    {line.durationMs} ms
                  </span>
                )}
              </div>
            );
          })
        ) : (
          <div
            style={{
              height: "100%",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--text-tertiary)",
            }}
          >
            No log lines yet.
          </div>
        )}
      </div>
    </div>
  );
}
