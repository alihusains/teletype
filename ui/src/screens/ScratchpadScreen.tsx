import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

interface ScratchEntry {
  id: string;
  createdAt: number;
  text: string;
}

export default function ScratchpadScreen() {
  const [entries, setEntries] = useState<ScratchEntry[]>([]);
  const [enabled, setEnabled] = useState(false);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(() => {
    invoke<ScratchEntry[]>("list_scratchpad").then(setEntries).catch(console.error);
    invoke<{ scratchpadEnabled: boolean }>("get_settings")
      .then((s) => setEnabled(s.scratchpadEnabled))
      .catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  const toggle = async (value: boolean) => {
    setEnabled(value);
    const settings = await invoke<Record<string, unknown>>("get_settings").catch(() => null);
    if (settings) {
      await invoke("save_settings", { settings: { ...settings, scratchpadEnabled: value } }).catch(console.error);
    }
  };

  const copyAll = async () => {
    const text = await invoke<string>("get_scratchpad_text").catch(() => "");
    if (!text) return;
    await navigator.clipboard.writeText(text).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };

  const clear = async () => {
    if (!confirm("Clear the whole scratchpad?")) return;
    await invoke("clear_scratchpad").catch(console.error);
    refresh();
  };

  const remove = async (id: string) => {
    await invoke("delete_scratchpad_entry", { id }).catch(console.error);
    refresh();
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, maxWidth: 760 }}>
      <div>
        <h2 style={{ fontSize: 20, fontWeight: 700 }}>Scratchpad</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>
          A private dictation target. When enabled and this window is focused, dictation
          lands here instead of the app you were typing in.
        </p>
      </div>

      <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
        <input type="checkbox" checked={enabled} onChange={(e) => toggle(e.target.checked)} />
        <span style={{ fontSize: 13 }}>
          Route dictation here while the Teletype window is focused
        </span>
      </label>

      <div style={{ display: "flex", gap: 8 }}>
        <button onClick={copyAll} disabled={entries.length === 0}>
          <Icon name={copied ? "check" : "copy"} size={15} /> {copied ? "Copied" : "Copy all"}
        </button>
        <button className="danger" onClick={clear} disabled={entries.length === 0}>
          <Icon name="trash" size={15} /> Clear
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
        {entries.length === 0 && (
          <p style={{ padding: 20, fontSize: 13, color: "var(--text-secondary)" }}>
            Empty. Focus this window, hold {`Fn`}, and talk.
          </p>
        )}
        {entries.map((e) => (
          <div
            key={e.id}
            style={{
              display: "flex",
              gap: 12,
              padding: "10px 16px",
              borderBottom: "1px solid var(--border)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
              {e.text}
            </div>
            <span
              style={{
                fontSize: 11,
                color: "var(--text-secondary)",
                flexShrink: 0,
                paddingTop: 2,
              }}
            >
              {new Date(e.createdAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
            </span>
            <button className="danger" onClick={() => remove(e.id)} title="Delete" aria-label="Delete entry">
              <Icon name="trash" size={14} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
