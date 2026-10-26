import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Icon } from "../components/Icon";
import { useTauriEvent } from "../lib/useTauriEvent";
import { Row, Section, Segmented, Toggle } from "../settings/primitives";
import { confirmDialog } from "../lib/dialogs";
import { errorMessage } from "../lib/errors";
import { LoadFailed, LoadingRow, WriteFailed } from "../components/StateRows";
import { useResource, useWrites } from "../lib/writeGuard";

interface Settings {
  typingAutotextEnabled: boolean;
  autotextTiming: string;
}

interface AutoTextEntry {
  id: string;
  trigger: string;
  replacement: string;
  description: string;
  enabled: boolean;
  scope: string;
  createdAt: number;
  updatedAt: number;
  snippet: string;
}

interface ImportCounts {
  imported: number;
  updated: number;
  skipped: number;
}

const ACCENT = "var(--accent)";

const VARIABLES = [
  { token: "{{date}}", hint: "Today's date, like 2026-10-25" },
  { token: "{{time}}", hint: "The time right now, like 14:05" },
  { token: "{{clipboard}}", hint: "The text you last copied" },
  { token: "{{app}}", hint: "The frontmost app, like Gmail" },
];

const VARIABLE_CHIP: React.CSSProperties = {
  background: "var(--surface-2)",
  border: "1px solid var(--border)",
  borderRadius: 999,
  padding: "3px 9px",
  fontSize: 12,
  color: "var(--text)",
  cursor: "pointer",
  fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
};

const EXAMPLE_SNIPPETS = [
  { say: "my email", to: "alex.sorathiya@example.com" },
  { say: "my number", to: "+1 (555) 014-2288" },
  { say: "sign off", to: "Best,\nAlex Sorathiya\n(+1) 555-0142" },
];

/**
 * Insight app labels are display strings and may carry a window title
 * ("Slack — #platform-eng") or a file name ("File: readout.wav"). A scope
 * needs the app itself, so keep the app part and drop non-app rows.
 */
function cleanAppLabel(label: string): string | null {
  const raw = label.trim();
  if (!raw || /^file:/i.test(raw)) return null;
  const name = raw.split(/\s+—\s+/)[0].trim();
  return name && name.length <= 48 ? name : null;
}

