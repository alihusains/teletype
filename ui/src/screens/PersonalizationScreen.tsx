import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";
import { confirmDialog } from "../lib/dialogs";
import { useWrites } from "../lib/writeGuard";
import { WriteFailed } from "../components/StateRows";

interface Preference {
  id: string;
  description: string;
  phrase: string;
  explicit: boolean;
  // Serialized from PreferenceScope: "global" or { appType: "email" | ... }.
  scope: string | { appType: string };
  count: number;
  createdAt: number;
  updatedAt: number;
  // Set on a learned word correction: the word the model produced and the
  // word the user typed over it (personalization::Preference).
  learnedFrom?: string;
  learnedTo?: string;
}

// Display names for the AppType scope variants (serde camelCase).
const APP_TYPE_LABELS: Record<string, string> = {
  email: "Email apps",
  chat: "Chat apps",
  social: "Social apps",
  coding: "Coding apps",
  document: "Document apps",
  browser: "Browser apps",
  terminal: "Terminal apps",
  unknown: "Other apps",
};

function appTypeOf(p: Preference): string | null {
  return typeof p.scope === "object" && p.scope !== null && "appType" in p.scope
    ? p.scope.appType
    : null;
}

// A learned rule only reaches the model once it has been seen twice; that is
// the filter in personalization::packet::resolve (`explicit || count >= 2`).
// The row has to say which side of that line it is on, or a single-observation
// rule looks like a rule the app is ignoring for no reason.
function isApplied(p: Preference): boolean {
  return p.explicit || p.count >= 2;
}

function scopeLabel(p: Preference): string {
  const t = appTypeOf(p);
  return t === null ? "Everywhere" : APP_TYPE_LABELS[t] ?? t;
}

function dateString(ms: number): string | null {
  if (!ms) return null;
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return null;
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

// BUG-08: the wire is camelCase (UserProfile is #[serde(rename_all = "camelCase")]),
// so the profile fields arrive as learnFromEdits / learnAppSpecific / learnTerminology.
// The old interface used snake_case, which only "worked" because setToggle re-mapped
// them; the rendered checkbox state therefore never reflected the real wire value.
interface Profile {
  language: string;
  preferences: Preference[];
  learnFromEdits: boolean;
  learnAppSpecific: boolean;
  learnTerminology: boolean;
}

// The scope a new explicit preference applies to. (T5.5)
const SCOPE_OPTIONS: { value: string; label: string }[] = [
  { value: "global", label: "Everywhere" },
  { value: "email", label: "Email" },
  { value: "chat", label: "Chat" },
  { value: "document", label: "Documents" },
  { value: "coding", label: "Coding" },
];

type Filter = "all" | "everywhere" | "apps";

const titleText: CSSProperties = {
  fontSize: 13,
  textTransform: "uppercase",
  letterSpacing: "0.04em",
  color: "var(--text-secondary)",
  margin: "0 0 10px",
};

const noteText: CSSProperties = {
  fontSize: 13,
  color: "var(--text-secondary)",
  lineHeight: 1.5,
};

const chip: CSSProperties = {
  fontSize: 11.5,
  lineHeight: "18px",
  padding: "0 8px",
  borderRadius: "var(--radius-pill)",
  border: "1px solid var(--border)",
  background: "var(--surface-2)",
  color: "var(--text-secondary)",
  whiteSpace: "nowrap",
};

const chipActive: CSSProperties = {
  ...chip,
  border: "1px solid color-mix(in srgb, var(--success) 40%, var(--border))",
  background: "var(--success-soft)",
  color: "var(--success)",
};

const chipWaiting: CSSProperties = {
  ...chip,
  border: "1px solid color-mix(in srgb, var(--warning) 40%, var(--border))",
  background: "var(--warning-soft)",
  color: "var(--warning)",
};

function Chip({
  tone = "plain",
  children,
}: {
  tone?: "plain" | "active" | "waiting";
  children: ReactNode;
}) {
  const style = tone === "active" ? chipActive : tone === "waiting" ? chipWaiting : chip;
  return <span style={style}>{children}</span>;
}

/** One thing Teletype knows. The evidence line is the point of the row: it
 *  says where the rule came from and whether it is in force yet. */
function RuleRow({
  pref,
  onRemove,
  onSave,
}: {
  pref: Preference;
  onRemove: (id: string) => void;
  onSave: (id: string, text: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(pref.description);

  if (editing) {
    const save = () => {
      const t = text.trim();
      if (!t) return;
      onSave(pref.id, t);
      setEditing(false);
    };
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 0" }}>
        <input
          aria-label={`Rewrite the rule: ${pref.description}`}
          style={{ flex: 1 }}
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") setEditing(false);
          }}
        />
        <button className="primary" onClick={save}>
          Save
        </button>
        <button
          onClick={() => {
            setText(pref.description);
            setEditing(false);
          }}
        >
          Cancel
        </button>
      </div>
    );
  }

  const applied = isApplied(pref);
  const when = dateString(pref.createdAt);
  const correction =
    pref.learnedFrom && pref.learnedTo
      ? `Hears “${pref.learnedFrom}”, writes “${pref.learnedTo}”`
      : null;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        padding: "9px 0",
        borderTop: "1px solid var(--border-subtle)",
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 14, lineHeight: 1.45 }}>{pref.description}</div>
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            alignItems: "center",
            gap: 6,
            marginTop: 5,
          }}
        >
          {pref.explicit ? (
            <Chip>You added this</Chip>
          ) : (
            <Chip>
              Learned from {pref.count} edit{pref.count === 1 ? "" : "s"}
            </Chip>
          )}
          {applied ? (
            <Chip tone="active">In force</Chip>
          ) : (
            <Chip tone="waiting">Needs one more match</Chip>
          )}
          <Chip>{scopeLabel(pref)}</Chip>
          {when ? <Chip>{pref.explicit ? `Added ${when}` : `Learned ${when}`}</Chip> : null}
        </div>
        {correction ? (
          <div style={{ ...noteText, marginTop: 5, fontSize: 12.5 }}>{correction}</div>
        ) : null}
      </div>
      <div style={{ display: "flex", gap: 6, paddingTop: 2 }}>
        <button
          onClick={() => {
            setText(pref.description);
            setEditing(true);
          }}
          aria-label={`Rewrite “${pref.description}”`}
        >
          Rewrite
        </button>
        <button
          className="danger"
          onClick={() => onRemove(pref.id)}
          aria-label={`Forget “${pref.description}”`}
        >
          Forget
        </button>
      </div>
    </div>
  );
}

