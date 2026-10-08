import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Icon } from "../components/Icon";
import { TabRail, TabPane, type TabRailItem } from "../components/TabRail";
import { useTauriEvent } from "../lib/useTauriEvent";
import { Explainer, Segmented, Toggle } from "../settings/primitives";

interface DictionaryWord {
  id: string;
  word: string;
  pronunciation: string;
  createdAt: number;
  fuzzy: boolean;
  learnedFrom?: string | null;
  learnedAt?: number | null;
  aliases: string[];
  usageCount: number;
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

interface ImportCounts {
  imported: number;
  updated: number;
  skipped: number;
}

type TeachMode = "idle" | "listening" | "heard" | "added";
type MatchStrictness = "loose" | "standard" | "strict";

interface Settings {
  matchStrictness: string;
  dictionaryEnabled: boolean;
}

const STRICTNESS_OPTIONS: { value: MatchStrictness; label: string; title: string }[] = [
  { value: "loose", label: "Loose", title: "Corrects more, may change a word you did not say" },
  { value: "standard", label: "Standard", title: "The balance Teletype ships with" },
  { value: "strict", label: "Strict", title: "Only corrects near-misses, leaves anything unusual alone" },
];

function strictnessValue(raw: string | undefined): MatchStrictness {
  return raw === "loose" || raw === "strict" ? raw : "standard";
}

const TABS: TabRailItem[] = [
  { id: "words", label: "Your Words", tagline: "Words you added", icon: "dictionary" },
  { id: "packs", label: "Vocabulary Packs", tagline: "Ready-made lists", icon: "library" },
  { id: "learn", label: "Learn from...", tagline: "Learn as you go", icon: "sparkles" },
  { id: "quick", label: "Quick Add", tagline: "Add from any app", icon: "zap" },
];

const pill: React.CSSProperties = {
  fontSize: 12,
  fontWeight: 600,
  padding: "2px 10px",
  borderRadius: 999,
};

export default function DictionaryScreen() {
  const [tab, setTab] = useState("words");
  const [words, setWords] = useState<DictionaryWord[]>([]);
  const [packs, setPacks] = useState<PackInfo[]>([]);
  const [userPacks, setUserPacks] = useState<PackInfo[]>([]);
  const [openPackId, setOpenPackId] = useState<string | null>(null);
  const [packTerms, setPackTerms] = useState<PackTerm[]>([]);
  const [packQuery, setPackQuery] = useState("");
  const [creatingPack, setCreatingPack] = useState(false);
  const [newPackName, setNewPackName] = useState("");
  const [newPackDesc, setNewPackDesc] = useState("");
  const [newWord, setNewWord] = useState("");
  const [newMishearing, setNewMishearing] = useState("");
  const [strictness, setStrictness] = useState<MatchStrictness>("standard");
  const [dictEnabled, setDictEnabled] = useState(true);
  const [saveError, setSaveError] = useState("");
  const [notice, setNotice] = useState("");

  // Your Words: add / edit form.
  const [word, setWord] = useState("");
  const [pronunciation, setPronunciation] = useState("");
  const [aliases, setAliases] = useState("");
  const [editing, setEditing] = useState<DictionaryWord | null>(null);
  // Voice teaching.
  const [teachWord, setTeachWord] = useState("");
  const [teachState, setTeachState] = useState<TeachMode>("idle");
  const [heard, setHeard] = useState("");
  const [teachError, setTeachError] = useState("");

  const refresh = useCallback(() => {
    invoke<DictionaryWord[]>("list_dictionary").then(setWords).catch(console.error);
    invoke<PackInfo[]>("list_packs").then(setPacks).catch(console.error);
    invoke<PackInfo[]>("list_user_packs").then(setUserPacks).catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  const loadSettings = useCallback(() => {
    invoke<Settings>("get_settings")
      .then((s) => {
        setStrictness(strictnessValue(s.matchStrictness));
        setDictEnabled(s.dictionaryEnabled !== false);
      })
      .catch(console.error);
  }, []);

  useEffect(loadSettings, [loadSettings]);
  useTauriEvent<void>("settings-changed", loadSettings);

  const saveSettings = async (patch: Partial<Settings>) => {
    const settings = await invoke<Settings>("get_settings").catch(() => null);
    if (settings) {
      await invoke("save_settings", { settings: { ...settings, ...patch } }).catch((e) =>
        setSaveError(String(e)),
      );
    }
  };

  const flash = (msg: string) => {
    setNotice(msg);
    window.setTimeout(() => setNotice(""), 4000);
  };

  // ---- Your Words ----

  const resetForm = () => {
    setEditing(null);
    setWord("");
    setPronunciation("");
    setAliases("");
    setSaveError("");
  };

  const startEdit = (w: DictionaryWord) => {
    setEditing(w);
    setWord(w.word);
    setPronunciation(w.pronunciation);
    setAliases(w.aliases.join(", "));
    setSaveError("");
  };

  const submitWord = async () => {
    const w = word.trim();
    if (!w) return;
    const aliasList = aliases.split(",").map((s) => s.trim()).filter(Boolean);
    try {
      if (editing) {
        await invoke("update_dictionary_word", {
          word: { ...editing, word: w, pronunciation: pronunciation.trim(), aliases: aliasList },
        });
      } else {
        await invoke("add_dictionary_word", {
          word: {
            id: "",
            word: w,
            pronunciation: pronunciation.trim(),
            createdAt: 0,
            fuzzy: true,
            aliases: aliasList,
            usageCount: 0,
          },
        });
      }
      resetForm();
      refresh();
    } catch (e) {
      setSaveError(String(e));
    }
  };

  const remove = async (id: string) => {
    await invoke("remove_dictionary_word", { id }).catch(console.error);
    if (editing?.id === id) resetForm();
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
      const target = w.toLowerCase();
      const heardLower = heardText.toLowerCase();
      const isMatch =
        heardLower === target || heardLower.includes(target) || target.includes(heardLower);
      if (isMatch) {
        await invoke("add_dictionary_word", {
          word: { id: "", word: w, pronunciation: "", createdAt: 0, fuzzy: true, aliases: [], usageCount: 0 },
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
      word: { id: "", word: w, pronunciation: heard.trim(), createdAt: 0, fuzzy: true, aliases: [], usageCount: 0 },
    }).catch(console.error);
    setTeachState("added");
    refresh();
  };

  const resetTeach = () => {
    setTeachState("idle");
    setHeard("");
    setTeachError("");
  };

  // ---- Import / export ----

  const doExport = async () => {
    try {
      const path = await save({
        defaultPath: "teletype-dictionary.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      await invoke("export_custom_words", { path });
      flash(`Exported ${words.length} words.`);
    } catch (e) {
      setSaveError(String(e));
    }
  };

  const doImport = async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path || typeof path !== "string") return;
      const counts = await invoke<ImportCounts>("import_custom_words", { path });
      flash(`Imported ${counts.imported}, updated ${counts.updated}, skipped ${counts.skipped}.`);
      refresh();
    } catch (e) {
      setSaveError(String(e));
    }
  };

  // ---- Packs ----

  const isUserPack = (id: string) => userPacks.some((p) => p.id === id);

  const setPackEnabled = async (id: string, enabled: boolean) => {
    if (userPacks.some((p) => p.id === id)) {
      await invoke("set_user_pack_enabled", { id, enabled }).catch(console.error);
    } else {
      await invoke("set_pack_enabled", { id, enabled }).catch(console.error);
    }
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

  const createPack = async () => {
    if (!newPackName.trim()) return;
    await invoke("create_user_pack", {
      name: newPackName.trim(),
      description: newPackDesc.trim(),
    }).catch(console.error);
    setCreatingPack(false);
    setNewPackName("");
    setNewPackDesc("");
    refresh();
  };

  const deletePack = async (id: string) => {
    if (!confirm("Delete this pack and all its words?")) return;
    await invoke("delete_user_pack", { id }).catch(console.error);
    if (openPackId === id) {
      setOpenPackId(null);
      setPackTerms([]);
    }
    refresh();
  };

  const addPackWord = async (packId: string) => {
    const w = newWord.trim();
    if (!w) return;
    const mis = newMishearing.split(",").map((s) => s.trim()).filter(Boolean);
    await invoke("add_user_pack_word", { id: packId, word: w, mishearings: mis }).catch(console.error);
    setNewWord("");
    setNewMishearing("");
    refresh();
    if (openPackId === packId) {
      setOpenPackId(null);
      setPackTerms([]);
    }
  };

  const removePackWord = async (packId: string, word: string) => {
    await invoke("remove_user_pack_word", { id: packId, word }).catch(console.error);
    refresh();
    if (openPackId === packId) {
      setOpenPackId(null);
      setPackTerms([]);
    }
  };

  const filteredPackTerms = useMemo(() => {
    const q = packQuery.trim().toLowerCase();
    if (!q) return packTerms;
    return packTerms.filter(
      (t) =>
        t.canonical.toLowerCase().includes(q) ||
        t.mishearings.some((m) => m.toLowerCase().includes(q)),
    );
  }, [packTerms, packQuery]);

  const allPacks = useMemo(() => [...packs, ...userPacks], [packs, userPacks]);
  const learnedCount = words.filter((w) => w.learnedFrom).length;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, width: "100%" }}>
      {/* Fixed banner */}
      <div>
        <h2 style={{ fontSize: 20, fontWeight: 700, margin: 0 }}>Dictionary</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13, marginTop: 4, marginBottom: 10 }}>
          Teach the app your words. When you dictate, Teletype corrects misheard words to match
          your list.
        </p>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            background: "var(--surface)",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius)",
            padding: "12px 16px",
          }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: "var(--text-sm)", fontWeight: 600 }}>
              Dictionary corrections
              <Explainer text="When on, words in this dictionary replace what the speech engine wrote. Turn it off to dictate without any dictionary corrections." />
            </div>
            <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 2 }}>
              {words.length} words · {learnedCount} auto-learned
            </div>
          </div>
          <Toggle
            label="Enable dictionary"
            checked={dictEnabled}
            onChange={(v) => {
              setDictEnabled(v);
              saveSettings({ dictionaryEnabled: v });
            }}
          />
        </div>
      </div>

      {notice && (
        <div
          role="status"
          style={{
            padding: "10px 14px",
            fontSize: 13,
            color: "var(--accent)",
            background: "var(--accent-soft)",
            border: "1px solid var(--accent)",
            borderRadius: "var(--radius-sm)",
          }}
        >
          {notice}
        </div>
      )}
      {saveError && (
        <div
          role="alert"
          style={{
            padding: "10px 14px",
            fontSize: 13,
            color: "var(--danger)",
            background: "color-mix(in srgb, var(--danger) 8%, transparent)",
            border: "1px solid var(--danger)",
            borderRadius: "var(--radius-sm)",
          }}
        >
          {saveError}
        </div>
      )}

      {/* Rail + pane */}
      <div style={{ display: "flex", gap: 16, alignItems: "stretch", height: 560 }}>
        <TabRail items={TABS} selected={tab} onSelect={setTab} />
        <TabPane>
          {tab === "words" && (
            <YourWordsTab
              words={words}
              form={word}
              onForm={setWord}
              pronunciation={pronunciation}
              onPronunciation={setPronunciation}
              aliases={aliases}
              onAliases={setAliases}
              editing={editing}
              onSubmit={submitWord}
              onCancelEdit={resetForm}
              onEdit={startEdit}
              onAddClick={resetForm}
              onRemove={remove}
              onExport={doExport}
              onImport={doImport}
              teachWord={teachWord}
              onTeachWord={setTeachWord}
              teachState={teachState}
              heard={heard}
              teachError={teachError}
              onStartTeach={startTeach}
              onAcceptHeard={acceptHeard}
              onResetTeach={resetTeach}
              strictness={strictness}
              onStrictness={(v) => {
                setStrictness(v);
                saveSettings({ matchStrictness: v });
              }}
            />
          )}
          {tab === "packs" && (
            <PacksTab
              packs={allPacks}
              isUserPack={isUserPack}
              openPackId={openPackId}
              onOpenPack={openPack}
              packTerms={filteredPackTerms}
              packTotal={openPackId ? allPacks.find((p) => p.id === openPackId)?.termCount ?? 0 : 0}
              packQuery={packQuery}
              onPackQuery={setPackQuery}
              onToggle={setPackEnabled}
              onDelete={deletePack}
              creating={creatingPack}
              onCreating={setCreatingPack}
              newPackName={newPackName}
              onNewPackName={setNewPackName}
              newPackDesc={newPackDesc}
              onNewPackDesc={setNewPackDesc}
              onCreate={createPack}
              newWord={newWord}
              onNewWord={setNewWord}
              newMishearing={newMishearing}
              onNewMishearing={setNewMishearing}
              onAddWord={addPackWord}
              onRemoveWord={removePackWord}
              openPackIsUser={openPackId ? isUserPack(openPackId) : false}
            />
          )}
          {tab === "learn" && <LearnTab learnedCount={learnedCount} total={words.length} />}
          {tab === "quick" && <QuickAddTab />}
        </TabPane>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------

