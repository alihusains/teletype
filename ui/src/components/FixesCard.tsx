import { Icon, type IconName } from "./Icon";

const ACCENT = "#2563eb";

interface Props {
  fillerRemoved: number;
  autotextExpansions: number;
  personalizationCorrections: number;
  dictionaryLearned: number;
  icon?: IconName;
  style?: React.CSSProperties;
}

interface Row {
  label: string;
  value: number;
}

/**
 * T5.3: "Fixes made by Teletype" card. Headline is the sum of every
 * correction Teletype has applied on the user's behalf: filler words
 * removed, AutoText expansions, learned personalization corrections, and
 * dictionary words it learned on its own.
 */
export default function FixesCard({
  fillerRemoved,
  autotextExpansions,
  personalizationCorrections,
  dictionaryLearned,
  icon = "wand",
  style,
}: Props) {
  const total = fillerRemoved + autotextExpansions + personalizationCorrections + dictionaryLearned;
  const rows: Row[] = [
    { label: "filler words removed", value: fillerRemoved },
    { label: "AutoText expansions", value: autotextExpansions },
    { label: "personalization corrections", value: personalizationCorrections },
    { label: "dictionary words learned", value: dictionaryLearned },
  ];

  return (
    <div
      style={{
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: "18px 20px",
        boxShadow: "var(--shadow-2)",
        ...style,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 9,
          fontSize: 15,
          fontWeight: 700,
          color: "var(--text)",
          letterSpacing: "var(--tracking-tight)",
          marginBottom: 18,
        }}
      >
        <span
          style={{
            width: 26,
            height: 26,
            borderRadius: 8,
            background: "var(--accent-soft)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <Icon name={icon} size={15} color={ACCENT} />
        </span>
        Fixes made by Teletype
      </div>
      {total === 0 ? (
        <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--text-secondary)" }}>
          <Icon name="wand" size={16} color="var(--text-secondary)" />
          No fixes yet — dictate and start using AutoText.
        </p>
      ) : (
        <>
          <div style={{ fontSize: 34, fontWeight: 800, lineHeight: 1, fontVariantNumeric: "tabular-nums", letterSpacing: "var(--tracking-tight)" }}>
            {total.toLocaleString()}
          </div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4 }}>
            lifetime total across all dictations
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 6, marginTop: 14 }}>
            {rows.map((r) => (
              <div key={r.label} style={{ display: "flex", justifyContent: "space-between", fontSize: 13, padding: "4px 0", borderTop: "1px solid var(--border-subtle)" }}>
                <span style={{ color: "var(--text-secondary)" }}>{r.label}</span>
                <span style={{ fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>{r.value.toLocaleString()}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
