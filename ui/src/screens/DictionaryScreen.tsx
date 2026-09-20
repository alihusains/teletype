import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

interface DictionaryWord {
  id: string;
  word: string;
  pronunciation: string;
  createdAt: number;
}

export default function DictionaryScreen() {
  const [words, setWords] = useState<DictionaryWord[]>([]);
  const [word, setWord] = useState("");
  const [pronunciation, setPronunciation] = useState("");

  const refresh = useCallback(() => {
    invoke<DictionaryWord[]>("list_dictionary").then(setWords).catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

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

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, maxWidth: 760 }}>
      <div>
        <h2 style={{ fontSize: 20, fontWeight: 700 }}>Dictionary</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>
          Custom words and pronunciation hints. Known words are protected from being
          "corrected" by transforms.
        </p>
      </div>

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
