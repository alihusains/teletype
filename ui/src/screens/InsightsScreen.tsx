import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";
import FixesCard from "../components/FixesCard";
import ImpactCard from "../components/ImpactCard";
import { useTauriEvent } from "../lib/useTauriEvent";

interface RankedItem {
  label: string;
  count: number;
  share: number;
}

interface Impact {
  /** null until a take long enough to time has been recorded. */
  wordsPerMinute: number | null;
  timesFaster: number | null;
  timeSavedMinutes: number;
  timeSavedLabel: string;
  /** null for the same reason as wordsPerMinute. */
  minutesSpoken: number | null;
  minutesTyped: number;
  essays: number;
  ratedTakes: number;
  skippedTakes: number;
}

interface Record {
  label: string;
  value: string;
}

interface Milestone {
  label: string;
  threshold: number;
  reached: boolean;
}

interface MilestoneRow {
  category: string;
  current: number;
  items: Milestone[];
}

interface HeatCell {
  date: number;
  words: number;
  level: number;
}

interface DayStat {
  label: string;
  date: number;
  words: number;
  dictations: number;
}

interface Insights {
  topPhrases: RankedItem[];
  topApps: RankedItem[];
  avgWordsPerDictation: number;
  busiestHour: number | null;
  vocabularySize: number;
  dictionaryWords: number;
  learnedWords: number;
  totalWords: number;
  totalDictations: number;
  streakDays: number;
  longestStreakDays: number;
  impact: Impact;
  records: Record[];
  milestones: MilestoneRow[];
  heatmap: HeatCell[];
  daily: DayStat[];
  wordsToday: number;
  wordsLast7Days: number;
  avgWordsPerDay: number;
  firstActivityAt: number | null;
  fillerRemoved: number;
  autotextExpansions: number;
  personalizationCorrections: number;
  dictionaryLearned: number;
  polishStatus: string | null;
}

const EMPTY: Insights = {
  topPhrases: [],
  topApps: [],
  avgWordsPerDictation: 0,
  busiestHour: null,
  vocabularySize: 0,
  dictionaryWords: 0,
  learnedWords: 0,
  totalWords: 0,
  totalDictations: 0,
  streakDays: 0,
  longestStreakDays: 0,
  impact: {
    wordsPerMinute: null,
    timesFaster: null,
    timeSavedMinutes: 0,
    timeSavedLabel: "0 min",
    minutesSpoken: null,
    minutesTyped: 0,
    essays: 0,
    ratedTakes: 0,
    skippedTakes: 0,
  },
  records: [],
  milestones: [],
  heatmap: [],
  daily: [],
  wordsToday: 0,
  wordsLast7Days: 0,
  avgWordsPerDay: 0,
  firstActivityAt: null,
  fillerRemoved: 0,
  autotextExpansions: 0,
  personalizationCorrections: 0,
  dictionaryLearned: 0,
  polishStatus: null,
};

const ACCENT = "#2563eb";
const ACCENT_HOVER = "#1d4ed8";
// Heatmap ramp: from the page background into the brand blue, so the
// "empty" cells read as part of the surface rather than a grey block.
const HEAT_COLORS = ["#eef1f6", "#cddcfb", "#9cbbfa", "#5e93f5", "#2563eb"];
const APP_COLORS = ["#2563eb", "#7c3aed", "#0891b2", "#ea580c", "#059669", "#db2777", "#4f46e5", "#ca8a04"];

function heatColor(level: number): string {
  return HEAT_COLORS[Math.max(0, Math.min(4, level))];
}

