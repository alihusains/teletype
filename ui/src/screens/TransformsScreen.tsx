import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Transform {
  id: string;
  name: string;
  description: string;
  instruction: string;
  shortcut: string;
  enabled: boolean;
  built_in: boolean;
  auto_apply: boolean;
}

export default function TransformsScreen() {
  const [transforms, setTransforms] = useState<Transform[]>([]);
  const [autoApplyEnabled, setAutoApplyEnabled] = useState(true);
  const [editing, setEditing] = useState<Transform | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [testInput, setTestInput] = useState("");
  const [testOutput, setTestOutput] = useState("");
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    invoke<Transform[]>("list_transforms").then(setTransforms).catch(console.error);
  }, []);

  const refresh = () => {
    invoke<Transform[]>("list_transforms").then(setTransforms).catch(console.error);
  };

  const toggle = async (t: Transform) => {
    await invoke("update_transform", { transform: { ...t, enabled: !t.enabled } });
    refresh();
  };

  const deleteTransform = async (id: string) => {
    if (!confirm("Delete this transform?")) return;
    await invoke("delete_transform", { id });
    refresh();
  };

  const resetDefaults = async () => {
    await invoke("reset_transforms");
    refresh();
  };

  const runTest = async () => {
    if (!editing || !testInput) return;
    setTesting(true);
    setTestOutput("");
    try {
      const result = await invoke<string>("test_transform", {
        transformId: editing.id,
        input: testInput,
      });
      setTestOutput(result);
    } catch (e) {
      setTestOutput(`Error: ${e}`);
    }
    setTesting(false);
  };

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 }}>
        <div>
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>Transforms <span style={{ fontSize: 11, background: "var(--surface)", padding: "2px 6px", borderRadius: 4, marginLeft: 8 }}>Beta</span></h2>
          <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>Transform your dictated text into clearer, cleaner, more useful writing.</p>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button onClick={() => setShowCreate(true)}>+ Create New</button>
          <button onClick={resetDefaults}>Reset to defaults</button>
        </div>
      </div>

      <label style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16, cursor: "pointer" }}>
        <input type="checkbox" checked={autoApplyEnabled} onChange={(e) => setAutoApplyEnabled(e.target.checked)} />
        Auto Apply After Dictation
      </label>

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 8 }}>My Transforms</h3>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {transforms.map((t) => (
          <div key={t.id} style={{
            display: "flex", alignItems: "center", gap: 12,
            padding: "12px 16px", background: "var(--surface)",
            borderRadius: "var(--radius)", border: "1px solid var(--border)",
          }}>
            <div style={{ flex: 1 }}>
              <div style={{ fontWeight: 500 }}>{t.name} {t.built_in && <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>built-in</span>}</div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>{t.description}</div>
            </div>
            {t.shortcut && <kbd style={{ fontSize: 11, background: "var(--bg)", padding: "2px 6px", borderRadius: 4 }}>{t.shortcut}</kbd>}
            <button onClick={() => setEditing(editing?.id === t.id ? null : t)}>{editing?.id === t.id ? "Close" : "Edit"}</button>
            {!t.built_in && <button className="danger" onClick={() => deleteTransform(t.id)}>Delete</button>}
            <input type="checkbox" checked={t.enabled} onChange={() => toggle(t)} title="Enabled" />
          </div>
        ))}
      </div>

      {editing && (
        <div style={{ marginTop: 16, padding: 16, background: "var(--surface)", borderRadius: "var(--radius)", border: "1px solid var(--border)" }}>
          <h4 style={{ marginBottom: 12 }}>{editing.built_in ? "Edit" : "Edit"}: {editing.name}</h4>
          <div style={{ display: "grid", gap: 8 }}>
            <label>Instruction
              <textarea rows={4} style={{ width: "100%", marginTop: 4 }} defaultValue={editing.instruction} id="edit-instruction" />
            </label>
            <label>Shortcut
              <input style={{ width: "100%", marginTop: 4 }} defaultValue={editing.shortcut} id="edit-shortcut" placeholder="e.g. Cmd+Shift+1" />
            </label>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <label style={{ display: "flex", alignItems: "center", gap: 4 }}>
                <input type="checkbox" defaultChecked={editing.auto_apply} id="edit-autoapply" />
                Auto Apply
              </label>
              <button onClick={async () => {
                const instruction = (document.getElementById("edit-instruction") as HTMLTextAreaElement).value;
                const shortcut = (document.getElementById("edit-shortcut") as HTMLInputElement).value;
                const autoApply = (document.getElementById("edit-autoapply") as HTMLInputElement).checked;
                await invoke("update_transform", { transform: { ...editing, instruction, shortcut, auto_apply: autoApply } });
                refresh();
              }}>Save</button>
            </div>
          </div>
          <div style={{ marginTop: 16, borderTop: "1px solid var(--border)", paddingTop: 12 }}>
            <h4 style={{ marginBottom: 8 }}>Test</h4>
            <textarea rows={2} style={{ width: "100%", marginBottom: 8 }} placeholder="Type or paste text to test…" value={testInput} onChange={(e) => setTestInput(e.target.value)} />
            <button className="primary" onClick={runTest} disabled={testing || !testInput}>
              {testing ? "Running…" : "Transform"}
            </button>
            {testOutput && (
              <pre style={{ marginTop: 8, padding: 12, background: "var(--bg)", borderRadius: "var(--radius-sm)", whiteSpace: "pre-wrap", fontSize: 13 }}>
                {testOutput}
              </pre>
            )}
          </div>
        </div>
      )}

      {showCreate && (
        <div style={{ marginTop: 16, padding: 16, background: "var(--surface)", borderRadius: "var(--radius)", border: "1px solid var(--border)" }}>
          <h4 style={{ marginBottom: 12 }}>Create Transform</h4>
          <div style={{ display: "grid", gap: 8 }}>
            <input id="new-name" placeholder="Name" />
            <input id="new-desc" placeholder="Description" />
            <textarea id="new-instruction" rows={3} placeholder="Instruction (what the model should do)" />
            <div style={{ display: "flex", gap: 8 }}>
              <button className="primary" onClick={async () => {
                const name = (document.getElementById("new-name") as HTMLInputElement).value;
                const description = (document.getElementById("new-desc") as HTMLInputElement).value;
                const instruction = (document.getElementById("new-instruction") as HTMLTextAreaElement).value;
                if (!name || !instruction) return;
                const now = Date.now();
                await invoke("create_transform", {
                  transform: {
                    id: crypto.randomUUID(),
                    name, description, instruction,
                    shortcut: "", enabled: true, built_in: false,
                    language: "en", sort_order: 99, auto_apply: false,
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
    </div>
  );
}
