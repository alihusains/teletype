import type { CSSProperties, ReactNode } from "react";

// Shared "time saved vs typing" card, used by the Home sidebar and available to
// any screen that wants the same figures on top of the same `impact` object.
// Keeping the computation and layout here is what stops the screens disagreeing
// with each other (they previously did: Home used wpm/52, Insights used the
// backend's 40-wpm timesFaster).
//
// Every number is read straight from the backend `impact` object. Nothing here
// invents a target: an earlier version rounded the user's own weekly average up
// to the next hundred, called it a "Weekly goal" and drew a progress ring
// against it, which is a made-up number presented as an achievement.

export interface ImpactData {
  wordsPerMinute: number | null;
  timesFaster: number | null;
  timeSavedLabel: string;
  totalWords: number;
  totalDictations: number;
  ratedTakes: number;
  streakDays: number;
  wordsLast7Days: number;
  avgWordsPerDay: number;
}

interface Props {
  impact: ImpactData;
  hasData: boolean;
  /** Density only: "hero" is the wide banner under a page heading, "compact"
      the narrower sidebar card. Both draw the same tokens. */
  variant?: "hero" | "compact";
  /** Extra node rendered under the stat rows, e.g. a screen-specific note. */
  footer?: ReactNode;
  style?: CSSProperties;
}

const label: CSSProperties = {
  fontSize: "var(--text-xs)",
  fontWeight: 600,
  color: "var(--text-secondary)",
};

const note: CSSProperties = {
  fontSize: "var(--text-sm)",
  lineHeight: 1.5,
  color: "var(--text-secondary)",
};

export default function ImpactCard({ impact, hasData, variant = "hero", footer, style }: Props) {
  const wpm = impact.wordsPerMinute;
  const faster = impact.timesFaster;

  const stats: { term: string; value: string }[] = [
    { term: "Words dictated", value: impact.totalWords.toLocaleString() },
    { term: "Dictations", value: impact.totalDictations.toLocaleString() },
    { term: "Speaking pace", value: wpm != null ? `${wpm} wpm` : "Not rated yet" },
    { term: "Last 7 days", value: impact.wordsLast7Days.toLocaleString() },
    {
      term: "Current streak",
      value:
        impact.streakDays > 0
          ? `${impact.streakDays} day${impact.streakDays === 1 ? "" : "s"}`
          : "None yet",
    },
  ];

  return (
    <div
      className="tt-card"
      style={{ padding: variant === "compact" ? "16px 18px" : "20px 22px", ...style }}
    >
      <div style={{ ...label, marginBottom: 6 }}>Time saved vs typing</div>

      <div style={{ display: "flex", alignItems: "baseline", gap: 10, flexWrap: "wrap" }}>
        <span
          style={{
            fontSize: "var(--text-2xl)",
            fontWeight: 700,
            lineHeight: 1.1,
            fontVariantNumeric: "tabular-nums",
            letterSpacing: "var(--tracking-tight)",
            color: "var(--text)",
          }}
        >
          {hasData ? impact.timeSavedLabel : "—"}
        </span>
        {hasData && faster != null && faster > 0 && (
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 5,
              background: "var(--accent-soft)",
              color: "var(--accent-ink)",
              borderRadius: "var(--radius-pill)",
              padding: "2px 9px",
              fontSize: "var(--text-xs)",
              fontWeight: 600,
            }}
          >
            {faster}× faster
          </span>
        )}
      </div>

      <div style={{ ...note, marginTop: 6 }}>
        {!hasData ? (
          "Dictate something and this starts counting."
        ) : wpm != null ? (
          <>
            You speak at {wpm} words a minute over {impact.ratedTakes} rated dictation
            {impact.ratedTakes === 1 ? "" : "s"}, against a 40 wpm typing baseline.
          </>
        ) : (
          <>Word count against a 40 wpm typing baseline.</>
        )}
      </div>

      {hasData ? (
        <dl
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: "12px 22px",
            marginTop: 16,
            paddingTop: 14,
            borderTop: "1px solid var(--border-subtle)",
          }}
        >
          {stats.map((s) => (
            <div key={s.term}>
              <dt style={{ ...label, fontWeight: 500 }}>{s.term}</dt>
              <dd
                style={{
                  margin: "2px 0 0",
                  fontSize: "var(--text-md)",
                  fontWeight: 650,
                  fontVariantNumeric: "tabular-nums",
                  color: "var(--text)",
                }}
              >
                {s.value}
              </dd>
            </div>
          ))}
        </dl>
      ) : null}

      {footer ? (
        <div style={{ marginTop: 14, paddingTop: 12, borderTop: "1px solid var(--border-subtle)" }}>
          {footer}
        </div>
      ) : null}
    </div>
  );
}
