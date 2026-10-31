import { useState } from "react";
import { useTauriEvent } from "../lib/useTauriEvent";

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
  const label = hotkey || "your hotkey";
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
        <span style={{ color: "var(--success)" }}>✓ Hotkey works. Teletype heard {label}.</span>
      ) : (
        <span>
          Try it: hold <strong>{label}</strong> and speak.
        </span>
      )}
    </div>
  );
}
