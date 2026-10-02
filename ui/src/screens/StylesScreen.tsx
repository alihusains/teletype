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

interface S1Control {
  styling: string;
  structure: string;
  context: string;
}

const S1_STYLING: [string, string][] = [
  ["casual", "Casual"],
  ["semi-casual", "Semi-casual"],
  ["semi-formal", "Semi-formal"],
  ["formal", "Formal"],
];
const S1_STRUCTURE: [string, string][] = [
  ["lists", "Lists"],
  ["prose", "Prose"],
];
const S1_CONTEXT: [string, string][] = [
  ["general", "General"],
  ["email", "Email"],
];

export default function StylesScreen() {
  const [profiles, setProfiles] = useState<StyleProfile[]>([]);
  const [activeId, setActiveId] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [phrases, setPhrases] = useState("");
  const [s1, setS1] = useState<S1Control>({ styling: "semi-formal", structure: "lists", context: "general" });
  const [activeModel, setActiveModel] = useState("");

  const refresh = useCallback(() => {
    invoke<StyleProfile[]>("list_style_profiles").then(setProfiles).catch(console.error);
    invoke<{ activeStyleProfile: string; selectedLlmModel: string }>("get_settings")
      .then((s) => {
        setActiveId(s.activeStyleProfile);
        setActiveModel(s.selectedLlmModel);
      })
      .catch(console.error);
    invoke<{ s1Control: S1Control }>("get_profile")
      .then((p) => setS1(p.s1Control))
      .catch(console.error);
  }, []);

  // Model compatibility: style phrases are only injected into the prompt for
  // generic LLMs. EG-1 and S1-mini use fixed training prompts and ignore them.
  const isSpecializedModel = activeModel === "eg-1" || activeModel === "s1-mini";
  const isS1Mini = activeModel === "s1-mini";

  useEffect(refresh, [refresh]);

  const saveS1 = async (next: S1Control) => {
    setS1(next);
    await invoke("set_s1_control", {
      styling: next.styling,
      structure: next.structure,
      context: next.context,
    }).catch(console.error);
  };

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

      {isSpecializedModel && (
        <div
          style={{
            background: "var(--surface)",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius)",
            padding: "10px 14px",
            fontSize: 12,
            color: "var(--text-secondary)",
            lineHeight: 1.5,
          }}
        >
          {activeModel === "eg-1"
            ? "EG-1 uses a fixed editing style and ignores custom style profiles. Switch to a general-purpose model (Fast or Quality) to use style profiles."
            : "S1-mini uses its own Tone / Structure / Context controls below. Custom style profiles are not applied."}
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 10, opacity: isSpecializedModel ? 0.5 : 1 }}>
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
                <button className="danger" onClick={() => remove(p.id)} title="Delete" aria-label={`Delete profile ${p.name}`}>
                  <Icon name="trash" size={15} />
                </button>
              )}
            </div>
          );
        })}
      </div>

      {!isSpecializedModel && (
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
      )}

      {isS1Mini && (
        <div
          style={{
            background: "var(--surface)",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius)",
            padding: 16,
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          <div>
            <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)" }}>
              S1-mini writing style
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 12, marginTop: 4 }}>
              S1-mini was trained on these three settings. Change them any time; a new pick
              applies to your next dictation.
            </p>
          </div>

          <S1Row label="Tone">
            {S1_STYLING.map(([value, label]) => (
              <S1Chip
                key={value}
                label={label}
                active={s1.styling === value}
                onClick={() => saveS1({ ...s1, styling: value })}
              />
            ))}
          </S1Row>

          <S1Row label="Structure">
            {S1_STRUCTURE.map(([value, label]) => (
              <S1Chip
                key={value}
                label={label}
                active={s1.structure === value}
                onClick={() => saveS1({ ...s1, structure: value })}
              />
            ))}
          </S1Row>

          <S1Row label="Context">
            {S1_CONTEXT.map(([value, label]) => (
              <S1Chip
                key={value}
                label={label}
                active={s1.context === value}
                onClick={() => saveS1({ ...s1, context: value })}
              />
            ))}
          </S1Row>
        </div>
      )}
    </div>
  );
}

function S1Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
      <span style={{ width: 72, fontSize: 12, color: "var(--text-secondary)" }}>{label}</span>
      <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>{children}</div>
    </div>
  );
}

function S1Chip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        padding: "5px 12px",
        borderRadius: "var(--radius)",
        fontSize: 12,
        border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`,
        background: active ? "var(--accent)" : "transparent",
        color: active ? "#fff" : "var(--text-secondary)",
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}
