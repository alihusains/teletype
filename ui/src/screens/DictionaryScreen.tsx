import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

interface DictionaryWord {
  id: string;
  word: string;
  pronunciation: string;
  createdAt: number;
}

interface PackInfo {
  id: string;
  name: string;
  description: string;
  termCount: number;
  enabled: boolean;
}

interface PackTerm {
  canonical: string;
  mishearings: string[];
}

type TeachMode = "idle" | "listening" | "heard" | "added";

export default function DictionaryScreen() {
  const [words, setWords] = useState<DictionaryWord[]>([]);
  const [word, setWord] = useState("");
  const [pronunciation, setPronunciation] = useState("");
  const [mode, setMode] = useState<"voice" | "manual">("voice");
  const [teachWord, setTeachWord] = useState("");
  const [teachState, setTeachState] = useState<TeachMode>("idle");
  const [heard, setHeard] = useState("");
  const [teachError, setTeachError] = useState("");
  const [packs, setPacks] = useState<PackInfo[]>([]);
  const [openPackId, setOpenPackId] = useState<string | null>(null);
  const [packTerms, setPackTerms] = useState<PackTerm[]>([]);
  const [packQuery, setPackQuery] = useState("");

  const refresh = useCallback(() => {
    invoke<DictionaryWord[]>("list_dictionary").then(setWords).catch(console.error);
    invoke<PackInfo[]>("list_packs").then(setPacks).catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  const setPackEnabled = async (id: string, enabled: boolean) => {
    await invoke("set_pack_enabled", { id, enabled }).catch(console.error);
    refresh();
  };

  const openPack = async (id: string) => {
    if (openPackId === id) {
      setOpenPackId(null);
      setPackTerms([]);
      setPackQuery("");
      return;
    }
    setOpenPackId(id);
    setPackQuery("");
    setPackTerms([]);
    const terms = await invoke<PackTerm[]>("list_pack_terms", { id }).catch(() => []);
    setPackTerms(terms);
  };

  const filteredPackTerms = packQuery.trim()
    ? packTerms.filter(
        (t) =>
          t.canonical.toLowerCase().includes(packQuery.toLowerCase()) ||
          t.mishearings.some((m) => m.toLowerCase().includes(packQuery.toLowerCase())),
      )
    : packTerms;

  const add = async () => {
    const w = word.trim();
    if (!w) return;
    await invoke("add_dictionary_word", {
      word: { id: "", word: w, pronunciation: pronunciation.trim(), createdAt: 0 },
    }).catch(console.error);
    setWord("");
    setPronunciation("");
    refresh();
  };

  const remove = async (id: string) => {
    await invoke("remove_dictionary_word", { id }).catch(console.error);
    refresh();
  };

  const startTeach = async () => {
    const w = teachWord.trim();
    if (!w) return;
    setTeachState("listening");
    setTeachError("");
    setHeard("");
    try {
      const result = await invoke<string>("transcribe_word");
      const heardText = result.trim();
      setHeard(heardText);
      // Check if the heard text matches or is close to the target word.
      const target = w.toLowerCase();
      const heardLower = heardText.toLowerCase();
      const isMatch =
        heardLower === target ||
        heardLower.includes(target) ||
        target.includes(heardLower);
      if (isMatch) {
        await invoke("add_dictionary_word", {
          word: { id: "", word: w, pronunciation: "", createdAt: 0 },
        }).catch(console.error);
        setTeachState("added");
        refresh();
      } else {
        setTeachState("heard");
      }
    } catch (e) {
      setTeachError(String(e));
      setTeachState("idle");
    }
  };

  const acceptHeard = async () => {
    const w = teachWord.trim();
    if (!w) return;
    await invoke("add_dictionary_word", {
      word: { id: "", word: w, pronunciation: heard.trim(), createdAt: 0 },
    }).catch(console.error);
    setTeachState("added");
    refresh();
  };

  const resetTeach = () => {
    setTeachState("idle");
    setHeard("");
    setTeachError("");
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, maxWidth: 760 }}>
      <div>
        <h2 style={{ fontSize: 20, fontWeight: 700 }}>Dictionary</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>
          Teach the app your words by voice or by typing. When you dictate, the app
          corrects misheard words to match your dictionary.
        </p>
      </div>

      {/* Teach Words card */}
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: 20,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
          <div
            style={{
              width: 32,
              height: 32,
              borderRadius: 8,
              background: "var(--accent-soft)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <Icon name="mic" size={16} color="var(--accent)" />
          </div>
          <div>
            <div style={{ fontWeight: 700, fontSize: 15 }}>Teach Words</div>
            <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
              Show the app the right spelling, by voice or by typing.
            </div>
          </div>
        </div>

        {/* Mode toggle */}
        <div
          style={{
            display: "flex",
            gap: 0,
            borderRadius: "var(--radius-sm)",
            overflow: "hidden",
            border: "1px solid var(--border)",
            marginBottom: 16,
          }}
        >
          <button
            onClick={() => { setMode("voice"); resetTeach(); }}
            style={{
              flex: 1,
              padding: "8px 12px",
              fontSize: 13,
              fontWeight: 600,
              border: "none",
              cursor: "pointer",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 6,
              background: mode === "voice" ? "var(--accent)" : "var(--surface)",
              color: mode === "voice" ? "#fff" : "var(--text)",
            }}
          >
            <Icon name="mic" size={14} /> Train by Voice
          </button>
          <button
            onClick={() => { setMode("manual"); resetTeach(); }}
            style={{
              flex: 1,
              padding: "8px 12px",
              fontSize: 13,
              fontWeight: 600,
              border: "none",
              cursor: "pointer",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 6,
              background: mode === "manual" ? "var(--accent)" : "var(--surface)",
              color: mode === "manual" ? "#fff" : "var(--text)",
            }}
          >
            <Icon name="keyboard" size={14} /> Add Manually
          </button>
        </div>

        {mode === "voice" ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <div>
              <label style={{ fontSize: 13, fontWeight: 600, display: "block", marginBottom: 6 }}>
                Type the correct word
              </label>
              <input
                style={{ width: "100%" }}
                placeholder='e.g. "Jaser"'
                value={teachWord}
                onChange={(e) => setTeachWord(e.target.value)}
                disabled={teachState === "listening"}
              />
            </div>

            {teachState === "idle" && (
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <button
                  className="primary"
                  onClick={startTeach}
                  disabled={!teachWord.trim()}
                  style={{ display: "flex", alignItems: "center", gap: 6 }}
                >
                  <Icon name="mic" size={15} /> Start
                </button>
                <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>
                  Say the word naturally, then the app listens.
                </span>
              </div>
            )}

            {teachState === "listening" && (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 12,
                  padding: "12px 16px",
                  background: "var(--accent-soft)",
                  borderRadius: "var(--radius-sm)",
                }}
              >
                <div
                  style={{
                    width: 10,
                    height: 10,
                    borderRadius: "50%",
                    background: "var(--accent)",
                    animation: "pulse 1s ease-in-out infinite",
                  }}
                />
                <span style={{ fontSize: 13, fontWeight: 600 }}>Listening… say the word now</span>
              </div>
            )}

            {teachState === "heard" && (
              <div
                style={{
                  padding: "14px 16px",
                  background: "var(--surface)",
                  border: "1px solid var(--border)",
                  borderRadius: "var(--radius-sm)",
                  display: "flex",
                  flexDirection: "column",
                  gap: 10,
                }}
              >
                <div style={{ fontSize: 13 }}>
                  I heard: <strong style={{ color: "var(--accent)" }}>"{heard}"</strong>
                </div>
                <div style={{ fontSize: 13 }}>
                  You wanted: <strong>"{teachWord.trim()}"</strong>
                </div>
                <div style={{ display: "flex", gap: 8 }}>
                  <button className="primary" onClick={acceptHeard} style={{ fontSize: 13 }}>
                    Add "{teachWord.trim()}" to dictionary
                  </button>
                  <button onClick={resetTeach} style={{ fontSize: 13 }}>
                    Try again
                  </button>
                </div>
              </div>
            )}

            {teachState === "added" && (
              <div
                style={{
                  padding: "12px 16px",
                  background: "var(--success-soft, #dcfce7)",
                  border: "1px solid var(--success, #22c55e)",
                  borderRadius: "var(--radius-sm)",
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                }}
              >
                <Icon name="check" size={16} color="var(--success, #22c55e)" />
                <span style={{ fontSize: 13, fontWeight: 600 }}>
                  "{teachWord.trim()}" added to dictionary
                </span>
                <button onClick={() => { resetTeach(); setTeachWord(""); }} style={{ marginLeft: "auto", fontSize: 12 }}>
                  Teach another
                </button>
              </div>
            )}

            {teachError && (
              <div style={{ fontSize: 13, color: "var(--danger, #ef4444)" }}>{teachError}</div>
            )}
          </div>
        ) : (
          <div style={{ display: "flex", gap: 8 }}>
            <input
              style={{ flex: 2 }}
              placeholder="Word, e.g. Teletype"
              value={word}
              onChange={(e) => setWord(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
            <input
              style={{ flex: 3 }}
              placeholder="Pronunciation hint (optional), e.g. tel-uh-type"
              value={pronunciation}
              onChange={(e) => setPronunciation(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
            <button className="primary" onClick={add} disabled={!word.trim()}>
              <Icon name="plus" size={15} /> Add
            </button>
          </div>
        )}
      </div>

      {/* Vocabulary packs card */}
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: 20,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 6 }}>
          <div
            style={{
              width: 32,
              height: 32,
              borderRadius: 8,
              background: "var(--accent-soft)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <Icon name="library" size={16} color="var(--accent)" />
          </div>
          <div>
            <div style={{ fontWeight: 700, fontSize: 15 }}>Vocabulary packs</div>
            <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
              Domain term packs. Turn one on to auto-correct jargon in that field.
              Off by default.
            </div>
          </div>
        </div>
        {packs.map((p) => {
          const isOpen = openPackId === p.id;
          return (
            <div key={p.id}>
              <div
                onClick={() => openPack(p.id)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 12,
                  padding: "10px 0",
                  borderBottom: "1px solid var(--border)",
                  cursor: "pointer",
                }}
              >
                <Icon
                  name="chevron-right"
                  size={14}
                  color="var(--text-secondary)"
                />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontWeight: 600 }}>{p.name}</div>
                  <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                    {p.description} {p.termCount} terms.
                  </div>
                </div>
                <span
                  onClick={(e) => {
                    e.stopPropagation();
                    setPackEnabled(p.id, !p.enabled);
                  }}
                  title={p.enabled ? "Disable pack" : "Enable pack"}
                  style={{
                    fontSize: 11,
                    fontWeight: 600,
                    padding: "2px 10px",
                    borderRadius: 999,
                    border: `1px solid ${p.enabled ? "var(--success, #22c55e)" : "var(--border)"}`,
                    color: p.enabled ? "var(--success, #22c55e)" : "var(--text-secondary)",
                    background: p.enabled ? "var(--success-soft, #dcfce7)" : "transparent",
                    cursor: "pointer",
                    userSelect: "none",
                  }}
                >
                  {p.enabled ? "On" : "Off"}
                </span>
              </div>

              {isOpen && (
                <div style={{ padding: "8px 0 12px 26px" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                    <input
                      style={{ flex: 1, maxWidth: 260 }}
                      placeholder={`Search ${p.name} terms…`}
                      value={packQuery}
                      onChange={(e) => setPackQuery(e.target.value)}
                    />
                    <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                      {filteredPackTerms.length} of {p.termCount}
                    </span>
                  </div>
                  <div
                    style={{
                      maxHeight: 320,
                      overflowY: "auto",
                      border: "1px solid var(--border)",
                      borderRadius: "var(--radius-sm)",
                    }}
                  >
                    {filteredPackTerms.length === 0 && (
                      <div style={{ padding: 16, fontSize: 13, color: "var(--text-secondary)" }}>
                        No terms match.
                      </div>
                    )}
                    {filteredPackTerms.map((t) => (
                      <div
                        key={t.canonical}
                        style={{
                          padding: "8px 12px",
                          borderBottom: "1px solid var(--border)",
                        }}
                      >
                        <div style={{ fontWeight: 600, fontSize: 13 }}>{t.canonical}</div>
                        {t.mishearings.length > 0 && (
                          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>
                            heard as: {t.mishearings.join(", ")}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* Word list */}
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {words.length === 0 && (
          <p style={{ padding: 20, fontSize: 13, color: "var(--text-secondary)" }}>
            No words yet. Add names, products or jargon the AI tends to mangle.
          </p>
        )}
        {words.map((w) => (
          <div
            key={w.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "10px 16px",
              borderBottom: "1px solid var(--border)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontWeight: 600 }}>{w.word}</div>
              {w.pronunciation && (
                <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>{w.pronunciation}</div>
              )}
            </div>
            <button className="danger" onClick={() => remove(w.id)} title="Delete">
              <Icon name="trash" size={15} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
