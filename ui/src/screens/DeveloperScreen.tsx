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

export default function DeveloperScreen() {
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [follow, setFollow] = useState(true);
  const preRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(true);
  followRef.current = follow;

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
