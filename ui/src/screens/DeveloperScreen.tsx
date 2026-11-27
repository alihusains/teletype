import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "../lib/useTauriEvent";

const POLL_MS = 500;

interface LogEntry {
  level: "Info" | "Success" | "Warn" | "Error";
  message: string;
  durationMs?: number;
}

// Pastel palette for the dark log surface.
const LEVEL_COLORS: Record<LogEntry["level"], string> = {
  Info: "#a5b4c4",
  Success: "#9ae6b4",
  Warn: "#f5d48f",
  Error: "#f7a8a8",
};

// Compact badge labels shown before each log line.
const LEVEL_BADGES: Record<LogEntry["level"], string> = {
  Info: "INFO",
  Success: "OK",
  Warn: "WARN",
  Error: "ERR",
};

// Duration chip color: green (<500ms), yellow (500 to 2000ms), red (>2000ms).
function durationColor(ms: number): string {
  if (ms < 500) return "#9ae6b4";
  if (ms <= 2000) return "#f5d48f";
  return "#f7a8a8";
}

interface S1Control {
  styling: string;
  structure: string;
  context: string;
}

interface LastDictationTimings {
  id: string;
  createdAt: number;
  textPreview: string;
  durationMs: number | null;
  polishMs: number | null;
  wordCount: number;
  asrModel: string | null;
  polishModel: string | null;
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

export default function DeveloperScreen({ active = true }: { active?: boolean }) {
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [follow, setFollow] = useState(true);
  const [s1, setS1] = useState<S1Control | null>(null);
  const [activeModel, setActiveModel] = useState<string>("");
  const [filter, setFilter] = useState<"all" | "info" | "warn" | "error">("all");
  const [lastTimings, setLastTimings] = useState<LastDictationTimings | null>(null);
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

  // All screens stay mounted (display: none), so the 500 ms log/timings poll
  // must not run while this screen is hidden. The one-shot get_profile /
  // get_settings reads are left ungated: they happen once at mount and the
  // screen is active when it first mounts (App renders it hidden but the
  // data is cheap and the settings-changed listener keeps it fresh).
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const tick = () => {
      if (cancelled) return;
      invoke<LogEntry[]>("get_logs")
        .then((next) => {
          if (!cancelled) setLines(next);
        })
        .catch(() => {});
      invoke<LastDictationTimings | null>("get_last_dictation_timings")
        .then((t) => {
          if (!cancelled) setLastTimings(t);
        })
        .catch(() => {});
    };
    tick();
    const id = setInterval(tick, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [active]);

  useEffect(() => {
    if (follow && preRef.current) {
      preRef.current.scrollTop = preRef.current.scrollHeight;
    }
  }, [lines, follow]);

  const clear = () => {
    setFollow(true);
    invoke("clear_logs")
      .then(() => setLines([]))
      .catch(() => {});
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
        <h2 className="tt-screen-title" style={{ flex: 1 }}>Developer</h2>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: 13,
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
        {import.meta.env.DEV && (
          <button
            onClick={() =>
              invoke("devtools_toggle").catch(() => {
                /* not a debug build */
              })
            }
          >
            Toggle DevTools
          </button>
        )}
      </div>
      <div style={{ display: "flex", gap: 8, marginBottom: 12 }}>
        {(["all", "info", "warn", "error"] as const).map((f) => {
          const active = filter === f;
          return (
            <button
              key={f}
              onClick={() => setFilter(f)}
              style={{
                padding: "4px 12px",
                borderRadius: 12,
                fontSize: 12,
                fontWeight: 500,
                cursor: "pointer",
                background: active ? "var(--accent)" : "var(--surface)",
                color: active ? "white" : "var(--text-secondary)",
                border: active ? "1px solid var(--accent)" : "1px solid var(--border)",
              }}
            >
              {f === "all" ? "All" : f[0].toUpperCase() + f.slice(1)}
            </button>
          );
        })}
      </div>
      {s1 && activeModel === "s1-mini" && (
        <div
          style={{
            marginBottom: 12,
            padding: "8px 12px",
            background: "#0f1115",
            borderRadius: 8,
            border: "1px solid var(--border)",
            fontSize: 12,
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            color: "#d1d5db",
          }}
        >
          <span style={{ color: "#8b93a1" }}>S1 control: </span>
          <span>[Styling: {s1.styling}] [Structure: {s1.structure}] [Context: {s1.context}]</span>
          {" "}
          <span style={{ color: "#8b93a1" }}>· wire tokens: </span>
          {S1_WIRE_TOKENS.map(({ label, axis, tokens }) => (
            <span key={axis}>
              {label}[{tokens.map((t) => (t === s1ValueFor(axis, s1) ? t : `⟨${t}⟩`)).join("/")}]
              {" "}
            </span>
          ))}
        </div>
      )}
      {/* Last dictation timing breakdown */}
      {lastTimings && (
        <div
          style={{
            marginBottom: 12,
            padding: "8px 12px",
            background: "#0f1115",
            borderRadius: 8,
            border: "1px solid var(--border)",
            fontSize: 12,
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            color: "#d1d5db",
          }}
        >
          <div style={{ color: "#8b93a1", marginBottom: 4 }}>Last dictation</div>
          <div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
            <span>
              <span style={{ color: "#8b93a1" }}>Mic: </span>
              {lastTimings.durationMs != null ? (
                <span style={{ color: durationColor(lastTimings.durationMs) }}>
                  {lastTimings.durationMs} ms
                </span>
              ) : (
                "—"
              )}
            </span>
            <span>
              <span style={{ color: "#8b93a1" }}>Polish: </span>
              {lastTimings.polishMs != null ? (
                <span style={{ color: durationColor(lastTimings.polishMs) }}>
                  {lastTimings.polishMs} ms
                </span>
              ) : (
                "—"
              )}
            </span>
            <span>
              <span style={{ color: "#8b93a1" }}>Words: </span>
              {lastTimings.wordCount}
            </span>
            {lastTimings.asrModel && (
              <span>
                <span style={{ color: "#8b93a1" }}>ASR: </span>
                {lastTimings.asrModel}
              </span>
            )}
            {lastTimings.polishModel && (
              <span>
                <span style={{ color: "#8b93a1" }}>Polish model: </span>
                {lastTimings.polishModel}
              </span>
            )}
          </div>
          <div style={{ color: "#6b7280", marginTop: 4, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {lastTimings.textPreview}
          </div>
        </div>
      )}
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
          background: "#0f1115",
          borderRadius: 8,
          border: "1px solid var(--border)",
          fontSize: 12,
          lineHeight: 1.6,
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
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
                    color: "#6b7280",
                    fontSize: 11,
                    flexShrink: 0,
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  {time}
                </span>
                <span
                  style={{
                    color,
                    fontSize: 10,
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
                      fontSize: 11,
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
              color: "#8b93a1",
            }}
          >
            No log lines yet.
          </div>
        )}
      </div>
    </div>
  );
}
