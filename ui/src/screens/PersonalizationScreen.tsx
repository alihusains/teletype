import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Preference {
  id: string;
  description: string;
  phrase: string;
  explicit: boolean;
  scope: string;
  count: number;
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
      {learned.map((p) => (
        <div key={p.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 0" }}>
          <span style={{ flex: 1 }}>{p.description}</span>
          <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>
            {p.count} observation{p.count !== 1 ? "s" : ""}
          </span>
          <button className="danger" onClick={() => removePref(p.id)}>Remove</button>
        </div>
      ))}
      {learned.length === 0 && <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>No learned preferences yet.</p>}
      {learned.length > 0 && (
        <button style={{ marginTop: 8 }} onClick={clearLearned}>Clear learned preferences</button>
      )}
    </div>
  );
}
