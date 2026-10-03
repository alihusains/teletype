import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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

interface Profile {
  language: string;
  preferences: Preference[];
  learn_from_edits: boolean;
  learn_app_specific: boolean;
  learn_terminology: boolean;
}

export default function PersonalizationScreen() {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [newPref, setNewPref] = useState("");

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
    if (!newPref.trim()) return;
    const now = Date.now();
    await invoke("add_preference", {
      preference: {
        id: crypto.randomUUID(),
        description: newPref,
        phrase: newPref,
        explicit: true,
        scope: "global",
        count: 0,
        created_at: now,
        updated_at: now,
      },
    });
    setNewPref("");
    refresh();
  };

  const setToggle = async (key: keyof Profile, value: boolean) => {
    await invoke("set_profile_settings", {
      learnFromEdits: key === "learn_from_edits" ? value : profile?.learn_from_edits ?? true,
      learnAppSpecific: key === "learn_app_specific" ? value : profile?.learn_app_specific ?? true,
      learnTerminology: key === "learn_terminology" ? value : profile?.learn_terminology ?? true,
    });
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

  const learnedRow = (p: Preference) => {
    const conf = confidenceLabel(p.count);
    return (
      <div key={p.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 0" }}>
        <span style={{ flex: 1 }}>{p.description}</span>
        {conf && (
          <span
            title={`Learned from ${p.count} observation${p.count !== 1 ? "s" : ""} (${conf.label})`}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              fontSize: 11,
              color: conf.color,
            }}
          >
            <span
              style={{
                width: 7,
                height: 7,
                borderRadius: "50%",
                background: conf.color,
                display: "inline-block",
              }}
            />
            {conf.label}
          </span>
        )}
        <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>
          {p.count} observation{p.count !== 1 ? "s" : ""}
        </span>
        <button className="danger" onClick={() => removePref(p.id)}>Remove</button>
      </div>
    );
  };

  const renderLearnedGroups = () => (
    <>
      {globalLearned.map(learnedRow)}
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
          {prefs.map(learnedRow)}
        </div>
      ))}
    </>
  );

  return (
    <div>
      <h2 style={{ fontSize: 18, fontWeight: 600, marginBottom: 4 }}>Personalization</h2>
      <p style={{ color: "var(--text-secondary)", marginBottom: 16 }}>
        Teletype learns your writing preferences locally. Nothing is sent to a server.
      </p>

      <div style={{ display: "grid", gap: 8, marginBottom: 24 }}>
        {([
          ["learn_from_edits", "Learn from my corrections"],
          ["learn_app_specific", "Learn app-specific preferences"],
          ["learn_terminology", "Remember preferred terminology"],
        ] as const).map(([key, label]) => (
          <label key={key} style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
            <input
              type="checkbox"
              checked={profile[key] as boolean}
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
        <button className="primary" onClick={addPref}>Add</button>
      </div>
      {explicit.map((p) => (
        <div key={p.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 0" }}>
          <span style={{ flex: 1 }}>{p.description}</span>
          <button className="danger" onClick={() => removePref(p.id)}>Remove</button>
        </div>
      ))}
      {explicit.length === 0 && <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>No explicit preferences yet.</p>}

      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", margin: "16px 0 8px" }}>
        Learned Preferences
      </h3>
      {renderLearnedGroups()}
      {learned.length === 0 && <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>No learned preferences yet.</p>}
      {learned.length > 0 && (
        <button style={{ marginTop: 8 }} onClick={clearLearned}>Clear learned preferences</button>
      )}
    </div>
  );
}
