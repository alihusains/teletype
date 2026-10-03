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
function confidenceLabel(count: number): { label: string; color: string } | null {
  if (count >= 8) return { label: "strong", color: "#16a34a" };
  if (count >= 4) return { label: "building", color: "#2563eb" };
  if (count >= 2) return { label: "new", color: "#9333ea" };
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

// T5.6: one preference row that can flip into an inline edit state.
function PrefRow({
  pref,
  confidence,
  onRemove,
  onSave,
}: {
  pref: Preference;
  confidence: { label: string; color: string } | null;
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
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 0" }}>
        <input
          style={{ flex: 1 }}
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") setEditing(false);
          }}
        />
        <button className="primary" onClick={save}>Save</button>
        <button onClick={() => setEditing(false)}>Cancel</button>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 0" }}>
      <span style={{ flex: 1 }}>{pref.description}</span>
      {confidence && (
        <span
          title={`Learned from ${pref.count} observation${pref.count !== 1 ? "s" : ""} (${confidence.label})`}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 4,
            fontSize: 11,
            color: confidence.color,
          }}
        >
          <span
            style={{
              width: 7,
              height: 7,
              borderRadius: "50%",
              background: confidence.color,
              display: "inline-block",
            }}
          />
          {confidence.label}
        </span>
      )}
      {confidence && (
        <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>
          {pref.count} observation{pref.count !== 1 ? "s" : ""}
        </span>
      )}
      <button onClick={() => { setText(pref.description); setEditing(true); }} title="Edit">Edit</button>
      <button className="danger" onClick={() => onRemove(pref.id)}>Remove</button>
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
    fontSize: 13,
    marginBottom: 6,
  };

  return (
    <>
      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", margin: "16px 0 8px" }}>
        Per-app language
      </h3>
      <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
        Overrides the global speech language for that app.
      </div>
      {Object.entries(langMap).map(([appKey, lang]) => (
        <div key={appKey} style={rowStyle}>
          <code style={{ flex: 1 }}>{appKey}</code>
          <span>{speechLanguages.find((l) => l.code === lang)?.name ?? lang}</span>
          <button
            onClick={async () => {
              await invoke("set_app_language_override", { appKey, lang: "" }).catch(console.error);
              refresh();
            }}
          >
            Remove
          </button>
        </div>
      ))}
      <div style={{ ...rowStyle, marginTop: 4 }}>
        <input
          placeholder="App name or bundle id, e.g. com.notion.id or Notion"
          value={newApp}
          onChange={(e) => setNewApp(e.target.value)}
          style={{ flex: 1 }}
        />
        <select value={newLang} onChange={(e) => setNewLang(e.target.value)}>
          {speechLanguages.map((l) => (
            <option key={l.code} value={l.code}>
              {l.name}
            </option>
          ))}
        </select>
        <button
          className="primary"
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
    </>
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

  if (!profile) return <p>Loading…</p>;

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
    <div>
      <h2 style={{ fontSize: 18, fontWeight: 600, marginBottom: 4 }}>Personalization</h2>
      <p style={{ color: "var(--text-secondary)", marginBottom: 16 }}>
        Teletype learns your writing preferences locally. Nothing is sent to a server.
      </p>

      {/* T5.9: master switch. */}
      <label
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          cursor: "pointer",
          padding: "10px 12px",
          marginBottom: 12,
          border: "1px solid var(--border)",
          borderRadius: 8,
          fontWeight: 600,
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
          gap: 8,
          marginBottom: 24,
          opacity: masterOn ? 1 : 0.5,
          pointerEvents: masterOn ? "auto" : "none",
        }}
      >
        {([
          ["learnFromEdits", "Learn from my corrections"],
          ["learnAppSpecific", "Learn app-specific preferences"],
          ["learnTerminology", "Remember preferred terminology"],
        ] as const).map(([key, label]) => (
          <label
            key={key}
            style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", fontSize: 13 }}
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

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 8 }}>
        Explicit Preferences
      </h3>
      <div style={{ display: "flex", gap: 8, marginBottom: 16 }}>
        <input
          style={{ flex: 1 }}
          placeholder="e.g. Keep my writing concise"
          value={newPref}
          onChange={(e) => setNewPref(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addPref()}
        />
        <select
          value={newScope}
          onChange={(e) => setNewScope(e.target.value)}
          title="Applies to"
        >
          {SCOPE_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <button className="primary" onClick={addPref}>Add</button>
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
      {explicit.length === 0 && <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>No explicit preferences yet.</p>}

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", margin: "16px 0 8px" }}>
        Learned Preferences
      </h3>
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
        <div key={type} style={{ marginTop: 12 }}>
          <h4
            style={{
              fontSize: 12,
              textTransform: "uppercase",
              color: "var(--text-secondary)",
              margin: "0 0 4px",
            }}
          >
            {APP_TYPE_LABELS[type] ?? type}
          </h4>
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
      {learned.length === 0 && <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>No learned preferences yet.</p>}
      {learned.length > 0 && (
        <button style={{ marginTop: 8 }} onClick={clearLearned}>Clear learned preferences</button>
      )}

      <PerAppLanguage />
    </div>
  );
}