/** Per-app ASR language overrides. (T5.4) */
function PerAppLanguage() {
  const [langMap, setLangMap] = useState<Record<string, string>>({});
  const [newApp, setNewApp] = useState("");
  const [newLang, setNewLang] = useState("en");
  const speechLanguages = useSpeechLanguages();
  const writes = useWrites();

  const refresh = async () => {
    try {
      setLangMap(await invoke<Record<string, string>>("get_app_language_overrides"));
    } catch {
      /* command unavailable; keep empty */
    }
  };
  useEffect(() => {
    refresh();
  }, []);

  // The picker shows the language the user asked for before the write settles,
  // so a rejected write has to put the old value back or the row lies.
  const setOverride = async (appKey: string, lang: string, previous: string) => {
    setLangMap((m) => {
      const next = { ...m };
      if (lang) next[appKey] = lang;
      else delete next[appKey];
      return next;
    });
    const result = await writes.run(`lang:${appKey}`, () =>
      invoke("set_app_language_override", { appKey, lang })
    );
    if (!result.ok) {
      setLangMap((m) => {
        const next = { ...m };
        if (previous) next[appKey] = previous;
        else delete next[appKey];
        return next;
      });
    }
  };

  const rowStyle: CSSProperties = {
    display: "flex",
    alignItems: "center",
    gap: 8,
    fontSize: 13,
    padding: "8px 0",
    borderTop: "1px solid var(--border-subtle)",
  };

  return (
    <section style={{ marginTop: 28 }}>
      <h3 style={titleText}>Per-app speech language</h3>
      <p style={{ ...noteText, margin: "0 0 10px" }}>
        Use a different speech language in one app. Overrides the global language for that app
        only.
      </p>
      {Object.entries(langMap).map(([appKey, lang]) => (
        <div key={appKey} style={{ borderTop: "1px solid var(--border-subtle)" }}>
          <div style={rowStyle}>
            <code style={{ flex: 1 }}>{appKey}</code>
            <span>
              {writes.busy(`lang:${appKey}`)
                ? "Saving…"
                : (speechLanguages.find((l) => l.code === lang)?.name ?? lang)}
            </span>
            <select
              value={lang}
              onChange={(e) => setOverride(appKey, e.target.value, lang)}
              aria-label={`Speech language for ${appKey}`}
            >
              {speechLanguages.map((l) => (
                <option key={l.code} value={l.code}>
                  {l.name}
                </option>
              ))}
            </select>
            <button
              onClick={() => setOverride(appKey, "", lang)}
              aria-label={`Remove the language override for ${appKey}`}
            >
              Remove
            </button>
          </div>
          {writes.error(`lang:${appKey}`) ? (
            <WriteFailed
              message={writes.error(`lang:${appKey}`) as string}
              label={`The language for ${appKey}`}
              onDismiss={() => writes.clear(`lang:${appKey}`)}
            />
          ) : null}
        </div>
      ))}
      {Object.keys(langMap).length === 0 ? (
        <p style={{ ...noteText, fontSize: 12.5, margin: "2px 0 8px" }}>
          No overrides. Speech uses your global language in every app.
        </p>
      ) : null}
      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        <input
          aria-label="App name or bundle id"
          placeholder="App name or bundle id, e.g. com.notion.id or Notion"
          value={newApp}
          onChange={(e) => setNewApp(e.target.value)}
          style={{ flex: 1 }}
        />
        <select
          value={newLang}
          onChange={(e) => setNewLang(e.target.value)}
          aria-label="Language for this app"
        >
          {speechLanguages.map((l) => (
            <option key={l.code} value={l.code}>
              {l.name}
            </option>
          ))}
        </select>
        <button
          className="primary"
          disabled={writes.busy(`lang:${newApp.trim().toLowerCase()}`)}
          onClick={async () => {
            const appKey = newApp.trim().toLowerCase();
            if (!appKey) return;
            const result = await writes.run(`lang:${appKey}`, () =>
              invoke("set_app_language_override", { appKey, lang: newLang })
            );
            if (result.ok) {
              setNewApp("");
              refresh();
            }
          }}
        >
          {writes.busy(`lang:${newApp.trim().toLowerCase()}`) ? "Adding…" : "Add"}
        </button>
      </div>
    </section>
  );
}

