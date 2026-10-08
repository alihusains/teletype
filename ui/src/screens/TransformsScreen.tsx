import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { Toggle } from "../settings/primitives";
import HotkeyRecorder, { displayHotkey } from "../components/HotkeyRecorder";
import { Segmented } from "../settings/primitives";

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

// The two list formats the transform can emit. Mirrors the backend
// `ListStyle` enum (crates/teletype-core/src/transforms/mod.rs): "bullets"
// ("- item") is the default, "numbered" is "1. item".
type ListStyle = "bullets" | "numbered";
const LIST_STYLE_OPTIONS: { value: ListStyle; label: string; title: string }[] = [
  { value: "bullets", label: "• Bullets", title: "Dash bullets: - item" },
  { value: "numbered", label: "1. 2. 3.", title: "Numbered: 1. item" },
];

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

// One user-added Polish instruction (Wispr-style list). Mirrors the
// backend `PolishInstruction` (crates/teletype-desktop/src/commands.rs).
export interface PolishInstruction {
  id: string;
  text: string;
  enabled: boolean;
}

function assembleInstruction(
  rules: Record<string, boolean>,
  instructions: PolishInstruction[],
): string {
  const active = POLISH_RULES.filter((r) => rules[r.id]).map((r) => r.fragment);
  const parts = [CORE_RULES_PREVIEW, ...active];
  const extra = instructions.filter((i) => i.enabled && i.text.trim());
  if (extra.length > 0) {
    parts.push(
      `Additional instructions from the user:\n${extra.map((i) => i.text.trim()).join("\n")}`,
    );
  }
  return parts.join("\n\n");
}

// The base (locked) prompt shown as the first list item, per preset. For the
// auto-apply Polish preset it is the rule-fragment assembly (minus the core
// safety preamble, which is always on and not worth showing twice); for any
// other preset it is the preset's own stored instruction.
function basePromptFor(t: Transform): string {
  if (t.id === "builtin-polish" || t.autoApply) {
    return (
      assembleInstruction(DEFAULT_RULES, [])
        .split("\n\n")
        .slice(1)
        .join("\n\n") || CORE_RULES_PREVIEW
    );
  }
  return t.instruction || "";
}

// The full instruction the engine would send for a preset: the base prompt
// plus the user's enabled instructions. The shared core rules are always
// prepended (they mirror the backend CORE_RULES that every transform carries).
// The Polish preset's base already bakes in the rule-fragment selection.
function composeInstruction(t: Transform, instructions: PolishInstruction[]): string {
  const base = basePromptFor(t);
  const extra = instructions.filter((i) => i.enabled && i.text.trim()).map((i) => i.text.trim());
  const parts = [CORE_RULES_PREVIEW, base];
  if (extra.length > 0) parts.push(`Additional instructions from the user:\n${extra.join("\n")}`);
  return parts.join("\n\n");
}