// Word-count tiers for the "you've written…" flourish. Each tier is a
// (min total words, singular, plural) — the app picks the highest tier the
// user has reached and expresses their word count in those units, so the
// praise keeps escalating (essay → article → blog post → short story →
// novella → book → collection) as they dictate more.
const WRITTEN_TIERS: { min: number; singular: string; plural: string }[] = [
  { min: 90000, singular: "book", plural: "books" },
  { min: 30000, singular: "novella", plural: "novellas" },
  { min: 12000, singular: "short story", plural: "short stories" },
  { min: 5000, singular: "blog post", plural: "blog posts" },
  { min: 1500, singular: "magazine article", plural: "magazine articles" },
  { min: 500, singular: "college essay", plural: "college essays" },
];

// "You've written 22 college essays!" — but the unit escalates with volume,
// so a heavy user reads "You've written 3 books!" instead of hundreds of essays.
function writtenTally(totalWords: number): string | null {
  let tier = WRITTEN_TIERS[WRITTEN_TIERS.length - 1];
  for (const t of WRITTEN_TIERS) {
    if (totalWords >= t.min) {
      tier = t;
      break;
    }
  }
  const n = Math.floor(totalWords / tier.min);
  if (n < 1) return null;
  return `You've written ${n.toLocaleString()} ${n === 1 ? tier.singular : tier.plural}!`;
}

function hourLabel(h: number | null): string {
  if (h === null) return "—";
  const ampm = h < 12 ? "AM" : "PM";
  const hr = h % 12 === 0 ? 12 : h % 12;
  return `${hr} ${ampm}`;
}

function Card({
  title,
  icon,
  children,
  style,
  span,
  action,
  section,
}: {
  title?: string;
  icon?: IconName;
  children: React.ReactNode;
  style?: React.CSSProperties;
  span?: number;
  action?: React.ReactNode;
  section?: string;
}) {
  return (
    <div
      style={{
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: "20px 22px",
        boxShadow: "var(--shadow-2)",
        gridColumn: span ? `span ${span}` : undefined,
        transition: "box-shadow var(--dur) var(--ease), transform var(--dur) var(--ease)",
        ...style,
      }}
    >
      {section && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: 11,
            fontWeight: 700,
            textTransform: "uppercase",
            letterSpacing: 0.9,
            color: "var(--text-tertiary)",
            marginBottom: 14,
          }}
        >
          <span style={{ width: 16, height: 2, borderRadius: 2, background: ACCENT, display: "inline-block" }} />
          {section}
        </div>
      )}
      {title && (
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 18 }}>
          <h3
            style={{
              display: "flex",
              alignItems: "center",
              gap: 9,
              fontSize: 15,
              fontWeight: 700,
              color: "var(--text)",
              letterSpacing: "var(--tracking-tight)",
              margin: 0,
            }}
          >
            {icon && (
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
            )}
            {title}
          </h3>
          {action}
        </div>
      )}
      {children}
    </div>
  );
}

