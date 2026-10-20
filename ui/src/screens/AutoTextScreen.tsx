import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { useTauriEvent } from "../lib/useTauriEvent";
import { Row, Section, Segmented, Toggle } from "../settings/primitives";
import { errorMessage } from "../lib/errors";

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
      scope: initial?.scope ?? "everywhere",
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

  return (
    <div className="tt-card" style={{ padding: 18, marginBottom: 16 }}>
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12, marginBottom: 14 }}>
        <h4 style={{ margin: 0, fontSize: "var(--text-md)", fontWeight: 650, letterSpacing: "var(--tracking-tight)" }}>
          {initial ? "Edit snippet" : "Add new snippet"}
        </h4>
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>
          Saved locally
        </span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))", gap: 12 }}>
          <div>
            <label htmlFor="autotext-snippet" style={{ fontSize: 13, fontWeight: 600 }}>Say (voice trigger)</label>
            <input
              id="autotext-snippet"
              autoFocus
              value={snippet}
              onChange={(e) => setSnippet(e.target.value)}
              placeholder="my email"
              style={{ marginTop: 4, width: "100%" }}
            />
            <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4 }}>
              The words you speak to insert this. Leave empty for a typed-only shortcut.
            </div>
          </div>
          <div>
            <label htmlFor="autotext-trigger" style={{ fontSize: 13, fontWeight: 600 }}>Type (shortcut)</label>
            <input
              id="autotext-trigger"
              value={trigger}
              onChange={(e) => setTrigger(e.target.value)}
              placeholder="/email"
              style={{ marginTop: 4, width: "100%" }}
            />
            <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4 }}>
              A /slash trigger for when you type instead of speak.
            </div>
          </div>
        </div>
        <div>
          <label htmlFor="autotext-replacement" style={{ fontSize: 13, fontWeight: 600 }}>Expands to</label>
          <textarea
            id="autotext-replacement"
            value={replacement}
            onChange={(e) => setReplacement(e.target.value)}
            placeholder="abcd@gmail.com"
            rows={2}
            style={{ marginTop: 4, width: "100%", resize: "vertical" }}
          />
        </div>
        <div>
          <label htmlFor="autotext-description" style={{ fontSize: 13, fontWeight: 600 }}>Description (optional)</label>
          <input
            id="autotext-description"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Personal email"
            style={{ marginTop: 4, width: "100%" }}
          />
        </div>
        {error && <div role="alert" style={{ fontSize: 13, color: "var(--danger)" }}>{error}</div>}
        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
          <button className="tt-btn tt-btn-primary" onClick={submit}>{initial ? "Save" : "Add snippet"}</button>
          <button className="tt-btn tt-btn-ghost" onClick={onCancel}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

type StrNum = { [k: string]: number };

