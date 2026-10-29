import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { confirmDialog } from "../lib/dialogs";

/** The wire shape for one note (Note in teletype-core, camelCase). */
export interface Note {
  id: string;
  title: string;
  body: string;
  preview: string;
  createdAt: number;
  updatedAt: number;
  source: "typed" | "dictated";
}

type SaveState = "idle" | "saving" | "saved";

const AUTOSAVE_MS = 600;

function sameDay(ts: number, now: number): boolean {
  const a = new Date(ts);
  const b = new Date(now);
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function sameWeek(ts: number, now: number): boolean {
  return now - ts < 7 * 24 * 60 * 60 * 1000;
}

function whenLabel(ts: number, now: number): string {
  const d = new Date(ts);
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (sameDay(ts, now)) return `Today ${time}`;
  if (sameWeek(ts, now)) return d.toLocaleDateString([], { weekday: "short" });
  return d.toLocaleDateString([], { day: "numeric", month: "short" });
}

/** A note as the backend stores it, before the list wrapper adds preview. */
type StoredNote = {
  id: string;
  title: string;
  body: string;
  createdAt: number;
  updatedAt: number;
  source: "typed" | "dictated";
};

export default function NotetakerScreen() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [enabled, setEnabled] = useState(false);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [now, setNow] = useState(() => Date.now());

  const dirtyRef = useRef(false);
  const dirtyTimer = useRef<number | null>(null);
  const savedTimer = useRef<number | null>(null);
  const noteRef = useRef<StoredNote | null>(null);

  const refresh = useCallback(async () => {
    const items = await invoke<Note[]>("list_notes").catch((e) => {
      console.error(e);
      return null;
    });
    if (items) setNotes(items);
    const open = await invoke<StoredNote | null>("get_open_note").catch(() => null);
    if (open) {
      noteRef.current = open;
      setSelectedId(open.id);
      setTitle(open.title);
      setBody(open.body);
      dirtyRef.current = false;
      setSaveState("idle");
    } else {
      noteRef.current = null;
      setSelectedId(null);
      setTitle("");
      setBody("");
      setSaveState("idle");
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = window.setInterval(() => setNow(Date.now()), 60 * 1000);
    return () => window.clearInterval(t);
  }, [refresh]);

  // Debounced autosave: any typed edit marks the note dirty and schedules a
  // save; switching notes or unmounting flushes a pending save first. The
  // timer callback goes through a ref so the debounce never captures a stale
  // editor snapshot.
  const flushSaveRef = useRef<() => Promise<void>>(async () => {});

  const markDirty = useCallback(() => {
    dirtyRef.current = true;
    setSaveState("saving");
    if (dirtyTimer.current) window.clearTimeout(dirtyTimer.current);
    dirtyTimer.current = window.setTimeout(() => {
      void flushSaveRef.current();
    }, AUTOSAVE_MS);
  }, []);

  const flushSave = useCallback(async () => {
    if (dirtyTimer.current) {
      window.clearTimeout(dirtyTimer.current);
      dirtyTimer.current = null;
    }
    const n = noteRef.current;
    if (!n || !dirtyRef.current) return;
    dirtyRef.current = false;
    setSaveState("saving");
    const res = await invoke<StoredNote | null>("save_note", {
      id: n.id,
      title,
      body,
    }).catch(() => null);
    if (res) {
      noteRef.current = res;
      setSaveState("saved");
      if (savedTimer.current) window.clearTimeout(savedTimer.current);
      savedTimer.current = window.setTimeout(() => setSaveState("idle"), 1500);
      setNow(Date.now());
      void refresh();
    } else {
      dirtyRef.current = false;
      setSaveState("idle");
    }
  }, [title, body, refresh]);

  flushSaveRef.current = flushSave;

  const selectNote = useCallback(
    async (id: string) => {
      if (id === selectedId) return;
      await flushSave();
      const n = notes.find((x) => x.id === id) ?? null;
      noteRef.current = n;
      setSelectedId(id);
      setTitle(n?.title ?? "");
      setBody(n?.body ?? "");
      setSaveState("idle");
      await invoke("open_note", { id }).catch(console.error);
    },
    [selectedId, notes, flushSave]
  );

  const createNote = useCallback(async () => {
    await flushSave();
    const n = await invoke<StoredNote>("create_note").catch(() => null);
    if (!n) return;
    noteRef.current = n;
    setSelectedId(n.id);
    setTitle("");
    setBody("");
    setSaveState("idle");
    await refresh();
  }, [flushSave, refresh]);

  const removeNote = useCallback(
    async (id: string) => {
      const n = notes.find((x) => x.id === id);
      if (!n) return;
      const label = n.title || "Untitled";
      const ok = await confirmDialog(`Delete "${label}"? This cannot be undone.`, {
        title: "Delete note",
      });
      if (!ok) return;
      await invoke("delete_note", { id }).catch(console.error);
      if (id === selectedId) {
        noteRef.current = null;
        setSelectedId(null);
        setTitle("");
        setBody("");
        setSaveState("idle");
      }
      await refresh();
    },
    [notes, selectedId, refresh]
  );

  const toggleEnabled = async (value: boolean) => {
    setEnabled(value);
    const settings = await invoke<Record<string, unknown>>("get_settings").catch(() => null);
    if (settings) {
      await invoke("save_settings", {
        settings: { ...settings, scratchpadEnabled: value },
      }).catch(console.error);
    }
  };

  useEffect(() => {
    invoke<{ scratchpadEnabled: boolean }>("get_settings")
      .then((s) => setEnabled(s.scratchpadEnabled))
      .catch(console.error);
  }, []);

  const q = query.trim().toLowerCase();
  const visible = q
    ? notes.filter(
        (n) =>
          n.title.toLowerCase().includes(q) || n.body.toLowerCase().includes(q)
      )
    : notes;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
      <div>
        <h2 className="tt-screen-title">Notetaker</h2>
        <p className="tt-screen-sub">
          Private notes for when you are not dictating into another app. Turn on
          routing, hold Fn, and your words land in the open note.
        </p>
      </div>

      <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => toggleEnabled(e.target.checked)}
          style={{ width: 16, height: 16, margin: 0, accentColor: "var(--accent)", flexShrink: 0 }}
        />
        <span style={{ fontSize: 13 }}>
          Route dictation here while the Teletype window is focused
        </span>
      </label>

      <div
        style={{
          display: "flex",
          gap: 0,
          height: "min(560px, calc(100vh - 340px))",
          minHeight: 360,
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {/* List pane */}
        <div
          style={{
            width: 260,
            flexShrink: 0,
            borderRight: "1px solid var(--border)",
            display: "flex",
            flexDirection: "column",
            minHeight: 0,
          }}
        >
          <div style={{ padding: 10, borderBottom: "1px solid var(--border)", display: "flex", flexDirection: "column", gap: 8 }}>
            <button className="primary" onClick={() => void createNote()} style={{ width: "100%" }}>
              <Icon name="plus" size={15} /> New note
            </button>
            <div style={{ position: "relative" }}>
              <span style={{ position: "absolute", left: 8, top: "50%", transform: "translateY(-50%)", color: "var(--text-secondary)", display: "grid", placeItems: "center" }}>
                <Icon name="search" size={13} />
              </span>
              <input
                type="text"
                placeholder="Search notes"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                style={{
                  width: "100%",
                  boxSizing: "border-box",
                  padding: "6px 8px 6px 28px",
                  fontSize: 13,
                  color: "var(--text)",
                  background: "var(--surface-2)",
                  border: "1px solid var(--border)",
                  borderRadius: "var(--radius-sm)",
                  outline: "none",
                }}
              />
            </div>
          </div>
          <div className="tt-scroll-thin" style={{ flex: 1, minHeight: 0 }}>
            {visible.length === 0 && (
              <p style={{ padding: 16, fontSize: 13, color: "var(--text-secondary)" }}>
                {q ? "No notes match." : "No notes yet."}
              </p>
            )}
            {visible.map((n) => (
              <div
                key={n.id}
                onClick={() => void selectNote(n.id)}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") void selectNote(n.id);
                }}
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 3,
                  padding: "10px 12px",
                  borderBottom: "1px solid var(--border)",
                  cursor: "pointer",
                  background: n.id === selectedId ? "var(--surface-2)" : "transparent",
                  transition: "background 150ms",
                }}
              >
                <div style={{ display: "flex", alignItems: "baseline", gap: 8, minWidth: 0 }}>
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      fontSize: 13,
                      fontWeight: 600,
                      color: "var(--text)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {n.title || "Untitled"}
                  </span>
                  <span style={{ flexShrink: 0, fontSize: 11, color: "var(--text-secondary)" }}>
                    {whenLabel(n.updatedAt, now)}
                  </span>
                </div>
                <div style={{ display: "flex", gap: 8, alignItems: "center", minWidth: 0 }}>
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      fontSize: 12,
                      color: "var(--text-secondary)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {n.preview || "\u00A0"}
                  </span>
                  {n.source === "dictated" && (
                    <span title="Contains dictated text" style={{ flexShrink: 0, display: "grid", placeItems: "center", color: "var(--text-secondary)" }}>
                      <Icon name="mic" size={12} />
                    </span>
                  )}
                </div>
                <div
                  onClick={(e) => e.stopPropagation()}
                  style={{ display: "flex", justifyContent: "flex-end" }}
                >
                  <button
                    className="danger"
                    onClick={() => void removeNote(n.id)}
                    title="Delete note"
                    aria-label="Delete note"
                    style={{ padding: "2px 6px", fontSize: 12 }}
                  >
                    <Icon name="trash" size={13} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Editor pane */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", minHeight: 0 }}>
          {selectedId && noteRef.current ? (
            <>
              <div style={{ padding: "10px 14px 0", display: "flex", gap: 8, alignItems: "center" }}>
                <input
                  id="note-title"
                  type="text"
                  value={title}
                  onChange={(e) => {
                    setTitle(e.target.value);
                    markDirty();
                  }}
                  placeholder="Title"
                  aria-label="Note title"
                  style={{
                    flex: 1,
                    minWidth: 0,
                    padding: "6px 8px",
                    fontSize: 15,
                    fontWeight: 650,
                    color: "var(--text)",
                    background: "transparent",
                    border: "1px solid var(--border)",
                    borderRadius: "var(--radius-sm)",
                    outline: "none",
                  }}
                />
                <span
                  style={{
                    flexShrink: 0,
                    fontSize: 11,
                    color: "var(--text-secondary)",
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 5,
                    minWidth: 74,
                    justifyContent: "flex-end",
                    transition: "opacity 150ms",
                    opacity: saveState === "idle" ? 0 : 1,
                  }}
                >
                  {saveState === "saving" ? "Saving" : "Saved"}
                  <Icon name="check" size={12} />
                </span>
              </div>
              <textarea
                id="note-body"
                value={body}
                onChange={(e) => {
                  setBody(e.target.value);
                  markDirty();
                }}
                placeholder="Start typing, or dictating, here."
                aria-label="Note body"
                style={{
                  flex: 1,
                  margin: "10px 14px 14px",
                  padding: "10px 12px",
                  fontSize: 14,
                  lineHeight: 1.55,
                  color: "var(--text)",
                  background: "transparent",
                  border: "1px solid var(--border)",
                  borderRadius: "var(--radius-sm)",
                  outline: "none",
                  resize: "none",
                  minHeight: 0,
                  whiteSpace: "pre-wrap",
                  overflowWrap: "anywhere",
                }}
              />
            </>
          ) : (
            <div
              style={{
                flex: 1,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 10,
                color: "var(--text-secondary)",
                padding: 24,
                textAlign: "center",
              }}
            >
              <Icon name="notetaker" size={28} />
              <p style={{ fontSize: 14, margin: 0 }}>No note open</p>
              <p style={{ fontSize: 13, margin: 0, maxWidth: 280 }}>
                Pick a note on the left, or create one. With routing on, dictation
                lands in the open note.
              </p>
              <button className="primary" onClick={() => void createNote()}>
                <Icon name="plus" size={15} /> New note
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
