import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { Toggle } from "../settings/primitives";

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
    <div style={{ width: "100%", maxWidth: 860 }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 16, marginBottom: 24 }}>
        <div style={{ flex: 1 }}>
          <h2
            style={{
              fontSize: 22,
              fontWeight: 650,
              letterSpacing: "var(--tracking-tight)",
              margin: 0,
            }}
          >
            Scratchpad
          </h2>
          <p
            style={{
              color: "var(--text-secondary)",
              fontSize: "var(--text-sm)",
              marginTop: 4,
              marginBottom: 0,
              maxWidth: "56ch",
            }}
          >
            A private dictation target. When enabled and this window is focused, dictation
            lands here instead of the app you were typing in.
          </p>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8, paddingTop: 4 }}>
          <button
            onClick={copyAll}
            disabled={entries.length === 0}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              fontSize: "var(--text-sm)",
              opacity: entries.length === 0 ? 0.45 : 1,
            }}
          >
            <Icon name={copied ? "check" : "copy"} size={15} /> {copied ? "Copied" : "Copy all"}
          </button>
          <button
            className="danger"
            onClick={clear}
            disabled={entries.length === 0}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              fontSize: "var(--text-sm)",
              color: "var(--danger)",
              border: "1px solid var(--danger)",
              opacity: entries.length === 0 ? 0.45 : 1,
            }}
          >
            <Icon name="trash" size={15} /> Clear
          </button>
        </div>
      </div>

      <div className="tt-card" style={{ overflow: "hidden", marginBottom: 20 }}>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "10px 14px",
            fontSize: "var(--text-sm)",
          }}
        >
          <Toggle
            label="Route dictation here while the Teletype window is focused"
            checked={enabled}
            onChange={toggle}
          />
          Route dictation here while the Teletype window is focused
        </label>
      </div>

      <div
        style={{
          background: "var(--surface)",
          borderRadius: "var(--radius)",
          boxShadow: "var(--shadow-1)",
          overflow: "hidden",
        }}
      >
        {entries.length === 0 && (
          <p
            style={{
              padding: 20,
              fontSize: "var(--text-sm)",
              color: "var(--text-tertiary)",
              margin: 0,
              maxWidth: "56ch",
            }}
          >
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
              borderTop: "1px solid var(--border-subtle)",
            }}
          >
            <div
              style={{
                flex: 1,
                minWidth: 0,
                whiteSpace: "pre-wrap",
                overflowWrap: "anywhere",
                lineHeight: 1.6,
                fontSize: "var(--text-md)",
              }}
            >
              {e.text}
            </div>
            <span
              style={{
                fontSize: "var(--text-xs)",
                color: "var(--text-tertiary)",
                flexShrink: 0,
                paddingTop: 2,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {new Date(e.createdAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
            </span>
            <button
              className="danger"
              onClick={() => remove(e.id)}
              title="Delete"
              aria-label="Delete entry"
              style={{ color: "var(--danger)", border: "1px solid var(--border)" }}
            >
              <Icon name="trash" size={14} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
