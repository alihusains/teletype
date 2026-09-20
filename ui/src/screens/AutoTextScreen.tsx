import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

interface AutoTextEntry {
  id: string;
  trigger: string;
  replacement: string;
  description: string;
  enabled: boolean;
  scope: { everywhere?: boolean; application?: { app_id: string } };
  created_at: number;
  updated_at: number;
  snippet: string;
}

const ACCENT = "#2563eb";

function EntryForm({
  initial,
  onSave,
  onCancel,
}: {
  initial: AutoTextEntry | null;
  onSave: (e: AutoTextEntry) => void;
  onCancel: () => void;
}) {
  const [trigger, setTrigger] = useState(initial?.trigger ?? "");
  const [snippet, setSnippet] = useState(initial?.snippet ?? "");
  const [replacement, setReplacement] = useState(initial?.replacement ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [error, setError] = useState("");

  const submit = async () => {
    if (!trigger.trim() || !replacement.trim()) {
      setError("Trigger and replacement are required.");
      return;
    }
    const now = Date.now();
    const entry: AutoTextEntry = {
      id: initial?.id ?? crypto.randomUUID(),
      trigger: trigger.trim(),
      snippet: snippet.trim(),
      replacement: replacement.trim(),
      description: description.trim(),
      enabled: initial?.enabled ?? true,
      scope: initial?.scope ?? { everywhere: true },
      created_at: initial?.created_at ?? now,
      updated_at: now,
    };
    try {
      if (initial) {
        await invoke("update_autotext", { entry });
      } else {
        await invoke("create_autotext", { entry });
      }
      onSave(entry);
    } catch (e) {
      setError(String(e));
    }
  };

  const inputStyle: React.CSSProperties = { width: "100%", marginTop: 4, background: "var(--surface-2)" };

  return (
    <div style={{ marginTop: 16, padding: 18, background: "var(--surface)", borderRadius: "var(--radius)", border: "1px solid var(--border)" }}>
      <h4 style={{ marginBottom: 14, fontSize: 15, fontWeight: 700 }}>{initial ? "Edit snippet" : "Add new snippet"}</h4>
      <div style={{ display: "grid", gap: 12 }}>
        <div>
          <label style={{ fontSize: 13, fontWeight: 600 }}>Say (voice trigger)</label>
          <input value={snippet} onChange={(e) => setSnippet(e.target.value)} placeholder="my email" style={inputStyle} />
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4 }}>
            The words you speak to insert this. Leave empty for a typed-only shortcut.
          </div>
        </div>
        <div>
          <label style={{ fontSize: 13, fontWeight: 600 }}>Type (shortcut)</label>
          <input value={trigger} onChange={(e) => setTrigger(e.target.value)} placeholder="/email" style={inputStyle} />
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4 }}>
            A /slash trigger for when you type instead of speak.
          </div>
        </div>
        <div>
          <label style={{ fontSize: 13, fontWeight: 600 }}>Expands to</label>
          <textarea value={replacement} onChange={(e) => setReplacement(e.target.value)} placeholder="abcd@gmail.com" rows={2} style={{ ...inputStyle, resize: "vertical" }} />
        </div>
        <div>
          <label style={{ fontSize: 13, fontWeight: 600 }}>Description (optional)</label>
          <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Personal email" style={inputStyle} />
        </div>
        {error && <div style={{ fontSize: 13, color: "var(--danger)" }}>{error}</div>}
        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
          <button className="primary" onClick={submit}>{initial ? "Save" : "Add snippet"}</button>
          <button onClick={onCancel}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