function Sparkline({ values, width = 120, height = 40, color = ACCENT }: { values: number[]; width?: number; height?: number; color?: string }) {
  if (values.length < 2) return null;
  const max = Math.max(1, ...values);
  const step = width / (values.length - 1);
  const pts = values.map((v, i) => `${i * step},${height - (v / max) * (height - 4) - 2}`).join(" ");
  const areaPts = `0,${height} ${pts} ${width},${height}`;
  return (
    <svg width={width} height={height} style={{ display: "block" }}>
      <polygon points={areaPts} fill={color} opacity={0.12} />
      <polyline points={pts} fill="none" stroke={color} strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function Donut({ items, size = 140, stroke = 22 }: { items: { label: string; count: number }[]; size?: number; stroke?: number }) {
  const total = items.reduce((s, i) => s + i.count, 0);
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  let acc = 0;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 18, flexWrap: "wrap" }}>
      <div style={{ position: "relative", width: size, height: size, flexShrink: 0 }}>
        <svg width={size} height={size} style={{ transform: "rotate(-90deg)" }}>
          <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="#eef1f6" strokeWidth={stroke} />
          {total > 0 &&
            items.map((item, i) => {
              const frac = item.count / total;
              const seg = frac * c;
              const el = (
                <circle
                  key={i}
                  cx={size / 2}
                  cy={size / 2}
                  r={r}
                  fill="none"
                  stroke={APP_COLORS[i % APP_COLORS.length]}
                  strokeWidth={stroke}
                  strokeDasharray={`${seg} ${c - seg}`}
                  strokeDashoffset={-acc}
                />
              );
              acc += seg;
              return el;
            })}
        </svg>
        <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center" }}>
          <div style={{ fontSize: 22, fontWeight: 800 }}>{items.length}</div>
          <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>apps</div>
        </div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8, flex: 1, minWidth: 160 }}>
        {items.length === 0 && <p style={{ fontSize: 13, color: "var(--text-secondary)" }}>No app data yet.</p>}
        {items.map((item, i) => (
          <div key={item.label} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13 }}>
            <span style={{ width: 10, height: 10, borderRadius: 3, background: APP_COLORS[i % APP_COLORS.length], flexShrink: 0 }} />
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1 }}>{item.label}</span>
            <span style={{ color: "var(--text-secondary)", flexShrink: 0 }}>{total > 0 ? Math.round((item.count / total) * 100) : 0}%</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function BarChart({ daily }: { daily: DayStat[] }) {
  const max = Math.max(1, ...daily.map((d) => d.words));
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 6, height: 158, paddingTop: 4 }}>
      {daily.map((d) => (
        <div
          key={d.date}
          style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", gap: 7, height: "100%", justifyContent: "flex-end" }}
          title={`${d.label}: ${d.words} words, ${d.dictations} dictations`}
        >
          <div style={{ fontSize: 10, color: "var(--text-tertiary)", height: 12, fontVariantNumeric: "tabular-nums", fontWeight: 600 }}>{d.words > 0 ? d.words : ""}</div>
          <div
            style={{
              width: "100%",
              maxWidth: 30,
              height: `${Math.max(4, (d.words / max) * 100)}%`,
              borderRadius: 5,
              background: d.words > 0 ? "linear-gradient(180deg, #60a5fa 0%, #2563eb 100%)" : "var(--border-subtle)",
              transition: "height 0.3s",
            }}
          />
          <div style={{ fontSize: 10, color: "var(--text-tertiary)", whiteSpace: "nowrap", fontWeight: 500 }}>{d.label.split(" ")[0]}</div>
        </div>
      ))}
    </div>
  );
}

function Heatmap({ cells }: { cells: HeatCell[] }) {
  const weeks: HeatCell[][] = [];
  for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
  return (
    <div style={{ overflowX: "auto", paddingBottom: 4 }}>
      <div style={{ display: "flex", gap: 3 }}>
        {weeks.map((week, wi) => (
          <div key={wi} style={{ display: "flex", flexDirection: "column", gap: 3 }}>
            {week.map((cell) => (
              <div key={cell.date} title={`${cell.words} words`} style={{ width: 13, height: 13, borderRadius: 3, background: heatColor(cell.level) }} />
            ))}
          </div>
        ))}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 10, fontSize: 11, color: "var(--text-secondary)" }}>
        <span>Less</span>
        {HEAT_COLORS.map((c) => (
          <div key={c} style={{ width: 11, height: 11, borderRadius: 3, background: c }} />
        ))}
        <span>More</span>
      </div>
    </div>
  );
}

function StatCard({ label, value, sub, spark, icon }: { label: string; value: string; sub?: string; spark?: number[]; icon?: IconName }) {
  return (
    <div
      style={{
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: "16px 18px",
        boxShadow: "var(--shadow-1)",
        display: "flex",
        flexDirection: "column",
        gap: 10,
        transition: "box-shadow var(--dur) var(--ease), transform var(--dur) var(--ease)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        {icon && (
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
            <Icon name={icon} size={15} color={ACCENT} />
          </span>
        )}
        <div style={{ fontSize: 12, color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: 0.5, fontWeight: 600 }}>
          {label}
        </div>
      </div>
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 8 }}>
        <div style={{ fontSize: 30, fontWeight: 800, lineHeight: 1, fontVariantNumeric: "tabular-nums", letterSpacing: "var(--tracking-tight)" }}>{value}</div>
        {spark && spark.length >= 2 && <Sparkline values={spark} width={76} height={36} />}
      </div>
      {sub && <div style={{ fontSize: 12, color: "var(--text-tertiary)" }}>{sub}</div>}
    </div>
  );
}

