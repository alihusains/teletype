import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSpeechLanguages } from "../lib/useSpeechLanguages";

interface Preference {
  id: string;
  description: string;
  phrase: string;
  explicit: boolean;
  // Serialized from PreferenceScope: "global" or { appType: "email" | ... }.
  scope: string | { appType: string };
  count: number;
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

// Maps the observation count to a confidence label, mirroring
// Confidence::from_count in teletype-core (weak >= 2, medium >= 4, strong >= 8).
function confidenceLabel(count: number): { label: string; tone: "neutral" | "warning" | "success" } | null {
  if (count >= 8) return { label: "strong", tone: "success" };
  if (count >= 4) return { label: "building", tone: "warning" };
  if (count >= 2) return { label: "new", tone: "neutral" };
  return null;
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

// T5.5: the scope a new explicit preference applies to.
const SCOPE_OPTIONS: { value: string; label: string }[] = [
  { value: "global", label: "Everywhere" },
  { value: "email", label: "Email" },
  { value: "chat", label: "Chat" },
  { value: "document", label: "Documents" },
  { value: "coding", label: "Coding" },
];

const inputStyle: CSSProperties = {
  padding: "5px 10px",
  fontSize: "var(--text-sm)",
  color: "var(--text)",
  background: "var(--surface)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius-sm)",
};

const actionBtn: CSSProperties = {
  fontSize: "var(--text-xs)",
  fontWeight: 500,
  padding: "4px 10px",
  borderRadius: "var(--radius-sm)",
  border: "1px solid var(--border)",
  background: "transparent",
  color: "var(--text-secondary)",
  cursor: "pointer",
  whiteSpace: "nowrap",
};

// T5.6: one preference row that can flip into an inline edit state.
function PrefRow({
  pref,
  confidence,
  onRemove,
  onSave,
}: {
  pref: Preference;
  confidence: { label: string; tone: "neutral" | "warning" | "success" } | null;
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
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "8px 14px",
          borderTop: "1px solid var(--border-subtle)",
        }}
      >
        <input
          style={{ ...inputStyle, flex: 1 }}
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") setEditing(false);
          }}
        />
        <button
          style={{
            ...actionBtn,
            fontWeight: 600,
            background: "var(--accent)",
            border: "1px solid var(--accent)",
            color: "var(--bg)",
          }}
          onClick={save}
        >
          Save
        </button>
        <button onClick={() => setEditing(false)} style={actionBtn}>
          Cancel
        </button>
      </div>
    );
  }

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "8px 14px",
        borderTop: "1px solid var(--border-subtle)",
        fontSize: "var(--text-sm)",
      }}
    >
      <span style={{ flex: 1, minWidth: 0 }}>{pref.description}</span>
      {confidence && (
        <span
          title={`Learned from ${pref.count} observation${pref.count !== 1 ? "s" : ""} (${confidence.label})`}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 5,
            fontSize: "var(--text-xs)",
            fontWeight: 600,
            padding: "1px 8px",
            borderRadius: "var(--radius-pill, 999px)",
            border: `1px solid var(${
              confidence.tone === "success"
                ? "--success"
                : confidence.tone === "warning"
                  ? "--warning"
                  : "--border-subtle"
            })`,
            color:
              confidence.tone === "success"
                ? "var(--success)"
                : confidence.tone === "warning"
                  ? "var(--warning)"
                  : "var(--text-tertiary)",
            whiteSpace: "nowrap",
          }}
        >
          {confidence.label}
        </span>
      )}
      {confidence && (
        <span
          style={{
            fontSize: "var(--text-xs)",
            color: "var(--text-tertiary)",
            fontVariantNumeric: "tabular-nums",
            whiteSpace: "nowrap",
          }}
        >
          {pref.count} observation{pref.count !== 1 ? "s" : ""}
        </span>
      )}
      <button onClick={() => { setText(pref.description); setEditing(true); }} title="Edit" style={actionBtn}>
        Edit
      </button>
      <button
        className="danger"
        onClick={() => onRemove(pref.id)}
        style={{ ...actionBtn, color: "var(--danger)", border: "1px solid var(--danger)" }}
      >
        Remove
      </button>
    </div>
  );
}

