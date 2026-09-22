import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

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

const ACCENT = "#2563eb";

const EXAMPLE_SNIPPETS = [
  { say: "my address", to: "42 Maple Street, Springfield, IL 62704" },
  { say: "sign off", to: "Best,\nAlex Sorathiya\n(+1) 555-0142" },
  { say: "meeting invite", to: "Hi team, quick sync tomorrow 10am — agenda attached…" },
];

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

type StrNum = { [k: string]: number };

export default function AutoTextScreen() {
  const [entries, setEntries] = useState<AutoTextEntry[]>([]);
  const [systemEntries, setSystemEntries] = useState<AutoTextEntry[]>([]);
  const [showSystem, setShowSystem] = useState(false);
  const [usage, setUsage] = useState<StrNum>({});
  const [showCreate, setShowCreate] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
    invoke<AutoTextEntry[]>("list_system_autotext").then(setSystemEntries).catch(console.error);
    invoke<{ autotextCounts: StrNum }>("get_usage_stats").then((u) => setUsage(u.autotextCounts)).catch(console.error);
  }, []);

  const refresh = () => {
    invoke<AutoTextEntry[]>("list_autotext").then(setEntries).catch(console.error);
    invoke<{ autotextCounts: StrNum }>("get_usage_stats").then((u) => setUsage(u.autotextCounts)).catch(console.error);
  };

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
            built-in · {systemEntries.length}
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
          {entries.length > 0
            ? (spoken.length ? spoken : entries).slice(0, 3).map((e) => (
                <div key={e.id} style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                  <span style={{ background: "rgba(255,255,255,0.14)", borderRadius: 8, padding: "6px 12px", fontSize: 13, fontStyle: "italic" }}>
                    "{e.snippet.trim() || e.trigger}"
                  </span>
                  <span style={{ opacity: 0.6 }}>→</span>
                  <span style={{ background: "rgba(255,255,255,0.10)", borderRadius: 8, padding: "6px 12px", fontSize: 13, maxWidth: 360, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {e.replacement}
                  </span>
                </div>
              ))
            : EXAMPLE_SNIPPETS.map((ex) => (
                <div key={ex.say} style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                  <span style={{ background: "rgba(255,255,255,0.14)", borderRadius: 8, padding: "6px 12px", fontSize: 13, fontStyle: "italic" }}>
                    "{ex.say}"
                  </span>
                  <span style={{ opacity: 0.6 }}>→</span>
                  <span style={{ background: "rgba(255,255,255,0.10)", borderRadius: 8, padding: "6px 12px", fontSize: 13, maxWidth: 360, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {ex.to}
                  </span>
                </div>
              ))}
        </div>
        <button
          onClick={() => { setShowCreate(true); setEditId(null); }}
          style={{ marginTop: 18, background: "#fff", color: "#111827", border: "none", borderRadius: 10, padding: "9px 16px", fontWeight: 600, fontSize: 13 }}
        >
          Add new snippet
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
                    <div style={{ height: "100%", width: `${Math.round((n / maxUsed) * 100)}%`, background: `linear-gradient(90deg, ${ACCENT}, #60a5fa)`, borderRadius: 4, transition: "width 0.4s" }} />
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
