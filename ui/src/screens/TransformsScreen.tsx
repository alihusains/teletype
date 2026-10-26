import { useEffect, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { Toggle } from "../settings/primitives";
import HotkeyRecorder, { displayHotkey } from "../components/HotkeyRecorder";
import { Segmented } from "../settings/primitives";
import { confirmDialog, messageDialog } from "../lib/dialogs";
import { errorMessage } from "../lib/errors";
import { runWrite } from "../lib/writeGuard";
import { WriteFailed } from "../components/StateRows";
import { useRuntimeStatus } from "../lib/runtimeStatus";

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

// ── Local band primitives ───────────────────────────────────────────────────
// ui/src/settings/primitives.tsx covers controls (Toggle, Segmented, Row) but
// has no collapsible band, and it is owned by another agent right now, so the
// disclosure and the section rhythm live here.

const BAND_TITLE: CSSProperties = { fontSize: 15, fontWeight: 700, margin: 0 };
const BAND_SUBTITLE: CSSProperties = {
  fontSize: 12.5,
  color: "var(--text-secondary)",
  margin: "3px 0 0",
};

// Label above one pane of the before/after preview.
const PANE_LABEL: CSSProperties = {
  fontSize: 12,
  fontWeight: 600,
  color: "var(--text-secondary)",
  marginBottom: 6,
};

// A band heading that reveals its content on demand. Collapsed by default so
// the settings a user touches once (a shortcut, the raw prompt) cannot push the
// choices they actually make down the page.
function Disclosure({
  title,
  subtitle,
  defaultOpen = false,
  children,
}: {
  title: string;
  subtitle?: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState<boolean>(defaultOpen);
  return (
    <section style={{ minWidth: 0 }}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 7,
          width: "100%",
          padding: 0,
          background: "none",
          border: "none",
          cursor: "pointer",
          color: "var(--text)",
          font: "inherit",
          textAlign: "left",
        }}
      >
        <Icon
          name="arrow"
          size={13}
          color="var(--text-secondary)"
          style={{
            flexShrink: 0,
            transform: open ? "rotate(90deg)" : "none",
            transition: "transform var(--dur-fast) var(--ease)",
          }}
        />
        <span style={BAND_TITLE}>{title}</span>
      </button>
      {subtitle ? <p style={BAND_SUBTITLE}>{subtitle}</p> : null}
      {open ? <div style={{ marginTop: 12 }}>{children}</div> : null}
    </section>
  );
}

