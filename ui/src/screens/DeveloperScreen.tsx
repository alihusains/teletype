import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const POLL_MS = 500;

export default function DeveloperScreen() {
  const [lines, setLines] = useState<string[]>([]);
  const [follow, setFollow] = useState(true);
  const preRef = useRef<HTMLPreElement>(null);
  const followRef = useRef(true);
  followRef.current = follow;

  useEffect(() => {
    let cancelled = false;
    const tick = () => {
      if (cancelled) return;
      invoke<string[]>("get_logs")
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
      <pre
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
          color: "#d1d5db",
          borderRadius: 8,
          border: "1px solid var(--border)",
          fontSize: 12,
          lineHeight: 1.5,
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
        }}
      >
        {lines.length ? lines.join("\n") : "No log lines yet."}
      </pre>
    </div>
  );
}
