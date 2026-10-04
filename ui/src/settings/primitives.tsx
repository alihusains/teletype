// Shared primitives for the settings sections ported from Envious Wispr.
//
// Every section component builds from these so nine sections cannot drift into
// nine different looks. The patterns follow EW's interaction model (whole row
// is the toggle target, helper text behind a "?", frozen-setting notice) while
// wearing Teletype's own tokens — nothing here imports EW's palette.

import type { CSSProperties, ReactNode } from "react";

export function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section style={{ marginBottom: 28 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8, marginBottom: 10 }}>
        <h2
          style={{
            margin: 0,
            fontSize: "var(--text-md)",
            fontWeight: 600,
            letterSpacing: "var(--tracking-tight)",
            color: "var(--text)",
          }}
        >
          {title}
        </h2>
        {hint ? <Explainer text={hint} /> : null}
      </div>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {children}
      </div>
    </section>
  );
}

// The "?" affordance from EW: keeps a paragraph of explanation available
// without putting it in everyone's face.
export function Explainer({ text }: { text: string }) {
  return (
    <details style={{ position: "relative", display: "inline-flex" }}>
      <summary
        aria-label="More information"
        style={{
          listStyle: "none",
          cursor: "help",
          width: 16,
          height: 16,
          borderRadius: "50%",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 10,
          fontWeight: 600,
          color: "var(--text-tertiary)",
          background: "var(--surface-2)",
          border: "1px solid var(--border-subtle)",
          userSelect: "none",
        }}
      >
        ?
      </summary>
      <span
        style={{
          position: "absolute",
          top: 22,
          left: 0,
          zIndex: 20,
          width: 260,
          padding: "10px 12px",
          fontSize: "var(--text-xs)",
          lineHeight: 1.5,
          fontWeight: 400,
          letterSpacing: 0,
          color: "var(--text-secondary)",
          background: "var(--surface-2)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius-sm)",
          boxShadow: "var(--shadow-2)",
        }}
      >
        {text}
      </span>
    </details>
  );
}

// A settings row. When `onClick` is present the WHOLE row is the hit target,
// as in EW: a 28px switch is a poor target for something you toggle daily.
export function Row({
  label,
  hint,
  children,
  onClick,
  disabled,
  first,
}: {
  label: string;
  hint?: string;
  children?: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  /** The first row in a section has no divider above it. */
  first?: boolean;
}) {
  const interactive = Boolean(onClick) && !disabled;
  return (
    <div
      onClick={onClick}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onKeyDown={
        interactive
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick?.();
              }
            }
          : undefined
      }
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "11px 14px",
        borderTop: first ? "none" : "1px solid var(--border-subtle)",
        cursor: interactive ? "pointer" : "default",
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: "var(--text-sm)", color: "var(--text)" }}>{label}</div>
        {hint ? (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 2 }}>
            {hint}
          </div>
        ) : null}
      </div>
      {children}
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={(e) => {
        // The row owns the click; stop it firing twice when the switch sits
        // inside a clickable row.
        e.stopPropagation();
        onChange(!checked);
      }}
      style={{
        width: 40,
        height: 24,
        flexShrink: 0,
        padding: 2,
        borderRadius: 999,
        border: "1px solid var(--border)",
        background: checked ? "var(--accent)" : "var(--surface-2)",
        cursor: disabled ? "default" : "pointer",
        transition: `background var(--dur-fast) var(--ease)`,
      }}
    >
      <span
        style={{
          display: "block",
          width: 18,
          height: 18,
          borderRadius: "50%",
          background: "var(--bg)",
          boxShadow: "var(--shadow-1)",
          transform: checked ? "translateX(16px)" : "translateX(0)",
          transition: `transform var(--dur-fast) var(--ease)`,
        }}
      />
    </button>
  );
}

