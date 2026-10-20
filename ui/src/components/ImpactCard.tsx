import { Icon, type IconName } from "./Icon";

// Shared "time saved vs typing" card, used by both the Home hero and the
// Insights hero. Keeping the computation and layout in one place is what
// keeps the two screens from drifting apart (they previously disagreed:
// Home used wpm/52 while Insights used the backend's 40-wpm timesFaster).
//
// Every figure is read from the backend `impact` object, so the "× faster"
// badge, the time-saved total, and the speaking pace are always the same
// numbers on every screen.

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
  /** "hero" = the big gradient banner (Insights); "compact" = the green
      sidebar card (Home). */
  variant?: "hero" | "compact";
  /** Extra line rendered under the speaking-pace line (hero only), e.g. the
      tiered "You've written N blog posts!" flourish. */
  footer?: React.ReactNode;
  style?: React.CSSProperties;
}

function weekGoal(avgWordsPerDay: number): number {
  return Math.max(1000, Math.round((avgWordsPerDay * 7 * 2) / 100) * 100);
}

export default function ImpactCard({ impact, hasData, variant = "hero", footer, style }: Props) {
  const goal = weekGoal(impact.avgWordsPerDay);
  const weekPct = Math.min(100, Math.round((impact.wordsLast7Days / goal) * 100));
  const wpm = impact.wordsPerMinute;
  const faster = impact.timesFaster;

  // Compact (Home sidebar): quiet-surface card, 2-col mini stats.
  if (variant === "compact") {
    return (
      <div
        style={{
          background: "var(--surface-2)",
          borderRadius: "var(--radius)",
          padding: "20px 22px",
          color: "var(--text)",
          ...style,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
          <Icon name="clock" size={15} color="var(--text-tertiary)" />
          <h3 style={{ fontSize: 11.5, fontWeight: 600, letterSpacing: "var(--tracking-tight)", color: "var(--text-tertiary)", margin: 0 }}>
            Time saved vs typing
          </h3>
        </div>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
          <span style={{ fontSize: 28, fontWeight: 650, lineHeight: 1, letterSpacing: "var(--tracking-display)", fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>
            {hasData ? impact.timeSavedLabel : "0m"}
          </span>
          {hasData && faster != null && faster > 0 && (
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 4,
                background: "var(--accent-soft)",
                borderRadius: 999,
                padding: "3px 10px",
                fontSize: 12,
                fontWeight: 600,
                color: "var(--accent-ink)",
              }}
            >
              <Icon name="trending-up" size={13} color="var(--accent-ink)" />
              {faster}× faster
            </span>
          )}
        </div>
        <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 8 }}>
          Across {impact.totalWords.toLocaleString()} words all-time · you speak at {wpm ?? "—"} wpm
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 10, marginTop: 18 }}>
          <MiniStat icon="zap" value={wpm ? `${wpm}` : "—"} label="wpm" />
          <MiniStat icon="flame" value={`${impact.streakDays}`} label="day streak" />
        </div>

        <div style={{ marginTop: 18, paddingTop: 16, borderTop: "1px solid var(--border-subtle)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 8 }}>
            <Icon name="target" size={14} color="var(--text-tertiary)" />
            <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}>Weekly goal</span>
            <span style={{ fontSize: 12, color: "var(--text-secondary)", marginLeft: "auto", fontVariantNumeric: "tabular-nums" }}>{weekPct}%</span>
          </div>
          <div style={{ height: 8, background: "var(--border-subtle)", borderRadius: 4, overflow: "hidden" }}>
            <div style={{ height: "100%", width: `${weekPct}%`, background: "var(--accent-strong)", borderRadius: 4, transition: "width 0.5s ease" }} />
          </div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 8, fontVariantNumeric: "tabular-nums" }}>
            {impact.wordsLast7Days.toLocaleString()} / {goal.toLocaleString()} words this week
          </div>
        </div>
      </div>
    );
  }

  // Hero (Insights): the big card with the goal ring.
  return (
    <div
      style={{
        display: "flex",
        gap: 20,
        flexWrap: "wrap",
        background: "var(--surface)",
        border: 0,
        borderRadius: "var(--radius)",
        padding: "24px 26px",
        color: "var(--text)",
        boxShadow: "var(--shadow-2)",
        position: "relative",
        overflow: "hidden",
        ...style,
      }}
    >
      <div style={{ flex: "1 1 320px", display: "flex", flexDirection: "column" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.06em", color: "var(--text-tertiary)" }}>
          <Icon name="zap" size={15} color="var(--accent)" />
          Time saved vs typing
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 14, marginTop: 6 }}>
          <div style={{ fontSize: 28, fontWeight: 650, lineHeight: 1, letterSpacing: "var(--tracking-display)", fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>{hasData ? impact.timeSavedLabel : "0m"}</div>
          {hasData && faster != null && faster > 0 && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 5,
                background: "var(--accent-soft)",
                borderRadius: 999,
                padding: "5px 12px",
                fontSize: 13,
                fontWeight: 600,
                color: "var(--accent-ink)",
              }}
            >
              <Icon name="trending-up" size={15} color="var(--accent-ink)" />
              {faster}× faster
            </div>
          )}
        </div>
        <div style={{ fontSize: 13, color: "var(--text-secondary)", marginTop: 8 }}>
          {!hasData ? (
            "Dictate your first message to start measuring the time you save."
          ) : wpm != null ? (
            <>
              You speak at <b>{wpm} wpm</b>, measured over {impact.ratedTakes} dictation{impact.ratedTakes === 1 ? "" : "s"}, against a
              40 wpm average typing pace.
            </>
          ) : (
            <>Based on your word count against a 40 wpm average typing pace.</>
          )}
        </div>
        {footer}
        <div style={{ display: "flex", gap: 24, marginTop: 16, flexWrap: "wrap" }}>
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 20, fontWeight: 650, fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>
              {impact.totalWords.toLocaleString()}
            </div>
            <div style={{ fontSize: 12, color: "var(--text-tertiary)" }}>total words</div>
          </div>
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 20, fontWeight: 650, fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>
              <Icon name="messages-square" size={16} color="var(--text-tertiary)" />
              <span>{impact.totalDictations.toLocaleString()}</span>
            </div>
            <div style={{ fontSize: 12, color: "var(--text-tertiary)" }}>dictations</div>
          </div>
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 20, fontWeight: 650, fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>
              <Icon name="zap" size={16} color="var(--text-tertiary)" />
              {wpm ?? "—"}
            </div>
            <div style={{ fontSize: 12, color: "var(--text-tertiary)" }}>wpm speaking</div>
          </div>
        </div>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 16, flex: "0 0 auto" }}>
        <Ring pct={weekPct / 100} size={128} stroke={12} color="var(--accent-strong)" track="var(--border-subtle)">
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
            <Icon name="target" size={16} color="var(--text-tertiary)" />
            <div style={{ fontSize: 24, fontWeight: 700, marginTop: 2, fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>{weekPct}%</div>
            <div style={{ fontSize: 11, color: "var(--text-tertiary)" }}>weekly goal</div>
          </div>
        </Ring>
        <div>
          <div style={{ fontSize: 13, color: "var(--text-secondary)", fontVariantNumeric: "tabular-nums" }}>{impact.wordsLast7Days.toLocaleString()} / {goal.toLocaleString()} words</div>
          <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 2 }}>last 7 days</div>
        </div>
      </div>
    </div>
  );
}