export default function TransformsScreen({ onNavigate }: { onNavigate?: (screen: string) => void }) {
  const runtime = useRuntimeStatus().status;
  const [transforms, setTransforms] = useState<Transform[]>([]);
  const [autoApplyEnabled, setAutoApplyEnabled] = useState(true);
  const [selected, setSelected] = useState<Transform | null>(null);
  const [rules, setRules] = useState<Record<string, boolean>>({ ...DEFAULT_RULES });
  // "Custom instructions" list. The base prompt is the first, locked item
  // (rendered from the rule assembly, not stored); these are the user-added
  // fragments, each with a trash + toggle.
  const [customInstructions, setCustomInstructions] = useState<PolishInstruction[]>([]);
  const [draftInstruction, setDraftInstruction] = useState("");
  const [showAssembled, setShowAssembled] = useState(false);
  const [previewInput, setPreviewInput] = useState(SAMPLE_TEXT);
  const [previewOutput, setPreviewOutput] = useState("");
  const [previewNotice, setPreviewNotice] = useState<string | null>(null);
  // Set whenever a run produced no real polished text (the model fell back
  // to returning the input, or the run errored). Rendered as an empty-state
  // block in the Polished pane so the input is never echoed as output.
  const [previewFailure, setPreviewFailure] = useState<string | null>(null);
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
  // True while the "Which polish runs" picker is writing per-transform
  // autoApply flags. Blocks a second click from racing the first into a state
  // where two transforms are flagged.
  const [autoApplySaving, setAutoApplySaving] = useState(false);
  // The last rejected auto-apply write. Held until the next attempt so a
  // failure cannot scroll out of view with the switch looking like it saved.
  const [autoApplyError, setAutoApplyError] = useState<string | null>(null);

  // Cleanup-only models (small transcript cleaners) can run the Polish preset
  // and nothing else; every other preset would just return the input. Surface
  // that before the user presses Show me instead of faking a polish.
  const cleanupOnlyModelId = runtime?.llm.modelId ?? "";
  const cleanupOnly = cleanupOnlyModelId === "s1-mini" || cleanupOnlyModelId === "speakoflow-mini";
  const selectedIsPolish = !!selected && (selected.id === "builtin-polish" || selected.autoApply);
  const selectedBlocked = cleanupOnly && !!selected && !selectedIsPolish;

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
    const previous = autoApplyEnabled;
    setAutoApplyEnabled(value);
    setAutoApplyError(null);
    const settings = await invoke<Record<string, unknown>>("get_settings").catch(() => null);
    if (!settings) {
      setAutoApplyEnabled(previous);
      setAutoApplyError("Could not read the current settings.");
      return;
    }
    const result = await runWrite(() =>
      invoke("save_settings", { settings: { ...settings, autoApplyTransform: value } })
    );
    if (!result.ok) {
      setAutoApplyEnabled(previous);
      setAutoApplyError(result.error);
    }
  };

  // Which transform runs after dictation is NOT a Settings value: the backend
  // picks it from the per-transform `auto_apply` flag
  // (crates/teletype-core/src/transforms/mod.rs TransformStore::auto_apply()
  // filters `enabled && auto_apply`, and dictation.rs uses exactly that).
  // Settings.autoApplyTransform is only the on/off switch, so this control must
  // write the flag on the chosen transform and clear it on every other one.
  // It deliberately does NOT call selectPreset: `selected` means "what I am
  // editing", `autoApply` means "what runs after dictation".
  const setAutoApplyTo = async (id: string) => {
    const chosen = transforms.find((t) => t.id === id);
    if (!chosen || autoApplySaving) return;
    setAutoApplySaving(true);
    const failures: string[] = [];
    try {
      // Written one at a time, clear-first: update_transform rewrites the whole
      // stored transform, so firing both at once could lose a write and leave
      // two transforms flagged.
      for (const other of transforms.filter((t) => t.autoApply && t.id !== id)) {
        await invoke("update_transform", {
          transform: { ...other, autoApply: false, updatedAt: Date.now() },
        }).catch((e) => {
          failures.push(errorMessage(e));
        });
      }
      await invoke("update_transform", {
        transform: { ...chosen, autoApply: true, updatedAt: Date.now() },
      }).catch((e) => {
        failures.push(errorMessage(e));
      });
      // Re-read the store instead of trusting the click, so the UI can never
      // show a selection the backend does not have. Exactly one flagged, and it
      // has to be enabled or the engine will silently skip it.
      const latest = await invoke<Transform[]>("list_transforms").catch(() => null);
      const flagged = (latest ?? []).filter((t) => t.autoApply);
      const settled = flagged.length === 1 && flagged[0].enabled;
      if (failures.length > 0) {
        await messageDialog(`Could not save which polish runs: ${failures[0]}`, {
          title: "Auto-apply not set",
          kind: "error",
        });
      } else if (!latest) {
        await messageDialog("Could not confirm which polish will run automatically. Check it again.", {
          title: "Auto-apply not confirmed",
          kind: "error",
        });
      } else if (!settled) {
        await messageDialog(
          flagged.length === 0
            ? `"${chosen.name}" could not be set to run automatically — no polish is flagged to run. Pick it again.`
            : flagged.length > 1
              ? `"${chosen.name}" could not be set to run automatically — more than one polish is still flagged. Pick it again.`
              : `"${flagged[0].name}" is turned off, so it cannot run automatically. Pick another polish.`,
          { title: "Auto-apply not set", kind: "error" },
        );
      }
    } finally {
      setAutoApplySaving(false);
      refresh();
    }
  };

  const deleteTransform = async (id: string) => {
    if (!(await confirmDialog("Delete this transform?", { title: "Delete transform" }))) return;
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

  // "Show me": run the active preset (with the current rule fragments and
  // custom instructions) on the sample text via the existing test_transform
  // IPC. The instruction is assembled client-side, exactly as the engine
  // would build it, so the preview is honest.
  const runPreview = async () => {
    if (!selected || !previewInput.trim()) return;
    setPreviewing(true);
    setPreviewOutput("");
    setPreviewNotice(null);
    setPreviewFailure(null);
    // Refresh the stored instruction from the current rules + user
    // instructions before previewing, so the preview uses exactly what the
    // engine would send.
    await invoke("update_transform", {
      transform: { ...selected, instruction: composeInstruction(selected, customInstructions), updatedAt: Date.now() },
    }).catch(console.error);
    try {
      const result = await invoke<{ text: string; transformed: boolean; skipMessage: string | null }>(
        "test_transform",
        { transformId: selected.id, input: previewInput },
      );
      if (result.transformed) {
        // A returned untransformed text is never presented as polished
        // output; an echoed input is shown with an honest notice instead.
        setPreviewOutput(result.text);
        if (result.text.trim() === previewInput.trim()) {
          setPreviewNotice("No changes - the model returned your text unchanged.");
        }
      } else {
        setPreviewFailure(result.skipMessage ?? "The model returned your words unchanged.");
      }
    } catch (e) {
      setPreviewFailure(`Could not run this polish: ${errorMessage(e)}`);
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
    setPreviewFailure(null);
    setPreviewNotice(null);
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

  // The at-a-glance truth for the header. The backend only auto-applies a
  // transform that is BOTH flagged and enabled, so a flagged-but-disabled
  // polish must not be announced as running.
  const autoApplyTransformId = transforms.find((t) => t.autoApply)?.id;
  const effectiveAutoTransform = autoApplyEnabled
    ? transforms.find((t) => t.autoApply && t.enabled)
    : undefined;
  const enabledTransforms = transforms.filter((t) => t.enabled);
  const statusLine = effectiveAutoTransform
    ? `Every dictation is polished with ${effectiveAutoTransform.name}.`
    : selected?.shortcut
      ? `Polish runs when you press ${displayHotkey(selected.shortcut)}.`
      : `Polish runs when you pick a shortcut below.`;

  return (
    <div>
      <div style={{ display: "flex", flexDirection: "column", gap: 22 }}>
        {/* ── Band 0: identity + the at-a-glance status line ── */}
        <section style={{ minWidth: 0 }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 12 }}>
            <h2 className="tt-screen-title">AI Polish</h2>
            <div style={{ display: "flex", alignItems: "center", gap: 12, fontSize: 13, color: "var(--text-secondary)" }}>
              <button onClick={resetDefaults} style={{ display: "inline-flex", alignItems: "center", gap: 5, background: "none", border: "none", cursor: "pointer", color: "var(--text)", fontSize: 13, fontWeight: 600 }}>
                <Icon name="refresh" size={14} />
                Reset
              </button>
            </div>
          </div>
          <p style={{ fontSize: 13, color: "var(--text-secondary)", margin: "7px 0 0", lineHeight: 1.5 }}>
            {statusLine}
          </p>
        </section>

        {/* ── Band 1: the decision that matters, first ── */}
        <section style={{ minWidth: 0 }}>
          <h3 style={BAND_TITLE}>Choose a polish</h3>
          <p style={BAND_SUBTITLE}>Polish rewrites your text to sound clearer, in your voice.</p>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(min(170px, 100%), 1fr))", gap: 10, marginTop: 12 }}>
            {transforms.map((t) => (
              <div
                key={t.id}
                role="button"
                tabIndex={0}
                aria-pressed={selected?.id === t.id}
                onClick={() => selectPreset(t)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    selectPreset(t);
                  }
                }}
                style={{
                  padding: "14px 14px 12px",
                  borderRadius: 12,
                  border: selected?.id === t.id ? "2px solid var(--accent)" : "1px solid var(--border)",
                  background: "var(--surface)",
                  cursor: "pointer",
                  position: "relative",
                  minWidth: 0,
                  textAlign: "left",
                }}
              >
                {t.shortcut && (
                  <kbd style={{ position: "absolute", top: 10, right: 10, fontSize: 10, background: "var(--bg)", padding: "1px 6px", borderRadius: 4, border: "1px solid var(--border)", color: "var(--text-secondary)" }}>
                    {t.shortcut}
                  </kbd>
                )}
                <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0, marginTop: t.shortcut ? 18 : 0 }}>
                  <span style={{ fontSize: 14, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {t.name}
                  </span>
                  {/* Selection cannot rest on the accent border alone. */}
                  {selected?.id === t.id && (
                    <span style={{ flexShrink: 0, padding: "1px 7px", borderRadius: 999, border: "1px solid var(--accent)", background: "var(--accent-soft)", color: "var(--accent)", fontSize: 10, fontWeight: 700, letterSpacing: 0.3, textTransform: "uppercase" }}>
                      Active
                    </span>
                  )}
                </div>
                <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 3, overflow: "hidden", textOverflow: "ellipsis", display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical" }}>
                  {t.description}
                </div>
                {cleanupOnly && !(t.id === "builtin-polish" || t.autoApply) && (
                  <div style={{ fontSize: 10, color: "var(--warning)", marginTop: 3 }}>Needs a general model</div>
                )}
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
            {/* The dashed card is the only entry point to the create modal. */}
            <button
              type="button"
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
                minWidth: 0,
                font: "inherit",
                color: "var(--text)",
              }}
            >
              <span style={{ width: 28, height: 28, borderRadius: 999, background: "var(--bg)", display: "grid", placeItems: "center", border: "1px solid var(--border)" }}>
                <Icon name="plus" size={14} color="var(--text-secondary)" />
              </span>
              <div style={{ fontSize: 13, fontWeight: 600 }}>Create your own</div>
              <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>Upload your own prompt</div>
            </button>
          </div>
        </section>

        {/* ── Band 2: the proof, full width instead of a 340px rail ── */}
        <section style={{ minWidth: 0 }}>
          <h3 style={BAND_TITLE}>Try it on your own words</h3>
          <p style={BAND_SUBTITLE}>Type what you actually said, then see what this polish does to it.</p>
          {selectedBlocked && (
            <div
              role="status"
              style={{
                marginTop: 12,
                padding: "12px 14px",
                borderRadius: 10,
                background: "var(--warning-soft)",
                border: "1px solid color-mix(in srgb, var(--warning) 30%, transparent)",
                fontSize: 13,
                lineHeight: 1.5,
              }}
            >
              <div style={{ fontWeight: 700, color: "var(--warning)" }}>
                {runtime?.llm.label || "This model"} can only clean transcripts
              </div>
              <div style={{ marginTop: 3, color: "var(--text-secondary)" }}>
                {selected?.name} needs a general model, so it would return your words unchanged. Pick the Polish preset, or switch to a general or cloud polish model.
              </div>
              {onNavigate && (
                <button
                  type="button"
                  onClick={() => onNavigate("models")}
                  style={{
                    marginTop: 10,
                    background: "var(--surface)",
                    border: "1px solid color-mix(in srgb, var(--warning) 30%, transparent)",
                    borderRadius: 8,
                    padding: "6px 12px",
                    fontSize: 12,
                    fontWeight: 600,
                    cursor: "pointer",
                    color: "var(--text)",
                  }}
                >
                  Open Models
                </button>
              )}
            </div>
          )}
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(280px, 100%), 1fr))", gap: 12, marginTop: 12 }}>
            <div style={{ minWidth: 0 }}>
              <div style={PANE_LABEL}>Your words</div>
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
                {previewing ? "Running…" : "Show me"}
              </button>
            </div>
            <div style={{ minWidth: 0 }}>
              <div style={PANE_LABEL}>Polished</div>
              <div
                aria-live="polite"
                style={{
                  minHeight: 208,
                  padding: "10px 12px",
                  borderRadius: 8,
                  border: previewOutput
                    ? "1px solid var(--accent, #3b82f6)"
                    : previewFailure
                      ? "1px solid color-mix(in srgb, var(--warning) 40%, transparent)"
                      : "1px solid var(--border)",
                  background: previewOutput
                    ? "var(--accent-soft, rgba(59,130,246,0.08))"
                    : "var(--bg)",
                  fontSize: 13,
                  lineHeight: 1.55,
                  whiteSpace: "pre-wrap",
                }}
              >
                {previewOutput ? (
                  previewOutput
                ) : previewFailure ? (
                  <div style={{ whiteSpace: "normal" }}>
                    <div style={{ fontWeight: 700, color: "var(--warning)" }}>No polished text</div>
                    <div style={{ marginTop: 6, color: "var(--text-secondary)" }}>{previewFailure}</div>
                    <div style={{ marginTop: 8, fontSize: 12, color: "var(--text-tertiary)" }}>
                      Your words are unchanged. Pick a preset this model can run, or switch model in Models.
                    </div>
                  </div>
                ) : (
                  <span style={{ color: "var(--text-tertiary)" }}>
                    {previewing ? "Running…" : "Nothing yet - press Show me to run this polish."}
                  </span>
                )}
              </div>
              {previewNotice && (
                <div
                  role="status"
                  style={{
                    marginTop: 8,
                    padding: "8px 10px",
                    borderRadius: 8,
                    fontSize: 12,
                    lineHeight: 1.45,
                    color: "var(--warning)",
                    background: "var(--warning-soft)",
                    border: "1px solid color-mix(in srgb, var(--warning) 30%, transparent)",
                  }}
                >
                  {previewNotice}
                </div>
              )}
            </div>
          </div>
        </section>

        {/* ── Band 3: everyday tuning ── */}
        <section style={{ minWidth: 0 }}>
          <h3 style={BAND_TITLE}>What Polish fixes</h3>
          <p style={BAND_SUBTITLE}>Select rules for Polish</p>
          <div style={{ background: "var(--surface)", borderRadius: 12, border: "1px solid var(--border)", overflow: "hidden", marginTop: 12 }}>
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

          {/* The always-on safety contract, kept behind a disclosure so a wall
              of prompt text does not sit in the main flow. Same content the
              assembled-prompt viewer shows. */}
          <div style={{ marginTop: 14 }}>
            <Disclosure
              title="What the model always does"
              subtitle="The rules that override every choice above. Always on, not editable here."
            >
              <pre
                style={{
                  margin: 0,
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
                {CORE_RULES_PREVIEW}
              </pre>
            </Disclosure>
          </div>

          {/* List style (item 3): how enumerated lists are formatted in the
              polished output. Persisted in Settings.listStyle and rendered
              into the prompt for every transform. */}
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, flexWrap: "wrap", marginTop: 18 }}>
            <div style={{ flex: 1, minWidth: 160 }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>List format</div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 1 }}>
                How polished output formats lists you dictate.
              </div>
            </div>
            <Segmented label="List format" value={listStyle} options={LIST_STYLE_OPTIONS} onChange={saveListStyle} />
          </div>
        </section>

        {/* ── Band 4: when it runs ── */}
        <section style={{ minWidth: 0 }}>
          <h3 style={BAND_TITLE}>When it runs</h3>
          <p style={BAND_SUBTITLE}>Decide whether Polish acts by itself, or only when you call it.</p>
          <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap", marginTop: 12 }}>
            <div style={{ flex: 1, minWidth: 160 }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>Polish every dictation automatically</div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 1 }}>
                {autoApplyEnabled
                  ? effectiveAutoTransform
                    ? "Runs as soon as you stop dictating, before the text is typed."
                    : "Nothing is picked below yet, so nothing will run."
                  : "Off: your words are typed exactly as you said them."}
              </div>
            </div>
            <Toggle label="Polish every dictation automatically" checked={autoApplyEnabled} onChange={saveAutoApply} />
          </div>
          {autoApplyError && (
            <div style={{ marginTop: 10 }}>
              <WriteFailed
                label="Auto-apply"
                message={autoApplyError}
                onDismiss={() => setAutoApplyError(null)}
              />
            </div>
          )}

          {/* Which transform auto-applies is a per-transform flag, not the
              editor selection — so this picker writes `autoApply` and never
              calls selectPreset. */}
          {autoApplyEnabled && (
            <div style={{ marginTop: 14 }}>
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Which polish runs</div>
              {enabledTransforms.length === 0 ? (
                <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
                  No polish is turned on, so there is nothing that can run automatically.
                </div>
              ) : (
                <div role="radiogroup" aria-label="Which polish runs" style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                  {enabledTransforms.map((t) => {
                    const active = t.id === autoApplyTransformId;
                    return (
                      <button
                        key={t.id}
                        type="button"
                        role="radio"
                        aria-checked={active}
                        disabled={autoApplySaving}
                        title={t.description}
                        onClick={() => void setAutoApplyTo(t.id)}
                        style={{
                          display: "inline-flex",
                          alignItems: "center",
                          gap: 6,
                          maxWidth: "100%",
                          padding: "6px 12px",
                          borderRadius: 999,
                          border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`,
                          background: active ? "var(--accent-soft)" : "var(--surface)",
                          color: active ? "var(--text)" : "var(--text-secondary)",
                          fontSize: 13,
                          fontWeight: active ? 600 : 500,
                          cursor: autoApplySaving ? "default" : "pointer",
                          opacity: autoApplySaving ? 0.6 : 1,
                          minWidth: 0,
                        }}
                      >
                        {active && <Icon name="check" size={12} color="var(--accent)" />}
                        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{t.name}</span>
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          )}
        </section>

        {/* ── Band 5: the once-in-a-lifetime settings, out of the way ── */}
        <Disclosure
          title="Advanced"
          subtitle="The keyboard shortcut and the exact prompt this polish follows."
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
            {/* Keyboard shortcut */}
            <div style={{ background: "var(--surface)", borderRadius: 12, border: "1px solid var(--border)", padding: "14px 16px" }}>
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Keyboard shortcut</div>
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
                          void messageDialog(`Could not set shortcut: ${e}`, {
                            title: "Shortcut not set",
                            kind: "error",
                          });
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

            {/* Custom prompt — a Wispr-style instruction list, available for
                every preset. The base prompt is the first, locked item; the
                user adds, toggles and removes instructions on top of it. */}
            {selected && (
              <div>
                <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>
                  Custom instructions
                  <span style={{ fontWeight: 500, color: "var(--text-secondary)" }}> for {selected.name}</span>
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
          </div>
        </Disclosure>
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