function YourWordsTab(props: {
  words: DictionaryWord[];
  form: string;
  onForm: (v: string) => void;
  pronunciation: string;
  onPronunciation: (v: string) => void;
  aliases: string;
  onAliases: (v: string) => void;
  editing: DictionaryWord | null;
  onSubmit: () => void;
  onCancelEdit: () => void;
  onEdit: (w: DictionaryWord) => void;
  onAddClick: () => void;
  onRemove: (id: string) => void;
  onExport: () => void;
  onImport: () => void;
  teachWord: string;
  onTeachWord: (v: string) => void;
  teachState: TeachMode;
  heard: string;
  teachError: string;
  onStartTeach: () => void;
  onAcceptHeard: () => void;
  onResetTeach: () => void;
  strictness: MatchStrictness;
  onStrictness: (v: MatchStrictness) => void;
}) {
  const p = props;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {/* Add / edit bar */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          flexWrap: "wrap",
          paddingBottom: 12,
          borderBottom: "1px solid var(--border-subtle)",
        }}
      >
        <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>
          {p.words.length} words
        </span>
        <span style={{ flex: 1 }} />
        <button className="primary" onClick={p.onAddClick} style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <Icon name="plus" size={14} /> {p.editing ? "Done editing" : "Add word"}
        </button>
        <button onClick={p.onImport} style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <Icon name="download" size={14} /> Import
        </button>
        <button onClick={p.onExport} style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <Icon name="upload" size={14} /> Export
        </button>
      </div>

      {/* Add / edit form */}
      <div
        style={{
          display: "flex",
          gap: 8,
          flexWrap: "wrap",
          padding: 12,
          border: `1px solid ${p.editing ? "var(--accent)" : "var(--border-subtle)"}`,
          borderRadius: "var(--radius-sm)",
          background: "var(--surface-2)",
        }}
      >
        <input
          placeholder={p.editing ? "Edit word" : "Word, e.g. Teletype"}
          value={p.form}
          onChange={(e) => p.onForm(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && p.onSubmit()}
          style={{ flex: "2 1 140px", minWidth: 120 }}
        />
        <input
          placeholder="Pronunciation hint (optional)"
          value={p.pronunciation}
          onChange={(e) => p.onPronunciation(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && p.onSubmit()}
          style={{ flex: "2 1 140px", minWidth: 120 }}
        />
        <input
          placeholder="Sound-alikes, comma-separated (optional)"
          value={p.aliases}
          onChange={(e) => p.onAliases(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && p.onSubmit()}
          style={{ flex: "3 1 180px", minWidth: 140 }}
        />
        <button className="primary" onClick={p.onSubmit} disabled={!p.form.trim()}>
          {p.editing ? "Save" : "Add"}
        </button>
        {p.editing && (
          <button onClick={p.onCancelEdit}>Cancel</button>
        )}
      </div>

      {/* Teach by voice */}
      <div
        style={{
          padding: 12,
          border: "1px solid var(--border-subtle)",
          borderRadius: "var(--radius-sm)",
          display: "flex",
          flexDirection: "column",
          gap: 10,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Icon name="mic" size={15} color="var(--accent)" />
          <span style={{ fontSize: 13, fontWeight: 600 }}>Train by voice</span>
          <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
            Type the word, then say it naturally.
          </span>
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <input
            placeholder='e.g. "Jaser"'
            value={p.teachWord}
            onChange={(e) => p.onTeachWord(e.target.value)}
            disabled={p.teachState === "listening"}
            style={{ flex: 1, minWidth: 120 }}
          />
          {p.teachState === "idle" && (
            <button className="primary" onClick={p.onStartTeach} disabled={!p.teachWord.trim()}>
              <Icon name="mic" size={13} /> Listen
            </button>
          )}
          {p.teachState === "listening" && (
            <span style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--accent)", fontWeight: 600, padding: "0 8px" }}>
              <span style={{ width: 8, height: 8, borderRadius: "50%", background: "var(--accent)", animation: "pulse 1s ease-in-out infinite" }} />
              Listening…
            </span>
          )}
          {p.teachState === "heard" && (
            <>
              <span style={{ fontSize: 13, alignSelf: "center" }}>
                Heard <strong style={{ color: "var(--accent)" }}>"{p.heard}"</strong> — close enough?
              </span>
              <button className="primary" onClick={p.onAcceptHeard}>
                Add "{p.teachWord.trim()}"
              </button>
              <button onClick={p.onResetTeach}>Try again</button>
            </>
          )}
          {p.teachState === "added" && (
            <button onClick={() => { p.onResetTeach(); p.onTeachWord(""); }}>
              <Icon name="check" size={13} /> Added — teach another
            </button>
          )}
        </div>
        {p.teachError && <div style={{ fontSize: 12, color: "var(--danger)" }}>{p.teachError}</div>}
      </div>

      {/* Match strictness */}
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <div style={{ flex: 1, minWidth: 200 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 13, fontWeight: 600 }}>
            Match strictness
            <Explainer text="How aggressively dictionary words replace what the speech engine wrote. Loose corrects more, and will occasionally change a word you did not say. Strict only corrects near-misses." />
          </div>
          <div style={{ fontSize: 11.5, color: "var(--text-tertiary)", marginTop: 2 }}>
            Applies from the next recording onward.
          </div>
        </div>
        <Segmented
          label="Match strictness"
          value={p.strictness}
          options={STRICTNESS_OPTIONS}
          onChange={p.onStrictness}
        />
      </div>

      {/* Word list */}
      {p.words.length === 0 && (
        <p style={{ padding: 16, fontSize: 13, color: "var(--text-secondary)", margin: 0 }}>
          No words yet. Add names, products or jargon the AI tends to mangle.
        </p>
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {p.words.map((w) => (
          <div
            key={w.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "9px 12px",
              borderRadius: "var(--radius-sm)",
              background: p.editing?.id === w.id ? "var(--accent-soft)" : "transparent",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                <span style={{ fontWeight: 600 }}>{w.word}</span>
                {w.learnedFrom && (
                  <span
                    title={`Learned automatically (it kept hearing \u201c${w.learnedFrom}\u201d)`}
                    style={{ ...pill, color: "var(--accent)", background: "var(--accent-soft)", fontSize: 10.5, display: "inline-flex", alignItems: "center", gap: 3 }}
                  >
                    <Icon name="sparkles" size={10} color="var(--accent)" /> Learned
                  </span>
                )}
                {!w.fuzzy && (
                  <span style={{ ...pill, color: "var(--text-secondary)", border: "1px solid var(--border)", fontSize: 10.5 }}>
                    exact only
                  </span>
                )}
                {w.usageCount > 0 && (
                  <span style={{ ...pill, color: "var(--text-tertiary)", border: "1px solid var(--border-subtle)", fontSize: 10.5 }}>
                    used {w.usageCount}×
                  </span>
                )}
              </div>
              {(w.pronunciation || w.aliases.length > 0) && (
                <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>
                  {[w.pronunciation, ...w.aliases].filter(Boolean).join(" · ")}
                </div>
              )}
            </div>
            <button onClick={() => p.onEdit(w)} title="Edit" aria-label={`Edit ${w.word}`} style={{ padding: 4 }}>
              <Icon name="more" size={14} />
            </button>
            <button
              className="danger"
              onClick={() => p.onRemove(w.id)}
              title="Delete"
              aria-label={`Delete ${w.word}`}
              style={{ padding: 4 }}
            >
              <Icon name="trash" size={14} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------

function PacksTab(props: {
  packs: PackInfo[];
  isUserPack: (id: string) => boolean;
  openPackId: string | null;
  onOpenPack: (id: string) => void;
  packTerms: PackTerm[];
  packTotal: number;
  packQuery: string;
  onPackQuery: (v: string) => void;
  onToggle: (id: string, enabled: boolean) => void;
  onDelete: (id: string) => void;
  creating: boolean;
  onCreating: (v: boolean) => void;
  newPackName: string;
  onNewPackName: (v: string) => void;
  newPackDesc: string;
  onNewPackDesc: (v: string) => void;
  onCreate: () => void;
  newWord: string;
  onNewWord: (v: string) => void;
  newMishearing: string;
  onNewMishearing: (v: string) => void;
  onAddWord: (packId: string) => void;
  onRemoveWord: (packId: string, word: string) => void;
  openPackIsUser: boolean;
}) {
  const p = props;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ fontSize: 13, color: "var(--text-secondary)", flex: 1 }}>
          Domain term packs, plus your own. Turn one on to auto-correct that jargon. Off by default.
        </span>
        <button className="primary" onClick={() => p.onCreating(!p.creating)} style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <Icon name="plus" size={14} /> New pack
        </button>
      </div>

      {p.creating && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "10px 0", borderBottom: "1px solid var(--border-subtle)" }}>
          <div style={{ display: "flex", gap: 8 }}>
            <input autoFocus placeholder="Pack name (e.g. My company)" value={p.newPackName} onChange={(e) => p.onNewPackName(e.target.value)} style={{ flex: 1 }} />
            <input placeholder="Description (optional)" value={p.newPackDesc} onChange={(e) => p.onNewPackDesc(e.target.value)} style={{ flex: 1.2 }} />
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="primary" onClick={p.onCreate} disabled={!p.newPackName.trim()}>Create</button>
            <button onClick={() => p.onCreating(false)}>Cancel</button>
          </div>
        </div>
      )}

      {p.packs.map((pk) => {
        const isOpen = p.openPackId === pk.id;
        const isUser = p.isUserPack(pk.id);
        return (
          <div key={pk.id}>
            <div
              onClick={() => p.onOpenPack(pk.id)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "10px 8px",
                borderBottom: "1px solid var(--border-subtle)",
                cursor: "pointer",
              }}
            >
              <Icon name="chevron-right" size={14} color="var(--text-secondary)" />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 600 }}>{pk.name}</div>
                <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                  {pk.description} {pk.termCount} terms.
                </div>
              </div>
              <span
                onClick={(e) => { e.stopPropagation(); p.onToggle(pk.id, !pk.enabled); }}
                title={pk.enabled ? "Disable pack" : "Enable pack"}
                style={{
                  ...pill,
                  border: `1px solid ${pk.enabled ? "var(--success, #22c55e)" : "var(--border)"}`,
                  color: pk.enabled ? "var(--success, #22c55e)" : "var(--text-secondary)",
                  background: pk.enabled ? "var(--success-soft, #dcfce7)" : "transparent",
                  cursor: "pointer",
                  userSelect: "none",
                }}
              >
                {pk.enabled ? "On" : "Off"}
              </span>
              {isUser && (
                <button
                  onClick={(e) => { e.stopPropagation(); p.onDelete(pk.id); }}
                  title="Delete pack"
                  aria-label="Delete pack"
                  style={{ background: "none", border: "none", cursor: "pointer", color: "var(--text-tertiary, var(--text-secondary))", padding: 2 }}
                >
                  <Icon name="trash" size={14} />
                </button>
              )}
            </div>

            {isOpen && (
              <div style={{ padding: "10px 0 12px 30px" }}>
                {isUser && (
                  <div style={{ display: "flex", gap: 6, marginBottom: 10, flexWrap: "wrap" }}>
                    <input placeholder="Add a word…" value={p.newWord} onChange={(e) => p.onNewWord(e.target.value)} onKeyDown={(e) => e.key === "Enter" && p.onAddWord(pk.id)} style={{ flex: "1 1 140px", minWidth: 120 }} />
                    <input placeholder="mis-hearings, comma-separated (optional)" value={p.newMishearing} onChange={(e) => p.onNewMishearing(e.target.value)} style={{ flex: "1 1 200px", minWidth: 160 }} />
                    <button className="primary" onClick={() => p.onAddWord(pk.id)} disabled={!p.newWord.trim()}>Add</button>
                  </div>
                )}
                <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                  <input style={{ flex: 1, maxWidth: 260 }} placeholder={`Search ${pk.name} terms…`} value={p.packQuery} onChange={(e) => p.onPackQuery(e.target.value)} />
                  <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                    {p.packTerms.length} of {p.packTotal}
                  </span>
                </div>
                <div style={{ maxHeight: 300, overflowY: "auto", border: "1px solid var(--border-subtle)", borderRadius: "var(--radius-sm)" }}>
                  {p.packTerms.length === 0 && (
                    <div style={{ padding: 16, fontSize: 13, color: "var(--text-secondary)" }}>No terms match.</div>
                  )}
                  {p.packTerms.map((t) => (
                    <div key={t.canonical} style={{ padding: "8px 12px", borderBottom: "1px solid var(--border-subtle)", display: "flex", alignItems: "center", gap: 8 }}>
                      <div style={{ fontWeight: 600, fontSize: 13, flex: 1, minWidth: 0 }}>{t.canonical}</div>
                      {isUser && (
                        <button
                          onClick={() => p.onRemoveWord(pk.id, t.canonical)}
                          title={`Remove ${t.canonical}`}
                          aria-label={`Remove ${t.canonical}`}
                          style={{ flex: "0 0 auto", width: 20, height: 20, borderRadius: 999, border: "1px solid var(--border)", background: "var(--surface-2)", color: "var(--text-secondary)", cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 12, lineHeight: 1 }}
                        >
                          ×
                        </button>
                      )}
                      {t.mishearings.length > 0 && (
                        <div style={{ display: "flex", flexWrap: "wrap", gap: 4, alignItems: "center" }}>
                          <span style={{ fontSize: 11.5, color: "var(--text-secondary)", marginRight: 2 }}>heard as:</span>
                          {t.mishearings.map((m) => (
                            <span key={m} style={{ fontSize: 11.5, color: "var(--text-secondary)", background: "var(--surface-2)", border: "1px solid var(--border)", borderRadius: 999, padding: "1px 8px" }}>
                              {m}
                            </span>
                          ))}
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
  );
}

// ---------------------------------------------------------------------------

function LearnTab({ learnedCount, total }: { learnedCount: number; total: number }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div style={{ width: 32, height: 32, borderRadius: 8, background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center" }}>
          <Icon name="sparkles" size={16} color="var(--accent)" />
        </div>
        <div>
          <div style={{ fontWeight: 700, fontSize: 15 }}>Learn as you go</div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
            Words Teletype picked up from your own edits, automatically.
          </div>
        </div>
      </div>
      <div style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.6 }}>
        When the app corrects a word and you keep that correction, Teletype quietly files the
        word into your dictionary so it gets it right next time. You have auto-learned{" "}
        <strong style={{ color: "var(--text)" }}>{learnedCount}</strong> of your{" "}
        <strong style={{ color: "var(--text)" }}>{total}</strong> words this way.
      </div>
      <div
        style={{
          padding: "12px 14px",
          background: "var(--accent-soft)",
          border: "1px solid var(--accent)",
          borderRadius: "var(--radius-sm)",
          fontSize: 13,
          color: "var(--text-secondary)",
        }}
      >
        Learned words are marked with a <Icon name="sparkles" size={12} color="var(--accent)" />{" "}
        badge on the <strong>Your Words</strong> tab. You can edit or delete them like any other
        word.
      </div>
    </div>
  );
}

function QuickAddTab() {
  const [hotkey, setHotkey] = useState("");
  useEffect(() => {
    invoke<Settings & { quickAddHotkey?: string }>("get_settings")
      .then((s) => setHotkey(s.quickAddHotkey ?? ""))
      .catch(() => {});
  }, []);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div style={{ width: 32, height: 32, borderRadius: 8, background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center" }}>
          <Icon name="zap" size={16} color="var(--accent)" />
        </div>
        <div>
          <div style={{ fontWeight: 700, fontSize: 15 }}>Quick Add</div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
            Add a selected word from any app, in one shortcut.
          </div>
        </div>
      </div>
      <div style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.6 }}>
        Highlight a misheard word anywhere — a name, a product, a term — and press the Quick Add
        shortcut to file it into your dictionary instantly.
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span style={{ fontSize: 13, fontWeight: 600 }}>Shortcut</span>
        <code
          style={{
            fontSize: 13,
            padding: "4px 10px",
            borderRadius: 6,
            background: "var(--surface-2)",
            border: "1px solid var(--border)",
          }}
        >
          {hotkey || "not set"}
        </code>
      </div>
      <div style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
        Configure the shortcut on the <strong>Keybinds</strong> screen. Terminal windows do not
        share their selection, so Quick Add will not work there.
      </div>
    </div>
  );
}