// T5.4: per-app ASR language overrides, matching SettingsScreen's existing usage.
function PerAppLanguage() {
  const [langMap, setLangMap] = useState<Record<string, string>>({});
  const [newApp, setNewApp] = useState("");
  const [newLang, setNewLang] = useState("en");
  const speechLanguages = useSpeechLanguages();

  const refresh = async () => {
    try {
      setLangMap(await invoke<Record<string, string>>("get_app_language_overrides"));
    } catch { /* command unavailable; keep empty */ }
  };
  useEffect(() => {
    refresh();
  }, []);

  const rowStyle: CSSProperties = {
    display: "flex",
    alignItems: "center",
    gap: 8,
    fontSize: "var(--text-sm)",
    padding: "8px 14px",
    borderTop: "1px solid var(--border-subtle)",
  };

  const entries = Object.entries(langMap);

  return (
    <section style={{ marginBottom: 20 }}>
      <div
        style={{
          fontSize: "var(--text-xs)",
          fontWeight: 600,
          color: "var(--text-tertiary)",
          marginBottom: 8,
        }}
      >
        Per-app language
      </div>
      <p style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", margin: "0 0 8px" }}>
        Overrides the global speech language for that app.
      </p>
      <div className="tt-card" style={{ overflow: "hidden" }}>
        {entries.length === 0 && (
          <div
            style={{
              padding: "12px 14px",
              fontSize: "var(--text-sm)",
              color: "var(--text-tertiary)",
            }}
          >
            No overrides yet. Add an app below to give it its own speech language.
          </div>
        )}
        {entries.map(([appKey, lang]) => (
          <div key={appKey} style={rowStyle}>
            <code
              style={{
                flex: 1,
                minWidth: 0,
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                fontSize: "var(--text-xs)",
              }}
            >
              {appKey}
            </code>
            <span style={{ color: "var(--text-secondary)" }}>
              {speechLanguages.find((l) => l.code === lang)?.name ?? lang}
            </span>
            <button
              style={actionBtn}
              onClick={async () => {
                await invoke("set_app_language_override", { appKey, lang: "" }).catch(console.error);
                refresh();
              }}
            >
              Remove
            </button>
          </div>
        ))}
        <div style={{ ...rowStyle, marginTop: entries.length === 0 ? 0 : 4 }}>
          <input
            placeholder="App name or bundle id, e.g. com.notion.id or Notion"
            value={newApp}
            onChange={(e) => setNewApp(e.target.value)}
            style={{ ...inputStyle, flex: 1, minWidth: 0 }}
          />
          <select
            value={newLang}
            onChange={(e) => setNewLang(e.target.value)}
            aria-label="App language"
            style={inputStyle}
          >
            {speechLanguages.map((l) => (
              <option key={l.code} value={l.code}>
                {l.name}
              </option>
            ))}
          </select>
          <button
            style={{
              ...actionBtn,
              fontWeight: 600,
              background: "var(--accent)",
              border: "1px solid var(--accent)",
              color: "var(--bg)",
            }}
            onClick={async () => {
              const appKey = newApp.trim();
              if (!appKey) return;
              await invoke("set_app_language_override", { appKey, lang: newLang }).catch(console.error);
              setNewApp("");
              refresh();
            }}
          >
            Add
          </button>
        </div>
      </div>
    </section>
  );
}

