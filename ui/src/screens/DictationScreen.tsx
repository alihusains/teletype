import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";
import { Icon } from "../components/Icon";

interface HistoryEntry {
  id: string;
  createdAt: number;
  text: string;
  appName: string;
  appType: string;
}

/// The file-transcription drop zone state on the Dictation screen.
type TranscribeState =
  | { status: "idle" }
  | { status: "transcribing"; name: string }
  | { status: "done"; words: number }
  | { status: "error"; message: string };

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

export default function DictationScreen() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [transcribe, setTranscribe] = useState<TranscribeState>({ status: "idle" });

  const refresh = useCallback(() => {
    invoke<HistoryEntry[]>("list_dictation_history").then(setEntries).catch(console.error);
  }, []);

  const transcribeFile = useCallback(
    async (path: string) => {
      const name = path.split(/[\\/]/).pop() ?? path;
      setTranscribe({ status: "transcribing", name });
      try {
        const res = await invoke<{ id: string; wordCount: number }>("transcribe_file", { path });
        setTranscribe({ status: "done", words: res.wordCount });
        refresh();
      } catch (err) {
        setTranscribe({ status: "error", message: String(err) });
      }
    },
    [refresh],
  );

  // Native file picker. In Tauri's WKWebView the DOM <input type="file"> does
  // not expose File.path, so the old picker silently did nothing. The dialog
  // plugin returns a real filesystem path we can hand to transcribe_file.
  const chooseFile = useCallback(async () => {
    const path = await open({
      multiple: false,
      filters: [
        {
          name: "Audio",
          extensions: ["m4a", "aac", "wav", "mp3", "flac", "ogg", "m4b", "aiff", "aif"],
        },
      ],
    });
    if (typeof path === "string") transcribeFile(path);
  }, [transcribeFile]);

  // Drag-drop: Tauri v2 delivers file paths directly on the drop event.
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    let unlisten: (() => void) | undefined;
    win
      .onDragDropEvent((e) => {
        if (e.payload.type === "drop") {
          for (const p of e.payload.paths) transcribeFile(p);
        }
      })
      .then((fn) => (unlisten = fn));
    return () => unlisten?.();
  }, [transcribeFile]);

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
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 20, gap: 12, flexWrap: "wrap" }}>
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

      {/* File transcription (P3.17): drop an audio file or browse for one.
          Drops are handled by the Tauri onDragDropEvent listener above, which
          delivers real filesystem paths (the DOM drop event does not). */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "12px 16px",
          marginBottom: 20,
          background: "var(--surface)",
          borderRadius: "var(--radius)",
          border: "1px dashed var(--border)",
        }}
      >
        <div style={{ fontSize: 17 }}>🎧</div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 500 }}>
            {transcribe.status === "transcribing" && `Transcribing ${transcribe.name}…`}
            {transcribe.status === "done" && `Transcribed ${transcribe.words} words`}
            {transcribe.status === "error" && transcribe.message}
            {transcribe.status === "idle" && "Transcribe an audio file (m4a, wav, mp3, flac, ogg)"}
          </div>
          {transcribe.status === "error" && (
            <div style={{ fontSize: 11, color: "var(--danger, #e5484d)", marginTop: 2 }}>{transcribe.message}</div>
          )}
        </div>
        {transcribe.status !== "transcribing" && (
          <button
            onClick={chooseFile}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "6px 12px",
              fontSize: 12,
              fontWeight: 500,
              borderRadius: 8,
              border: "1px solid var(--border)",
              background: "var(--surface-2)",
              color: "var(--text)",
              cursor: "pointer",
            }}
          >
            <Icon name="plus" size={14} color="var(--text-secondary)" />
            Choose file
          </button>
        )}
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
                  alignItems: "flex-start",
                  gap: 12,
                  padding: "12px 16px",
                  background: "var(--surface)",
                  borderRadius: "var(--radius)",
                  border: "1px solid var(--border)",
                  minWidth: 0,
                }}
              >
                <div style={{ width: 36, flex: "none", textAlign: "center", paddingTop: 1 }}>
                  <div style={{ fontSize: 17 }}>{APP_ICON[e.appType] ?? "🎤"}</div>
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div
                    style={{
                      fontSize: 14,
                      lineHeight: 1.5,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      color: "var(--text)",
                    }}
                    title={e.text}
                  >
                    {e.text}
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
                    <span style={{ fontSize: 11, color: "var(--text-secondary)", fontVariantNumeric: "tabular-nums" }}>
                      {timeLabel(e.createdAt)}
                    </span>
                    {e.appName && (
                      <>
                        <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>·</span>
                        <span
                          style={{
                            fontSize: 11,
                            color: "var(--text-secondary)",
                            background: "var(--surface-2)",
                            borderRadius: 6,
                            padding: "1px 8px",
                            maxWidth: 120,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {e.appName}
                        </span>
                      </>
                    )}
                  </div>
                </div>
                <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 4, flex: "none", paddingTop: 2 }}>
                  <button
                    onClick={() => copy(e.id, e.text)}
                    title={copiedId === e.id ? "Copied!" : "Copy to clipboard"}
                    aria-label={copiedId === e.id ? "Copied" : "Copy to clipboard"}
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      width: 28,
                      height: 28,
                      borderRadius: 7,
                      background: "none",
                      border: "none",
                      cursor: "pointer",
                      color: copiedId === e.id ? "var(--success)" : "var(--text-secondary)",
                      opacity: 0.7,
                    }}
                  >
                    {copiedId === e.id ? (
                      <Icon name="check" size={16} color="var(--success)" />
                    ) : (
                      <Icon name="copy" size={16} color="var(--text-secondary)" />
                    )}
                  </button>
                  <button
                    onClick={() => remove(e.id)}
                    title="Delete"
                    aria-label="Delete entry"
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      width: 28,
                      height: 28,
                      borderRadius: 7,
                      background: "none",
                      border: "none",
                      cursor: "pointer",
                      color: "var(--text-secondary)",
                      opacity: 0.7,
                    }}
                  >
                    <Icon name="trash" size={16} color="var(--text-secondary)" />
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
