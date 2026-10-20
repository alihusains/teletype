import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { Segmented } from "../settings/primitives";

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

const inputStyle = {
  padding: "6px 10px",
  fontSize: "var(--text-sm)",
  color: "var(--text)",
  background: "var(--surface)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius-sm)",
} as const;

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
  // generic LLMs. S1-mini uses a fixed training prompt and ignores them.
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
          Style
        </h2>
        <p
          style={{
            color: "var(--text-secondary)",
            fontSize: "var(--text-sm)",
            marginTop: 4,
            marginBottom: 0,
          }}
        >
          Writing-style profiles. The active profile's style guides every dictation.
        </p>
      </div>

      {isS1Mini && (
        <div
          style={{
            marginBottom: 16,
            padding: "10px 14px",
            background: "var(--surface-2)",
            borderRadius: "var(--radius-sm)",
            fontSize: "var(--text-xs)",
            color: "var(--text-secondary)",
            lineHeight: 1.5,
          }}
        >
          S1-mini uses its own Tone / Structure / Context controls below.
          Custom style profiles are not applied.
        </div>
      )}

      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 10,
          opacity: isS1Mini ? 0.5 : 1,
          marginBottom: 20,
        }}
      >
        {profiles.map((p) => {
          const active = p.id === activeId;
          return (
            <div
              key={p.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 14,
                background: active ? "var(--accent-soft)" : "var(--surface)",
                borderRadius: "var(--radius)",
                boxShadow: "var(--shadow-1)",
                padding: "12px 16px",
              }}
            >
              <button
                onClick={() => activate(active ? "" : p.id)}
                className={active ? "primary" : ""}
                style={{
                  minWidth: 96,
                  fontSize: "var(--text-sm)",
                  fontWeight: 600,
                  padding: "5px 12px",
                  borderRadius: "var(--radius-sm)",
                  ...(active
                    ? {
                        background: "transparent",
                        border: "1px solid var(--accent)",
                        color: "var(--accent-ink)",
                      }
                    : {}),
                }}
              >
                {active ? "Active" : "Activate"}
              </button>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  style={{
                    fontWeight: 600,
                    fontSize: "var(--text-md)",
                    color: active ? "var(--accent-ink)" : "var(--text)",
                    display: "flex",
                    alignItems: "center",
                    gap: 6,
                  }}
                >
                  {active ? <Icon name="check" size={14} /> : null}
                  {p.name}
                  {BUILTIN_IDS.has(p.id) && (
                    <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", fontWeight: 400 }}>
                      built-in
                    </span>
                  )}
                </div>
                <div style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)" }}>
                  {p.description}
                </div>
                {p.stylePhrases.length > 0 && (
                  <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 2 }}>
                    {p.stylePhrases.join(" · ")}
                  </div>
                )}
              </div>
              {!BUILTIN_IDS.has(p.id) && (
                <button
                  className="danger"
                  onClick={() => remove(p.id)}
                  title="Delete"
                  aria-label={`Delete profile ${p.name}`}
                  style={{ color: "var(--danger)", border: "1px solid var(--border)" }}
                >
                  <Icon name="trash" size={15} />
                </button>
              )}
            </div>
          );
        })}
      </div>

      {!isS1Mini && (
        <div
          style={{
            background: "var(--surface)",
            borderRadius: "var(--radius)",
            boxShadow: "var(--shadow-1)",
            padding: 16,
            display: "flex",
            flexDirection: "column",
            gap: 8,
          }}
        >
          <div
            style={{
              fontSize: "var(--text-xs)",
              fontWeight: 600,
              color: "var(--text-tertiary)",
            }}
          >
            New profile
          </div>
          <input
            placeholder="Name, e.g. Support emails"
            value={name}
            onChange={(e) => setName(e.target.value)}
            style={inputStyle}
          />
          <input
            placeholder="Description"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            style={inputStyle}
          />
          <input
            placeholder="Style phrases, comma-separated, e.g. be warm, keep it short"
            value={phrases}
            onChange={(e) => setPhrases(e.target.value)}
            style={inputStyle}
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
            borderRadius: "var(--radius)",
            boxShadow: "var(--shadow-1)",
            padding: 16,
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          <div>
            <div
              style={{
                fontSize: "var(--text-xs)",
                fontWeight: 600,
                color: "var(--text-tertiary)",
              }}
            >
              S1-mini writing style
            </div>
            <p style={{ color: "var(--text-secondary)", fontSize: "var(--text-xs)", marginTop: 4 }}>
              S1-mini was trained on these three settings. Change them any time; a new pick
              applies to your next dictation.
            </p>
          </div>

          <S1Row label="Tone">
            <Segmented
              label="Tone"
              value={s1.styling}
              options={S1_STYLING.map(([value, label]) => ({ value, label }))}
              onChange={(v) => saveS1({ ...s1, styling: v })}
            />
          </S1Row>

          <S1Row label="Structure">
            <Segmented
              label="Structure"
              value={s1.structure}
              options={S1_STRUCTURE.map(([value, label]) => ({ value, label }))}
              onChange={(v) => saveS1({ ...s1, structure: v })}
            />
          </S1Row>

          <S1Row label="Context">
            <Segmented
              label="Context"
              value={s1.context}
              options={S1_CONTEXT.map(([value, label]) => ({ value, label }))}
              onChange={(v) => saveS1({ ...s1, context: v })}
            />
          </S1Row>
        </div>
      )}
    </div>
  );
}

function S1Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
      <span style={{ width: 72, fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>
        {label}
      </span>
      {children}
    </div>
  );
}