export default function PersonalizationScreen() {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [newPref, setNewPref] = useState("");
  const [newScope, setNewScope] = useState("global");

  useEffect(() => {
    invoke<Profile>("get_profile").then(setProfile).catch(console.error);
  }, []);

  const refresh = () => {
    invoke<Profile>("get_profile").then(setProfile).catch(console.error);
  };

  const removePref = async (id: string) => {
    await invoke("remove_preference", { id });
    refresh();
  };

  const clearLearned = async () => {
    if (!confirm("Remove all learned preferences?")) return;
    await invoke("clear_learned");
    refresh();
  };

  const addPref = async () => {
    const text = newPref.trim();
    if (!text) return;
    const now = Date.now();
    // BUG-07: the wire is camelCase, so send createdAt/updatedAt (the old
    // snake_case keys were silently dropped by serde defaults).
    const scope =
      newScope === "global" ? "global" : { appType: newScope };
    await invoke("add_preference", {
      preference: {
        id: crypto.randomUUID(),
        description: text,
        phrase: text,
        explicit: true,
        scope,
        count: 0,
        createdAt: now,
        updatedAt: now,
      },
    });
    setNewPref("");
    setNewScope("global");
    refresh();
  };

  // T5.6: edit an existing preference (explicit or learned).
  const savePref = async (id: string, text: string) => {
    await invoke("update_preference", { id, description: text, phrase: text });
    refresh();
  };

  const setToggle = async (key: keyof Profile, value: boolean) => {
    await invoke("set_profile_settings", {
      learnFromEdits: key === "learnFromEdits" ? value : profile?.learnFromEdits ?? true,
      learnAppSpecific: key === "learnAppSpecific" ? value : profile?.learnAppSpecific ?? true,
      learnTerminology: key === "learnTerminology" ? value : profile?.learnTerminology ?? true,
    });
    refresh();
  };

  // T5.9: master switch. Off = all three learning gates false; on = all true.
  const setMaster = async (enabled: boolean) => {
    await invoke("set_personalization_enabled", { enabled });
    refresh();
  };

  if (!profile)
    return (
      <div style={{ width: "100%", maxWidth: 860 }}>
        <div
          style={{
            height: 24,
            width: 160,
            borderRadius: 6,
            background: "var(--surface-2)",
            marginBottom: 12,
          }}
        />
        <div
          style={{
            height: 120,
            width: "100%",
            borderRadius: "var(--radius)",
            background: "var(--surface-2)",
          }}
        />
      </div>
    );

  const learned = profile.preferences.filter((p) => !p.explicit);
  const explicit = profile.preferences.filter((p) => p.explicit);

  // Group learned prefs by app scope (T5.8): AppType-scoped prefs render under
  // an app header; global/unknown prefs render first, ungrouped.
  const globalLearned = learned.filter((p) => {
    const t = appTypeOf(p);
    return t === null || t === "unknown";
  });
  const appGroups = new Map<string, Preference[]>();
  for (const p of learned) {
    const t = appTypeOf(p);
    if (t === null || t === "unknown") continue;
    const list = appGroups.get(t) ?? [];
    list.push(p);
    appGroups.set(t, list);
  }

  const masterOn = profile.learnFromEdits;

  return (
    <div style={{ width: "100%", maxWidth: 860 }}>
      <div style={{ marginBottom: 24 }}>
        <h2
          style={{
            fontSize: 22,
            fontWeight: 650,
            letterSpacing: "var(--tracking-tight)",
            margin: 0,
          }}
        >
          Personalization
        </h2>
        <p
          style={{
            color: "var(--text-secondary)",
            fontSize: "var(--text-sm)",
            marginTop: 4,
            marginBottom: 0,
          }}
        >
          Teletype learns your writing preferences locally. Nothing is sent to a server.
        </p>
      </div>

      {/* T5.9: master switch. */}
      <div className="tt-card" style={{ marginBottom: 20, overflow: "hidden" }}>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            cursor: "pointer",
            padding: "10px 14px",
            fontWeight: 600,
            fontSize: "var(--text-sm)",
          }}
        >
          <input
            type="checkbox"
            checked={masterOn}
            onChange={(e) => setMaster(e.target.checked)}
            style={{ width: 18, height: 18 }}
          />
          Personalization (learn from your edits)
        </label>

        {/* Advanced controls: the three fine-grained gates, subordinated under the master. */}
        <div
          style={{
            display: "grid",
            gap: 2,
            opacity: masterOn ? 1 : 0.5,
            pointerEvents: masterOn ? "auto" : "none",
          }}
        >
          {([
            ["learnFromEdits", "Learn from my corrections"],
            ["learnAppSpecific", "Learn app-specific preferences"],
            ["learnTerminology", "Remember preferred terminology"],
          ] as const).map(([key, label], i) => (
            <label
              key={key}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                cursor: "pointer",
                fontSize: "var(--text-sm)",
                padding: "8px 14px",
                borderTop: i === 0 ? "1px solid var(--border-subtle)" : "none",
              }}
            >
              <input
                type="checkbox"
                checked={profile[key]}
                onChange={(e) => setToggle(key, e.target.checked)}
              />
              {label}
            </label>
          ))}
        </div>
      </div>

      <section style={{ marginBottom: 20 }}>
        <div
          style={{
            fontSize: "var(--text-xs)",
            fontWeight: 600,
            color: "var(--text-tertiary)",
            marginBottom: 8,
          }}
        >
          Explicit Preferences
        </div>
        <div className="tt-card" style={{ overflow: "hidden" }}>
          <div
            style={{
              display: "flex",
              gap: 8,
              padding: "10px 14px",
              borderBottom: "1px solid var(--border-subtle)",
            }}
          >
            <input
              style={{ ...inputStyle, flex: 1, minWidth: 0 }}
              placeholder="e.g. Keep my writing concise"
              value={newPref}
              onChange={(e) => setNewPref(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && addPref()}
            />
            <select
              value={newScope}
              onChange={(e) => setNewScope(e.target.value)}
              aria-label="Applies to"
              style={inputStyle}
            >
              {SCOPE_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <button
              style={{
                ...actionBtn,
                fontWeight: 600,
                background: "var(--accent)",
                border: "1px solid var(--accent)",
                color: "var(--bg)",
              }}
              onClick={addPref}
            >
              Add
            </button>
          </div>
          {explicit.map((p) => (
            <PrefRow
              key={p.id}
              pref={p}
              confidence={null}
              onRemove={removePref}
              onSave={savePref}
            />
          ))}
          {explicit.length === 0 && (
            <p
              style={{
                padding: "12px 14px",
                color: "var(--text-tertiary)",
                fontSize: "var(--text-sm)",
                margin: 0,
              }}
            >
              No explicit preferences yet.
            </p>
          )}
        </div>
      </section>

      <section style={{ marginBottom: 20 }}>
        <div
          style={{
            fontSize: "var(--text-xs)",
            fontWeight: 600,
            color: "var(--text-tertiary)",
            marginBottom: 8,
          }}
        >
          Learned Preferences
        </div>
        <div className="tt-card" style={{ overflow: "hidden" }}>
          {globalLearned.map((p) => (
            <PrefRow
              key={p.id}
              pref={p}
              confidence={confidenceLabel(p.count)}
              onRemove={removePref}
              onSave={savePref}
            />
          ))}
          {[...appGroups.entries()].map(([type, prefs]) => (
            <div key={type}>
              <div
                style={{
                  padding: "8px 14px 4px",
                  fontSize: "var(--text-xs)",
                  fontWeight: 600,
                  color: "var(--text-tertiary)",
                }}
              >
                {APP_TYPE_LABELS[type] ?? type}
              </div>
              {prefs.map((p) => (
                <PrefRow
                  key={p.id}
                  pref={p}
                  confidence={confidenceLabel(p.count)}
                  onRemove={removePref}
                  onSave={savePref}
                />
              ))}
            </div>
          ))}
          {learned.length === 0 && (
            <p
              style={{
                padding: "12px 14px",
                color: "var(--text-tertiary)",
                fontSize: "var(--text-sm)",
                margin: 0,
              }}
            >
              No learned preferences yet.
            </p>
          )}
        </div>
        {learned.length > 0 && (
          <button
            style={{
              marginTop: 8,
              ...actionBtn,
              color: "var(--danger)",
              border: "1px solid var(--danger)",
            }}
            onClick={clearLearned}
          >
            Clear learned preferences
          </button>
        )}
      </section>

      <PerAppLanguage />
    </div>
  );
}