function EntryForm({
  initial,
  appSuggestions = [],
  onSave,
  onCancel,
}: {
  initial: AutoTextEntry | null;
  appSuggestions?: string[];
  onSave: (e: AutoTextEntry) => void;
  onCancel: () => void;
}) {
  const [trigger, setTrigger] = useState(initial?.trigger ?? "");
  const [snippet, setSnippet] = useState(initial?.snippet ?? "");
  const [replacement, setReplacement] = useState(initial?.replacement ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const initialScope = initial?.scope?.trim() || "everywhere";
  const [scopeMode, setScopeMode] = useState<"everywhere" | "application">(
    initialScope === "everywhere" ? "everywhere" : "application",
  );
  const [appTarget, setAppTarget] = useState(
    initialScope === "everywhere" ? "" : initialScope,
  );
  const [error, setError] = useState("");
  const replacementRef = useRef<HTMLTextAreaElement>(null);

  const insertVariable = (token: string) => {
    const el = replacementRef.current;
    if (!el) {
      setReplacement((r) => r + token);
      return;
    }
    const start = el.selectionStart ?? replacement.length;
    const end = el.selectionEnd ?? start;
    setReplacement(replacement.slice(0, start) + token + replacement.slice(end));
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(start + token.length, start + token.length);
    });
  };

  const submit = async () => {
    if (!trigger.trim() || !replacement.trim()) {
      setError("Trigger and replacement are required.");
      return;
    }
    if (scopeMode === "application" && !appTarget.trim()) {
      setError("Choose an app, or switch to Everywhere.");
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
      scope: scopeMode === "application" ? appTarget.trim() : "everywhere",
      createdAt: initial?.createdAt ?? now,
      updatedAt: now,
    };
    try {
      if (initial) {
        await invoke("update_autotext", { entry });
      } else {
        await invoke("create_autotext", { entry });
      }
      onSave(entry);
    } catch (e) {
      setError(errorMessage(e));
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
          <textarea
            ref={replacementRef}
            value={replacement}
            onChange={(e) => setReplacement(e.target.value)}
            placeholder="abcd@gmail.com"
            rows={2}
            style={{ ...inputStyle, resize: "vertical" }}
          />
          <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap", marginTop: 8 }}>
            <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>Insert variable:</span>
            {VARIABLES.map((v) => (
              <button
                key={v.token}
                type="button"
                title={v.hint}
                onClick={() => insertVariable(v.token)}
                style={VARIABLE_CHIP}
              >
                {v.token}
              </button>
            ))}
          </div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 6, lineHeight: 1.5 }}>
            {"{{date}} today, {{time}} now, {{clipboard}} what you last copied, {{app}} the frontmost app."}
          </div>
        </div>
        <div>
          <label style={{ fontSize: 13, fontWeight: 600 }}>Where it applies</label>
          <div style={{ marginTop: 6 }}>
            <Segmented
              label="Where it applies"
              value={scopeMode}
              options={[
                { value: "everywhere", label: "Everywhere", title: "Expand in any app (default)" },
                { value: "application", label: "One app", title: "Only expand in a specific app" },
              ]}
              onChange={setScopeMode}
            />
          </div>
          {scopeMode === "application" && (
            <div style={{ marginTop: 8 }}>
              <input
                aria-label="Application"
                value={appTarget}
                onChange={(e) => setAppTarget(e.target.value)}
                placeholder="Slack or com.tinyspeck.slackmacgap"
                style={inputStyle}
              />
              {appSuggestions.length > 0 && (
                <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap", marginTop: 8 }}>
                  <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>Your apps:</span>
                  {appSuggestions.map((a) => (
                    <button
                      key={a}
                      type="button"
                      aria-label={`Use ${a}`}
                      onClick={() => setAppTarget(a)}
                      style={VARIABLE_CHIP}
                    >
                      {a}
                    </button>
                  ))}
                </div>
              )}
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 6 }}>
                Use the app's name or its bundle id. Matching is case-insensitive.
              </div>
            </div>
          )}
        </div>
        <div>
          <label style={{ fontSize: 13, fontWeight: 600 }}>Description (optional)</label>
          <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Personal email" style={inputStyle} />
        </div>
        {error && <div role="alert" style={{ fontSize: 13, color: "var(--danger)" }}>{error}</div>}
        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
          <button className="primary" onClick={submit}>{initial ? "Save" : "Add snippet"}</button>
          <button onClick={onCancel}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

type StrNum = { [k: string]: number };

