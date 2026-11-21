import type { CSSProperties } from "react";
import {
  parseProfileKey,
  PROFILES,
  supportsProfile,
  type ModelCapability,
  type ProfileKey,
} from "../lib/profileLabels";

export interface ProfilePickerProps {
  value: ProfileKey | string | null | undefined;
  onChange: (key: ProfileKey) => void;
  capability?: ModelCapability;
  label?: string;
  compact?: boolean;
}

const CARD: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 3,
  padding: "10px 11px",
  borderRadius: 10,
  border: "1px solid var(--border)",
  background: "var(--surface)",
  minWidth: 0,
};

export default function ProfilePicker({
  value,
  onChange,
  capability = "instruction-capable",
  label = "Polish profile",
  compact = false,
}: ProfilePickerProps) {
  const selected = parseProfileKey(value);
  return (
    <div
      role="radiogroup"
      aria-label={label}
      data-testid="profile-picker"
      style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))",
        gap: 8,
      }}
    >
      {PROFILES.map((p) => {
        const supported = supportsProfile(capability, p.key);
        const active = selected === p.key;
        return (
          <div
            key={p.key}
            style={{
              ...CARD,
              border: active ? "2px solid var(--accent)" : "1px solid var(--border)",
              opacity: supported ? 1 : 0.5,
            }}
          >
            <button
              type="button"
              role="radio"
              aria-checked={active}
              aria-label={p.label}
              disabled={!supported}
              onClick={() => onChange(p.key)}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                alignSelf: "flex-start",
                padding: 0,
                border: "none",
                background: "none",
                cursor: supported ? "pointer" : "not-allowed",
                color: "var(--text)",
                font: "inherit",
                fontSize: "var(--text-sm)",
                fontWeight: 600,
              }}
            >
              <span aria-hidden="true" style={{ fontSize: 13 }}>
                {active ? "◆" : "◇"}
              </span>
              <span>{p.label}</span>
            </button>
            {!compact ? (
              <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)" }}>
                {p.promise}
              </span>
            ) : null}
            <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>
              {p.budgetLabel}
            </span>
            {!supported ? (
              <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>
                Unsupported on a Cleanup-only model
              </span>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
