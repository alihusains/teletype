import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { errorMessage } from "../lib/errors";

interface StyleProfile {
  id: string;
  name: string;
  description: string;
  stylePhrases: string[];
  createdAt: number;
  updatedAt: number;
}

/** One rung of the cascade, as `style_resolution` reports it. */
interface StyleLayer {
  rank: number;
  key: string;
  label: string;
  styleId: string;
  styleName: string;
  winner: boolean;
  note: string;
}

interface StyleResolution {
  frontmostApp: string;
  frontmostAppType: string;
  layers: StyleLayer[];
  effectivePhrases: string[];
}

const COMPARE_SEED =
  "hey so i was thinking maybe we could possibly try to circle back on the thing from yesterday when you get a chance no rush at all";

const BUILT_IN_ORDER = ["style-concise", "style-professional", "style-casual"];
const BUILT_IN_NAMES: Record<string, string> = {
  "style-concise": "Concise",
  "style-professional": "Professional",
  "style-casual": "Casual",
};

/** Words in a run of text, counted the way the measurement line counts them. */
export function countWords(text: string): number {
  const trimmed = text.trim();
  return trimmed ? trimmed.split(/\s+/).length : 0;
}

export interface WordDelta {
  words: number;
  /** Positive when the output is shorter than the seed, in percent. */
  percent: number;
  label: string;
}

/** The measurable line under a compared output: words, and the change vs. the original. */
export function measureWords(input: string, output: string): WordDelta {
  const from = countWords(input);
  const words = countWords(output);
  if (from === 0) {
    return {
      words,
      percent: 0,
      label: words === 0 ? "same length as the original" : "no original to measure against",
    };
  }
  const percent = Math.round(((from - words) / from) * 100);
  let label = "same length as the original";
  if (percent > 0) label = `shorter by ${percent}%`;
  else if (percent < 0) label = `longer by ${Math.abs(percent)}%`;
  return { words, percent, label };
}

const BUILTIN_IDS = new Set(BUILT_IN_ORDER);

interface ComparePane {
  id: string;
  name: string;
  phrases: string[];
  text: string;
  error: string | null;
}

const bandTitle: React.CSSProperties = {
  fontSize: 13,
  textTransform: "uppercase",
  color: "var(--text-secondary)",
};

const noteText: React.CSSProperties = {
  fontSize: 12,
  color: "var(--text-secondary)",
  lineHeight: 1.5,
};

const inUsePill: React.CSSProperties = {
  fontSize: 11,
  padding: "2px 8px",
  borderRadius: "var(--radius)",
  background: "var(--accent)",
  color: "#fff",
  whiteSpace: "nowrap",
};

