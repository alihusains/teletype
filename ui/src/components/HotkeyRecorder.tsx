import { useCallback, useEffect, useRef, useState } from "react";

interface Props {
  value: string;
  onSave: (hotkey: string) => void;
}

const MODS = ["Shift", "Control", "Alt", "Meta"] as const;

/** Map a KeyboardEvent to a Tauri global-shortcut string, e.g. "Cmd+Shift+Space". */
function eventToHotkey(e: KeyboardEvent): string | null {
  // Ignore bare modifier presses — the user is still building the combo.
  if (MODS.includes(e.key as (typeof MODS)[number])) return null;

  const parts: string[] = [];
  if (e.metaKey) parts.push("Cmd");
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");

  // Normalize the key name to Tauri's accepted vocabulary.
  let key = e.key;
  if (key === " ") key = "Space";
  else if (key === "ArrowUp") key = "Up";
  else if (key === "ArrowDown") key = "Down";
  else if (key === "ArrowLeft") key = "Left";
  else if (key === "ArrowRight") key = "Right";
  else if (key.length === 1) key = key.toUpperCase();
  // Tauri uses "Super" is not a thing on macOS; Meta = Cmd (already handled).

  if (parts.length === 0) return null; // require at least one modifier
  parts.push(key);
  return parts.join("+");
}

/** Pretty-print for display: "Cmd+Shift+Space" → "⌘⇧ Space". */
function displayHotkey(hotkey: string): string {
  if (!hotkey) return "—";
  const symbols: Record<string, string> = {
    Cmd: "⌘",
    Ctrl: "⌃",
    Alt: "⌥",
    Shift: "⇧",
    Space: "Space",
  };
  return hotkey
    .split("+")
    .map((p) => symbols[p] ?? p)
    .join(" ");
}

export default function HotkeyRecorder({ value, onSave }: Props) {
  const [listening, setListening] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const listenerRef = useRef(false);

  const startListening = useCallback(() => {
    setPending(null);
    setError(null);
    setListening(true);
  }, []);

  const stopListening = useCallback(() => {
    setListening(false);
    setPending(null);
  }, []);

  const confirm = useCallback(async () => {
    if (!pending) return;
    try {
      onSave(pending);
      stopListening();
    } catch (e) {
      setError(String(e));
    }
  }, [pending, onSave, stopListening]);

  useEffect(() => {
    if (!listening) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape") {
        stopListening();
        return;
      }

      const hotkey = eventToHotkey(e);
      if (hotkey) {
        setPending(hotkey);
        setListening(false);
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    listenerRef.current = true;
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      listenerRef.current = false;
    };
  }, [listening, stopListening]);

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
      <kbd
        style={{
          fontFamily: "monospace",
          fontSize: 15,
          padding: "5px 12px",
          background: "var(--surface)",
          border: `1px solid ${listening ? "var(--accent)" : "var(--border)"}`,
          borderRadius: 6,
          minWidth: 120,
          textAlign: "center",
          color: listening ? "var(--accent)" : "var(--text)",
        }}
      >
        {listening ? (
          <span style={{ color: "var(--accent)" }}>Press keys…</span>
        ) : pending ? (
          displayHotkey(pending)
        ) : (
          displayHotkey(value)
        )}
      </kbd>

      {!listening && !pending && (
        <button onClick={startListening}>Change</button>
      )}

      {listening && (
        <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>
          Press a key combination (Esc to cancel)
        </span>
      )}

      {pending && !listening && (
        <>
          <button className="primary" onClick={confirm}>
            Confirm
          </button>
          <button onClick={stopListening}>Cancel</button>
        </>
      )}

      {error && (
        <span style={{ fontSize: 12, color: "var(--danger)" }}>{error}</span>
      )}
    </div>
  );
}