export default function AutoTextScreen() {
  const [entries, setEntries] = useState<AutoTextEntry[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
  }, []);

  const refresh = () => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
  };

  const remove = async (id: string) => {
    if (!confirm("Delete this snippet?")) return;
    await invoke("delete_autotext", { id });
    refresh();
  };

  const toggle = async (e: AutoTextEntry) => {
    await invoke("update_autotext", { entry: { ...e, enabled: !e.enabled } });
    refresh();
  };

  const filtered = entries.filter((e) => {
    if (!query.trim()) return true;
    const q = query.toLowerCase();
    return (
      e.trigger.toLowerCase().includes(q) ||
      e.snippet.toLowerCase().includes(q) ||
      e.replacement.toLowerCase().includes(q) ||
      e.description.toLowerCase().includes(q)
    );
  });

  const spoken = entries.filter((e) => e.snippet.trim().length > 0);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 860 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 12 }}>
        <div>
          <h2 style={{ fontSize: 20, fontWeight: 800 }}>Snippets</h2>
          <p style={{ color: "var(--text-secondary)", marginTop: 4, fontSize: 13 }}>
            The stuff you shouldn't have to re-type. Say a phrase to drop in an email, link, or signature instantly.
          </p>
        </div>
        <button className="primary" onClick={() => { setShowCreate(true); setEditId(null); }}>+ Add new</button>
      </div>

      {/* Hero */}
      <div
        style={{
          borderRadius: "var(--radius)",
          padding: "24px 26px",
          color: "#fff",
          background: "linear-gradient(135deg, #111827 0%, #1e293b 60%, #334155 100%)",
          position: "relative",
        }}
      >
        <h3 style={{ fontSize: 22, fontWeight: 700, margin: 0, letterSpacing: -0.3 }}>
          The stuff <em style={{ fontFamily: "Georgia, serif" }}>you</em> shouldn't have to re-type.
        </h3>
        <p style={{ opacity: 0.85, marginTop: 8, fontSize: 13, maxWidth: 560 }}>
          Save text you type often — an email, intro, or prompt — then say a word to drop it in instantly.
        </p>
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 18, maxWidth: 640 }}>
          {(spoken.length ? spoken : entries).slice(0, 3).map((e) => (
            <div key={e.id} style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
              <span style={{ background: "rgba(255,255,255,0.14)", borderRadius: 8, padding: "6px 12px", fontSize: 13, fontStyle: "italic" }}>
                "{e.snippet.trim() || e.trigger}"
              </span>
              <span style={{ opacity: 0.6 }}>→</span>
              <span style={{ background: "rgba(255,255,255,0.10)", borderRadius: 8, padding: "6px 12px", fontSize: 13, maxWidth: 360, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {e.replacement}
              </span>
            </div>
          ))}
          {entries.length === 0 && (
            <p style={{ opacity: 0.7, fontSize: 13 }}>No snippets yet — add your first one below.</p>
          )}
        </div>
        <button
          onClick={() => { setShowCreate(true); setEditId(null); }}
          style={{ marginTop: 18, background: "#fff", color: "#111827", border: "none", borderRadius: 10, padding: "9px 16px", fontWeight: 600, fontSize: 13 }}
        >
          Add new snippet
        </button>
      </div>

      {/* Search */}
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div style={{ position: "relative", flex: 1, maxWidth: 320 }}>
          <span style={{ position: "absolute", left: 10, top: "50%", transform: "translateY(-50%)", color: "var(--text-secondary)" }}>
            <Icon name="search" size={16} />
          </span>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search snippets…"
            style={{ width: "100%", paddingLeft: 34, background: "var(--surface)", border: "1px solid var(--border)", borderRadius: "var(--radius-sm)" }}
          />
        </div>
      </div>

      {/* List */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {filtered.map((e) => (
          <div
            key={e.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "12px 16px",
              background: "var(--surface)",
              borderRadius: "var(--radius)",
              border: "1px solid var(--border)",
              opacity: e.enabled ? 1 : 0.6,
            }}
          >
            <div style={{ width: 32, height: 32, borderRadius: 8, background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
              <Icon name="messages-square" size={17} color={ACCENT} />
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                {e.snippet.trim() ? (
                  <span style={{ fontSize: 13, fontWeight: 600, fontStyle: "italic" }}>"{e.snippet.trim()}"</span>
                ) : (
                  <code style={{ color: ACCENT, fontSize: 13 }}>{e.trigger}</code>
                )}
                <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>→</span>
                <span style={{ fontSize: 13, color: e.enabled ? "var(--text)" : "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {e.replacement}
                </span>
              </div>
              {e.description && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>{e.description}</div>}
            </div>
            <button onClick={() => { setEditId(e.id); setShowCreate(false); }}>Edit</button>
            <button className="danger" onClick={() => remove(e.id)}>Delete</button>
            <input type="checkbox" checked={e.enabled} onChange={() => toggle(e)} title="Enabled" />
          </div>
        ))}
        {filtered.length === 0 && (
          <p style={{ color: "var(--text-secondary)", padding: 24, textAlign: "center" }}>
            {entries.length === 0 ? "No snippets yet. Add one to get started." : "No snippets match your search."}
          </p>
        )}
      </div>

      {showCreate && (
        <EntryForm
          initial={null}
          onCancel={() => setShowCreate(false)}
          onSave={() => { setShowCreate(false); refresh(); }}
        />
      )}
      {editId && (
        <EntryForm
          initial={entries.find((e) => e.id === editId) ?? null}
          onCancel={() => setEditId(null)}
          onSave={() => { setEditId(null); refresh(); }}
        />
      )}
    </div>
  );
}
