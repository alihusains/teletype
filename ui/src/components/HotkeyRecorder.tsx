import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/errors";

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

/** Compact "⌥ 1" style rendering of a Tauri hotkey string. */
export function displayHotkey(hotkey: string): string {
  if (!hotkey) return "—";
  return hotkey
    .split("+")
    .map((p) => MODS[p] ?? p)
    .join(" ");
}

const keycapStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  minWidth: 22,
  height: 22,
  padding: "0 6px",
  borderRadius: 5,
  border: "1px solid var(--border-subtle)",
  background: "var(--surface-2)",
  color: "var(--text)",
  fontFamily: "inherit",
  fontSize: 11.5,
  fontWeight: 600,
  lineHeight: 1,
  boxShadow: "0 1px 0 var(--border-subtle)",
};

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
      const pollStart = Date.now();
      const POLL_TIMEOUT_MS = 60_000;
      pollRef.current = setInterval(async () => {
        // Safety timeout: if the native panel is stuck or the user
        // somehow can't dismiss it, stop after 60 s.
        if (Date.now() - pollStart > POLL_TIMEOUT_MS) {
          stopPolling();
          setCapturing(false);
          setError("Timed out waiting for key input.");
          await invoke("stop_hotkey_capture").catch(() => {});
          return;
        }
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
      setError(errorMessage(e));
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

  // Tap the keycap to toggle capture. A div (not a <button>) so it can
  // never swallow key events while the native capture panel is open.
  const toggleCapture = () => {
    if (capturing) {
      void cancelCapture();
    } else {
      void startCapture();
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 4 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <div
          role="button"
          tabIndex={0}
          aria-label={capturing ? "Recording, press a key combination" : `Hotkey ${displayHotkey(value)}, tap to change`}
          onPointerDown={toggleCapture}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              toggleCapture();
            }
          }}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 4,
            minHeight: 30,
            padding: "4px 8px",
            borderRadius: "var(--radius-sm)",
            cursor: "pointer",
            userSelect: "none",
            background: capturing ? "var(--accent-soft)" : "var(--surface)",
            border: capturing ? "1px solid var(--accent-strong)" : "1px solid var(--border)",
            boxShadow: "var(--shadow-1)",
            transition: "background 0.2s, border-color 0.2s",
          }}
        >
          {capturing ? (
            <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 12.5, fontWeight: 600, color: "var(--accent-ink)" }}>
              Press keys…
              <span
                style={{
                  display: "inline-block",
                  width: 2,
                  height: 14,
                  borderRadius: 1,
                  background: "var(--accent-strong)",
                  animation: "cursorBlink 0.5s ease-in-out infinite",
                }}
              />
            </span>
          ) : (
            displayHotkey(value)
              .split(" ")
              .map((key, i) => (
                <kbd key={`${key}-${i}`} style={keycapStyle}>
                  {key}
                </kbd>
              ))
          )}
        </div>
        <button
          type="button"
          onClick={toggleCapture}
          style={{
            height: "auto",
            minHeight: 0,
            padding: "4px 6px",
            border: "none",
            background: "transparent",
            color: "var(--text-tertiary)",
            fontSize: 12,
            fontWeight: 500,
          }}
        >
          {capturing ? "Cancel" : "Change"}
        </button>
      </div>
      {capturing && (
        <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
          Press a key combination. <strong>Esc</strong> cancels.
        </span>
      )}
      {error && <span style={{ fontSize: 11.5, color: "var(--danger)" }}>{error}</span>}
    </div>
  );
}
