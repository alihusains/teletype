import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { Segmented } from "../settings/primitives";
import { errorMessage } from "../lib/errors";

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
  width: "100%",
  minWidth: 0,
  padding: "6px 10px",
  fontSize: "var(--text-sm)",
  color: "var(--text)",
  background: "var(--surface)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius-sm)",
} as const;

const badgeStyle = {
  fontSize: 10.5,
  fontWeight: 600,
  color: "var(--text-tertiary)",
  background: "var(--surface-2)",
  borderRadius: 4,
  padding: "1px 5px",
  whiteSpace: "nowrap",
} as const;

export default function StylesScreen({ active = true }: { active?: boolean }) {
  const [profiles, setProfiles] = useState<StyleProfile[]>([]);
  const [activeId, setActiveId] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [phrases, setPhrases] = useState("");
  const [s1, setS1] = useState<S1Control>({
    styling: "semi-formal",
    structure: "lists",
    context: "general",
  });
  const [activeModel, setActiveModel] = useState("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

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

  // The screen stays mounted while hidden. Refresh when it becomes visible so
  // the S1-mini layout follows a model change made on the Models screen.
  useEffect(() => {
    if (active) refresh();
  }, [active, refresh]);

  const saveS1 = async (next: S1Control) => {
    setS1(next);
    setError("");
    await invoke("set_s1_control", {
      styling: next.styling,
      structure: next.structure,
      context: next.context,
    }).catch((e) => setError(errorMessage(e)));
  };

  const activate = async (id: string) => {
    if (isS1Mini) return;
    setActiveId(id);
    setError("");
    await invoke("set_active_style_profile", { id }).catch((e) => {
      setError(errorMessage(e));
      refresh();
    });
  };

  const create = async () => {
    const n = name.trim();
    if (!n || creating) return;
    setCreating(true);
    setError("");
    try {
      await invoke("create_style_profile", {
        profile: {
          id: "",
          name: n,
          description: description.trim(),
          stylePhrases: phrases.split(",").map((p) => p.trim()).filter(Boolean),
          createdAt: 0,
          updatedAt: 0,
        },
      });
      setName("");
      setDescription("");
      setPhrases("");
      refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setCreating(false);
    }
  };

  const remove = async (profile: StyleProfile) => {
    if (BUILTIN_IDS.has(profile.id) || isS1Mini) return;
    if (!confirm(`Delete “${profile.name}”?`)) return;
    setError("");
    try {
      await invoke("delete_style_profile", { id: profile.id });
      if (activeId === profile.id) setActiveId("");
      refresh();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="tt-page" style={{ paddingBottom: 24 }}>
      <div className="tt-page-header">
        <div>
          <h2 className="tt-page-title">Style</h2>
          <p className="tt-page-sub">
            Writing-style profiles. The active profile's style guides every dictation.
          </p>
        </div>
      </div>

      {isS1Mini && (
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 10,
            marginBottom: 16,
            padding: "10px 14px",
            background: "var(--surface-2)",
            borderRadius: "var(--radius-sm)",
            fontSize: "var(--text-xs)",
            color: "var(--text-secondary)",
            lineHeight: 1.5,
          }}
        >
          <Icon
            name="sparkles"
            size={16}
            color="var(--accent)"
            style={{ flexShrink: 0, marginTop: 1 }}
          />
          <span>
            S1-mini uses its own Tone / Structure / Context controls below. Custom style profiles
            are not applied.
          </span>
        </div>
      )}

      {error && (
        <div
          role="alert"
          style={{
            marginBottom: 16,
            padding: "10px 14px",
            borderRadius: "var(--radius-sm)",
            background: "var(--danger-soft)",
            color: "var(--danger)",
            fontSize: "var(--text-sm)",
          }}
        >
          {error}
        </div>
      )}

      {isS1Mini ? (
        <>
          <ProfilesSection
            profiles={profiles}
            activeId={activeId}
            disabled
            onActivate={activate}
            onRemove={remove}
          />
          <S1Card s1={s1} onChange={saveS1} />
        </>
      ) : (
        <div className="tt-style-layout">
          <div className="tt-style-main">
            <ProfilesSection
              profiles={profiles}
              activeId={activeId}
              disabled={false}
              onActivate={activate}
              onRemove={remove}
            />
          </div>
          <aside className="tt-style-side">
            <CreateProfileCard
              name={name}
              description={description}
              phrases={phrases}
              creating={creating}
              onName={setName}
              onDescription={setDescription}
              onPhrases={setPhrases}
              onCreate={create}
            />
          </aside>
        </div>
      )}
    </div>
  );
}

