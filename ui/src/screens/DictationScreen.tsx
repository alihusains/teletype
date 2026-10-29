import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";
import { Icon } from "../components/Icon";
import { LoadFailed, LoadingRow, WriteFailed } from "../components/StateRows";
import { errorMessage } from "../lib/errors";
import { useNow } from "../lib/useNow";
import { useResource, useWrites } from "../lib/writeGuard";
import { confirmDialog } from "../lib/dialogs";

interface HistoryEntry {
  id: string;
  createdAt: number;
  text: string;
  appName: string;
  appType: string;
}

/**
 * The file-transcription pipeline state.
 *
 * `queue` is the list of dropped/selected files; one is processed at a time
 * and `index` is the position of the one currently in flight. This replaces
 * the single `transcribe` state, which the old code overwrote on every drop
 * so three dropped files read as one.
 */
type TranscribeState =
  | { status: "idle"; words: number | null; error: string | null }
  | { status: "transcribing"; queue: string[]; index: number; startedAt: number };

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

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function elapsedLabel(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m ${s % 60}s`;
}

export default function DictationScreen() {
  const history = useResource(() => invoke<HistoryEntry[]>("list_dictation_history"));
  const entries = history.data ?? [];
  const [query, setQuery] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [copyFailedId, setCopyFailedId] = useState<string | null>(null);
  const [transcribe, setTranscribe] = useState<TranscribeState>({
    status: "idle",
    words: null,
    error: null,
  });
  const writes = useWrites();
  // Monotonic job id: only the newest run may write the result state, so a
  // Cancel that supersedes an in-flight job cannot resurrect it.
  const jobRef = useRef(0);
  // Whether an in-flight transcription should be dropped when it settles.
  const cancelledRef = useRef(false);

  // A live clock while a transcription runs, so a long file reads as working
  // rather than hung.
  const now = useNow(1000, transcribe.status === "transcribing");

  const busy = transcribe.status === "transcribing";

  // Process the queue one file at a time. Appends never overwrite the in-
  // flight job; they extend `queue` so the row can say "2 of 3".
  const enqueue = useCallback(
    (paths: string[]) => {
      const clean = paths.filter(Boolean);
      if (clean.length === 0) return;
      cancelledRef.current = false;
      setTranscribe((prev) => {
        if (prev.status === "transcribing") {
          return { ...prev, queue: [...prev.queue, ...clean] };
        }
        return { status: "transcribing", queue: [...clean], index: 0, startedAt: Date.now() };
      });
    },
    [],
  );

  const activeIndex = transcribe.status === "transcribing" ? transcribe.index : -1;
  useEffect(() => {
    if (transcribe.status !== "transcribing") return;
    const { queue, index } = transcribe;
    if (index >= queue.length) {
      setTranscribe({ status: "idle", words: null, error: null });
      return;
    }
    const job = ++jobRef.current;
    const path = queue[index];
    void invoke<{ id: string; wordCount: number }>("transcribe_file", { path })
      .then((res) => {
        if (job !== jobRef.current) return;
        if (cancelledRef.current) return;
        const isLast = index === queue.length - 1;
        if (isLast) {
          setTranscribe({ status: "idle", words: res.wordCount, error: null });
        } else {
          // Advance to the next file in the queue. The elapsed clock restarts
          // per file, which is fine — the row shows position, not total time.
          setTranscribe({
            status: "transcribing",
            queue,
            index: index + 1,
            startedAt: Date.now(),
          });
        }
        history.reload();
      })
      .catch((err) => {
        if (job !== jobRef.current) return;
        if (cancelledRef.current) return;
        setTranscribe({ status: "idle", words: null, error: errorMessage(err) });
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeIndex]);

  // Cancel: there is no backend cancel command, so this drops the remaining
  // queue items and stops showing the in-flight one (its result is ignored
  // when it settles). The row's title attribute says exactly that.
  const cancelTranscribe = useCallback(() => {
    cancelledRef.current = true;
    jobRef.current++;
    setTranscribe({ status: "idle", words: null, error: null });
  }, []);

  // Native file picker. In Tauri's WKWebView the DOM <input type="file"> does
  // not expose File.path, so the old picker silently did nothing. The dialog
  // plugin returns a real filesystem path we can hand to transcribe_file.
  const chooseFile = useCallback(async () => {
    if (busy) return;
    const path = await open({
      multiple: false,
      filters: [
        {
          name: "Audio",
          extensions: ["m4a", "aac", "wav", "mp3", "flac", "ogg", "m4b", "aiff", "aif"],
        },
      ],
    });
    if (typeof path === "string") enqueue([path]);
  }, [busy, enqueue]);

  // Drag-drop: Tauri v2 delivers file paths directly on the drop event. The
  // listener is gated on the same busy condition the Choose-file button uses,
  // so a drag cannot start a second job mid-flight.
  const busyRef = useRef(busy);
  busyRef.current = busy;
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    let unlisten: (() => void) | undefined;
    win
      .onDragDropEvent((e) => {
        if (e.payload.type === "drop" && !busyRef.current) {
          enqueue(e.payload.paths);
        }
      })
      .then((fn) => (unlisten = fn));
    return () => unlisten?.();
  }, [enqueue]);

  // Refresh when the window regains focus (after dictating in another app)
  // or becomes visible, so new entries appear without switching tabs.
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    const onFocus = () => history.reload();
    window.addEventListener("focus", onFocus);
    let unlistenFocus: (() => void) | undefined;
    win.onFocusChanged(({ payload: focused }) => {
      if (focused) history.reload();
    }).then((fn) => (unlistenFocus = fn));
    document.addEventListener("visibilitychange", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onFocus);
      unlistenFocus?.();
    };
  }, [history]);

  // A-10: the "Copied" flag is set only on success. A rejected writeText
  // shows "Copy failed" in the same place, so the affordance is honest.
  const copy = async (id: string, text: string) => {
    setCopiedId(null);
    setCopyFailedId(null);
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      setTimeout(() => setCopiedId((cur) => (cur === id ? null : cur)), 1200);
    } catch {
      setCopyFailedId(id);
      setTimeout(() => setCopyFailedId((cur) => (cur === id ? null : cur)), 2400);
    }
  };

  // A-1: confirm first (window.confirm is dead in this webview, so this is
  // the Tauri dialog path), then route through useWrites so a rejected
  // delete keeps the row and renders a per-row error with a Retry.
  const remove = async (id: string) => {
    if (!(await confirmDialog("Delete this dictation?", { title: "Delete dictation" }))) return;
    // The result has to come from the promise, not from `writes.error(id)`
    // read afterwards: that closure belongs to the render before the await,
    // so it reports the pre-write error and a rejected delete would still
    // refresh.
    const res = await writes.run(id, () => invoke("delete_dictation_entry", { id }));
    if (res.ok) history.reload();
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
          <h2 className="tt-screen-title">Dictation</h2>
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
        <span
          style={{
            width: 30,
            height: 30,
            borderRadius: 9,
            background: "var(--accent-soft)",
            display: "grid",
            placeItems: "center",
            flexShrink: 0,
          }}
        >
          <Icon name="wave" size={15} color="var(--accent)" />
        </span>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 500 }}>
            {transcribe.status === "transcribing" &&
              `Transcribing ${transcribe.index + 1} of ${transcribe.queue.length}, ${fileName(
                transcribe.queue[transcribe.index]
              )}…`}
            {transcribe.status === "idle" &&
              transcribe.error === null &&
              transcribe.words !== null &&
              `Transcribed ${transcribe.words} words`}
            {transcribe.status === "idle" &&
              transcribe.error === null &&
              transcribe.words === null &&
              "Transcribe an audio file (m4a, wav, mp3, flac, ogg)"}
            {transcribe.status === "idle" && transcribe.error !== null && transcribe.error}
          </div>
          {transcribe.status === "transcribing" && (
            <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2 }}>
              {elapsedLabel(now - transcribe.startedAt)} elapsed
            </div>
          )}
          {transcribe.status === "idle" && transcribe.error !== null && (
            <div style={{ fontSize: 11, color: "var(--danger)", marginTop: 2 }}>
              The file was not transcribed: {transcribe.error}
            </div>
          )}
        </div>
        {busy ? (
          <button
            onClick={cancelTranscribe}
            title="There is no backend cancel: this drops the remaining queue items and stops showing the current file when it finishes."
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "7px 14px",
              fontSize: 12,
              fontWeight: 500,
              borderRadius: "var(--radius-pill)",
              border: "1px solid var(--border-strong)",
              background: "var(--surface-2)",
              color: "var(--text)",
              cursor: "pointer",
            }}
          >
            Cancel
          </button>
        ) : (
          <button
            onClick={chooseFile}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "7px 14px",
              fontSize: 12,
              fontWeight: 500,
              borderRadius: "var(--radius-pill)",
              border: "1px solid var(--border-strong)",
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

      {/* A-3: a rejected read says so with a retry. The empty state only
          appears when the read succeeded and returned zero rows. */}
      {history.status === "loading" && <LoadingRow label="Loading your dictations…" />}
      {history.status === "error" && (
        <LoadFailed
          what="Your dictation history"
          detail={history.error ?? undefined}
          onRetry={history.reload}
          retrying={false}
        />
      )}
      {history.status === "ready" && entries.length === 0 && (
        <div style={{ padding: 44, textAlign: "center", color: "var(--text-secondary)" }}>
          <span
            style={{
              width: 46,
              height: 46,
              borderRadius: 14,
              background: "var(--accent-soft)",
              display: "inline-grid",
              placeItems: "center",
              marginBottom: 12,
            }}
          >
            <Icon name="mic" size={22} color="var(--accent)" />
          </span>
          <div style={{ fontSize: "var(--text-md)" }}>
            No dictations yet. Focus any app, hold your hotkey and speak.
          </div>
        </div>
      )}

      {groups.map((g) => (
        <div key={g.label} style={{ marginBottom: 20 }}>
          <div style={{ fontSize: 11, letterSpacing: 1, color: "var(--text-secondary)", marginBottom: 8 }}>
            {g.label}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {g.items.map((e) => {
              const deleteError = writes.error(e.id);
              const deleting = writes.busy(e.id);
              return (
                <div key={e.id}>
                  <div
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
                      {e.appName && (
                        <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
                          <span
                            style={{
                              fontSize: 11,
                              color: "var(--text-secondary)",
                              background: "var(--surface-2)",
                              borderRadius: 6,
                              padding: "1px 8px",
                              display: "inline-block",
                              maxWidth: "100%",
                              overflowWrap: "anywhere",
                            }}
                            title={e.appName}
                          >
                            {e.appName}
                          </span>
                        </div>
                      )}
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 6, flex: "none", paddingTop: 2 }}>
                      <span
                        style={{
                          fontSize: 11,
                          color: "var(--text-secondary)",
                          fontVariantNumeric: "tabular-nums",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {timeLabel(e.createdAt)}
                      </span>
                      <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 4 }}>
                      <button
                        type="button"
                        title={
                          copiedId === e.id
                            ? "Copied!"
                            : copyFailedId === e.id
                            ? "Copy failed"
                            : "Copy to clipboard"
                        }
                        aria-label={
                          copiedId === e.id
                            ? "Copied"
                            : copyFailedId === e.id
                            ? "Copy failed"
                            : "Copy to clipboard"
                        }
                        onClick={() => void copy(e.id, e.text)}
                        style={{
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          padding: 5,
                          borderRadius: 6,
                          background: "none",
                          border: "none",
                          cursor: "pointer",
                        }}
                      >
                        {copiedId === e.id ? (
                          <Icon name="check" size={20} color="var(--success)" />
                        ) : (
                          <Icon
                            name="copy"
                            size={20}
                            color={copyFailedId === e.id ? "var(--danger)" : "var(--text-secondary)"}
                          />
                        )}
                      </button>
                      <button
                        type="button"
                        title={deleting ? "Deleting…" : "Delete"}
                        aria-label={deleting ? "Deleting" : "Delete entry"}
                        disabled={deleting}
                        onClick={() => void remove(e.id)}
                        style={{
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          padding: 5,
                          borderRadius: 6,
                          background: "none",
                          border: "none",
                          cursor: "pointer",
                        }}
                      >
                        <Icon name="trash" size={20} color="var(--text-secondary)" />
                      </button>
                      </div>
                    </div>
                  </div>
                  {deleteError && (
                    <div style={{ marginTop: 6 }}>
                      <WriteFailed
                        message={deleteError}
                        label="This entry"
                        onRetry={() => void remove(e.id)}
                        onDismiss={() => writes.clear(e.id)}
                      />
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