function MiniStat({ value, label, icon }: { value: string; label: string; icon: IconName }) {
  return (
    <div
      style={{
        background: "var(--surface)",
        borderRadius: 10,
        boxShadow: "var(--shadow-1)",
        padding: "12px 14px",
        display: "flex",
        alignItems: "center",
        gap: 10,
      }}
    >
      <span
        style={{
          width: 28,
          height: 28,
          borderRadius: 8,
          background: "var(--accent-soft)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        <Icon name={icon} size={15} color="var(--accent-ink)" />
      </span>
      <div>
        <div style={{ fontSize: 22, fontWeight: 700, lineHeight: 1, fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>{value}</div>
        <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 2 }}>{label}</div>
      </div>
    </div>
  );
}

function Ring({
  pct,
  size = 120,
  stroke = 12,
  color = "var(--accent)",
  track = "var(--border-subtle)",
  children,
}: {
  pct: number;
  size?: number;
  stroke?: number;
  color?: string;
  track?: string;
  children?: React.ReactNode;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const clamped = Math.max(0, Math.min(1, pct));
  const offset = c * (1 - clamped);
  return (
    <div style={{ position: "relative", width: size, height: size, flexShrink: 0 }}>
      <svg width={size} height={size} style={{ transform: "rotate(-90deg)" }}>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke={track} strokeWidth={stroke} />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke={color}
          strokeWidth={stroke}
          strokeDasharray={c}
          strokeDashoffset={offset}
          strokeLinecap="round"
        />
      </svg>
      <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center" }}>{children}</div>
    </div>
  );
}
