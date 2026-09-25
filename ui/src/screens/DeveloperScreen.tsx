import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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

export default function DeveloperScreen() {
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [follow, setFollow] = useState(true);
  const [s1, setS1] = useState<S1Control | null>(null);
  const preRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(true);
  followRef.current = follow;

  const refreshS1 = () => {
    invoke<{ s1Control: S1Control }>("get_profile")
      .then((p) => setS1(p.s1Control))
      .catch(() => {});
  };

  useEffect(refreshS1, []);

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

  const clear = () => {
    setFollow(true);
    invoke("clear_logs")
      .then(() => setLines([]))
      .catch(() => {});
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
        <h2 style={{ fontSize: 18, fontWeight: 600, flex: 1 }}>Developer</h2>
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
      </div>
      {s1 && (
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
          lines.map((line, i) => (
            <div key={i} style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
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
                    color: "#8b93a1",
                    marginLeft: "auto",
                    flexShrink: 0,
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  {line.durationMs} ms
                </span>
              )}
            </div>
          ))
        ) : (
          <span style={{ color: "#8b93a1" }}>No log lines yet.</span>
        )}
      </div>
    </div>
  );
}