type Range = "today" | "week" | "month" | "year" | "lifetime" | "custom";

const RANGES: { id: Range; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "week", label: "Week" },
  { id: "month", label: "Month" },
  { id: "year", label: "Year" },
  { id: "lifetime", label: "Lifetime" },
  { id: "custom", label: "Custom" },
];

/** epoch ms -> yyyy-mm-dd for <input type="date"> (local time). */
function toInputDate(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** yyyy-mm-dd -> epoch ms at local midnight; null when unset/invalid. */
function fromInputDate(s: string): number | null {
  if (!s) return null;
  const [y, m, d] = s.split("-").map(Number);
  if (!y || !m || !d) return null;
  return new Date(y, m - 1, d).getTime();
}

/** epoch ms -> "16 Sep" style label (local time). */
function shortDate(ms: number): string {
  const d = new Date(ms);
  const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

export default function InsightsScreen() {
  const [data, setData] = useState<Insights>(EMPTY);
  const [range, setRange] = useState<Range>("today");
  const [customFrom, setCustomFrom] = useState<string>("");
  const [customTo, setCustomTo] = useState<string>("");
  // First activity discovered from the backend; clamps the custom range.
  const [firstActivityAt, setFirstActivityAt] = useState<number | null>(null);

  const refresh = useCallback(() => {
    const from = fromInputDate(customFrom);
    const to = fromInputDate(customTo);
    invoke<Insights>("get_insights", {
      range,
      customFrom: range === "custom" ? from : null,
      customTo: range === "custom" ? to : null,
    }).then((d) => {
      setData(d);
      if (d.firstActivityAt != null) setFirstActivityAt(d.firstActivityAt);
    }).catch(console.error);
  }, [range, customFrom, customTo]);

  useEffect(refresh, [refresh]);

  // Refresh in real-time when a dictation completes.
  useTauriEvent<{ phase: string }>("dictation-state", ({ payload }) => {
    if (payload.phase === "idle") refresh();
  });

  // Re-fetch when the window becomes visible again (dictations/typed
  // expansions in other apps change the data while this window is hidden).
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onVisible);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onVisible);
    };
  }, [refresh]);

  const impact = data.impact;
  const hasData = data.totalWords > 0;
  // weekly-goal math now lives in the shared ImpactCard.
  const last7words = data.daily.slice(-7).map((d) => d.words);

  // Range-scoped usage figures come from the backend (recomputed from the
  // range-filtered history; the persisted counters are all-time only).
  const fillerRemoved = data.fillerRemoved;
  const autotextExpansions = data.autotextExpansions;
  const personalizationCorrections = data.personalizationCorrections;
  const dictionaryLearned = data.dictionaryLearned;

  const customMin = firstActivityAt ? toInputDate(firstActivityAt) : undefined;
  const customMax = toInputDate(Date.now());

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, width: "100%" }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div>
          <h2 style={{ fontSize: 26, fontWeight: 800, letterSpacing: "var(--tracking-tight)", margin: 0 }}>Insights</h2>
          <p style={{ color: "var(--text-secondary)", fontSize: 13.5, marginTop: 4 }}>
            Your impact, habit, and records — computed locally from your dictation history.
          </p>
        </div>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 4 }}>
          <div style={{ display: "flex", background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 999, padding: 4, boxShadow: "var(--shadow-1)" }}>
            {RANGES.map((r) => (
              <button
                key={r.id}
                onClick={() => setRange(r.id)}
                title="Sets the window for all insights below"
                style={{
                  border: "none",
                  background: range === r.id ? "var(--accent)" : "transparent",
                  color: range === r.id ? "#fff" : "var(--text-secondary)",
                  fontSize: 13,
                  fontWeight: 600,
                  padding: "6px 16px",
                  borderRadius: 999,
                  cursor: "pointer",
                  transition: "background var(--dur) var(--ease), color var(--dur) var(--ease)",
                }}
              >
                {r.label}
              </button>
            ))}
          </div>
          {range === "custom" ? (
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input
                type="date"
                value={customFrom}
                min={customMin}
                max={customMax}
                onChange={(e) => setCustomFrom(e.target.value)}
                aria-label="Custom range start"
                style={{
                  border: "1px solid var(--border)",
                  borderRadius: 6,
                  padding: "4px 8px",
                  fontSize: 12,
                  background: "var(--surface)",
                  color: "var(--text)",
                }}
              />
              <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>to</span>
              <input
                type="date"
                value={customTo}
                min={customMin}
                max={customMax}
                onChange={(e) => setCustomTo(e.target.value)}
                aria-label="Custom range end"
                style={{
                  border: "1px solid var(--border)",
                  borderRadius: 6,
                  padding: "4px 8px",
                  fontSize: 12,
                  background: "var(--surface)",
                  color: "var(--text)",
                }}
              />
              {firstActivityAt && (
                <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
                  (earliest activity {shortDate(firstActivityAt)})
                </span>
              )}
            </div>
          ) : (
            <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
              All insights are scoped to this window
            </span>
          )}
        </div>
      </div>

      {/* P1-16: polish status banner */}
      {data.polishStatus && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "10px 14px",
            borderRadius: 8,
            background: "var(--warning-soft, #fef3c7)",
            color: "var(--warning, #92400e)",
            fontSize: 13,
            fontWeight: 500,
          }}
        >
          <Icon name="info" size={16} color="var(--warning, #92400e)" />
          {data.polishStatus}
        </div>
      )}

      {/* Hero: impact + weekly goal ring (shared with Home so the numbers
          can never drift between the two screens). */}
      <ImpactCard
        variant="hero"
        hasData={hasData}
        footer={
          hasData && writtenTally(data.totalWords) ? (
            <div style={{ fontSize: 12, opacity: 0.8, marginTop: 6 }}>{writtenTally(data.totalWords)}</div>
          ) : null
        }
        impact={{
          wordsPerMinute: impact.wordsPerMinute,
          timesFaster: impact.timesFaster,
          timeSavedLabel: impact.timeSavedLabel,
          totalWords: data.totalWords,
          totalDictations: data.totalDictations,
          ratedTakes: impact.ratedTakes,
          streakDays: data.streakDays,
          wordsLast7Days: data.wordsLast7Days,
          avgWordsPerDay: data.avgWordsPerDay,
        }}
      />

      {/* Band 2 · Impact: what Teletype fixed for you (all-time) */}
      <FixesCard
        fillerRemoved={fillerRemoved}
        autotextExpansions={autotextExpansions}
        personalizationCorrections={personalizationCorrections}
        dictionaryLearned={dictionaryLearned}
      />

      {/* Band 3 · Activity: the day-to-day, scoped by the range control */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))", gap: 16 }}>
        <Card section="Activity" title="Words per day" icon="chart-column">
          {hasData ? (
            <BarChart daily={data.daily} />
          ) : (
            <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--text-secondary)" }}>
              <Icon name="chart-column" size={16} color="var(--text-secondary)" />
              No activity yet — your daily words will chart here.
            </p>
          )}
        </Card>
        <Card title="Your habit · last 26 weeks" icon="flame">
          <Heatmap cells={data.heatmap} />
        </Card>
      </div>

      {/* Band 4 · Habits: the numbers and patterns behind the activity */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 16 }}>
        <StatCard label="Words today" value={data.wordsToday.toLocaleString()} spark={last7words} icon="calendar-check" />
        <StatCard label="Avg / dictation" value={data.avgWordsPerDictation.toLocaleString()} icon="messages-square" />
        <StatCard label="Streak" value={`${data.streakDays}`} sub={`best ${data.longestStreakDays} days`} icon="flame" />
        <StatCard label="Busiest hour" value={hourLabel(data.busiestHour)} icon="clock" />
        <StatCard label="Vocabulary" value={data.vocabularySize.toLocaleString()} sub="distinct words" icon="library" />
        {(data.dictionaryWords ?? 0) > 0 && (
          <StatCard label="Dictionary" value={data.dictionaryWords.toLocaleString()} sub={`${data.learnedWords ?? 0} auto-learned`} icon="target" />
        )}
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))", gap: 16 }}>
        <Card title="Where you dictate" icon="messages-square">
          <Donut items={data.topApps} />
        </Card>
        <Card title="Top phrases" icon="sparkles">
          {data.topPhrases.length === 0 ? (
            <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--text-secondary)" }}>
              <Icon name="sparkles" size={16} color="var(--text-secondary)" />
              Nothing here yet — dictate a few times and patterns will appear.
            </p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {data.topPhrases.map((item) => (
                <div key={item.label}>
                  <div style={{ display: "flex", justifyContent: "space-between", fontSize: 13, marginBottom: 5 }}>
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{item.label}</span>
                    <span style={{ color: "var(--text-secondary)", flexShrink: 0, marginLeft: 8, fontVariantNumeric: "tabular-nums", fontWeight: 600 }}>{item.count}×</span>
                  </div>
                  <div style={{ height: 8, background: "var(--surface-2)", borderRadius: 4 }}>
                    <div style={{ height: "100%", width: `${Math.round(item.share * 100)}%`, background: "linear-gradient(90deg, #60a5fa 0%, #2563eb 100%)", borderRadius: 4 }} />
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>

      {/* Band 5 · Achievements: records + milestones */}
      <Card section="Achievements" title="Your records" icon="trophy">
        {!hasData ? (
          <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--text-secondary)" }}>
            <Icon name="trophy" size={16} color="var(--text-secondary)" />
            No records yet — your best dictations will show up here.
          </p>
        ) : (
          <div style={{ display: "flex", gap: 14, flexWrap: "wrap" }}>
            {data.records.map((r) => (
              <div key={r.label} style={{ flex: 1, minWidth: 160, display: "flex", alignItems: "center", gap: 10 }}>
                <div style={{ width: 34, height: 34, borderRadius: 9, background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
                  <Icon name="trophy" size={18} color={ACCENT_HOVER} />
                </div>
                <div>
                  <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>{r.label}</div>
                  <div style={{ fontSize: 20, fontWeight: 800, marginTop: 1, fontVariantNumeric: "tabular-nums", letterSpacing: "var(--tracking-tight)" }}>{r.value}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>

      <Card title="Milestones" icon="medal">
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          {data.milestones.map((row) => (
            <div key={row.category}>
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>
                {row.category}
                <span style={{ color: "var(--text-secondary)", fontWeight: 400, marginLeft: 8 }}>{row.current.toLocaleString()}</span>
              </div>
              <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                {row.items.map((m) => (
                  <div
                    key={m.label}
                    style={{
                      padding: "4px 12px",
                      borderRadius: 999,
                      fontSize: 12,
                      fontWeight: 600,
                      background: m.reached ? "var(--accent-soft)" : "var(--surface-2)",
                      color: m.reached ? ACCENT_HOVER : "var(--text-secondary)",
                      border: `1px solid ${m.reached ? ACCENT : "var(--border)"}`,
                    }}
                  >
                    {m.reached ? "✓ " : ""}
                    {m.label}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}
