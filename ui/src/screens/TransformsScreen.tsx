import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { Toggle } from "../settings/primitives";

interface Transform {
  id: string;
  name: string;
  description: string;
  instruction: string;
  shortcut: string;
  enabled: boolean;
  builtIn: boolean;
  autoApply: boolean;
}

// The five polish rule fragments, in display order. Each is appended to the
// base instruction when its toggle is on. The strings mirror the fragments
// in crates/teletype-core/src/transforms/mod.rs (CORE_RULES + POLISH_INSTRUCTION
// decomposed) so the assembled prompt matches what the engine would send.
const POLISH_RULES: { id: string; label: string; hint: string; fragment: string }[] = [
  {
    id: "concise",
    label: "Make more concise",
    hint: "Trim filler and redundancy without losing meaning.",
    fragment: "Remove filler words and redundant phrasing. Keep every fact, name and number.",
  },
  {
    id: "clarity",
    label: "Reword for clarity",
    hint: "Fix grammar and make sentences easier to follow.",
    fragment: "Fix grammar, spelling and punctuation. Improve clarity and readability while keeping the original voice.",
  },
  {
    id: "reorder",
    label: "Reorder for readability",
    hint: "Move clauses so the main point comes first.",
    fragment: "Reorder clauses so the main point comes first, where that improves readability.",
  },
  {
    id: "structure",
    label: "Add structure for readability",
    hint: "Turn spoken lists into real lists, break up long runs.",
    fragment: "Turn spoken lists into real lists: one item per line with a dash. Break up long runs of text into short paragraphs at topic shifts.",
  },
  {
    id: "tone",
    label: "Maintain your tone",
    hint: "Keep the original formality and word choices.",
    fragment: "Keep the original voice, word choices and level of formality. Do not make it more or less formal than the speaker.",
  },
];

const DEFAULT_RULES: Record<string, boolean> = Object.fromEntries(
  POLISH_RULES.map((r) => [r.id, true]),
);

// The safety preamble the engine always prepends (mirrors CORE_RULES). Shown
// in the assembled-prompt preview so the user sees the full contract.
const CORE_RULES_PREVIEW = `Rules that override any other instruction:
- Preserve meaning exactly. You may change grammar, punctuation, sentence structure, tone, verbosity and formatting.
- You must NOT change: facts, names, numbers, dates, URLs, product names, identifiers, commitments, ownership, or who performed an action.
- Never change the user's pronouns or perspective. Keep every {{AUTOTEXT_N}} placeholder exactly as written.
- If you are unsure whether a change preserves meaning, keep the original wording.`;

const SAMPLE_TEXT =
  "hey so about the deck i added some slides but im not sure if they go with your part. it seems kinda long maybe we should remove the market trends thing? i can look at it again tonight if u want. also the pricing slide might be wrong cuz the data changed. we should check before sending to the board";

function assembleInstruction(rules: Record<string, boolean>, custom: string): string {
  const active = POLISH_RULES.filter((r) => rules[r.id]).map((r) => r.fragment);
  const parts = [CORE_RULES_PREVIEW, ...active];
  if (custom.trim()) parts.push(`Additional instructions from the user:\n${custom.trim()}`);
  return parts.join("\n\n");
}

