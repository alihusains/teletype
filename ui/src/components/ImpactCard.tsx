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

  // Compact (Home sidebar): green gradient, smaller, 2-col mini stats.
  if (variant === "compact") {
    return (
      <div
        style={{
          background: "linear-gradient(135deg, #065f46 0%, #059669 60%, #10b981 100%)",
          borderRadius: "var(--radius)",
          padding: "20px 22px",
          color: "#fff",
          ...style,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
          <Icon name="clock" size={15} color="#fff" />
          <h3 style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.5, opacity: 0.9, margin: 0 }}>
            Lifetime stats · time saved vs typing
          </h3>
        </div>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
          <span style={{ fontSize: 36, fontWeight: 800, lineHeight: 1, fontVariantNumeric: "tabular-nums" }}>
            {hasData ? impact.timeSavedLabel : "—"}
          </span>
          {hasData && faster != null && faster > 0 && (
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 4,
                background: "rgba(255,255,255,0.18)",
                borderRadius: 999,
                padding: "3px 10px",
                fontSize: 12,
                fontWeight: 700,
              }}
            >
              <Icon name="trending-up" size={13} color="#fff" />
              {faster}× faster
            </span>
          )}
        </div>
        <div style={{ fontSize: 12, opacity: 0.85, marginTop: 8 }}>
          Across {impact.totalWords.toLocaleString()} words all-time · you speak at {wpm ?? "—"} wpm
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 10, marginTop: 18 }}>
          <MiniStat icon="zap" value={wpm ? `${wpm}` : "—"} label="wpm" />
          <MiniStat icon="flame" value={`${impact.streakDays}`} label="day streak" />
        </div>

        <div style={{ marginTop: 18, paddingTop: 16, borderTop: "1px solid rgba(255,255,255,0.2)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 8 }}>
            <Icon name="target" size={14} color="#fff" />
            <span style={{ fontSize: 12, fontWeight: 700, opacity: 0.9 }}>Weekly goal</span>
            <span style={{ fontSize: 12, opacity: 0.85, marginLeft: "auto", fontVariantNumeric: "tabular-nums" }}>{weekPct}%</span>
          </div>
          <div style={{ height: 8, background: "rgba(255,255,255,0.25)", borderRadius: 4, overflow: "hidden" }}>
            <div style={{ height: "100%", width: `${weekPct}%`, background: "#fff", borderRadius: 4, transition: "width 0.5s ease" }} />
          </div>
          <div style={{ fontSize: 12, opacity: 0.85, marginTop: 8, fontVariantNumeric: "tabular-nums" }}>
            {impact.wordsLast7Days.toLocaleString()} / {goal.toLocaleString()} words this week
          </div>
        </div>
      </div>
    );
  }

  // Hero (Insights): the big blue gradient banner with the goal ring.
  return (
    <div
      style={{
        display: "flex",
        gap: 20,
        flexWrap: "wrap",
        background:
          "radial-gradient(130% 160% at 100% 0%, rgba(56,189,248,0.42) 0%, rgba(56,189,248,0) 46%)," +
          "radial-gradient(120% 150% at 0% 100%, rgba(139,92,246,0.36) 0%, rgba(139,92,246,0) 44%)," +
          "linear-gradient(135deg, #1e3a8a 0%, #2563eb 52%, #3b82f6 100%)",
        border: "1px solid rgba(255,255,255,0.08)",
        borderRadius: "var(--radius)",
        padding: "24px 26px",
        color: "#fff",
        boxShadow: "0 12px 40px -12px rgba(37,99,235,0.55), var(--shadow-3)",
        position: "relative",
        overflow: "hidden",
        ...style,
      }}
    >
      <div style={{ flex: "1 1 320px", display: "flex", flexDirection: "column" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, textTransform: "uppercase", letterSpacing: 0.5, opacity: 0.85 }}>
          <Icon name="zap" size={15} color="#fff" />
          Time saved vs typing
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 14, marginTop: 6 }}>
          <div style={{ fontSize: 42, fontWeight: 800, lineHeight: 1 }}>{hasData ? impact.timeSavedLabel : "—"}</div>
          {hasData && faster != null && faster > 0 && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 5,
                background: "rgba(255,255,255,0.18)",
                borderRadius: 999,
                padding: "5px 12px",
                fontSize: 13,
                fontWeight: 700,
              }}
            >
              <Icon name="trending-up" size={15} color="#fff" />
              {faster}× faster
            </div>
          )}
        </div>
        <div style={{ fontSize: 13, opacity: 0.9, marginTop: 8 }}>
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
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 22, fontWeight: 800 }}>
              {impact.totalWords.toLocaleString()}
            </div>
            <div style={{ fontSize: 12, opacity: 0.85 }}>total words</div>
          </div>
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 22, fontWeight: 800 }}>
              <Icon name="messages-square" size={16} color="rgba(255,255,255,0.9)" />
              <span>{impact.totalDictations.toLocaleString()}</span>
            </div>
            <div style={{ fontSize: 12, opacity: 0.85 }}>dictations</div>
          </div>
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 22, fontWeight: 800 }}>
              <Icon name="zap" size={16} color="rgba(255,255,255,0.9)" />
              {wpm ?? "—"}
            </div>
            <div style={{ fontSize: 12, opacity: 0.85 }}>wpm speaking</div>
          </div>
        </div>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 16, flex: "0 0 auto" }}>
        <Ring pct={weekPct / 100} size={128} stroke={12} color="#ffffff" track="rgba(255,255,255,0.25)">
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
            <Icon name="target" size={16} color="rgba(255,255,255,0.9)" />
            <div style={{ fontSize: 24, fontWeight: 800, marginTop: 2 }}>{weekPct}%</div>
            <div style={{ fontSize: 11, opacity: 0.85 }}>weekly goal</div>
          </div>
        </Ring>
        <div>
          <div style={{ fontSize: 13, opacity: 0.9 }}>{impact.wordsLast7Days.toLocaleString()} / {goal.toLocaleString()} words</div>
          <div style={{ fontSize: 12, opacity: 0.7, marginTop: 2 }}>last 7 days</div>
        </div>
      </div>
    </div>
  );
}

function MiniStat({ value, label, icon }: { value: string; label: string; icon: IconName }) {
  return (
    <div
      style={{
        background: "rgba(255,255,255,0.12)",
        borderRadius: 10,
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
          background: "rgba(255,255,255,0.18)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        <Icon name={icon} size={15} color="#fff" />
      </span>
      <div>
        <div style={{ fontSize: 22, fontWeight: 800, lineHeight: 1, fontVariantNumeric: "tabular-nums" }}>{value}</div>
        <div style={{ fontSize: 11, opacity: 0.85, marginTop: 2 }}>{label}</div>
      </div>
    </div>
  );
}

function Ring({
  pct,
  size = 120,
  stroke = 12,
  color = "#fff",
  track = "rgba(255,255,255,0.25)",
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