export default function AutoTextScreen() {
  const [entries, setEntries] = useState<AutoTextEntry[]>([]);
  const [systemEntries, setSystemEntries] = useState<AutoTextEntry[]>([]);
  const [showSystem, setShowSystem] = useState(false);
  const [usage, setUsage] = useState<StrNum>({});
  const [showCreate, setShowCreate] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [typingEnabled, setTypingEnabled] = useState(true);
  const [autotextTiming, setAutotextTiming] = useState<string>("delimiter");
  const [typingSaveError, setTypingSaveError] = useState("");
  const formAnchor = useRef<HTMLDivElement>(null);

  const saveTypingEnabled = async (value: boolean) => {
    setTypingSaveError("");
    setTypingEnabled(value);
    const settings = await invoke<Settings>("get_settings").catch(() => null);
    if (settings) {
      await invoke("save_settings", {
        settings: { ...settings, typingAutotextEnabled: value },
      }).catch((e) => {
        setTypingSaveError(errorMessage(e));
      });
    }
  };

  const saveAutotextTiming = async (value: string) => {
    setTypingSaveError("");
    setAutotextTiming(value);
    const settings = await invoke<Settings>("get_settings").catch(() => null);
    if (settings) {
      await invoke("save_settings", {
        settings: { ...settings, autotextTiming: value },
      }).catch((e) => {
        setTypingSaveError(errorMessage(e));
      });
    }
  };

  useEffect(() => {
    if (showCreate || editId) {
      // The form now sits under the page header, so "nearest" brings it into
      // view without scrolling the title off-screen for people who are
      // already at the top of the page.
      formAnchor.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }, [showCreate, editId]);

  useEffect(() => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
    invoke<AutoTextEntry[]>("list_system_autotext").then(setSystemEntries).catch(console.error);
    invoke<{ autotextCounts: StrNum }>("get_usage_stats").then((u) => setUsage(u.autotextCounts)).catch(console.error);
    invoke<Settings>("get_settings").then((s) => setTypingEnabled(s.typingAutotextEnabled)).catch(console.error);
    invoke<Settings>("get_settings").then((s) => setAutotextTiming(s.autotextTiming)).catch(console.error);
  }, []);

  // Re-sync when Settings (or any other screen) flips the same toggle, so
  // this copy never shows a stale value.
  useTauriEvent<void>("settings-changed", () => {
    invoke<Settings>("get_settings").then((s) => setTypingEnabled(s.typingAutotextEnabled)).catch(console.error);
    invoke<Settings>("get_settings").then((s) => setAutotextTiming(s.autotextTiming)).catch(console.error);
  });

  const refresh = () => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
    invoke<{ autotextCounts: StrNum }>("get_usage_stats").then((u) => setUsage(u.autotextCounts)).catch(console.error);
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
    for (const e of systemEntries) {
      const name = cat(e.snippet);
      const g = groups.find((x) => x.name === name);
      if (g) g.items.push(e);
      else groups.push({ name, items: [e] });
    }
    return groups.sort((a, b) => order.indexOf(a.name) - order.indexOf(b.name));
  }, [systemEntries]);

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


  return (
    <div className="tt-page">
      <div className="tt-page-header">
        <div>
          <h2 className="tt-page-title">Snippets</h2>
          <p className="tt-page-sub">
            The stuff you shouldn't have to re-type. Say a phrase to drop in an email, link, or signature instantly.
          </p>
        </div>
        <button className="tt-btn tt-btn-primary" onClick={() => { setShowCreate(true); setEditId(null); }} aria-expanded={showCreate} aria-controls="autotext-form">
          <Icon name="plus" size={14} /> Add new
        </button>
      </div>

      {/* The form sits directly under the header instead of below the whole
          page. Clicking "Add new" used to scroll you past the list, the usage
          card and the system section to a form at the very bottom. */}
      <div ref={formAnchor} id="autotext-form">
        {(showCreate || editId) && (
          <EntryForm
            key={editId ?? "new"}
            initial={editId ? entries.find((e) => e.id === editId) ?? null : null}
            onCancel={() => { setShowCreate(false); setEditId(null); }}
            onSave={() => { setShowCreate(false); setEditId(null); refresh(); }}
          />
        )}
      </div>

      {/* Search */}
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div style={{ position: "relative", flex: 1, maxWidth: 320 }}>
          <span style={{ position: "absolute", left: 10, top: "50%", transform: "translateY(-50%)", color: "var(--text-tertiary)" }}>
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
      <div className="tt-card" style={{ padding: 0, overflow: "hidden" }}>
        {filtered.map((e, i) => (
          <div
            key={e.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "12px 16px",
              borderTop: i > 0 ? "1px solid var(--border-subtle)" : "none",
              opacity: e.enabled ? 1 : 0.6,
              transition: "background var(--dur-fast) var(--ease)",
            }}
          >
            <div style={{ width: 32, height: 32, borderRadius: 8, background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
              <Icon name="messages-square" size={17} color="var(--accent)" />
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", rowGap: 3 }}>
                {e.snippet.trim() ? (
                  <span style={{ fontSize: 13.5, fontWeight: 600, fontStyle: "italic" }}>"{e.snippet.trim()}"</span>
                ) : null}
                {e.snippet.trim() && e.trigger.trim() ? (
                  <span aria-hidden style={{ color: "var(--text-tertiary)", fontSize: 12 }}>·</span>
                ) : null}
                {e.trigger.trim() ? (
                  <code
                    style={{
                      color: "var(--accent-ink)",
                      fontSize: 12.5,
                      fontWeight: 600,
                      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                      background: "var(--accent-soft)",
                      borderRadius: 5,
                      padding: "1px 6px",
                    }}
                  >
                    {e.trigger}
                  </code>
                ) : null}
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0, marginTop: 2 }}>
                <span aria-hidden style={{ color: "var(--text-tertiary)", fontSize: 13, flexShrink: 0 }}>
                  →
                </span>
                <span
                  title={e.replacement}
                  style={{
                    fontSize: 13,
                    color: e.enabled ? "var(--text-secondary)" : "var(--text-tertiary)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    flex: 1,
                    minWidth: 0,
                  }}
                >
                  {e.replacement}
                </span>
              </div>
              {e.description && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 3 }}>{e.description}</div>}
            </div>
            {usageFor(e) > 0 && (
              <span
                title={`Used ${usageFor(e)} times`}
                style={{
                  flexShrink: 0,
                  fontSize: 11.5,
                  fontWeight: 600,
                  fontVariantNumeric: "tabular-nums",
                  color: "var(--accent-ink)",
                  background: "var(--accent-soft)",
                  borderRadius: "var(--radius-pill)",
                  padding: "2px 9px",
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 4,
                }}
              >
                <Icon name="trending-up" size={11} color="var(--accent-ink)" />
                {usageFor(e)}×
              </span>
            )}
            <button className="tt-btn tt-btn-ghost" style={{ flexShrink: 0 }} onClick={() => { setEditId(e.id); setShowCreate(false); }}>Edit</button>
            <button className="tt-btn tt-btn-ghost danger" style={{ color: "var(--danger)", flexShrink: 0 }} onClick={() => remove(e.id)}>Delete</button>
            <div style={{ flexShrink: 0 }}>
              <Toggle
                label={`Enable ${e.snippet.trim() || e.trigger}`}
                checked={e.enabled}
                onChange={() => toggle(e)}
              />
            </div>
          </div>
        ))}
        {filtered.length === 0 && (
          <div style={{ padding: 24, textAlign: "center" }}>
            <p style={{ color: "var(--text-secondary)", margin: "0 0 12px", fontSize: 13 }}>
              {entries.length === 0
                ? "No snippets yet. Add your first replacement — say a phrase like “my email” to drop in any saved text."
                : "No snippets match your search."}
            </p>
            {entries.length === 0 && (
              <button className="tt-btn tt-btn-primary" onClick={() => { setShowCreate(true); setEditId(null); }}>
                <Icon name="plus" size={14} /> Add your first replacement
              </button>
            )}
            {entries.length > 0 && query.trim() !== "" && (
              <button className="tt-btn tt-btn-ghost" onClick={() => setQuery("")}>Clear search</button>
            )}
          </div>
        )}
      </div>

      {/* Usage summary */}
      {entries.length > 0 && (
        <div className="tt-card" style={{ padding: "16px 20px" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: mostUsed.length ? 14 : 0 }}>
            <h3 style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-xs)", fontWeight: 600, color: "var(--text-tertiary)", margin: 0 }}>
              <Icon name="chart-column" size={16} color="var(--accent)" />
              Most used snippets
            </h3>
            <span style={{ fontSize: 12, color: "var(--text-secondary)", fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>{totalUsed.toLocaleString()} expansions</span>
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
                      {e.snippet.trim() ? `"${e.snippet.trim()}"` : <code style={{ color: "var(--accent-ink)" }}>{e.trigger}</code>}
                    </span>
                    <span style={{ fontSize: 12, color: "var(--text-secondary)", flexShrink: 0, marginLeft: 10, fontVariantNumeric: "tabular-nums" }}>{n}×</span>
                  </div>
                  <div style={{ height: 8, background: "var(--surface-2)", borderRadius: 4, overflow: "hidden" }}>
                    <div style={{ height: "100%", width: `${Math.round((n / maxUsed) * 100)}%`, background: "var(--accent-strong)", borderRadius: 4, transition: "width 0.4s" }} />
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* System AutoText (built-in, read-only) */}
      <div className="tt-card" style={{ overflow: "hidden" }}>
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
          <Icon name="wand" size={16} color="var(--accent)" />
          <span style={{ fontSize: 14, fontWeight: 600 }}>System AutoText</span>
          <span style={{ fontSize: 11, color: "var(--text-secondary)", background: "var(--surface-2)", borderRadius: "var(--radius-pill)", padding: "2px 8px" }}>
            built-in · {systemEntries.length}
          </span>
          <span style={{ marginLeft: "auto", fontSize: 12, color: "var(--text-secondary)" }}>
            {showSystem ? "Hide" : "Show"}
          </span>
        </button>
        {showSystem && (
          <div style={{ padding: "16px 18px 20px", borderTop: "1px solid var(--border-subtle)" }}>
            <p style={{ fontSize: 12.5, color: "var(--text-secondary)", margin: "0 0 18px", lineHeight: 1.55, maxWidth: 640 }}>
              Built-in spoken commands that insert punctuation, symbols, and line breaks. Say one to use it
              (e.g. say <b style={{ color: "var(--text)" }}>"comma"</b> to type <b style={{ color: "var(--text)" }}>,</b>).
              Your custom snippets override these when they share a phrase.
            </p>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))", columnGap: 28, rowGap: 20, alignItems: "start" }}>
              {systemGroups.map((g) => (
                <div key={g.name}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
                    <span style={{ fontSize: "var(--text-xs)", fontWeight: 600, color: "var(--text-tertiary)" }}>
                      {g.name}
                    </span>
                    <span style={{ fontSize: 11, color: "var(--text-secondary)", background: "var(--surface-2)", borderRadius: "var(--radius-pill)", padding: "1px 8px", fontVariantNumeric: "tabular-nums" }}>
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
                          borderRadius: "var(--radius-xs)",
                          fontSize: 13,
                        }}
                      >
                        <span style={{ color: "var(--text)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{e.snippet}</span>
                        <code
                          style={{
                            color: "var(--accent-ink)",
                            fontSize: 12.5,
                            flexShrink: 0,
                            background: "var(--accent-soft)",
                            borderRadius: "var(--radius-xs)",
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

      {/* Settings */}
      <div style={{ marginTop: 20 }}>
      <Section
        title="How triggers work"
        hint="Each snippet has its own trigger. Type the trigger (the /slash one shown in the list) followed by a space or Enter, and Teletype replaces it with the saved text. A snippet can also have a spoken phrase: saying it while dictating drops the saved text in."
      >
        <Row label="Expand AutoText while typing" first>
          <Toggle
            label="Expand AutoText while typing"
            checked={typingEnabled}
            onChange={saveTypingEnabled}
          />
        </Row>
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
        {typingSaveError !== "" && (
          <div style={{ padding: "9px 14px", fontSize: "var(--text-xs)", color: "var(--danger)" }} role="alert">
            Failed to save: {typingSaveError}
          </div>
        )}
      </Section>
      </div>
    </div>
  );
}