export default function TransformsScreen() {
  const [transforms, setTransforms] = useState<Transform[]>([]);
  const [autoApplyEnabled, setAutoApplyEnabled] = useState(true);
  const [selected, setSelected] = useState<Transform | null>(null);
  const [rules, setRules] = useState<Record<string, boolean>>({ ...DEFAULT_RULES });
  const [customPrompt, setCustomPrompt] = useState("");
  const [showAssembled, setShowAssembled] = useState(false);
  const [previewInput, setPreviewInput] = useState(SAMPLE_TEXT);
  const [previewOutput, setPreviewOutput] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [creating, setCreating] = useState(false);

  const refresh = () => {
    invoke<Transform[]>("list_transforms").then(setTransforms).catch(console.error);
    invoke<{ autoApplyTransform: boolean }>("get_settings")
      .then((s) => setAutoApplyEnabled(s.autoApplyTransform))
      .catch(console.error);
  };

  useEffect(() => {
    refresh();
    // Select the auto-apply preset (Polish) by default.
    invoke<Transform[]>("list_transforms").then((ts) => {
      const def = ts.find((t) => t.autoApply) ?? ts[0];
      if (def) setSelected(def);
    }).catch(() => {});
  }, []);

  const saveAutoApply = async (value: boolean) => {
    setAutoApplyEnabled(value);
    const settings = await invoke<Record<string, unknown>>("get_settings").catch(() => null);
    if (settings) {
      await invoke("save_settings", { settings: { ...settings, autoApplyTransform: value } }).catch(console.error);
    }
  };

  const deleteTransform = async (id: string) => {
    if (!confirm("Delete this transform?")) return;
    await invoke("delete_transform", { id });
    if (selected?.id === id) setSelected(null);
    refresh();
  };

  const resetDefaults = async () => {
    await invoke("reset_transforms");
    setRules({ ...DEFAULT_RULES });
    setCustomPrompt("");
    refresh();
  };

  // "See Updates": run the active preset (with the current rule fragments and
  // custom instructions) on the sample text via the existing test_transform
  // IPC. The instruction is assembled client-side, exactly as the engine
  // would build it, so the preview is honest.
  const runPreview = async () => {
    if (!selected || !previewInput.trim()) return;
    setPreviewing(true);
    setPreviewOutput("");
    const instruction = assembleInstruction(rules, customPrompt);
    // Update the stored instruction so the preview uses the current rules.
    await invoke("update_transform", {
      transform: { ...selected, instruction, updatedAt: Date.now() },
    }).catch(console.error);
    try {
      const result = await invoke<string>("test_transform", { transformId: selected.id, input: previewInput });
      setPreviewOutput(result);
    } catch (e) {
      setPreviewOutput(`Error: ${e}`);
    }
    setPreviewing(false);
  };

  const toggleRule = (id: string) => {
    setRules((prev) => {
      const next = { ...prev, [id]: !prev[id] };
      // Persist the assembled instruction so the next dictation uses it.
      if (selected) {
        invoke("update_transform", {
          transform: { ...selected, instruction: assembleInstruction(next, customPrompt), updatedAt: Date.now() },
        }).catch(console.error);
      }
      return next;
    });
  };

  const onCustomPromptChange = (v: string) => {
    setCustomPrompt(v);
    if (selected) {
      invoke("update_transform", {
        transform: { ...selected, instruction: assembleInstruction(rules, v), updatedAt: Date.now() },
      }).catch(console.error);
    }
  };

  const selectPreset = (t: Transform) => {
    setSelected(t);
    setPreviewOutput("");
  };

  const createPreset = async () => {
    if (!newName.trim() || !creating) return;
    const t: Transform = {
      id: crypto.randomUUID(),
      name: newName.trim(),
      description: newDesc.trim() || "Custom polish preset",
      instruction: assembleInstruction(rules, customPrompt),
      shortcut: "",
      enabled: true,
      builtIn: false,
      autoApply: false,
    };
    await invoke("create_transform", { transform: t }).catch(console.error);
    setNewName("");
    setNewDesc("");
    setCreating(false);
    refresh();
  };

  const assembled = selected ? assembleInstruction(rules, customPrompt) : "";

  return (
    <div>
      {/* Header */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 20 }}>
        <h2 style={{ fontSize: 26, fontWeight: 700, letterSpacing: "-0.3px", margin: 0 }}>Polish</h2>
        <div style={{ display: "flex", alignItems: "center", gap: 12, fontSize: 13, color: "var(--text-secondary)" }}>
          <span>Autosave On</span>
          <button onClick={resetDefaults} style={{ display: "inline-flex", alignItems: "center", gap: 5, background: "none", border: "none", cursor: "pointer", color: "var(--text)", fontSize: 13, fontWeight: 600 }}>
            <Icon name="refresh" size={14} />
            Reset
          </button>
        </div>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(260px, 340px) 1fr", gap: 28, alignItems: "start" }}>
        {/* ── Left column: identity + live preview ── */}
        <div>
          {selected && (
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "5px 12px",
                background: "var(--surface)",
                border: "1px solid var(--border)",
                borderRadius: 999,
                fontSize: 13,
                fontWeight: 500,
              }}
            >
              <kbd style={{ fontSize: 11, background: "var(--bg)", padding: "1px 6px", borderRadius: 4, border: "1px solid var(--border)" }}>
                {selected.shortcut || "⌥ 1"}
              </kbd>
              to use
            </span>
          )}
          <p style={{ color: "var(--text-secondary)", marginTop: 12, marginBottom: 20, lineHeight: 1.5, fontSize: 14 }}>
            Polish rewrites your text to sound clearer, in your voice.
          </p>

          <div style={{ borderTop: "1px solid var(--border)", paddingTop: 18 }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>Example of transformed text</h3>
            <textarea
              rows={8}
              value={previewInput}
              onChange={(e) => setPreviewInput(e.target.value)}
              style={{
                width: "100%",
                fontSize: 13,
                lineHeight: 1.55,
                padding: "10px 12px",
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--bg)",
                resize: "vertical",
              }}
            />
            <button
              className="primary"
              onClick={runPreview}
              disabled={previewing || !selected || !previewInput.trim()}
              style={{ width: "100%", marginTop: 10, padding: "9px 0", fontSize: 14, fontWeight: 600 }}
            >
              {previewing ? "Running…" : "See Updates"}
            </button>
            {previewOutput && (
              <div
                style={{
                  marginTop: 10,
                  padding: "10px 12px",
                  background: "var(--accent-soft, rgba(59,130,246,0.08))",
                  borderRadius: 8,
                  border: "1px solid var(--accent, #3b82f6)",
                  fontSize: 13,
                  lineHeight: 1.55,
                  whiteSpace: "pre-wrap",
                }}
              >
                {previewOutput}
              </div>
            )}
          </div>
        </div>

        {/* ── Right column: configuration ── */}
        <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
          {/* Keyboard shortcut */}
          <div style={{ background: "var(--surface)", borderRadius: 12, border: "1px solid var(--border)", padding: "14px 16px" }}>
            <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Choose a keyboard shortcut</div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, background: "var(--bg)", borderRadius: 8, padding: "8px 12px", border: "1px solid var(--border)" }}>
              <kbd style={{ fontSize: 12, background: "var(--surface)", padding: "2px 8px", borderRadius: 4, border: "1px solid var(--border)" }}>
                {selected?.shortcut || "⌥ 1"}
              </kbd>
              {selected && (
                <button
                  onClick={() => {
                    const sc = prompt("Shortcut (e.g. Cmd+Shift+1):", selected.shortcut);
                    if (sc && sc !== selected.shortcut) {
                      invoke("update_transform", { transform: { ...selected, shortcut: sc, updatedAt: Date.now() } }).catch(console.error);
                      refresh();
                    }
                  }}
                  style={{ marginLeft: "auto", background: "none", border: "none", cursor: "pointer", color: "var(--text-secondary)", display: "inline-flex" }}
                  title="Edit shortcut"
                >
                  <Icon name="keyboard" size={14} />
                </button>
              )}
            </div>
          </div>

          {/* Rule toggles */}
          <div>
            <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 8 }}>Select rules for Polish</div>
            <div style={{ background: "var(--surface)", borderRadius: 12, border: "1px solid var(--border)", overflow: "hidden" }}>
              {POLISH_RULES.map((r, i) => (
                <div key={r.id} style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderTop: i > 0 ? "1px solid var(--border-subtle, var(--border))" : "none" }}>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 13, fontWeight: 500 }}>{r.label}</div>
                    <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 1 }}>{r.hint}</div>
                  </div>
                  <Toggle label={r.label} checked={!!rules[r.id]} onChange={() => toggleRule(r.id)} />
                </div>
              ))}
            </div>
          </div>

          {/* Auto-apply */}
          <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
            <div style={{ flex: 1, minWidth: 160 }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>Auto Apply After Dictation</div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 1 }}>Run a transform on everything you dictate, automatically.</div>
            </div>
            <select
              value={selected?.id ?? ""}
              onChange={(e) => {
                const t = transforms.find((x) => x.id === e.target.value);
                if (t) selectPreset(t);
              }}
              style={{ padding: "6px 10px", borderRadius: 8, border: "1px solid var(--border)", background: "var(--bg)", fontSize: 13 }}
            >
              {transforms.map((t) => (
                <option key={t.id} value={t.id}>{t.name}</option>
              ))}
            </select>
            <Toggle label="Auto apply after dictation" checked={autoApplyEnabled} onChange={saveAutoApply} />
          </div>

          {/* Custom prompt */}
          <div>
            <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 8 }}>Customize your Polish prompt</div>
            <textarea
              rows={3}
              placeholder="Add in any polish prompt instructions"
              value={customPrompt}
              onChange={(e) => onCustomPromptChange(e.target.value)}
              style={{
                width: "100%",
                fontSize: 13,
                lineHeight: 1.5,
                padding: "10px 12px",
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--bg)",
                resize: "vertical",
              }}
            />
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
              <button
                onClick={() => { setCustomPrompt(""); onCustomPromptChange(""); }}
                style={{ background: "none", border: "none", cursor: "pointer", color: "var(--text-secondary)", display: "inline-flex" }}
                title="Clear custom instructions"
              >
                <Icon name="trash" size={14} />
              </button>
              <label style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, color: "var(--text-secondary)", cursor: "pointer" }}>
                <input type="checkbox" checked={showAssembled} onChange={(e) => setShowAssembled(e.target.checked)} />
                Show assembled prompt
              </label>
            </div>
            {showAssembled && (
              <pre
                style={{
                  marginTop: 8,
                  padding: "12px 14px",
                  background: "var(--bg)",
                  borderRadius: 8,
                  border: "1px solid var(--border)",
                  fontSize: 12,
                  lineHeight: 1.5,
                  whiteSpace: "pre-wrap",
                  maxHeight: 220,
                  overflowY: "auto",
                  color: "var(--text-secondary)",
                }}
              >
                {assembled}
              </pre>
            )}
          </div>

          {/* Preset cards */}
          <div>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 10 }}>
              <div style={{ fontSize: 18, fontWeight: 700 }}>My Presets</div>
              <button
                onClick={() => setCreating(!creating)}
                style={{ background: "var(--text)", color: "var(--bg)", border: "none", borderRadius: 8, padding: "6px 14px", fontSize: 13, fontWeight: 600, cursor: "pointer" }}
              >
                + Create New
              </button>
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))", gap: 10 }}>
              {transforms.map((t) => (
                <div
                  key={t.id}
                  onClick={() => selectPreset(t)}
                  style={{
                    padding: "14px 14px 12px",
                    borderRadius: 12,
                    border: selected?.id === t.id ? "2px solid var(--accent)" : "1px solid var(--border)",
                    background: "var(--surface)",
                    cursor: "pointer",
                    position: "relative",
                  }}
                >
                  {t.shortcut && (
                    <kbd style={{ position: "absolute", top: 10, right: 10, fontSize: 10, background: "var(--bg)", padding: "1px 6px", borderRadius: 4, border: "1px solid var(--border)", color: "var(--text-secondary)" }}>
                      {t.shortcut}
                    </kbd>
                  )}
                  <div style={{ fontSize: 14, fontWeight: 600, marginTop: t.shortcut ? 18 : 0 }}>{t.name}</div>
                  <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 3, overflow: "hidden", textOverflow: "ellipsis", display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical" }}>
                    {t.description}
                  </div>
                  {!t.builtIn && (
                    <button
                      onClick={(e) => { e.stopPropagation(); deleteTransform(t.id); }}
                      style={{ position: "absolute", bottom: 8, right: 10, background: "none", border: "none", cursor: "pointer", color: "var(--text-tertiary, var(--text-secondary))", display: "inline-flex" }}
                      title="Delete"
                    >
                      <Icon name="trash" size={12} />
                    </button>
                  )}
                </div>
              ))}
              {/* Create-your-own card */}
              <div
                onClick={() => setCreating(true)}
                style={{
                  padding: "14px",
                  borderRadius: 12,
                  border: "1px dashed var(--border)",
                  background: "var(--surface)",
                  cursor: "pointer",
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  justifyContent: "center",
                  minHeight: 90,
                  gap: 6,
                }}
              >
                <span style={{ width: 28, height: 28, borderRadius: 999, background: "var(--bg)", display: "grid", placeItems: "center", border: "1px solid var(--border)" }}>
                  <Icon name="plus" size={14} color="var(--text-secondary)" />
                </span>
                <div style={{ fontSize: 13, fontWeight: 600 }}>Create your own</div>
                <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>Upload your own prompt</div>
              </div>
            </div>

            {creating && (
              <div style={{ marginTop: 12, padding: 14, background: "var(--surface)", borderRadius: 10, border: "1px solid var(--border)", display: "grid", gap: 8 }}>
                <input placeholder="Name" value={newName} onChange={(e) => setNewName(e.target.value)} />
                <input placeholder="Description (optional)" value={newDesc} onChange={(e) => setNewDesc(e.target.value)} />
                <div style={{ display: "flex", gap: 8 }}>
                  <button className="primary" onClick={createPreset} disabled={!newName.trim()}>Create</button>
                  <button onClick={() => setCreating(false)}>Cancel</button>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