export default function AutoTextScreen() {
  const [showSystem, setShowSystem] = useState(false);
  const [usage, setUsage] = useState<StrNum>({});
  const [showCreate, setShowCreate] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [appSuggestions, setAppSuggestions] = useState<string[]>([]);
  const [ioNotice, setIoNotice] = useState("");
  const [ioError, setIoError] = useState("");
  const formAnchor = useRef<HTMLDivElement>(null);
  const writes = useWrites();

  // A-3: the snippet list and its settings are two independent reads. Each has
  // its own resource so a rejected list read cannot blank the toggles, and a
  // rejected settings read must not be dressed up as "feature off" (A-12).
  const list = useResource<AutoTextEntry[]>(() => invoke<AutoTextEntry[]>("list_autotext"));
  const systemList = useResource<AutoTextEntry[]>(() =>
    invoke<AutoTextEntry[]>("list_system_autotext"),
  );
  const settings = useResource<Settings>(() => invoke<Settings>("get_settings"));
  const typingEnabled = settings.data?.typingAutotextEnabled ?? true;
  const autotextTiming = settings.data?.autotextTiming ?? "delimiter";

  // A-2: hold-and-restore. The switch flips optimistically and comes back to
  // its previous position when the save rejects; a rejected get_settings is a
  // different fact from a disabled feature, so it is reported, not swallowed.
  const saveTypingEnabled = async (value: boolean) => {
    const prev = typingEnabled;
    settings.patch((s) => ({ ...(s ?? ({} as Settings)), typingAutotextEnabled: value }));
    const res = await writes.run("typingEnabled", async () => {
      const s = await invoke<Settings>("get_settings");
      await invoke("save_settings", { settings: { ...s, typingAutotextEnabled: value } });
    });
    if (!res.ok) settings.patch((s) => ({ ...(s ?? ({} as Settings)), typingAutotextEnabled: prev }));
  };

  const saveAutotextTiming = async (value: string) => {
    const prev = autotextTiming;
    settings.patch((s) => ({ ...(s ?? ({} as Settings)), autotextTiming: value }));
    const res = await writes.run("autotextTiming", async () => {
      const s = await invoke<Settings>("get_settings");
      await invoke("save_settings", { settings: { ...s, autotextTiming: value } });
    });
    if (!res.ok) settings.patch((s) => ({ ...(s ?? ({} as Settings)), autotextTiming: prev }));
  };

  const entries = list.data ?? [];
  const systemEntries = systemList.data ?? [];

  useEffect(() => {
    if (showCreate || editId) {
      formAnchor.current?.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }, [showCreate, editId]);

  useEffect(() => {
    invoke<{ autotextCounts: StrNum }>("get_usage_stats").then((u) => setUsage(u.autotextCounts)).catch(console.error);
    // Scope suggestions come from the apps the user actually dictates into.
    // A missing history (fresh install, preview shim) simply offers none.
    invoke<{ topApps?: { label: string }[] }>("get_insights", { range: "month" })
      .then((d) => {
        const names: string[] = [];
        for (const a of d.topApps ?? []) {
          const name = cleanAppLabel(a.label);
          if (name && !names.some((n) => n.toLowerCase() === name.toLowerCase())) {
            names.push(name);
          }
        }
        setAppSuggestions(names.slice(0, 6));
      })
      .catch(() => {});
  }, []);

  // Apps already used as a scope plus the user's most-dictated-into apps.
  const scopeSuggestions = useMemo(() => {
    const seen = new Set<string>();
    for (const e of entries) {
      if (e.scope && e.scope !== "everywhere") seen.add(e.scope);
    }
    for (const a of appSuggestions) seen.add(a);
    return Array.from(seen).slice(0, 8);
  }, [entries, appSuggestions]);

  // Re-sync when Settings (or any other screen) flips the same toggle, so
  // this copy never shows a stale value.
  useTauriEvent<void>("settings-changed", () => {
    settings.reload();
  });

  const refresh = () => {
    list.reload();
    invoke<{ autotextCounts: StrNum }>("get_usage_stats").then((u) => setUsage(u.autotextCounts)).catch(console.error);
  };

  // ---- Import / export ----

  const doExport = async () => {
    setIoNotice("");
    setIoError("");
    try {
      const path = await save({
        defaultPath: "autotext.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const count = await invoke<number>("export_autotext", { path });
      setIoNotice(`Exported ${count} ${count === 1 ? "snippet" : "snippets"}.`);
    } catch (e) {
      setIoError(`Export failed: ${errorMessage(e)}`);
    }
  };

  const doImport = async () => {
    setIoNotice("");
    setIoError("");
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path || typeof path !== "string") return;
      const counts = await invoke<ImportCounts>("import_autotext", { path });
      setIoNotice(`Imported ${counts.imported}, updated ${counts.updated}, skipped ${counts.skipped}.`);
      refresh();
    } catch (e) {
      setIoError(`Import failed: ${errorMessage(e)}`);
    }
  };

  // Re-fetch usage + entries whenever the window becomes visible again:
  // typed expansions happen in OTHER apps while this window is hidden, so
  // the counts change without any event reaching this screen.
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onVisible);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onVisible);
    };
  }, []);

  // Group system entries into balanced, meaningful categories.
  const systemGroups = useMemo(() => {
    const PUNCT = new Set(["comma", "period", "full stop", "question mark", "exclamation mark", "exclamation point", "colon", "semicolon", "quote", "quotation mark", "apostrophe", "single quote", "hyphen", "dash", "em dash", "en dash", "ellipsis", "dot dot dot"]);
    const BRACKETS = new Set(["open parenthesis", "close parenthesis", "open paren", "close paren", "open bracket", "close bracket", "open square bracket", "close square bracket", "open curly bracket", "close curly bracket", "open brace", "close brace"]);
    const LINE = new Set(["new line", "next line", "line break", "new paragraph"]);
    const MATH = new Set(["plus", "plus sign", "minus", "minus sign", "equals", "equals sign", "less than", "greater than"]);
    const CODE = new Set(["double equals", "triple equals", "not equals", "arrow", "fat arrow", "double colon", "double slash", "question dot", "question question", "and and", "or or", "colon equals"]);
    const CURRENCY = new Set(["dollar sign", "euro sign", "pound sign", "yen sign", "rupee sign", "degree sign", "degree symbol", "copyright", "trademark", "registered trademark"]);
    const cat = (phrase: string): string => {
      if (PUNCT.has(phrase)) return "Punctuation";
      if (BRACKETS.has(phrase)) return "Brackets & parens";
      if (LINE.has(phrase)) return "Line breaks";
      if (MATH.has(phrase)) return "Math";
      if (CODE.has(phrase)) return "Code";
      if (CURRENCY.has(phrase)) return "Currency & marks";
      return "Symbols";
    };
    const order = ["Punctuation", "Brackets & parens", "Line breaks", "Math", "Code", "Currency & marks", "Symbols"];
    const groups: { name: string; items: AutoTextEntry[] }[] = [];
    for (const e of systemEntries ?? []) {
      const name = cat(e.snippet);
      const g = groups.find((x) => x.name === name);
      if (g) g.items.push(e);
      else groups.push({ name, items: [e] });
    }
    return groups.sort((a, b) => order.indexOf(a.name) - order.indexOf(b.name));
  }, [systemList.data]);

  // How many times an entry was used, summed across its trigger and spoken phrase.
  const usageFor = (e: AutoTextEntry): number => {
    let n = usage[e.trigger] ?? 0;
    const phrase = e.snippet.trim();
    if (phrase) n += usage[phrase] ?? 0;
    return n;
  };
  const totalUsed = Object.values(usage).reduce((s, n) => s + n, 0);
  const mostUsed = [...entries]
    .map((e) => ({ e, n: usageFor(e) }))
    .filter((x) => x.n > 0)
    .sort((a, b) => b.n - a.n)
    .slice(0, 5);
  const maxUsed = Math.max(...mostUsed.map((x) => x.n), 1);

  // A-1: confirm, then write through useWrites. A rejected delete keeps the
  // row and shows the failure under it with a Retry instead of vanishing.
  // The row is dropped optimistically and re-fetched on rejection; the ref
  // only keeps the WriteFailed card mounted while the row is absent, because
  // a re-fetch must not wipe the per-key error out from under it.
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const remove = async (id: string) => {
    if (!(await confirmDialog("Delete this snippet?", { title: "Delete AutoText snippet" }))) return;
    list.patch((prev) => (prev ?? []).filter((e) => e.id !== id));
    const res = await writes.run(`delete:${id}`, () => invoke("delete_autotext", { id }));
    if (res.ok) {
      setDeleteId(null);
      refresh();
    } else {
      setDeleteId(id);
      list.reload();
    }
  };

  // A-2: the enabled checkbox is an optimistic write with hold-and-restore.
  const toggle = async (e: AutoTextEntry) => {
    const wasEnabled = e.enabled;
    list.patch((prev) => (prev ?? []).map((x) => (x.id === e.id ? { ...x, enabled: !wasEnabled } : x)));
    const res = await writes.run(`enabled:${e.id}`, () =>
      invoke("update_autotext", { entry: { ...e, enabled: !wasEnabled } }),
    );
    if (res.ok) {
      refresh();
    } else {
      list.patch((prev) => (prev ?? []).map((x) => (x.id === e.id ? { ...x, enabled: wasEnabled } : x)));
    }
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

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, width: "100%" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 12 }}>
        <div>
          <h2 className="tt-screen-title">Snippets</h2>
          <p style={{ color: "var(--text-secondary)", marginTop: 4, fontSize: 13 }}>
            The stuff you shouldn't have to re-type. Say a phrase to drop in an email, link, or signature instantly.
          </p>
        </div>
        <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
          <button onClick={doExport} title="Save your snippets to a JSON file">Export</button>
          <button onClick={doImport} title="Load snippets from a JSON file">Import</button>
          <button className="primary" onClick={() => { setShowCreate(true); setEditId(null); }}>+ Add new</button>
        </div>
      </div>

      {ioNotice !== "" && (
        <div role="status" style={{ padding: "9px 14px", fontSize: "var(--text-xs)", color: "var(--text-secondary)", background: "var(--surface-2)", borderRadius: "var(--radius-sm)" }}>
          {ioNotice}
        </div>
      )}
      {ioError !== "" && (
        <div role="alert" style={{ padding: "9px 14px", fontSize: "var(--text-xs)", color: "var(--danger)" }}>
          {ioError}
        </div>
      )}

      {/* Settings */}
      <Section
        title="How triggers work"
        hint="Each snippet has its own trigger. Type the trigger (the /slash one shown in the list) followed by a space or Enter, and Teletype replaces it with the saved text. A snippet can also have a spoken phrase: saying it while dictating drops the saved text in."
      >
        {settings.status === "error" ? (
          // A-12: a failed read is not "the feature is off". Show the failure
          // with a retry instead of rendering both toggles as off.
          <LoadFailed what="The snippet settings" detail={settings.error ?? undefined} onRetry={() => settings.reload()} />
        ) : (
          <>
            <Row label="Expand AutoText while typing" first>
              <Toggle
                label="Expand AutoText while typing"
                checked={typingEnabled}
                onChange={saveTypingEnabled}
              />
            </Row>
            {writes.error("typingEnabled") && (
              <WriteFailed
                label="Expand-while-typing"
                message={writes.error("typingEnabled") as string}
                onRetry={() => void saveTypingEnabled(typingEnabled)}
                onDismiss={() => writes.clear("typingEnabled")}
              />
            )}
            <Row
              label="Expand timing"
              hint="When a typed /trigger becomes a snippet: on the delimiter (space, tab, Enter) that finishes it, or the moment the trigger is complete."
            >
              <Segmented
                label="Expand timing"
                value={autotextTiming === "immediate" ? "immediate" : "delimiter"}
                options={[
                  { value: "delimiter", label: "On space/Enter", title: "Expand when a delimiter completes the trigger (default)" },
                  { value: "immediate", label: "Immediate", title: "Expand the moment the buffer ends with a trigger" },
                ]}
                onChange={saveAutotextTiming}
              />
            </Row>
            {writes.error("autotextTiming") && (
              <WriteFailed
                label="Expand timing"
                message={writes.error("autotextTiming") as string}
                onRetry={() => void saveAutotextTiming(autotextTiming)}
                onDismiss={() => writes.clear("autotextTiming")}
              />
            )}
          </>
        )}
      </Section>

      {/* System AutoText (built-in, read-only) */}
      <div style={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: "var(--radius)", overflow: "hidden" }}>
        <button
          onClick={() => setShowSystem((s) => !s)}
          style={{
            width: "100%",
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "14px 18px",
            background: "none",
            border: "none",
            cursor: "pointer",
            textAlign: "left",
          }}
        >
          <Icon name="wand" size={16} color={ACCENT} />
          <span style={{ fontSize: 14, fontWeight: 700 }}>System AutoText</span>
          <span style={{ fontSize: 11, color: "var(--text-secondary)", background: "var(--surface-2)", borderRadius: 999, padding: "2px 8px" }}>
            built-in · {(systemList.data ?? []).length}
          </span>
          <span style={{ marginLeft: "auto", fontSize: 12, color: "var(--text-secondary)" }}>
            {showSystem ? "Hide" : "Show"}
          </span>
        </button>
        {showSystem && (
          <div style={{ padding: "16px 18px 20px", borderTop: "1px solid var(--border)" }}>
            <p style={{ fontSize: 12.5, color: "var(--text-secondary)", margin: "0 0 18px", lineHeight: 1.55, maxWidth: 640 }}>
              Built-in spoken commands that insert punctuation, symbols, and line breaks. Say one to use it
              (e.g. say <b style={{ color: "var(--text)" }}>"comma"</b> to type <b style={{ color: "var(--text)" }}>,</b>).
              Your custom snippets override these when they share a phrase.
            </p>
            <div style={{ columnCount: 2, columnGap: 28 }}>
              {systemGroups.map((g) => (
                <div key={g.name} style={{ breakInside: "avoid", marginBottom: 20 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
                    <span style={{ fontSize: 11.5, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.6, color: ACCENT }}>
                      {g.name}
                    </span>
                    <span style={{ fontSize: 11, color: "var(--text-secondary)", background: "var(--surface-2)", borderRadius: 999, padding: "1px 8px", fontVariantNumeric: "tabular-nums" }}>
                      {g.items.length}
                    </span>
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                    {g.items.map((e) => (
                      <div
                        key={e.id}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "space-between",
                          gap: 12,
                          padding: "5px 10px",
                          borderRadius: 8,
                          fontSize: 13,
                        }}
                      >
                        <span style={{ color: "var(--text)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{e.snippet}</span>
                        <code
                          style={{
                            color: ACCENT,
                            fontSize: 12.5,
                            flexShrink: 0,
                            background: "var(--accent-soft)",
                            borderRadius: 6,
                            padding: "1px 8px",
                            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                          }}
                        >
                          {e.replacement.replace(/\n/g, "↵")}
                        </code>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Hero */}
      <div
        style={{
          borderRadius: "var(--radius)",
          padding: "24px 26px",
          color: "#fff",
          position: "relative",
          overflow: "hidden",
          background: "url('/autotext-hero.jpeg') center right / cover no-repeat",
        }}
      >
        <div style={{ position: "absolute", inset: 0, background: "linear-gradient(90deg, rgba(17,24,39,0.92) 0%, rgba(17,24,39,0.75) 55%, rgba(17,24,39,0.35) 100%)" }} />
        <div style={{ position: "relative" }}>
        <h3 style={{ fontSize: 22, fontWeight: 700, margin: 0, letterSpacing: -0.3 }}>
          The stuff <em style={{ fontFamily: "Georgia, serif" }}>you</em> shouldn't have to re-type.
        </h3>
        <p style={{ opacity: 0.85, marginTop: 8, fontSize: 13, maxWidth: 560 }}>
          Save text you type often — an email, intro, or prompt — then say a word to drop it in instantly.
        </p>
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 18, maxWidth: 640 }}>
          <span style={{ fontSize: 11, color: "rgba(255,255,255,0.65)", letterSpacing: 0.6, textTransform: "uppercase" }}>
            For example
          </span>
          {EXAMPLE_SNIPPETS.map((ex) => (
            <div key={ex.say} style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
              <span style={{ background: "rgba(255,255,255,0.14)", borderRadius: 8, padding: "6px 12px", fontSize: 13, fontStyle: "italic" }}>
                "{ex.say}"
              </span>
              <span style={{ opacity: 0.6 }}>→</span>
              <span style={{ background: "rgba(255,255,255,0.10)", borderRadius: 8, padding: "6px 12px", fontSize: 13, maxWidth: 380, whiteSpace: "pre-line", overflowWrap: "anywhere" }}>
                {ex.to}
              </span>
            </div>
          ))}
        </div>
        <button
          onClick={() => { setShowCreate(true); setEditId(null); }}
          style={{ marginTop: 18, background: "#fff", color: "#111827", border: "none", borderRadius: 10, padding: "9px 16px", fontWeight: 600, fontSize: 13 }}
        >
          {entries.length > 0 ? "Add another snippet" : "Add new snippet"}
        </button>
        </div>
      </div>

      {/* Usage summary */}
      {entries.length > 0 && (
        <div style={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: "var(--radius)", padding: "16px 20px" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: mostUsed.length ? 14 : 0 }}>
            <h3 style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", letterSpacing: 0.4, margin: 0 }}>
              <Icon name="chart-column" size={16} color={ACCENT} />
              Most used snippets
            </h3>
            <span style={{ fontSize: 12, color: "var(--text-secondary)", fontWeight: 600 }}>{totalUsed.toLocaleString()} expansions</span>
          </div>
          {mostUsed.length === 0 ? (
            <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--text-secondary)", margin: 0 }}>
              <Icon name="chart-column" size={16} color="var(--text-secondary)" />
              No usage yet — say a snippet or type its trigger and it will rank here.
            </p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {mostUsed.map(({ e, n }) => (
                <div key={e.id}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 4 }}>
                    <span style={{ fontSize: 13, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {e.snippet.trim() ? `"${e.snippet.trim()}"` : <code style={{ color: ACCENT }}>{e.trigger}</code>}
                    </span>
                    <span style={{ fontSize: 12, color: "var(--text-secondary)", flexShrink: 0, marginLeft: 10, fontVariantNumeric: "tabular-nums" }}>{n}×</span>
                  </div>
                  <div style={{ height: 8, background: "var(--surface-2)", borderRadius: 4, overflow: "hidden" }}>
                    <div style={{ height: "100%", width: `${Math.round((n / maxUsed) * 100)}%`, background: `linear-gradient(90deg, ${ACCENT}, var(--accent-hover))`, borderRadius: 4, transition: "width 0.4s" }} />
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

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
        {list.status === "loading" && <LoadingRow label="Loading snippets…" />}
        {list.status === "error" && (
          // A-3: a rejected read must not render as "No snippets yet".
          <LoadFailed what="Your snippets" detail={list.error ?? undefined} onRetry={() => list.reload()} />
        )}
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
              {e.scope && e.scope !== "everywhere" && (
                <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>
                  Only in {e.scope}
                </div>
              )}
            </div>
            {usageFor(e) > 0 && (
              <span
                title={`Used ${usageFor(e)} times`}
                style={{
                  flexShrink: 0,
                  fontSize: 11,
                  fontWeight: 700,
                  fontVariantNumeric: "tabular-nums",
                  color: ACCENT,
                  background: "var(--accent-soft)",
                  borderRadius: 999,
                  padding: "3px 9px",
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 4,
                }}
              >
                <Icon name="trending-up" size={11} color={ACCENT} />
                {usageFor(e)}×
              </span>
            )}
            <button onClick={() => { setEditId(e.id); setShowCreate(false); }}>Edit</button>
            <button className="danger" onClick={() => remove(e.id)}>Delete</button>
            <input type="checkbox" checked={e.enabled} onChange={() => toggle(e)} title="Enabled" aria-label="Enabled" />
            {writes.error(`enabled:${e.id}`) && (
              <WriteFailed
                label={`Enable ${e.snippet.trim() ? `\u201c${e.snippet.trim()}\u201d` : e.trigger}`}
                message={writes.error(`enabled:${e.id}`) as string}
                onRetry={() => void toggle(e)}
                onDismiss={() => writes.clear(`enabled:${e.id}`)}
              />
            )}
          </div>
        ))}
        {deleteId && writes.error(`delete:${deleteId}`) && (
          <WriteFailed
            label="Delete"
            message={writes.error(`delete:${deleteId}`) as string}
            onRetry={() => void remove(deleteId)}
            onDismiss={() => {
              writes.clear(`delete:${deleteId}`);
              setDeleteId(null);
            }}
          />
        )}
        {list.status === "ready" && filtered.length === 0 && (
          <p style={{ color: "var(--text-secondary)", padding: 24, textAlign: "center" }}>
            {entries.length === 0 ? "No snippets yet. Add one to get started." : "No snippets match your search."}
          </p>
        )}
      </div>

      <div ref={formAnchor}>
      {showCreate && (
        <EntryForm
          initial={null}
          appSuggestions={scopeSuggestions}
          onCancel={() => setShowCreate(false)}
          onSave={() => { setShowCreate(false); refresh(); }}
        />
      )}
      {editId && (
        <EntryForm
          initial={entries.find((e) => e.id === editId) ?? null}
          appSuggestions={scopeSuggestions}
          onCancel={() => setEditId(null)}
          onSave={() => { setEditId(null); refresh(); }}
        />
      )}
      </div>
    </div>
  );
}