export type SegmentedOption<T extends string> = {
  value: T;
  label: string;
  title?: string;
};

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      style={{
        display: "inline-flex",
        padding: 2,
        gap: 2,
        background: "var(--surface-2)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-sm)",
      }}
    >
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={active}
            title={o.title}
            onClick={() => onChange(o.value)}
            style={{
              padding: "4px 10px",
              fontSize: "var(--text-xs)",
              fontWeight: active ? 600 : 400,
              color: active ? "var(--text)" : "var(--text-tertiary)",
              background: active ? "var(--surface)" : "transparent",
              border: "none",
              borderRadius: "calc(var(--radius-sm) - 1px)",
              boxShadow: active ? "var(--shadow-1)" : "none",
              cursor: "pointer",
              transition: `background var(--dur-fast) var(--ease)`,
            }}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  disabled,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <select
      aria-label={label}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value as T)}
      style={{
        padding: "5px 8px",
        fontSize: "var(--text-xs)",
        color: "var(--text)",
        background: "var(--surface-2)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-sm)",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

// Filled: the one action the row is about.
export function PrimaryButton({
  onClick,
  children,
  disabled,
}: {
  onClick: () => void;
  children: ReactNode;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: "5px 12px",
        fontSize: "var(--text-xs)",
        fontWeight: 600,
        color: "var(--bg)",
        background: disabled ? "var(--surface-hover)" : "var(--accent)",
        border: "none",
        borderRadius: "var(--radius-sm)",
        cursor: disabled ? "default" : "pointer",
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </button>
  );
}

// Outlined: every button that is NOT the row's primary action. Destructive
// actions use this plus the danger colour, so they never look like the
// affirmative next to them.
export function SecondaryButton({
  onClick,
  children,
  disabled,
  danger,
}: {
  onClick: () => void;
  children: ReactNode;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: "5px 12px",
        fontSize: "var(--text-xs)",
        fontWeight: 500,
        color: disabled ? "var(--text-tertiary)" : danger ? "var(--danger)" : "var(--text)",
        background: "transparent",
        border: `1px solid ${danger ? "var(--danger)" : "var(--border)"}`,
        borderRadius: "var(--radius-sm)",
        cursor: disabled ? "default" : "pointer",
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </button>
  );
}

// A read-only chip. Never used to imply an action the user cannot take.
export function Chip({
  tone = "neutral",
  children,
}: {
  tone?: "neutral" | "success" | "warning" | "danger";
  children: ReactNode;
}) {
  const tones: Record<string, CSSProperties> = {
    neutral: { color: "var(--text-tertiary)", borderColor: "var(--border)" },
    success: { color: "var(--success)", borderColor: "var(--success)" },
    warning: { color: "var(--warning)", borderColor: "var(--warning)" },
    danger: { color: "var(--danger)", borderColor: "var(--danger)" },
  };
  return (
    <span
      style={{
        padding: "2px 8px",
        fontSize: "var(--text-xs)",
        fontWeight: 500,
        background: "transparent",
        border: `1px solid ${tones[tone].borderColor}`,
        borderRadius: 999,
        color: tones[tone].color,
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </span>
  );
}

// EW's frozen-per-recording banner. Use when a change cannot affect the take
// already in progress — telling the user it applies "next recording" is the
// difference between a setting that feels broken and one that feels deliberate.
export function FrozenNotice({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        gap: 8,
        alignItems: "flex-start",
        marginBottom: 10,
        padding: "9px 12px",
        fontSize: "var(--text-xs)",
        lineHeight: 1.5,
        color: "var(--warning)",
        background: "var(--warning-soft)",
        border: "1px solid var(--warning)",
        borderRadius: "var(--radius-sm)",
      }}
    >
      <span aria-hidden="true">ⓘ</span>
      <span>{children}</span>
    </div>
  );
}

export function EmptyState({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        padding: "22px 14px",
        textAlign: "center",
        fontSize: "var(--text-xs)",
        color: "var(--text-tertiary)",
        borderTop: "1px solid var(--border-subtle)",
      }}
    >
      {children}
    </div>
  );
}

export function Divider() {
  return <div style={{ height: 1, background: "var(--border-subtle)" }} />;
}

// Text input used by the filler-word and keyword fields.
export function TextInput({
  value,
  onChange,
  placeholder,
  label,
  width = 180,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  label: string;
  width?: number;
}) {
  return (
    <input
      aria-label={label}
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      style={{
        width,
        padding: "5px 8px",
        fontSize: "var(--text-xs)",
        color: "var(--text)",
        background: "var(--surface-2)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-sm)",
      }}
    />
  );
}