import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AutoTextEntry {
  id: string;
  trigger: string;
  replacement: string;
  description: string;
  enabled: boolean;
  scope: { everywhere?: boolean; application?: { app_id: string } };
}

export default function AutoTextScreen() {
  const [entries, setEntries] = useState<AutoTextEntry[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);

  useEffect(() => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
  }, []);

  const refresh = () => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
  };

  const remove = async (id: string) => {
    if (!confirm("Delete this entry?")) return;
    await invoke("delete_autotext", { id });
    refresh();
  };

  const toggle = async (e: AutoTextEntry) => {
    await invoke("update_autotext", { entry: { ...e, enabled: !e.enabled } });
    refresh();
  };

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 }}>
        <div>
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>AutoText</h2>
          <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
            Type short triggers that instantly expand into frequently used text.
          </p>
        </div>
        <button onClick={() => setShowCreate(true)}>+ Create New</button>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {entries.map((e) => (
          <div key={e.id} style={{
            display: "flex", alignItems: "center", gap: 12,
            padding: "10px 14px", background: "var(--surface)",
            borderRadius: "var(--radius)", border: "1px solid var(--border)",
          }}>
            <code style={{ color: "var(--accent)", minWidth: 100 }}>{e.trigger}</code>
            <span style={{ flex: 1, color: e.enabled ? "var(--text)" : "var(--text-secondary)" }}>{e.replacement}</span>
            {e.description && <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>{e.description}</span>}
            <button onClick={() => setEditId(editId === e.id ? null : e.id)}>{editId === e.id ? "Close" : "Edit"}</button>
            <button className="danger" onClick={() => remove(e.id)}>Delete</button>
            <input type="checkbox" checked={e.enabled} onChange={() => toggle(e)} title="Enabled" />
          </div>
        ))}
        {entries.length === 0 && (
          <p style={{ color: "var(--text-secondary)", padding: 20, textAlign: "center" }}>
            No AutoText entries yet. Create one to get started.
          </p>
        )}
      </div>

      {showCreate && (
        <div style={{ marginTop: 16, padding: 16, background: "var(--surface)", borderRadius: "var(--radius)", border: "1px solid var(--border)" }}>
          <h4 style={{ marginBottom: 12 }}>Create AutoText</h4>
          <div style={{ display: "grid", gap: 8 }}>
            <label>Trigger <input id="at-trigger" placeholder="/email" style={{ width: "100%", marginTop: 4 }} /></label>
            <label>Replacement <input id="at-replacement" placeholder="user@example.com" style={{ width: "100%", marginTop: 4 }} /></label>
            <label>Description <input id="at-desc" placeholder="Work email" style={{ width: "100%", marginTop: 4 }} /></label>
            <div style={{ display: "flex", gap: 8 }}>
              <button className="primary" onClick={async () => {
                const trigger = (document.getElementById("at-trigger") as HTMLInputElement).value;
                const replacement = (document.getElementById("at-replacement") as HTMLInputElement).value;
                const description = (document.getElementById("at-desc") as HTMLInputElement).value;
                if (!trigger || !replacement) return;
                const now = Date.now();
                await invoke("create_autotext", {
                  entry: {
                    id: crypto.randomUUID(),
                    trigger, replacement, description,
                    enabled: true, scope: { everywhere: true },
                    created_at: now, updated_at: now,
                  },
                });
                setShowCreate(false);
                refresh();
              }}>Create</button>
              <button onClick={() => setShowCreate(false)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {editId && (
        <div style={{ marginTop: 16, padding: 16, background: "var(--surface)", borderRadius: "var(--radius)", border: "1px solid var(--border)" }}>
          <h4 style={{ marginBottom: 12 }}>Edit Entry</h4>
          <div style={{ display: "grid", gap: 8 }}>
            <label>Trigger <input id="ed-trigger" defaultValue={entries.find(e => e.id === editId)?.trigger} style={{ width: "100%", marginTop: 4 }} /></label>
            <label>Replacement <input id="ed-replacement" defaultValue={entries.find(e => e.id === editId)?.replacement} style={{ width: "100%", marginTop: 4 }} /></label>
            <label>Description <input id="ed-desc" defaultValue={entries.find(e => e.id === editId)?.description} style={{ width: "100%", marginTop: 4 }} /></label>
            <div style={{ display: "flex", gap: 8 }}>
              <button className="primary" onClick={async () => {
                const entry = entries.find(e => e.id === editId);
                if (!entry) return;
                await invoke("update_autotext", {
                  entry: {
                    ...entry,
                    trigger: (document.getElementById("ed-trigger") as HTMLInputElement).value,
                    replacement: (document.getElementById("ed-replacement") as HTMLInputElement).value,
                    description: (document.getElementById("ed-desc") as HTMLInputElement).value,
                  },
                });
                setEditId(null);
                refresh();
              }}>Save</button>
              <button onClick={() => setEditId(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