function ProfilesSection({
  profiles,
  activeId,
  disabled,
  onActivate,
  onRemove,
}: {
  profiles: StyleProfile[];
  activeId: string;
  disabled: boolean;
  onActivate: (id: string) => void;
  onRemove: (profile: StyleProfile) => void;
}) {
  return (
    <section aria-labelledby="style-profiles-heading">
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          gap: 12,
          marginBottom: 10,
        }}
      >
        <h3
          id="style-profiles-heading"
          style={{ margin: 0, fontSize: 13, fontWeight: 650, color: "var(--text)" }}
        >
          Profiles
        </h3>
        <span style={{ fontSize: 11.5, color: "var(--text-tertiary)" }}>
          {profiles.length} {profiles.length === 1 ? "profile" : "profiles"}
        </span>
      </div>

      {profiles.length === 0 ? (
        <div className="tt-card" style={{ padding: "28px 20px", textAlign: "center" }}>
          <div
            style={{
              width: 40,
              height: 40,
              borderRadius: 10,
              background: "var(--accent-soft)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              margin: "0 auto 10px",
            }}
          >
            <Icon name="sparkles" size={20} color="var(--accent)" />
          </div>
          <div style={{ fontSize: 13, fontWeight: 600 }}>No style profiles yet</div>
          <p
            style={{
              margin: "3px 0 0",
              fontSize: 12,
              color: "var(--text-secondary)",
              lineHeight: 1.5,
            }}
          >
            Create a profile to describe how your dictation should sound.
          </p>
        </div>
      ) : (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 8,
            opacity: disabled ? 0.55 : 1,
          }}
        >
          {profiles.map((profile) => (
            <ProfileRow
              key={profile.id}
              profile={profile}
              active={profile.id === activeId}
              disabled={disabled}
              onActivate={() => onActivate(profile.id === activeId ? "" : profile.id)}
              onRemove={() => onRemove(profile)}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function ProfileRow({
  profile,
  active,
  disabled,
  onActivate,
  onRemove,
}: {
  profile: StyleProfile;
  active: boolean;
  disabled: boolean;
  onActivate: () => void;
  onRemove: () => void;
}) {
  const builtin = BUILTIN_IDS.has(profile.id);
  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        background: active ? "var(--accent-soft)" : "var(--surface)",
        borderRadius: "var(--radius)",
        boxShadow: active ? "0 0 0 1.5px var(--accent)" : "var(--shadow-1)",
        padding: "14px 16px",
      }}
    >
      <button
        type="button"
        aria-pressed={active}
        aria-label={`${active ? "Deactivate" : "Activate"} ${profile.name}`}
        title={active ? "Deactivate" : "Activate"}
        disabled={disabled}
        onClick={onActivate}
        style={{
          width: 24,
          height: 24,
          minHeight: 0,
          padding: 0,
          flex: "0 0 auto",
          marginTop: 1,
          borderRadius: 999,
          border: `1.5px solid ${active ? "var(--accent-strong)" : "var(--border)"}`,
          background: active ? "var(--accent-strong)" : "var(--surface)",
          color: "var(--on-accent)",
          display: "grid",
          placeItems: "center",
          cursor: disabled ? "not-allowed" : "pointer",
        }}
      >
        {active ? <Icon name="check" size={13} color="var(--on-accent)" /> : null}
      </button>

      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
          <span
            style={{
              fontWeight: 600,
              fontSize: "var(--text-md)",
              color: active ? "var(--accent-ink)" : "var(--text)",
            }}
          >
            {profile.name}
          </span>
          {builtin && <span style={badgeStyle}>Built-in</span>}
          {active && (
            <span
              style={{
                fontSize: 10.5,
                fontWeight: 600,
                color: "var(--accent-ink)",
                background: "var(--surface)",
                borderRadius: 4,
                padding: "1px 5px",
                whiteSpace: "nowrap",
              }}
            >
              Active
            </span>
          )}
        </div>
        {profile.description && (
          <div
            style={{
              fontSize: "var(--text-xs)",
              color: "var(--text-secondary)",
              marginTop: 1,
              lineHeight: 1.45,
            }}
          >
            {profile.description}
          </div>
        )}
        {profile.stylePhrases.length > 0 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 6 }}>
            {profile.stylePhrases.map((phrase, index) => (
              <span
                key={`${phrase}-${index}`}
                style={{
                  fontSize: 11.5,
                  color: "var(--text-secondary)",
                  background: "var(--surface-2)",
                  borderRadius: 4,
                  padding: "1px 6px",
                }}
              >
                {phrase}
              </span>
            ))}
          </div>
        )}
      </div>

      {!builtin && (
        <button
          type="button"
          className="tt-icon-btn danger"
          onClick={onRemove}
          disabled={disabled}
          title={disabled ? "Unavailable while S1-mini is selected" : "Delete profile"}
          aria-label={`Delete profile ${profile.name}`}
          style={{ flexShrink: 0, marginTop: -2 }}
        >
          <Icon name="trash" size={15} />
        </button>
      )}
    </div>
  );
}