export default function PersonalizationScreen() {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [failed, setFailed] = useState(false);
  const [newPref, setNewPref] = useState("");
  const [newScope, setNewScope] = useState("global");
  const [filter, setFilter] = useState<Filter>("all");

  const load = () => {
    invoke<Profile>("get_profile")
      .then((p) => {
        setProfile(p);
        setFailed(false);
      })
      .catch(() => setFailed(true));
  };
  useEffect(load, []);

  const removePref = async (id: string) => {
    // Drop it from the list immediately; the reload only reconciles. A settings
    // list that waits for a round trip before reacting feels broken.
    setProfile((p) => (p ? { ...p, preferences: p.preferences.filter((x) => x.id !== id) } : p));
    await invoke("remove_preference", { id }).catch(() => setFailed(true));
    load();
  };

  const clearLearned = async () => {
    if (
      !(await confirmDialog("Forget everything Teletype learned from your edits?", {
        title: "Forget learned rules",
      }))
    )
      return;
    setProfile((p) => (p ? { ...p, preferences: p.preferences.filter((x) => x.explicit) } : p));
    await invoke("clear_learned").catch(() => setFailed(true));
    load();
  };

  const addPref = async () => {
    const text = newPref.trim();
    if (!text) return;
    const now = Date.now();
    const scope = newScope === "global" ? "global" : { appType: newScope };
    const pref: Preference = {
      // BUG-07: the wire is camelCase, so send createdAt/updatedAt (the old
      // snake_case keys were silently dropped by serde defaults).
      id: crypto.randomUUID(),
      description: text,
      phrase: text,
      explicit: true,
      scope,
      count: 0,
      createdAt: now,
      updatedAt: now,
    };
    setProfile((p) => (p ? { ...p, preferences: [pref, ...p.preferences] } : p));
    setNewPref("");
    setNewScope("global");
    await invoke("add_preference", { preference: pref }).catch(() => setFailed(true));
    load();
  };

  // Edit an existing preference, explicit or learned. (T5.6)
  const savePref = async (id: string, text: string) => {
    setProfile((p) =>
      p
        ? {
            ...p,
            preferences: p.preferences.map((x) =>
              x.id === id ? { ...x, description: text, phrase: text } : x,
            ),
          }
        : p,
    );
    await invoke("update_preference", { id, description: text, phrase: text }).catch(() =>
      setFailed(true),
    );
    load();
  };

  const setToggle = async (key: "learnAppSpecific" | "learnTerminology", value: boolean) => {
    if (!profile) return;
    const next = { ...profile, [key]: value };
    setProfile(next);
    await invoke("set_profile_settings", {
      learnFromEdits: next.learnFromEdits,
      learnAppSpecific: next.learnAppSpecific,
      learnTerminology: next.learnTerminology,
    }).catch(() => setFailed(true));
    load();
  };

  // The master switch: off sets all three learning gates false, on restores
  // them. (T5.9)
  const setMaster = async (enabled: boolean) => {
    if (!profile) return;
    setProfile({
      ...profile,
      learnFromEdits: enabled,
      learnAppSpecific: enabled,
      learnTerminology: enabled,
    });
    await invoke("set_personalization_enabled", { enabled }).catch(() => setFailed(true));
    load();
  };

  if (failed && !profile) {
    return (
      <div>
        <h2 className="tt-screen-title" style={{ marginBottom: 4 }}>
          Personalization
        </h2>
        <p style={noteText}>
          Teletype could not read your rules from this Mac. Nothing was lost; the file may be in
          use by another Teletype window.
        </p>
        <button className="primary" style={{ marginTop: 12 }} onClick={load}>
          Try again
        </button>
      </div>
    );
  }

  if (!profile) {
    return (
      <div>
        <h2 className="tt-screen-title" style={{ marginBottom: 4 }}>
          Personalization
        </h2>
        <p style={noteText} role="status">
          Reading your rules…
        </p>
      </div>
    );
  }

  const learned = profile.preferences.filter((p) => !p.explicit);
  const explicit = profile.preferences.filter((p) => p.explicit);
  const inForce = profile.preferences.filter(isApplied);
  const masterOn = profile.learnFromEdits;

  const buckets: Record<Filter, Preference[]> = {
    all: profile.preferences,
    everywhere: profile.preferences.filter((p) => appTypeOf(p) === null),
    apps: profile.preferences.filter((p) => appTypeOf(p) !== null),
  };
  const shown = buckets[filter];

  // App-scoped rules read better under the app they belong to than in one long
  // line, so that filter groups by scope; the other two stay a single list.
  const byApp = new Map<string, Preference[]>();
  if (filter === "apps") {
    for (const p of shown) {
      const t = appTypeOf(p) ?? "unknown";
      byApp.set(t, [...(byApp.get(t) ?? []), p]);
    }
  }

  const filters: { id: Filter; label: string; count: number }[] = [
    { id: "all", label: "All", count: buckets.all.length },
    { id: "everywhere", label: "Everywhere", count: buckets.everywhere.length },
    { id: "apps", label: "In specific apps", count: buckets.apps.length },
  ];

  return (
    <div>
      <h2 className="tt-screen-title" style={{ marginBottom: 4 }}>
        Personalization
      </h2>
      <p style={{ ...noteText, margin: "0 0 16px" }}>
        What Teletype knows about how you write, and what it is allowed to learn next. This lives
        in a file on this Mac and is never sent anywhere.
      </p>

      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 6,
          marginBottom: 18,
          alignItems: "center",
        }}
      >
        <Chip tone="active">{inForce.length} in force</Chip>
        <Chip>{learned.length} learned from your edits</Chip>
        <Chip>{explicit.length} you added</Chip>
        {learned.filter((p) => !isApplied(p)).length > 0 ? (
          <Chip tone="waiting">
            {learned.filter((p) => !isApplied(p)).length} waiting for a second match
          </Chip>
        ) : null}
      </div>

      <label
        style={{
          display: "flex",
          alignItems: "flex-start",
          gap: 10,
          cursor: "pointer",
          padding: "12px 14px",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          background: "var(--surface)",
          boxShadow: "var(--shadow-1)",
        }}
      >
        <input
          type="checkbox"
          checked={masterOn}
          onChange={(e) => setMaster(e.target.checked)}
          style={{ width: 18, height: 18, marginTop: 1 }}
        />
        <span>
          <span style={{ display: "block", fontWeight: 600, fontSize: 14 }}>
            Learn from my edits
          </span>
          <span style={{ ...noteText, display: "block", marginTop: 2 }}>
            When this is off, Teletype stops watching what you change, and none of the rules below
            are added to your text.
          </span>
        </span>
      </label>

      <div
        style={{
          display: "grid",
          gap: 10,
          margin: "12px 0 26px",
          paddingLeft: 14,
          borderLeft: "2px solid var(--border)",
          opacity: masterOn ? 1 : 0.45,
          pointerEvents: masterOn ? "auto" : "none",
        }}
      >
        {(
          [
            [
              "learnTerminology",
              "Words I keep correcting",
              "You type “Kubernetes” over “kubernetes” and it remembers the spelling.",
            ],
            [
              "learnAppSpecific",
              "Rules for one kind of app",
              "Allow rules that only apply in email, chat, documents or code.",
            ],
          ] as const
        ).map(([key, label, help]) => (
          <label
            key={key}
            style={{ display: "flex", alignItems: "flex-start", gap: 8, cursor: "pointer" }}
          >
            <input
              type="checkbox"
              checked={profile[key]}
              onChange={(e) => setToggle(key, e.target.checked)}
              style={{ marginTop: 2 }}
            />
            <span>
              <span style={{ display: "block", fontSize: 13.5 }}>{label}</span>
              <span style={{ ...noteText, display: "block", fontSize: 12 }}>{help}</span>
            </span>
          </label>
        ))}
      </div>

      <section>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            flexWrap: "wrap",
          }}
        >
          <h3 style={{ ...titleText, margin: 0 }}>What it knows</h3>
          <div style={{ display: "flex", gap: 6 }} role="group" aria-label="Filter rules">
            {filters.map((f) => (
              <button
                key={f.id}
                onClick={() => setFilter(f.id)}
                aria-pressed={filter === f.id}
                style={{
                  fontSize: 12,
                  padding: "3px 10px",
                  borderRadius: "var(--radius-pill)",
                  border: `1px solid ${filter === f.id ? "var(--accent)" : "var(--border)"}`,
                  background: filter === f.id ? "var(--accent-soft)" : "transparent",
                  color: filter === f.id ? "var(--accent-ink)" : "var(--text-secondary)",
                }}
              >
                {f.label} ({f.count})
              </button>
            ))}
          </div>
        </div>

        {shown.length === 0 ? (
          <p style={{ ...noteText, margin: "14px 0 0" }}>
            {filter === "apps"
              ? "Nothing learned for a specific app yet. Teletype picks these up when you edit the same way several times in one kind of app."
              : filter === "everywhere"
                ? "No rules that apply everywhere yet. Add one below, or edit the same word twice after dictating and Teletype will suggest it."
                : "Nothing yet. Two ways to start: fix a word Teletype dictated, twice, the same way; or add a rule yourself below."}
          </p>
        ) : (
          <div style={{ marginTop: 10 }}>
            {filter === "apps"
              ? [...byApp.entries()].map(([type, prefs]) => (
                  <div key={type} style={{ marginTop: 10 }}>
                    <h4
                      style={{
                        fontSize: 12,
                        color: "var(--text-tertiary)",
                        margin: "0 0 2px",
                      }}
                    >
                      {APP_TYPE_LABELS[type] ?? type}
                    </h4>
                    {prefs.map((p) => (
                      <RuleRow key={p.id} pref={p} onRemove={removePref} onSave={savePref} />
                    ))}
                  </div>
                ))
              : shown.map((p) => (
                  <RuleRow key={p.id} pref={p} onRemove={removePref} onSave={savePref} />
                ))}
          </div>
        )}

        <div
          style={{
            display: "flex",
            gap: 8,
            marginTop: 16,
            paddingTop: 14,
            borderTop: "1px solid var(--border)",
          }}
        >
          <input
            aria-label="A rule Teletype should always follow"
            placeholder="e.g. Always write Q3, not quarter three"
            value={newPref}
            onChange={(e) => setNewPref(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addPref()}
            style={{ flex: 1 }}
          />
          <select
            value={newScope}
            onChange={(e) => setNewScope(e.target.value)}
            aria-label="Applies to"
          >
            {SCOPE_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          <button className="primary" onClick={addPref}>
            Add rule
          </button>
        </div>
        <p style={{ ...noteText, fontSize: 12, marginTop: 6 }}>
          A rule you add here is in force from the next dictation. A rule Teletype learns has to be
          seen twice first.
        </p>
        {learned.length > 0 ? (
          <button className="danger" style={{ marginTop: 12 }} onClick={clearLearned}>
            Forget everything it learned
          </button>
        ) : null}
      </section>

      <PerAppLanguage />
    </div>
  );
}
