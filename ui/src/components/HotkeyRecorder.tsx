import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  value: string;
  onSave: (hotkey: string) => void;
}

const MODS: Record<string, string> = {
  Cmd: "⌘",
  Ctrl: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Fn: "🌐",
  Space: "Space",
};

function displayHotkey(hotkey: string): string {
  if (!hotkey) return "—";
  return hotkey
    .split("+")
    .map((p) => MODS[p] ?? p)
    .join(" ");
}

export default function HotkeyRecorder({ value, onSave }: Props) {
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  const startCapture = useCallback(async () => {
    setError(null);
    try {
      await invoke("start_hotkey_capture");
      setCapturing(true);

      // Poll for the result — the native panel writes to a temp file
      // when the user presses Enter (confirm) or Esc (cancel).
      pollRef.current = setInterval(async () => {
        try {
          const result = await invoke<string | null>("get_captured_hotkey");
          if (result !== null) {
            stopPolling();
            setCapturing(false);
            await invoke("stop_hotkey_capture").catch(() => {});
            if (result) {
              onSave(result);
            }
          }
        } catch {
          // ignore polling errors
        }
      }, 300);
    } catch (e) {
      setError(String(e));
      setCapturing(false);
    }
  }, [onSave, stopPolling]);

  const cancelCapture = useCallback(async () => {
    stopPolling();
    setCapturing(false);
    await invoke("stop_hotkey_capture").catch(() => {});
  }, [stopPolling]);

  // Clean up on unmount.
  useEffect(() => {
    return () => {
      stopPolling();
      invoke("stop_hotkey_capture").catch(() => {});
    };
  }, [stopPolling]);

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
      <kbd
        style={{
          fontFamily: "monospace",
          fontSize: 15,
          padding: "5px 12px",
          background: "var(--surface)",
          border: `1px solid ${capturing ? "var(--accent)" : "var(--border)"}`,
          borderRadius: 6,
          minWidth: 120,
          textAlign: "center",
          color: capturing ? "var(--accent)" : "var(--text)",
        }}
      >
        {capturing ? "Press keys…" : displayHotkey(value)}
      </kbd>

      {!capturing ? (
        <button onClick={startCapture}>Change</button>
      ) : (
        <button onClick={cancelCapture}>Cancel</button>
      )}

      {error && (
        <span style={{ fontSize: 12, color: "var(--danger)" }}>{error}</span>
      )}
    </div>
  );
}
