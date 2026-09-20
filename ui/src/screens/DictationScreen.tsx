import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Icon } from "../components/Icon";

interface HistoryEntry {
  id: string;
  createdAt: number;
  text: string;
  appName: string;
  appType: string;
}

const APP_ICON: Record<string, string> = {
  Email: "✉️",
  Chat: "💬",
  Social: "🔗",
  Coding: "💻",
  Document: "📄",
  Browser: "🌐",
  Terminal: "🖥️",
  Unknown: "🎤",
};

function dayLabel(ts: number): string {
  const d = new Date(ts);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  const same = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate();
  if (same(d, today)) return "TODAY";
  if (same(d, yesterday)) return "YESTERDAY";
  return d
    .toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric" })
    .toUpperCase();
}

function timeLabel(ts: number): string {
  return new Date(ts).toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
  });
}

/// Icon button base style.
const iconBtn: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  width: 30,
  height: 30,
  borderRadius: "var(--radius-sm)",
  background: "transparent",
  border: "1px solid var(--border)",
  color: "var(--text-secondary)",
  cursor: "pointer",
};

export default function DictationScreen() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const refresh = useCallback(() => {
    invoke<HistoryEntry[]>("list_dictation_history").then(setEntries).catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  // Refresh when the window regains focus (after dictating in another app)
  // or becomes visible, so new entries appear without switching tabs.
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    let unlistenFocus: (() => void) | undefined;
    win.onFocusChanged(({ payload: focused }) => {
      if (focused) refresh();
    }).then((fn) => (unlistenFocus = fn));
    document.addEventListener("visibilitychange", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onFocus);
      unlistenFocus?.();
    };
  }, [refresh]);

  const copy = async (id: string, text: string) => {
    await navigator.clipboard.writeText(text).catch(() => {});
    setCopiedId(id);
    setTimeout(() => setCopiedId((cur) => (cur === id ? null : cur)), 1200);
  };

  const remove = async (id: string) => {
    await invoke("delete_dictation_entry", { id }).catch(console.error);
    refresh();
  };

  // Group entries by day (they're already newest-first).
  const groups = useMemo(() => {
    const filtered = query
      ? entries.filter((e) => e.text.toLowerCase().includes(query.toLowerCase()))
      : entries;
    const out: { label: string; items: HistoryEntry[] }[] = [];
    for (const e of filtered) {
      const label = dayLabel(e.createdAt);
      const last = out[out.length - 1];
      if (last && last.label === label) last.items.push(e);
      else out.push({ label, items: [e] });
    }
    return out;
  }, [entries, query]);

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 20 }}>
        <div>
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>Dictation</h2>
          <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
            Every dictation, saved. Copy or re-paste any of them.
          </p>
        </div>
        <input
          style={{ maxWidth: 220 }}
          placeholder="Search…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>

      {entries.length === 0 && (
        <div style={{ padding: 40, textAlign: "center", color: "var(--text-secondary)" }}>
          <div style={{ fontSize: 28, marginBottom: 8 }}>🎤</div>
          No dictations yet. Focus any app, hold your hotkey and speak.
        </div>
      )}

      {groups.map((g) => (
        <div key={g.label} style={{ marginBottom: 20 }}>
          <div style={{ fontSize: 11, letterSpacing: 1, color: "var(--text-secondary)", marginBottom: 8 }}>
            {g.label}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {g.items.map((e) => (
              <div
                key={e.id}
                style={{
                  display: "flex",
                  gap: 14,
                  padding: "12px 16px",
                  background: "var(--surface)",
                  borderRadius: "var(--radius)",
                  border: "1px solid var(--border)",
                }}
              >
                <div style={{ width: 44, flex: "none", textAlign: "center" }}>
                  <div style={{ fontSize: 18 }}>{APP_ICON[e.appType] ?? "🎤"}</div>
                  <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4 }}>
                    {timeLabel(e.createdAt)}
                  </div>
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <p
                    style={{
                      fontSize: 14,
                      lineHeight: 1.5,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                    }}
                  >
                    {e.text}
                  </p>
                  {e.appName && (
                    <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 6 }}>
                      in {e.appName}
                    </div>
                  )}
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 6, flex: "none" }}>
                  <button
                    onClick={() => copy(e.id, e.text)}
                    title={copiedId === e.id ? "Copied!" : "Copy to clipboard"}
                    style={{
                      ...iconBtn,
                      color: copiedId === e.id ? "var(--success)" : "var(--text-secondary)",
                    }}
                  >
                    {copiedId === e.id ? (
                      <Icon name="check" size={16} color="var(--success)" />
                    ) : (
                      <Icon name="copy" size={16} />
                    )}
                  </button>
                  <button
                    onClick={() => remove(e.id)}
                    title="Delete"
                    style={{ ...iconBtn, color: "var(--danger)" }}
                  >
                    <Icon name="trash" size={16} color="var(--danger)" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
