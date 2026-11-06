import { useState } from "react";
import { useTauriEvent } from "../lib/useTauriEvent";
import { isMacOS } from "../lib/hotkeyCapture";

const MAC_GLYPHS: Record<string, string> = {
  Cmd: "\u2318",
  Ctrl: "\u2303",
  Alt: "\u2325",
  Shift: "\u21E7",
};

const READABLE: Record<string, string> = {
  Cmd: "Command",
  Ctrl: "Control",
  Alt: "Option",
  Shift: "Shift",
  Fn: "Globe (Fn)",
  Space: "Space",
};

const keycapStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 3,
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
  verticalAlign: "middle",
};

function Keycap({ segment, mac }: { segment: string; mac: boolean }) {
  const glyph = mac ? MAC_GLYPHS[segment] : undefined;
  const label = mac ? (READABLE[segment] ?? segment) : segment;
  return (
    <kbd style={keycapStyle}>
      {mac && glyph ? (
        <>
          <span aria-hidden="true">{glyph}</span> {label}
        </>
      ) : (
        label
      )}
    </kbd>
  );
}

/**
 * "Try it" check: tells the user to hold the hotkey and confirms the moment
 * the backend reports any dictation activity, which proves the hotkey tap is
 * live (the thing a missing Accessibility grant silently breaks).
 */
export default function TryHotkeyCheck({ hotkey }: { hotkey: string }) {
  const [fired, setFired] = useState(false);
  useTauriEvent<{ phase: string }>("dictation-state", ({ payload }) => {
    if (payload?.phase && payload.phase !== "idle") setFired(true);
  });

  const segments = hotkey ? hotkey.split("+") : [];
  const mac = isMacOS();

  const hotkeyDisplay = segments.length > 0 ? (
    <span style={{ display: "inline-flex", gap: 4, flexWrap: "wrap", verticalAlign: "middle" }}>
      {segments.map((seg, i) => (
        <Keycap key={`${seg}-${i}`} segment={seg} mac={mac} />
      ))}
    </span>
  ) : (
    <strong>your hotkey</strong>
  );

  return (
    <div
      role="status"
      data-testid="try-hotkey"
      style={{
        padding: "12px 14px",
        background: "var(--bg)",
        borderRadius: "var(--radius-sm)",
        border: `1px solid ${fired ? "var(--success)" : "var(--border)"}`,
        fontSize: 13,
        marginBottom: 20,
      }}
    >
      {fired ? (
        <span style={{ color: "var(--success)" }}>
          ✓ Hotkey works. Teletype heard {hotkeyDisplay}.
        </span>
      ) : (
        <span>
          Try it: hold {hotkeyDisplay} and speak.
        </span>
      )}
    </div>
  );
}
