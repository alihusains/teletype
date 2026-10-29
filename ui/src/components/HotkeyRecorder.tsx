import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/errors";

interface Props {
  value: string;
  onSave: (hotkey: string) => void;
  /** The supporting line under the keycap. Pass null where the row already
      explains the shortcut (Keybinds), so it is not repeated per row. */
  hint?: string | null;
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
  if (typeof hotkey !== "string" || !hotkey) return "—";
  return hotkey
    .split("+")
    .map((p) => MODS[p] ?? p)
    .join(" ");
}

/** Uppercase key name for the label under the keycap, e.g. "LEFT OPTION". */
function keyLabel(hotkey: string): string {
  if (typeof hotkey !== "string" || !hotkey) return "No key set";
  return hotkey
    .split("+")
    .map((p) => (p === "Fn" ? "GLOBE (FN)" : p.toUpperCase()))
    .join(" + ");
}

export default function HotkeyRecorder({
  value,
  onSave,
  hint = "Hold to dictate. Release to transcribe.",
}: Props) {
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
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 4, paddingTop: 14 }}>
      {/* Keycap */}
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
          position: "relative",
          width: 160,
          height: 70,
          borderRadius: 18,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          cursor: "pointer",
          userSelect: "none",
          background: capturing
            ? "var(--accent-soft)"
            : "linear-gradient(180deg, var(--surface) 0%, var(--surface-2) 100%)",
          border: capturing
            ? "2px solid var(--accent)"
            : "1px solid var(--border-strong)",
          boxShadow: capturing ? undefined : "var(--shadow-1)",
          animation: capturing ? "pulseGlow 0.8s ease-in-out infinite" : undefined,
          transition: "background 0.2s, border-color 0.2s",
        }}
      >
        {capturing ? (
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              fontSize: 14,
              fontWeight: 600,
              color: "var(--accent)",
            }}
          >
            Press keys…
            <span
              style={{
                display: "inline-block",
                width: 2,
                height: 18,
                borderRadius: 1,
                background: "var(--accent)",
                animation: "cursorBlink 0.5s ease-in-out infinite",
              }}
            />
          </span>
        ) : (
          <span
            style={{
              fontSize: 28,
              fontWeight: 700,
              color: "var(--accent)",
              lineHeight: 1,
              transform: "translateY(2px)",
            }}
          >
            {displayHotkey(value)}
          </span>
        )}

        {!capturing && (
          <span
            style={{
              position: "absolute",
              top: -10,
              right: 12,
              padding: "3px 8px",
              borderRadius: "var(--radius-pill)",
              background: "var(--accent-soft)",
              border: "1px solid color-mix(in srgb, var(--accent) 30%, transparent)",
              color: "var(--accent-ink)",
              fontSize: "var(--text-xs)",
              fontWeight: 700,
              textTransform: "uppercase",
              letterSpacing: 0.4,
            }}
          >
            Change
          </span>
        )}
      </div>

      {/* Label under the keycap */}
      <span
        style={{
          marginLeft: 2,
          fontSize: 11,
          fontWeight: 600,
          letterSpacing: 0.55,
          textTransform: "uppercase",
          color: capturing ? "var(--accent)" : "var(--text-secondary)",
          opacity: capturing ? 0.7 : 1,
        }}
      >
        {capturing ? "Listening for input" : keyLabel(value)}
      </span>

      {/* Divider + hint. The capture instruction always shows (the keycap
          label alone does not say what to press); the supporting `hint` line
          is only shown at rest, and rows that already explain the shortcut
          (Keybinds) pass `hint={null}` to avoid repeating it. */}
      {(capturing || error || hint !== null) && (
        <div
          style={{
            width: "100%",
            borderTop: "1px solid var(--border)",
            padding: "10px 0 0",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          {(capturing || hint !== null) && (
            <span style={{ fontSize: 12.5, color: "var(--text-secondary)" }}>
              {capturing ? (
                <>
                  Press a key combination to save it. <strong>Esc</strong> cancels.
                </>
              ) : (
                hint
              )}
            </span>
          )}
          {error && (
            <span style={{ fontSize: 12, color: "var(--danger)" }}>{error}</span>
          )}
        </div>
      )}
    </div>
  );
}
