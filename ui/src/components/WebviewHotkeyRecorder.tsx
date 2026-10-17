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

import { useCallback, useEffect, useRef, useState } from "react";
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
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 4 }}>
      {/* Keycap */}
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
            : "1.5px solid color-mix(in srgb, var(--accent) 25%, transparent)",
          boxShadow: capturing
            ? undefined
            : "0 5px 12px color-mix(in srgb, var(--accent) 12%, transparent), 0 1px 2px rgba(0,0,0,0.07)",
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
            {inProgress || "Press keys…"}
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
              fontSize: 20,
              fontWeight: 700,
              color: "var(--accent)",
              lineHeight: 1,
              transform: "translateY(2px)",
            }}
          >
            {value || "No key set"}
          </span>
        )}

        {!capturing && (
          <span
            style={{
              position: "absolute",
              top: -10,
              right: 12,
              padding: "3px 8px",
              borderRadius: 999,
              background: "var(--accent)",
              color: "#fff",
              fontSize: 10,
              fontWeight: 700,
              textTransform: "uppercase",
              letterSpacing: 0.4,
              boxShadow: "0 2px 6px color-mix(in srgb, var(--accent) 35%, transparent)",
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
        {capturing
          ? "Listening for input"
          : value
            ? value.split("+").map((p) => (p === "Command" ? "WIN" : p.toUpperCase())).join(" + ")
            : "No key set"}
      </span>

      {/* Divider + hint */}
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
        <span style={{ fontSize: 12.5, color: "var(--text-secondary)" }}>
          {capturing ? (
            <>
              Press a key combination to save it. <strong>Esc</strong> cancels.
            </>
          ) : (
            "Hold to dictate. Release to transcribe."
          )}
        </span>

      </div>
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

