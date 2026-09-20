import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

interface StyleProfile {
  id: string;
  name: string;
  description: string;
  stylePhrases: string[];
  createdAt: number;
  updatedAt: number;
}

const BUILTIN_IDS = new Set(["style-concise", "style-professional", "style-casual"]);

export default function StylesScreen() {
  const [profiles, setProfiles] = useState<StyleProfile[]>([]);
  const [activeId, setActiveId] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [phrases, setPhrases] = useState("");

  const refresh = useCallback(() => {
    invoke<StyleProfile[]>("list_style_profiles").then(setProfiles).catch(console.error);
    invoke<{ active_style_profile: string }>("get_settings")
      .then((s) => setActiveId(s.active_style_profile))
      .catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  const activate = async (id: string) => {
    setActiveId(id);
    await invoke("set_active_style_profile", { id }).catch(console.error);
  };

  const create = async () => {
    const n = name.trim();
    if (!n) return;
    await invoke("create_style_profile", {
      profile: {
        id: "",
        name: n,
        description: description.trim(),
        stylePhrases: phrases.split(",").map((p) => p.trim()).filter(Boolean),
        createdAt: 0,
        updatedAt: 0,
      },
    }).catch(console.error);
    setName("");
    setDescription("");
    setPhrases("");
    refresh();
  };

  const remove = async (id: string) => {
    if (BUILTIN_IDS.has(id)) return;
    await invoke("delete_style_profile", { id }).catch(console.error);
    refresh();
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, maxWidth: 820 }}>
      <div>
        <h2 style={{ fontSize: 20, fontWeight: 700 }}>Style</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>
          Writing-style profiles. The active profile's style guides every dictation.
        </p>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        {profiles.map((p) => {
          const active = p.id === activeId;
          return (
            <div
              key={p.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 14,
                background: "var(--surface)",
                border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`,
                borderRadius: "var(--radius)",
                padding: "12px 16px",
              }}
            >
              <button
                onClick={() => activate(active ? "" : p.id)}
                className={active ? "primary" : ""}
                style={{ minWidth: 96 }}
              >
                {active ? "Active" : "Activate"}
              </button>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 600 }}>
                  {p.name}
                  {BUILTIN_IDS.has(p.id) && (
                    <span style={{ fontSize: 11, color: "var(--text-secondary)", marginLeft: 8 }}>built-in</span>
                  )}
                </div>
                <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>{p.description}</div>
                {p.stylePhrases.length > 0 && (
                  <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2 }}>
                    {p.stylePhrases.join(" · ")}
                  </div>
                )}
              </div>
              {!BUILTIN_IDS.has(p.id) && (
                <button className="danger" onClick={() => remove(p.id)} title="Delete">
                  <Icon name="trash" size={15} />
                </button>
              )}
            </div>
          );
        })}
      </div>

      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: 16,
          display: "flex",
          flexDirection: "column",
          gap: 8,
        }}
      >
        <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)" }}>New profile</h3>
        <input placeholder="Name, e.g. Support emails" value={name} onChange={(e) => setName(e.target.value)} />
        <input placeholder="Description" value={description} onChange={(e) => setDescription(e.target.value)} />
        <input
          placeholder="Style phrases, comma-separated, e.g. be warm, keep it short"
          value={phrases}
          onChange={(e) => setPhrases(e.target.value)}
        />
        <div>
          <button className="primary" onClick={create} disabled={!name.trim()}>
            <Icon name="plus" size={15} /> Create profile
          </button>
        </div>
      </div>
    </div>
  );
}