const phraseChip: React.CSSProperties = {
  fontSize: 11,
  color: "var(--text-secondary)",
  background: "var(--surface-2)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius)",
  padding: "2px 8px",
};

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
  const [resolution, setResolution] = useState<StyleResolution | null>(null);
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  const [sample, setSample] = useState(COMPARE_SEED);
  const [compareId, setCompareId] = useState("");
  const [panes, setPanes] = useState<ComparePane[] | null>(null);
  const [running, setRunning] = useState(false);
  const [pendingPane, setPendingPane] = useState("");

  /**
   * Re-reads the cascade for the app in front. Runs on mount and after anything
   * that can change it, so Band A never shows a stale or guessed answer.
   */
  const refreshResolution = useCallback(() => {
    invoke<StyleResolution>("style_resolution")
      .then(setResolution)
      .catch((e) => {
        // Hide Band A rather than show a cascade we did not resolve.
        console.error(e);
        setResolution(null);
      });
  }, []);

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
    invoke<Record<string, string>>("get_app_style_overrides")
      .then(setOverrides)
      .catch(console.error);
    refreshResolution();
  }, [refreshResolution]);

  // Model compatibility: style phrases are only injected into the prompt for
  // generic LLMs. EG-1 and S1-mini use fixed training prompts and ignore them.
  const isSpecializedModel = activeModel === "eg-1" || activeModel === "s1-mini";
  const isS1Mini = activeModel === "s1-mini";
  const modelName = activeModel === "eg-1" ? "EG-1" : "S1-mini";

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
    refreshResolution();
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

  const nameOf = (id: string) =>
    profiles.find((p) => p.id === id)?.name || BUILT_IN_NAMES[id] || "Custom style";
  const phrasesOf = (id: string) => profiles.find((p) => p.id === id)?.stylePhrases || [];
  /** Which app keys pin this style, from the per-app override map. */
  const pinsOf = (id: string) =>
    Object.entries(overrides)
      .filter(([, styleId]) => styleId === id)
      .map(([appKey]) => appKey);

  const winner = resolution?.layers.find((l) => l.winner) || null;
  const effectiveId = winner?.styleId || "";
  const appLabel = resolution?.frontmostApp || "your current app";
  // Compare runs the active style, or the first built-in when none is active.
  const baseId =
    activeId && profiles.some((p) => p.id === activeId) ? activeId : "style-concise";
  const otherId =
    compareId || BUILT_IN_ORDER.find((id) => id !== baseId) || "style-professional";
  // Two panes, or one when the user picked the active style to compare against.
  const compareIds = [baseId, otherId].filter((id, i, all) => all.indexOf(id) === i);
  const sampleWords = countWords(sample);

  const runCompare = async () => {
    const text = sample.trim();
    if (!text || running) return;
    setRunning(true);
    setPanes([]);
    const results: ComparePane[] = [];
    for (const id of compareIds) {
      setPendingPane(nameOf(id));
      let pane: ComparePane = { id, name: nameOf(id), phrases: phrasesOf(id), text: "", error: null };
      try {
        const out = await invoke<string>("preview_style_profile", { profileId: id, input: text });
        pane = { ...pane, text: out };
      } catch (e) {
        // Inline in the pane: a failure must not become a dialog storm.
        pane = { ...pane, error: errorMessage(e) };
      }
      results.push(pane);
      // Land each result as it arrives, so a slow first pane still shows.
      setPanes([...results]);
    }
    setPendingPane("");
    setPanes(results);
    setRunning(false);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, width: "100%" }}>
      <div>
        <h2 className="tt-screen-title">Style</h2>
      </div>

      {/* Band A - which level of the cascade is deciding, for the app in front. */}
      {resolution && (
        <section
          style={{
            background: "var(--surface)",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius)",
            padding: 16,
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          <div>
            <h3 style={bandTitle}>Style right now</h3>
            <p style={{ fontSize: 14, marginTop: 6, lineHeight: 1.5 }}>
              In {appLabel}, Teletype{" "}
              {effectiveId
                ? `writes like ${winner?.styleName || "a style that no longer exists"}`
                : "adds no style of its own"}
              .
            </p>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {resolution.layers.map((layer) => (
              <div
                key={layer.key}
                style={{
                  display: "flex",
                  gap: 12,
                  alignItems: "flex-start",
                  border: `1px solid ${layer.winner ? "var(--accent)" : "var(--border)"}`,
                  background: layer.winner ? "var(--accent-soft)" : "transparent",
                  borderRadius: "var(--radius)",
                  padding: "8px 12px",
                }}
              >
                <span
                  style={{
                    fontSize: 11,
                    fontWeight: 700,
                    color: "var(--text-secondary)",
                    background: "var(--surface-2)",
                    border: "1px solid var(--border)",
                    borderRadius: "var(--radius)",
                    padding: "1px 7px",
                    marginTop: 2,
                  }}
                >
                  {layer.rank}
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                    <span style={{ fontWeight: 600, fontSize: 13 }}>{layer.label}</span>
                    <span
                      style={{
                        fontSize: 12,
                        color: layer.styleId ? "var(--text)" : "var(--text-tertiary)",
                      }}
                    >
                      {layer.styleId
                        ? layer.styleName || "a style that no longer exists"
                        : "not set"}
                    </span>
                    {layer.winner && <span style={inUsePill}>In use</span>}
                  </div>
                  <div style={{ ...noteText, marginTop: 2 }}>{layer.note}</div>
                </div>
              </div>
            ))}
          </div>

          <div>
            <div style={{ ...noteText, marginBottom: 6 }}>What the model is told:</div>
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              {resolution.effectivePhrases.length > 0 ? (
                resolution.effectivePhrases.map((phrase) => (
                  <span key={phrase} style={phraseChip}>
                    {phrase}
                  </span>
                ))
              ) : (
                <span style={phraseChip}>nothing about style, it only cleans up the wording</span>
              )}
            </div>
          </div>
        </section>
      )}

      {/* Band B - the measurement: one sentence, two writing styles. */}
      <section
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: 16,
          display: "flex",
          flexDirection: "column",
          gap: 10,
        }}
      >
        <div>
          <h3 style={bandTitle}>See the difference</h3>
          <p style={{ ...noteText, marginTop: 4 }}>
            The same sentence polished twice. Nothing in your settings changes.
          </p>
        </div>

        {isSpecializedModel ? (
          <div
            style={{
              border: "1px dashed var(--border)",
              borderRadius: "var(--radius)",
              padding: "10px 12px",
              ...noteText,
            }}
          >
            Compare is unavailable while {modelName} is the model. It ignores style phrases, so
            both results would come back the same.
          </div>
        ) : (
          <>
            <textarea
              value={sample}
              onChange={(e) => setSample(e.target.value)}
              rows={3}
              aria-label="Sentence to compare"
              style={{
                width: "100%",
                boxSizing: "border-box",
                minHeight: 76,
                resize: "vertical",
                fontFamily: "inherit",
                fontSize: 13,
                lineHeight: 1.5,
                padding: "10px 12px",
                borderRadius: "var(--radius)",
                border: "1px solid var(--border)",
                background: "var(--bg)",
                color: "var(--text)",
              }}
            />
            <label style={noteText}>
              Compare {nameOf(baseId)} with{" "}
              <select
                value={otherId}
                onChange={(e) => setCompareId(e.target.value)}
                aria-label="Style to compare against"
                style={{
                  fontSize: 12,
                  padding: "3px 6px",
                  borderRadius: "var(--radius)",
                  border: "1px solid var(--border)",
                  background: "var(--surface-2)",
                  color: "var(--text)",
                }}
              >
                {BUILT_IN_ORDER.map((id) => (
                  <option key={id} value={id}>
                    {nameOf(id)}
                  </option>
                ))}
              </select>
            </label>
            <div>
              <button className="primary" onClick={runCompare} disabled={!sampleWords || running}>
                {running ? "Running…" : "Compare"}
              </button>
              <span style={{ ...noteText, marginLeft: 10 }}>
                {sampleWords ? `${sampleWords} words to polish` : "Write something to compare first."}
              </span>
            </div>

            {panes && (
              <>
                <div
                  style={{
                    background: "var(--surface-2)",
                    border: "1px solid var(--border)",
                    borderRadius: "var(--radius)",
                    padding: "10px 12px",
                  }}
                >
                  <div style={{ ...noteText, marginBottom: 4 }}>Original</div>
                  <div style={{ fontSize: 13, lineHeight: 1.5 }}>{sample.trim()}</div>
                </div>
                <div
                  style={{
                    display: "grid",
                    gap: 10,
                    gridTemplateColumns: "repeat(auto-fit, minmax(min(260px, 100%), 1fr))",
                  }}
                >
                  {panes.map((pane) => {
                    const measure = measureWords(sample, pane.text);
                    return (
                      <div
                        key={pane.id}
                        style={{
                          border: "1px solid var(--border)",
                          borderRadius: "var(--radius)",
                          padding: "10px 12px",
                          display: "flex",
                          flexDirection: "column",
                          gap: 6,
                          minWidth: 0,
                        }}
                      >
                        <div style={{ fontWeight: 600, fontSize: 13 }}>{pane.name}</div>
                        <div style={{ ...noteText, fontSize: 11 }}>
                          {pane.phrases.length ? pane.phrases.join(" · ") : "no style phrases"}
                        </div>
                        {pane.error ? (
                          <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                            {pane.error}
                          </div>
                        ) : (
                          <>
                            <div
                              style={{
                                fontSize: 13,
                                lineHeight: 1.5,
                                whiteSpace: "pre-wrap",
                                overflowWrap: "anywhere",
                              }}
                            >
                              {running && pendingPane === pane.name && !pane.text
                                ? "Running…"
                                : pane.text}
                            </div>
                            {!running && !pane.error && (
                              <div style={{ ...noteText, fontSize: 11 }}>
                                {measure.words} words · {measure.label}
                              </div>
                            )}
                          </>
                        )}
                      </div>
                    );
                  })}
                </div>
              </>
            )}
          </>
        )}
      </section>

      {/* Band C - the styles themselves. */}
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
            ? "EG-1 uses a fixed editing style and ignores the styles below. Switch to a general-purpose model (Fast or Quality) to use them."
            : "S1-mini uses its own Tone / Structure / Context controls at the bottom of this page. The styles below are not applied."}
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 10, opacity: isSpecializedModel ? 0.5 : 1, pointerEvents: isSpecializedModel ? "none" : "auto" }}>
        {profiles.map((p) => {
          const active = p.id === activeId;
          const inUseNow = resolution !== null && p.id === effectiveId;
          const pins = pinsOf(p.id);
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
                <div
                  style={{
                    fontWeight: 600,
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    flexWrap: "wrap",
                  }}
                >
                  {p.name}
                  {BUILTIN_IDS.has(p.id) && (
                    <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>built-in</span>
                  )}
                  {inUseNow && <span style={inUsePill}>In use now in {appLabel}</span>}
                  {pins.length > 0 && (
                    <span style={{ fontSize: 11, fontWeight: 400, color: "var(--text-secondary)" }}>
                      pinned for {pins.join(", ")}
                    </span>
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
                <button className="danger" onClick={() => remove(p.id)} title="Delete" aria-label={`Delete style ${p.name}`}>
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
          <h3 style={bandTitle}>New style</h3>
          <input placeholder="Name, e.g. Support emails" value={name} onChange={(e) => setName(e.target.value)} />
          <input placeholder="Description" value={description} onChange={(e) => setDescription(e.target.value)} />
          <input
            placeholder="Style phrases, comma-separated, e.g. be warm, keep it short"
            value={phrases}
            onChange={(e) => setPhrases(e.target.value)}
          />
          <div>
            <button className="primary" onClick={create} disabled={!name.trim()}>
              <Icon name="plus" size={15} /> Create style
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