function CreateProfileCard({
  name,
  description,
  phrases,
  creating,
  onName,
  onDescription,
  onPhrases,
  onCreate,
}: {
  name: string;
  description: string;
  phrases: string;
  creating: boolean;
  onName: (value: string) => void;
  onDescription: (value: string) => void;
  onPhrases: (value: string) => void;
  onCreate: () => void;
}) {
  return (
    <div className="tt-card" style={{ padding: 18 }}>
      <div style={{ marginBottom: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13, fontWeight: 650 }}>New profile</h3>
        <p
          style={{
            margin: "3px 0 0",
            fontSize: 12,
            color: "var(--text-secondary)",
            lineHeight: 1.5,
          }}
        >
          Describe the voice once; every dictation uses it while active.
        </p>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))",
          gap: 8,
        }}
      >
        <input
          placeholder="Name, e.g. Support emails"
          aria-label="Profile name"
          value={name}
          onChange={(e) => onName(e.target.value)}
          style={inputStyle}
        />
        <input
          placeholder="Description"
          aria-label="Profile description"
          value={description}
          onChange={(e) => onDescription(e.target.value)}
          style={inputStyle}
        />
        <input
          placeholder="Style phrases, comma-separated, e.g. be warm, keep it short"
          aria-label="Style phrases"
          value={phrases}
          onChange={(e) => onPhrases(e.target.value)}
          style={{ ...inputStyle, gridColumn: "1 / -1" }}
        />
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 12 }}>
        <button
          className="tt-btn tt-btn-primary"
          onClick={onCreate}
          disabled={!name.trim() || creating}
        >
          <Icon name="plus" size={15} /> {creating ? "Creating…" : "Create profile"}
        </button>
        <span style={{ fontSize: 11.5, color: "var(--text-tertiary)" }}>
          Applies only when you activate it.
        </span>
      </div>
    </div>
  );
}

function S1Card({ s1, onChange }: { s1: S1Control; onChange: (next: S1Control) => void }) {
  return (
    <div className="tt-card" style={{ padding: 18, marginTop: 20 }}>
      <div style={{ marginBottom: 14 }}>
        <h3 style={{ margin: 0, fontSize: 13, fontWeight: 650 }}>S1-mini writing style</h3>
        <p
          style={{
            margin: "3px 0 0",
            fontSize: 12,
            color: "var(--text-secondary)",
            lineHeight: 1.5,
            maxWidth: 560,
          }}
        >
          S1-mini was trained on these three settings. Change them any time; a new pick applies to
          your next dictation.
        </p>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))",
          gap: 16,
          alignItems: "start",
          maxWidth: 1120,
        }}
      >
        <S1Row label="Tone">
          <Segmented
            label="Tone"
            value={s1.styling}
            options={S1_STYLING.map(([value, label]) => ({ value, label }))}
            onChange={(v) => onChange({ ...s1, styling: v })}
          />
        </S1Row>

        <S1Row label="Structure">
          <Segmented
            label="Structure"
            value={s1.structure}
            options={S1_STRUCTURE.map(([value, label]) => ({ value, label }))}
            onChange={(v) => onChange({ ...s1, structure: v })}
          />
        </S1Row>

        <S1Row label="Context">
          <Segmented
            label="Context"
            value={s1.context}
            options={S1_CONTEXT.map(([value, label]) => ({ value, label }))}
            onChange={(v) => onChange({ ...s1, context: v })}
          />
        </S1Row>
      </div>
    </div>
  );
}

function S1Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 6, minWidth: 0 }}>
      <span style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>{label}</span>
      <div style={{ maxWidth: "100%", overflowX: "auto" }}>{children}</div>
    </div>
  );
}
