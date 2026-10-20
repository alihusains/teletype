// Webview hotkey recorder for non-macOS platforms (plan P8.5a).
//
// macOS uses the native capture panel (see HotkeyRecorder.tsx) because the
// webview KeyboardEvent API cannot see the Fn key. Everywhere else this
// component listens for keydown/keyup on window and builds the chord from
// `event.code` (layout-stable physical key), mirroring HotkeyRecorder's
// keycap styling so both platforms look identical.
//
// On commit the chord is handed to the same `onSave` callback the macOS path
// uses; the screens already persist it through `save_settings`/`update_settings_patch`,
// which accept the same wire format ("Ctrl+Shift+Space"), so no new IPC is
// needed to store a webview-captured chord.

import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import {
  chordDisplay,
  chordOnKeydown,
  chordOnKeyup,
  chordResult,
  initialChord,
  isMacOS,
  type ChordState,
} from "../lib/hotkeyCapture";

interface Props {
  value: string;
  onSave: (hotkey: string) => void;
}

/**
 * Non-macOS hotkey recorder. The caller only mounts this on non-macOS
 * platforms (macOS keeps the native-panel HotkeyRecorder), so the two
 * paths never run at once.
 */
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

export default function WebviewHotkeyRecorder({ value, onSave }: Props) {
  const [capturing, setCapturing] = useState(false);
  const [chord, setChord] = useState<ChordState>(initialChord);
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;

  const finish = useCallback(
    (next: ChordState) => {
      setCapturing(false);
      setChord(initialChord());
      const result = chordResult(next);
      if (result.status === "committed") {
        onSaveRef.current(result.hotkey);
      }
    },
    [],
  );

  const startCapture = useCallback(() => {
    setChord(initialChord());
    setCapturing(true);
  }, []);

  const cancelCapture = useCallback(() => {
    setCapturing(false);
    setChord(initialChord());
  }, []);


  // Window-level capture-phase listeners: the keycap is a plain div so it
  // cannot swallow keys, and the chord must still build when focus is
  // anywhere in the settings window. State updates stay in the reducer;
  // `finish` is scheduled via queueMicrotask so it never runs as a side
  // effect inside the state updater.
  useEffect(() => {
    if (!capturing) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setChord((prev) => {
        const next = chordOnKeydown(prev, { code: e.code, repeat: e.repeat });
        if (next.key !== null && prev.key === null) {
          queueMicrotask(() => finish(next));
        }
        return next;
      });
    };
    const onKeyUp = (e: KeyboardEvent) => {
      e.preventDefault();
      setChord((prev) => chordOnKeyup(prev, e.code));
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [capturing, finish]);

  const toggleCapture = () => {
    if (capturing) {
      cancelCapture();
    } else {
      startCapture();
    }
  };

  const inProgress = chordDisplay(chord);

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 4 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <div
          role="button"
          tabIndex={0}
          aria-label={capturing ? "Recording, press a key combination" : `Hotkey ${value || "not set"}, tap to change`}
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
              {inProgress || "Press keys…"}
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
            (value ? value.split("+") : ["No key"]).map((key, i) => (
              <kbd key={`${key}-${i}`} style={keycapStyle}>
                {key === "Command" ? "Win" : key === "Control" ? "Ctrl" : key === "Option" ? "Alt" : key}
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
    </div>
  );
}

/**
 * Platform gate for the two recorder paths: true when this platform should
 * use the webview recorder (i.e. NOT macOS). Exported so screens and tests
 * share one definition of the gate.
 */
export function usesWebviewCapture(): boolean {
  return !isMacOS();
}