export default function TransformsScreen() {
  const [transforms, setTransforms] = useState<Transform[]>([]);
  const [autoApplyEnabled, setAutoApplyEnabled] = useState(true);
  const [selected, setSelected] = useState<Transform | null>(null);
  const [rules, setRules] = useState<Record<string, boolean>>({ ...DEFAULT_RULES });
  // Wispr-style "Customize your Polish prompt" list. The base prompt is the
  // first, locked item (rendered from the rule assembly, not stored); these
  // are the user-added fragments, each with a trash + toggle.
  const [customInstructions, setCustomInstructions] = useState<PolishInstruction[]>([]);
  const [draftInstruction, setDraftInstruction] = useState("");
  const [showAssembled, setShowAssembled] = useState(false);
  const [previewInput, setPreviewInput] = useState(SAMPLE_TEXT);
  const [previewOutput, setPreviewOutput] = useState("");
  const [previewing, setPreviewing] = useState(false);
  // "Create your own" modal (item 4): a proper dialog with a name field, a
  // keyboard-shortcut recorder, and a custom prompt, plus Create/Cancel.
  const [modalOpen, setModalOpen] = useState(false);
  const [modalName, setModalName] = useState("");
  const [modalShortcut, setModalShortcut] = useState("");
  const [modalPrompt, setModalPrompt] = useState("");
  // The shortcut currently shown in the "Choose a keyboard shortcut" field.
  // Kept in state (not read live from `selected`) so the recorder's value
  // prop is stable while a capture is in flight.
  const [shortcutDraft, setShortcutDraft] = useState<string>("");
  // How lists should be formatted in polished output (item 3). Persisted in
  // Settings.listStyle so it reaches every transform run.
  const [listStyle, setListStyle] = useState<ListStyle>("bullets");

  const refresh = () => {
    invoke<Transform[]>("list_transforms").then(setTransforms).catch(console.error);
    invoke<{
      autoApplyTransform: boolean;
      polishRules?: Record<string, boolean>;
      listStyle?: string;
      polishCustomInstructions?: PolishInstruction[];
    }>("get_settings")
      .then((s) => {
        setAutoApplyEnabled(s.autoApplyTransform);
        if (s.polishRules && Object.keys(s.polishRules).length > 0) {
          setRules((prev) => ({ ...prev, ...s.polishRules }));
        }
        if (s.listStyle === "numbered" || s.listStyle === "bullets") {
          setListStyle(s.listStyle);
        }
        if (Array.isArray(s.polishCustomInstructions)) {
          setCustomInstructions(s.polishCustomInstructions);
        }
      })
      .catch(console.error);
  };

  // Persist the Wispr-style instruction list to Settings, then refresh the
  // stored Polish instruction so the next dictation run uses it.
  const persistInstructions = (next: PolishInstruction[]) => {
    setCustomInstructions(next);
    invoke<Record<string, unknown>>("get_settings").then((s) => {
      invoke("save_settings", { settings: { ...s, polishCustomInstructions: next } }).catch(console.error);
    }).catch(() => {});
    // Compose and store the full instruction for the currently selected
    // preset (base prompt + enabled user instructions). Works for every
    // preset, so the list applies to Professional / Rewriter / Prompt
    // Engineer / custom presets too.
    if (selected) {
      invoke("update_transform", {
        transform: { ...selected, instruction: composeInstruction(selected, next), updatedAt: Date.now() },
      }).catch(console.error);
    }
  };

  const addInstruction = () => {
    const text = draftInstruction.trim();
    if (!text) return;
    persistInstructions([...customInstructions, { id: crypto.randomUUID(), text, enabled: true }]);
    setDraftInstruction("");
  };

  useEffect(() => {
    refresh();
    // Select the auto-apply preset (Polish) by default.
    invoke<Transform[]>("list_transforms").then((ts) => {
      const def = ts.find((t) => t.autoApply) ?? ts[0];
      if (def) {
        setSelected(def);
        setShortcutDraft(def.shortcut || "");
      }
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
    setCustomInstructions([]);
    setDraftInstruction("");
    setShortcutDraft("");
    // Reset the stored Polish instruction to the default assembly and clear
    // the user-added instruction list.
    const settings = await invoke<Record<string, unknown>>("get_settings").catch(() => null);
    if (settings) {
      await invoke("save_settings", { settings: { ...settings, polishCustomInstructions: [] } }).catch(console.error);
    }
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
    // Refresh the stored instruction from the current rules + user
    // instructions before previewing, so the preview uses exactly what the
    // engine would send.
    await invoke("update_transform", {
      transform: { ...selected, instruction: composeInstruction(selected, customInstructions), updatedAt: Date.now() },
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
      // Persist the rule state in Settings so it survives app restarts.
      invoke<{ polishRules?: Record<string, boolean> }>("get_settings").then((s) => {
        invoke("save_settings", { settings: { ...s, polishRules: next } }).catch(console.error);
      }).catch(() => {});
      // Persist the assembled instruction so the next dictation uses it.
      if (selected) {
        invoke("update_transform", {
          transform: { ...selected, instruction: composeInstruction(selected, customInstructions), updatedAt: Date.now() },
        }).catch(console.error);
      }
      return next;
    });
  };

  const selectPreset = (t: Transform) => {
    setSelected(t);
    setPreviewOutput("");
    // Load the preset's actual stored instruction so the user sees and can
    // edit THAT preset's prompt (e.g. Prompt Engineer), not a reassembly of
    // the Polish rule toggles. The rule toggles below only apply to the
    // auto-apply Polish preset; for other presets the stored instruction is
    // the source of truth.
    setShortcutDraft(t.shortcut || "");
  };

  // Persist the list-style choice to Settings (item 3). The backend reads
  // Settings.listStyle on every transform run and renders it into the prompt.
  const saveListStyle = (v: ListStyle) => {
    setListStyle(v);
    invoke<Record<string, unknown>>("get_settings").then((s) => {
      invoke("save_settings", { settings: { ...s, listStyle: v } }).catch(console.error);
    }).catch(() => {});
  };

  // Create a preset from the "Create your own" modal (item 4). The shortcut
  // is registered as a global transform shortcut by the backend at save time,
  // so the user can press it anywhere to polish the current selection.
  const createFromModal = async () => {
    if (!modalName.trim()) return;
    const t: Transform = {
      id: crypto.randomUUID(),
      name: modalName.trim(),
      description: "Custom polish preset",
      instruction: modalPrompt.trim() || "Polish the text: fix grammar, spelling and punctuation.",
      shortcut: modalShortcut,
      enabled: true,
      builtIn: false,
      autoApply: false,
    };
    await invoke("create_transform", { transform: t }).catch(console.error);
    setModalOpen(false);
    setModalName("");
    setModalShortcut("");
    setModalPrompt("");
    refresh();
  };

  // The full prompt shown in the "Show assembled prompt" preview: the base
  // prompt plus the user's enabled instructions, exactly what the engine
  // sends for the selected preset.
  const shownPrompt = selected ? composeInstruction(selected, customInstructions) : "";

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
                {selected.shortcut ? displayHotkey(selected.shortcut) : "—"}
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
            {selected ? (
              <div style={{ display: "flex", alignItems: "center", gap: 14, flexWrap: "wrap" }}>
                <HotkeyRecorder
                  value={shortcutDraft}
                  onSave={(sc) => {
                    setShortcutDraft(sc);
                    invoke("update_transform", {
                      transform: { ...selected, shortcut: sc, updatedAt: Date.now() },
                    })
                      .then(() => refresh())
                      .catch((e) => {
                        // Registration failed (e.g. bare modifiers): revert
                        // the display so the UI never shows a dead binding.
                        setShortcutDraft(selected.shortcut || "");
                        alert(`Could not set shortcut: ${e}`);
                      });
                  }}
                />
                {shortcutDraft && (
                  <button
                    onClick={() => {
                      setShortcutDraft("");
                      invoke("update_transform", {
                        transform: { ...selected, shortcut: "", updatedAt: Date.now() },
                      })
                        .then(() => refresh())
                        .catch(console.error);
                    }}
                    style={{ background: "none", border: "none", cursor: "pointer", color: "var(--text-secondary)", fontSize: 12 }}
                  >
                    Clear
                  </button>
                )}
              </div>
            ) : (
              <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>Select a preset to set its shortcut.</div>
            )}
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

          {/* List style (item 3): how enumerated lists are formatted in the
              polished output. Persisted in Settings.listStyle and rendered
              into the prompt for every transform. */}
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}>
            <div style={{ flex: 1, minWidth: 160 }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>List format</div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 1 }}>
                How polished output formats lists you dictate.
              </div>
            </div>
            <Segmented label="List format" value={listStyle} options={LIST_STYLE_OPTIONS} onChange={saveListStyle} />
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

          {/* Custom prompt — a Wispr-style instruction list, available for
              every preset. The base prompt is the first, locked item; the
              user adds, toggles and removes instructions on top of it. */}
          {selected && (
            <div>
              <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 8 }}>
                Customize your {selected.name} prompt
              </div>
              <div style={{ background: "var(--surface)", borderRadius: 12, border: "1px solid var(--border)", overflow: "hidden" }}>
                {/* Base prompt: the first, locked item (no trash/toggle). */}
                <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--border-subtle, var(--border))", fontSize: 13, lineHeight: 1.5, color: "var(--text)", whiteSpace: "pre-wrap" }}>
                  {basePromptFor(selected)}
                </div>
                {/* User-added instructions: each with trash + toggle. */}
                {customInstructions.map((ins) => (
                  <div
                    key={ins.id}
                    style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "12px 16px", borderTop: "1px solid var(--border-subtle, var(--border))" }}
                  >
                    <div style={{ flex: 1, minWidth: 0, fontSize: 13, lineHeight: 1.5, whiteSpace: "pre-wrap" }}>
                      {ins.text}
                    </div>
                    <button
                      onClick={() => persistInstructions(customInstructions.filter((x) => x.id !== ins.id))}
                      style={{ background: "none", border: "none", cursor: "pointer", color: "var(--text-tertiary, var(--text-secondary))", display: "inline-flex" }}
                      title="Remove instruction"
                    >
                      <Icon name="trash" size={14} />
                    </button>
                    <Toggle
                      label={ins.text}
                      checked={ins.enabled}
                      onChange={() =>
                        persistInstructions(customInstructions.map((x) => (x.id === ins.id ? { ...x, enabled: !x.enabled } : x)))
                      }
                    />
                  </div>
                ))}
                {/* Add a new instruction. */}
                <div style={{ padding: "12px 16px", borderTop: "1px solid var(--border-subtle, var(--border))" }}>
                  <textarea
                    rows={2}
                    placeholder="Add an instruction on top of this prompt"
                    value={draftInstruction}
                    onChange={(e) => setDraftInstruction(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                        e.preventDefault();
                        addInstruction();
                      }
                    }}
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
                  <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 8 }}>
                    <button
                      onClick={() => setDraftInstruction("")}
                      disabled={!draftInstruction.trim()}
                      style={{ fontSize: 13 }}
                    >
                      Cancel
                    </button>
                    <button
                      className="primary"
                      onClick={addInstruction}
                      disabled={!draftInstruction.trim()}
                      style={{ fontSize: 13 }}
                    >
                      Add
                    </button>
                  </div>
                </div>
              </div>
              <label style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, color: "var(--text-secondary)", cursor: "pointer", marginTop: 8 }}>
                <input type="checkbox" checked={showAssembled} onChange={(e) => setShowAssembled(e.target.checked)} />
                Show assembled prompt
              </label>
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
                  {shownPrompt}
                </pre>
              )}
            </div>
          )}

          {/* Preset cards */}
          <div>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 10 }}>
              <div style={{ fontSize: 18, fontWeight: 700 }}>My Presets</div>
              <button
                onClick={() => setModalOpen(true)}
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
                onClick={() => setModalOpen(true)}
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

          </div>
        </div>
      </div>

      {/* "Create your own" modal (item 4). A real dialog: name, a keyboard
          shortcut recorder, and a custom prompt, with Create/Cancel at the
          bottom — matching the Wispr Flow / EnviousWispr flow. */}
      {modalOpen && (
        <div
          onClick={() => setModalOpen(false)}
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.45)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
            padding: 20,
          }}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-label="Create your own polish preset"
            style={{
              width: "100%",
              maxWidth: 440,
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: 16,
              boxShadow: "var(--shadow-2, 0 12px 40px rgba(0,0,0,0.35))",
              display: "flex",
              flexDirection: "column",
              maxHeight: "90vh",
            }}
          >
            <div style={{ padding: "18px 20px 0" }}>
              <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Create your own</h3>
              <p style={{ margin: "4px 0 0", fontSize: 13, color: "var(--text-secondary)" }}>
                Name it, give it a shortcut, and write the prompt it should follow.
              </p>
            </div>

            <div style={{ padding: "16px 20px", display: "flex", flexDirection: "column", gap: 16, overflowY: "auto" }}>
              {/* Name */}
              <div>
                <label style={{ display: "block", fontSize: 13, fontWeight: 600, marginBottom: 6 }}>Name</label>
                <input
                  autoFocus
                  value={modalName}
                  onChange={(e) => setModalName(e.target.value)}
                  placeholder="e.g. Executive Summary"
                  style={{ width: "100%", padding: "9px 12px", borderRadius: 8, border: "1px solid var(--border)", background: "var(--bg)", fontSize: 14 }}
                />
              </div>

              {/* Keyboard shortcut */}
              <div>
                <label style={{ display: "block", fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
                  Choose a keyboard shortcut
                </label>
                <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
                  <HotkeyRecorder value={modalShortcut} onSave={setModalShortcut} />
                  {modalShortcut && (
                    <button
                      onClick={() => setModalShortcut("")}
                      style={{ background: "none", border: "1px solid var(--border)", borderRadius: 8, padding: "5px 12px", fontSize: 12, cursor: "pointer", color: "var(--text-secondary)" }}
                    >
                      Clear
                    </button>
                  )}
                </div>
                <p style={{ margin: "8px 0 0", fontSize: 12, color: "var(--text-tertiary, var(--text-secondary))" }}>
                  Press it anywhere to polish the text you have selected in that app.
                </p>
              </div>

              {/* Custom prompt */}
              <div>
                <label style={{ display: "block", fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
                  Customize prompt
                </label>
                <textarea
                  rows={4}
                  value={modalPrompt}
                  onChange={(e) => setModalPrompt(e.target.value)}
                  placeholder="e.g. Rewrite as a tight executive summary, max 3 sentences, no jargon."
                  style={{ width: "100%", padding: "10px 12px", borderRadius: 8, border: "1px solid var(--border)", background: "var(--bg)", fontSize: 13, lineHeight: 1.5, resize: "vertical" }}
                />
              </div>
            </div>

            <div
              style={{
                padding: "14px 20px",
                borderTop: "1px solid var(--border)",
                display: "flex",
                justifyContent: "flex-end",
                gap: 10,
              }}
            >
              <button
                onClick={() => setModalOpen(false)}
                style={{ background: "none", border: "1px solid var(--border)", borderRadius: 8, padding: "8px 16px", fontSize: 13, cursor: "pointer", color: "var(--text)" }}
              >
                Cancel
              </button>
              <button
                onClick={createFromModal}
                disabled={!modalName.trim()}
                style={{
                  background: modalName.trim() ? "var(--accent)" : "var(--surface-hover)",
                  color: "var(--bg)",
                  border: "none",
                  borderRadius: 8,
                  padding: "8px 18px",
                  fontSize: 13,
                  fontWeight: 600,
                  cursor: modalName.trim() ? "pointer" : "default",
                }}
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
