import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";
import { LatencyPanel, type LatencySummary } from "../components/LatencyPanel";
import { errorMessage } from "../lib/errors";
import { useTauriEvent } from "../lib/useTauriEvent";

interface RankedItem {
  label: string;
  count: number;
  /** Share of the *most used* item, not of the total. */
  share: number;
}

interface HabitWords {
  total: number;
  per1kWords: number;
  items: RankedItem[];
  /** -1 = decreasing, 0 = stable, 1 = increasing. */
  trend: number;
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

/** A mishearing the user corrected, and how often. */
interface TrackedFix {
  from: string;
  to: string;
  count: number;
}

interface LearnedTerm {
  term: string;
  aliases?: string[];
  learnedAt?: number;
}

/** One day of edit activity from the learning store. */
export interface LearningDay {
  /** UTC day start in ms. */
  day: number;
  words: number;
  editedWords: number;
}

/** One replay run's error rate before and after the personalization boost. */
export interface AccuracyPoint {
  ts: number;
  /** Word error rate as a 0..1 fraction. Lower is better. */
  werBefore: number;
  werAfter: number;
}

interface LearningSummary {
  topFixes?: TrackedFix[];
  termsLearnedWeek?: LearnedTerm[];
  /** Edited words per 100 dictated words over the last 7 days. */
  editRate7d?: number | null;
  /** Last 30 days of edit activity, oldest first. */
  daily?: LearningDay[];
  /** Oldest first, at most 10 points. Empty until a replay run is stored. */
  accuracy?: AccuracyPoint[];
  /**
   * Polish corrections the user kept: the corrected form was still in the
   * field 30 minutes after it landed. Survival, not a click.
   */
  keptCorrections?: number;
  /** The same corrections the user undid or wrote over. */
  undoneCorrections?: number;
}

/**
 * Exported so test fixtures and the dev preview can be typed against it: an
 * unannotated hand-written mock drifts silently and fails at render time
 * instead of at `tsc --noEmit`.
 */
export interface Insights {
  topPhrases: RankedItem[];
  topApps: RankedItem[];
  /** Dictations per detected app category (Email, Chat, Code …). */
  appCategories: RankedItem[];
  avgWordsPerDictation: number;
  busiestHour: number | null;
  topDays: RankedItem[];
  bestDay: string | null;
  hourBands: RankedItem[];
  topWords: RankedItem[];
  habitWords: HabitWords;
  vocabularySize: number;
  dictionaryWords: number;
  learnedWords: number;
  totalWords: number;
  totalDictations: number;
  streakDays: number;
  longestStreakDays: number;
  /** Days with at least one dictation in the last 7 days (0..=7). */
  activeDaysLast7: number;
  /** Words in the equal-length window before the selected one; null for lifetime. */
  priorWindowWords: number | null;
  /** Percent change vs the prior window, 1 decimal; null when not comparable. */
  wordsChangePct: number | null;
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
  /** All-time AutoText trigger/snippet usage, from persisted counters. */
  topAutotext: RankedItem[];
  /** All-time filler words removed, from persisted counters. */
  topFillers: RankedItem[];
  personalizationCorrections: number;
  dictionaryLearned: number;
  polishStatus: string | null;
  /** The backend sends an all-empty object when there is no learning store;
      the dev preview and the test harness send null. Both are "nothing yet". */
  learning?: LearningSummary | null;
}

const EMPTY: Insights = {
  topPhrases: [],
  topApps: [],
  appCategories: [],
  avgWordsPerDictation: 0,
  busiestHour: null,
  topDays: [],
  bestDay: null,
  hourBands: [],
  topWords: [],
  habitWords: { total: 0, per1kWords: 0, items: [], trend: 0 },
  vocabularySize: 0,
  dictionaryWords: 0,
  learnedWords: 0,
  totalWords: 0,
  totalDictations: 0,
  streakDays: 0,
  longestStreakDays: 0,
  activeDaysLast7: 0,
  priorWindowWords: null,
  wordsChangePct: null,
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
  topAutotext: [],
  topFillers: [],
  personalizationCorrections: 0,
  dictionaryLearned: 0,
  polishStatus: null,
  learning: null,
};

/** crates/teletype-core/src/insights.rs TYPING_WPM — the baseline every
    "times faster"/"time saved" figure is measured against. */
const TYPING_WPM = 40;
/** crates/teletype-core/src/insights.rs MAX_ITEMS — the backend truncates the
    ranked lists to this many entries, so the UI must not imply it sees more. */
const RANK_CAP = 8;

// Heatmap ramp: the accent mixed into the card surface, so "empty" cells stay
// part of the surface while every step remains visible in both themes.
const HEAT_RAMP = [
  "color-mix(in srgb, var(--accent) 10%, var(--surface))",
  "color-mix(in srgb, var(--accent) 26%, var(--surface))",
  "color-mix(in srgb, var(--accent) 46%, var(--surface))",
  "color-mix(in srgb, var(--accent) 72%, var(--surface))",
  "var(--accent)",
];

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const NUMERIC: React.CSSProperties = {
  fontVariantNumeric: "tabular-nums",
  whiteSpace: "nowrap",
};

/** Every grid on this screen collapses to one column rather than forcing a
    horizontal scrollbar. */
const CARD_GRID: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(min(300px, 100%), 1fr))",
  gap: 16,
  width: "100%",
  minWidth: 0,
};

const TILE_GRID: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(min(190px, 100%), 1fr))",
  gap: 12,
  width: "100%",
  minWidth: 0,
};

/* ------------------------------------------------------------------ *
 * Pure helpers
 * ------------------------------------------------------------------ */

export function formatMinutes(total: number): string {
  const n = Math.max(0, Math.round(total));
  if (n < 60) return `${n} min`;
  const hr = Math.floor(n / 60);
  const min = n % 60;
  return min === 0 ? `${hr} hr` : `${hr} hr ${min} min`;
}

export interface SpokenImpact {
  minutesSpoken: number | null;
  minutesTyped: number;
  timeSavedMinutes: number;
}

/** Speaking time: measured when a take was long enough to time, otherwise the
    difference the backend already used (its 150 wpm fallback) is surfaced as
    an estimate instead of silently pretending it was measured. */
export function spokenMinutes(impact: SpokenImpact): { minutes: number; estimated: boolean } {
  if (impact.minutesSpoken != null) return { minutes: impact.minutesSpoken, estimated: false };
  return {
    minutes: Math.max(0, impact.minutesTyped - impact.timeSavedMinutes),
    estimated: impact.minutesTyped > 0,
  };
}

/** The widest of the two bars, plus breathing room, so the shorter bar never
    fills the track and implies it is the maximum. */
export function speedScale(wpm: number | null): number {
  return Math.max(TYPING_WPM, wpm ?? 0) * 1.08;
}

export type Range = "week" | "month" | "year" | "lifetime" | "custom";

const RANGES: { id: Range; label: string }[] = [
  { id: "week", label: "7 days" },
  { id: "month", label: "30 days" },
  { id: "year", label: "12 months" },
  { id: "lifetime", label: "All time" },
  { id: "custom", label: "Custom" },
];

export function rangeWindowLabel(range: Range): string {
  switch (range) {
    case "week":
      return "the last 7 days";
    case "month":
      return "the last 30 days";
    case "year":
      return "the last 12 months";
    case "lifetime":
      return "all time";
    case "custom":
      return "the selected range";
  }
}

/** The same window phrased to follow "learned…". */
export function rangeLearnedLabel(range: Range): string {
  switch (range) {
    case "week":
      return "in the last 7 days";
    case "month":
      return "in the last 30 days";
    case "year":
      return "in the last 12 months";
    case "lifetime":
      return "all time";
    case "custom":
      return "in this range";
  }
}

/** The same window in the shortest form that fits a tile label. */
export function rangeShortLabel(range: Range): string {
  switch (range) {
    case "week":
      return "7 days";
    case "month":
      return "30 days";
    case "year":
      return "12 months";
    case "lifetime":
      return "all time";
    case "custom":
      return "custom";
  }
}

export function statFontSize(value: string): number {
  const n = value.length;
  if (n <= 7) return 30;
  if (n <= 10) return 24;
  if (n <= 13) return 20;
  return 17;
}

export function bucketDaily(daily: DayStat[], maxBars: number): DayStat[] {
  if (daily.length <= maxBars) return daily;
  const size = Math.ceil(daily.length / maxBars);
  const out: DayStat[] = [];
  for (let i = 0; i < daily.length; i += size) {
    const chunk = daily.slice(i, i + size);
    out.push({
      label: chunk[0].label,
      date: chunk[0].date,
      words: chunk.reduce((s, d) => s + d.words, 0),
      dictations: chunk.reduce((s, d) => s + d.dictations, 0),
    });
  }
  return out;
}

/** How the selected window compares with the one before it. */
export function changeLabel(pct: number | null): string {
  if (pct == null) return "No earlier window to compare";
  const abs = Math.abs(pct);
  if (abs < 0.5) return "Flat on the previous window";
  return `${pct > 0 ? "Up" : "Down"} ${abs.toFixed(abs < 10 ? 1 : 0)}% on the previous window`;
}

/** Edit rate per 100 dictated words, oldest first, days without words dropped.
    Falling is improvement: the transcript needed less fixing. */
export function editRateSeries(days: LearningDay[] | undefined, limit = 14): { day: number; rate: number }[] {
  if (!days) return [];
  return days
    .filter((d) => d.words > 0)
    .slice(-limit)
    .map((d) => ({ day: d.day, rate: Math.max(0, (d.editedWords / d.words) * 100) }));
}

/** The most recent replay run's error rate, as percentages of words wrong. */
export function accuracyTrend(
  points: AccuracyPoint[] | undefined
): { before: number; after: number; deltaPts: number; points: number } | null {
  if (!points || points.length === 0) return null;
  const last = points[points.length - 1];
  return {
    before: last.werBefore * 100,
    after: last.werAfter * 100,
    deltaPts: (last.werAfter - last.werBefore) * 100,
    points: points.length,
  };
}

// Word-count tiers for the "you've written…" flourish, used only in the
// lifetime view. The unit escalates with volume so a heavy user reads
// "3 books" rather than hundreds of essays.
const WRITTEN_TIERS: { min: number; singular: string; plural: string }[] = [
  { min: 90000, singular: "book", plural: "books" },
  { min: 30000, singular: "novella", plural: "novellas" },
  { min: 12000, singular: "short story", plural: "short stories" },
  { min: 5000, singular: "blog post", plural: "blog posts" },
  { min: 1500, singular: "magazine article", plural: "magazine articles" },
  { min: 500, singular: "college essay", plural: "college essays" },
];

export function writtenTally(totalWords: number): string | null {
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

function plural(n: number, one: string, many: string): string {
  return n === 1 ? one : many;
}

function pctOf(part: number, total: number): number {
  return total > 0 ? Math.round((part / total) * 100) : 0;
}

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
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

/* ------------------------------------------------------------------ *
 * Primitives
 * ------------------------------------------------------------------ */

function Hairline({ style }: { style?: React.CSSProperties }) {
  return (
    <div
      style={{
        height: 1,
        background: "var(--border-subtle)",
        margin: "14px 0",
        minWidth: 0,
        ...style,
      }}
    />
  );
}

/** A small "?" disc for a number whose counting rule is easy to over-read.
    The exact rule lives in the native tooltip. */
function InfoMark({ text }: { text: string }) {
  return (
    <span
      title={text}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: 15,
        height: 15,
        borderRadius: 999,
        border: "1px solid var(--border)",
        color: "var(--text-tertiary)",
        cursor: "help",
        flexShrink: 0,
      }}
    >
      <Icon name="info" size={10} color="var(--text-tertiary)" />
    </span>
  );
}

function Card({
  title,
  icon,
  children,
  action,
  quiet,
}: {
  title?: string;
  icon?: IconName;
  children: React.ReactNode;
  action?: React.ReactNode;
  quiet?: boolean;
}) {
  return (
    <div
      className="tt-card"
      style={{
        padding: quiet ? "16px 18px" : "20px 22px",
        boxShadow: quiet ? "var(--shadow-1)" : "var(--shadow-2)",
        minWidth: 0,
      }}
    >
      {title && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 10,
            flexWrap: "wrap",
            marginBottom: quiet ? 12 : 16,
            minWidth: 0,
          }}
        >
          <h3
            style={{
              display: "flex",
              alignItems: "center",
              gap: 9,
              fontSize: quiet ? "var(--text-md)" : "var(--text-lg)",
              fontWeight: quiet ? 600 : 650,
              color: "var(--text)",
              letterSpacing: "var(--tracking-tight)",
              margin: 0,
              minWidth: 0,
            }}
          >
            {icon && !quiet && (
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
                <Icon name={icon} size={15} color="var(--accent)" />
              </span>
            )}
            <span style={{ minWidth: 0 }}>{title}</span>
          </h3>
          {action}
        </div>
      )}
      {children}
    </div>
  );
}

/** Right-aligned "LABEL | value" meta in a card header. */
function CardMeta({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <span
      className="tt-eyebrow"
      title={title}
      style={{ display: "inline-flex", alignItems: "baseline", gap: 6, whiteSpace: "nowrap", flexShrink: 0, marginLeft: "auto" }}
    >
      <span>{label}</span>
      <span style={{ color: "var(--text)" }}>|</span>
      <span style={{ color: "var(--text)", ...NUMERIC }}>{value}</span>
    </span>
  );
}

/** Big number + uppercase label: the headline unit every card shares. */
function Headline({
  value,
  label,
  labelInfo,
  action,
}: {
  value: string;
  label: string;
  labelInfo?: string;
  action?: React.ReactNode;
}) {
  return (
    <div style={{ minWidth: 0 }}>
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 10,
          flexWrap: "wrap",
          minWidth: 0,
        }}
      >
        <span
          style={{
            fontSize: statFontSize(value),
            fontWeight: 650,
            lineHeight: 1.05,
            letterSpacing: "var(--tracking-display)",
            color: "var(--text)",
            minWidth: 0,
            ...NUMERIC,
          }}
        >
          {value}
        </span>
        {action}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 6, minWidth: 0 }}>
        <span className="tt-eyebrow" style={{ minWidth: 0 }}>
          {label}
        </span>
        {labelInfo && <InfoMark text={labelInfo} />}
      </div>
    </div>
  );
}

function Cta({ icon, children, onClick }: { icon: IconName; children: React.ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      className="tt-btn tt-btn-ghost tt-btn-sm"
      onClick={onClick}
      style={{
        border: "1px solid var(--border-subtle)",
        background: "var(--surface)",
        color: "var(--text)",
      }}
    >
      <Icon name={icon} size={14} color="var(--text-secondary)" />
      {children}
    </button>
  );
}

/** A labelled counter row with its scope stated next to the label, so an
    all-time total is never mistaken for a windowed one. */
function MetricRow({
  icon,
  label,
  scope,
  value,
  info,
}: {
  icon: IconName;
  label: string;
  scope: string;
  value: string;
  info: string;
}) {
  return (
    <div
      title={info}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "9px 0",
        borderTop: "1px solid var(--border-subtle)",
        minWidth: 0,
      }}
    >
      <span
        style={{
          width: 26,
          height: 26,
          borderRadius: 8,
          background: "var(--surface-2)",
          display: "grid",
          placeItems: "center",
          flexShrink: 0,
        }}
      >
        <Icon name={icon} size={13} color="var(--text-secondary)" />
      </span>
      <div style={{ flex: "1 1 0%", minWidth: 0 }}>
        <div style={{ fontSize: "var(--text-sm)", color: "var(--text)", overflowWrap: "anywhere" }}>{label}</div>
        <div style={{ fontSize: 11, color: "var(--text-tertiary)" }}>{scope}</div>
      </div>
      <span style={{ ...NUMERIC, fontWeight: 650, fontSize: "var(--text-md)" }}>{value}</span>
    </div>
  );
}

/** A section divider: eyebrow label, rule, optional scope hint. Every group
    of insights is named so nothing hides in one undifferentiated scroll. */
function SectionHeader({ title, hint }: { title: string; hint?: string }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0, marginTop: 4 }}>
      <span className="tt-eyebrow" style={{ flexShrink: 0 }}>{title}</span>
      <span style={{ flex: 1, height: 1, background: "var(--border-subtle)", minWidth: 0 }} />
      {hint && (
        <span
          style={{
            fontSize: "var(--text-xs)",
            color: "var(--text-tertiary)",
            flexShrink: 1,
            minWidth: 0,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {hint}
        </span>
      )}
    </div>
  );
}

/** One glance tile: label, headline figure, and a one-line qualifier. */
function Tile({
  icon,
  label,
  value,
  sub,
  tone,
  info,
}: {
  icon: IconName;
  label: string;
  value: string;
  sub: string;
  tone?: "up" | "down" | "flat";
  info: string;
}) {
  const toneColor =
    tone === "up" ? "var(--success)" : tone === "down" ? "var(--warning)" : "var(--text-tertiary)";
  return (
    <div
      className="tt-card"
      title={info}
      style={{
        padding: "16px 18px",
        boxShadow: "var(--shadow-1)",
        display: "flex",
        flexDirection: "column",
        gap: 8,
        minWidth: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
        <Icon name={icon} size={13} color="var(--text-tertiary)" />
        <span className="tt-eyebrow" style={{ minWidth: 0, overflowWrap: "anywhere" }}>
          {label}
        </span>
      </div>
      <div
        style={{
          fontSize: statFontSize(value),
          fontWeight: 650,
          lineHeight: 1.05,
          letterSpacing: "var(--tracking-display)",
          color: "var(--text)",
          ...NUMERIC,
        }}
      >
        {value}
      </div>
      <div
        style={{
          fontSize: "var(--text-xs)",
          color: toneColor,
          lineHeight: 1.4,
          minWidth: 0,
          overflowWrap: "anywhere",
        }}
      >
        {sub}
      </div>
    </div>
  );
}

/** A compact horizontal bar list used for categories, weekdays and time
    bands. Bars are scaled to the most used item, never to the total. */
function RankedBars({
  items,
  empty,
  unit,
}: {
  items: RankedItem[];
  empty: string;
  unit: string;
}) {
  if (items.length === 0) {
    return (
      <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
        {empty}
      </p>
    );
  }
  const top = Math.max(1, ...items.map((i) => i.count));
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 9, minWidth: 0 }}>
      {items.map((item) => (
        <div key={item.label} style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
          <span
            style={{
              fontSize: "var(--text-xs)",
              color: "var(--text-secondary)",
              width: 84,
              flexShrink: 0,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={item.label}
          >
            {item.label}
          </span>
          <div style={{ flex: "1 1 0%", height: 8, background: "var(--surface-2)", borderRadius: 4, minWidth: 0 }}>
            <div
              style={{
                height: "100%",
                width: `${Math.round(Math.max(item.count > 0 ? 3 : 0, (item.count / top) * 100))}%`,
                background: "var(--accent)",
                borderRadius: 4,
              }}
            />
          </div>
          <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", fontWeight: 600, flexShrink: 0, ...NUMERIC }}>
            {item.count.toLocaleString()}
            <span style={{ color: "var(--text-tertiary)", fontWeight: 400 }}> {unit}</span>
          </span>
        </div>
      ))}
    </div>
  );
}

/** Tiny bars for a short series (edit rate). Height is relative to the
    series' own maximum, so a single spike does not flatten the rest. */
function MiniBars({ items }: { items: { day: number; rate: number }[] }) {
  const max = Math.max(0.1, ...items.map((i) => i.rate));
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 3, height: 38, minWidth: 0 }} aria-hidden="true">
      {items.map((item) => (
        <div
          key={item.day}
          title={`${shortDate(item.day)}: ${item.rate.toFixed(1)} edits per 100 words`}
          style={{
            flex: 1,
            minWidth: 0,
            height: `${Math.max(8, (item.rate / max) * 100)}%`,
            borderRadius: 2,
            background: "color-mix(in srgb, var(--accent) 60%, var(--surface-2))",
          }}
        />
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ *
 * Visuals
 * ------------------------------------------------------------------ */

function heatColor(level: number): string {
  return HEAT_RAMP[Math.max(0, Math.min(4, level))];
}

/** 26 weeks of daily activity, one square per day. Cells are placed
    explicitly by weekday row, so the grid is always 7 rows tall and the
    columns are real calendar weeks (auto-flow would fill columns first and
    produce a staircase for the leading partial week). */
function Heatmap({ cells }: { cells: HeatCell[] }) {
  if (cells.length === 0) {
    return (
      <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
        No activity yet — the grid fills in one square per day.
      </p>
    );
  }
  const gap = 3;
  const lead = new Date(cells[0].date).getDay();
  const weeks = Math.max(1, Math.ceil((lead + cells.length) / 7));
  const tracks: React.CSSProperties = {
    display: "grid",
    gridTemplateColumns: `repeat(${weeks}, minmax(0, 1fr))`,
    columnGap: gap,
    minWidth: 0,
    width: "100%",
  };

  const squares = cells.map((c, i) => {
    const idx = lead + i;
    const d = new Date(c.date);
    const label = `${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()}`;
    return (
      <div
        key={c.date}
        title={`${label}: ${c.words.toLocaleString()} words`}
        style={{
          gridColumn: Math.floor(idx / 7) + 1,
          gridRow: (idx % 7) + 1,
          aspectRatio: "1",
          width: "100%",
          minWidth: 0,
          borderRadius: 2,
          background: heatColor(c.level),
          boxShadow: c.level === 0 ? "inset 0 0 0 1px var(--border)" : undefined,
        }}
      />
    );
  });

  const labels: React.ReactNode[] = [];
  let lastMonth = -1;
  for (let w = 0; w < weeks; w++) {
    const firstIndex = Math.max(0, w * 7 - lead);
    const month = new Date(cells[Math.min(firstIndex, cells.length - 1)].date).getMonth();
    const show = month !== lastMonth && weeks - w >= 2;
    labels.push(
      <div
        key={`m-${w}`}
        style={{
          fontSize: "var(--text-xs)",
          color: "var(--text-tertiary)",
          whiteSpace: "nowrap",
          overflow: "visible",
          minWidth: 0,
        }}
      >
        {show ? MONTHS[month] : ""}
      </div>
    );
    lastMonth = month;
  }

  return (
    <div style={{ minWidth: 0, width: "100%" }}>
      <div style={{ ...tracks, marginBottom: 4 }} aria-hidden="true">
        {labels}
      </div>
      <div style={{ ...tracks, rowGap: gap }}>{squares}</div>
      <div style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 10, fontSize: "var(--text-xs)", color: "var(--text-secondary)", flexWrap: "wrap", minWidth: 0 }}>
        <span>Less</span>
        {HEAT_RAMP.map((c, i) => (
          <div
            key={c}
            title={["0 words", "1-49 words", "50-149 words", "150-399 words", "400+ words"][i]}
            style={{
              width: 11,
              height: 11,
              borderRadius: 3,
              background: c,
              boxShadow: i === 0 ? "inset 0 0 0 1px var(--border)" : undefined,
            }}
          />
        ))}
        <span>More</span>
        <span style={{ color: "var(--text-tertiary)", marginLeft: "auto", whiteSpace: "nowrap" }}>words per day</span>
      </div>
    </div>
  );
}

function BarChart({ daily }: { daily: DayStat[] }) {
  const bars = bucketDaily(daily, 22);
  const showNumbers = bars.length <= 12;
  const max = Math.max(1, ...bars.map((d) => d.words));
  const monthOf = (d: DayStat) => d.label.split(" ")[0];
  // Label a month only at its first bucket and only when at least three bars
  // separate it from the previous label. A bucket that straddles a month
  // boundary can sit one bar after the previous label, and at narrow widths
  // the two would collide ("JulAug").
  const monthStarts = new Set<number>();
  let shownMonth = "";
  let shownIndex = -99;
  bars.forEach((d, i) => {
    const m = monthOf(d);
    if (m !== shownMonth && i - shownIndex >= 3) {
      monthStarts.add(d.date);
      shownMonth = m;
      shownIndex = i;
    }
  });
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 6, height: 150, paddingTop: 4, width: "100%", minWidth: 0 }}>
      {bars.map((d) => (
        <div
          key={d.date}
          style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", alignItems: "center", gap: 7, height: "100%", justifyContent: "flex-end" }}
          title={`${d.label}: ${d.words} words, ${d.dictations} dictations`}
        >
          <div style={{ fontSize: "var(--text-xs)", height: 14, color: "var(--text-tertiary)", fontWeight: 600, ...NUMERIC }}>
            {showNumbers && d.words > 0 ? d.words : ""}
          </div>
          <div
            style={{
              width: "100%",
              maxWidth: 30,
              height: `${Math.max(4, (d.words / max) * 100)}%`,
              borderRadius: 5,
              background: d.words > 0 ? "var(--accent)" : "var(--border-subtle)",
              transition: "height var(--dur) var(--ease)",
            }}
          />
          <div style={{ position: "relative", height: 14, width: "100%" }}>
            {monthStarts.has(d.date) && (
              <span
                style={{
                  position: "absolute",
                  left: "50%",
                  transform: "translateX(-50%)",
                  whiteSpace: "nowrap",
                  fontSize: "var(--text-xs)",
                  color: "var(--text-tertiary)",
                  fontWeight: 500,
                }}
              >
                {monthOf(d)}
              </span>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

/** One bar of the speed comparison. `unmeasured` renders the empty state in
    place, so the two rows always line up. */
function SpeedBar({
  label,
  value,
  pct,
  accent,
  unmeasured,
}: {
  label: string;
  value: string;
  pct: number;
  accent?: boolean;
  unmeasured?: boolean;
}) {
  return (
    <div style={{ minWidth: 0 }}>
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 8, marginBottom: 5, minWidth: 0 }}>
        <span style={{ fontSize: "var(--text-sm)", fontWeight: 600, color: "var(--text-secondary)", minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {label}
        </span>
        <span style={{ fontSize: "var(--text-sm)", fontWeight: 700, color: accent ? "var(--accent)" : "var(--text)", ...NUMERIC }}>{value}</span>
      </div>
      <div
        role={unmeasured ? undefined : "img"}
        aria-label={unmeasured ? undefined : `${label}: ${value}`}
        style={{
          height: 16,
          borderRadius: 999,
          background: unmeasured ? "transparent" : "var(--surface-2)",
          border: unmeasured ? "1px dashed var(--border)" : "none",
          overflow: "hidden",
          display: "flex",
          alignItems: "center",
          padding: unmeasured ? "0 8px" : 0,
          minWidth: 0,
        }}
      >
        {unmeasured ? (
          <span style={{ fontSize: 10, color: "var(--text-tertiary)", whiteSpace: "nowrap" }}>Not measured yet</span>
        ) : (
          <div
            style={{
              width: `${Math.max(accent ? 2 : 0, Math.min(100, pct))}%`,
              height: "100%",
              borderRadius: 999,
              background: accent ? "var(--accent)" : "color-mix(in srgb, var(--text-tertiary) 55%, var(--surface-2))",
              transition: "width 0.4s var(--ease)",
            }}
          />
        )}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ *
 * The proof
 * ------------------------------------------------------------------ */

function Hero({
  data,
  loaded,
  range,
  onSeeAllTime,
}: {
  data: Insights;
  loaded: boolean;
  range: Range;
  onSeeAllTime: () => void;
}) {
  const impact = data.impact;
  const hasWords = data.totalWords > 0;
  const wpm = impact.wordsPerMinute;
  const scale = speedScale(wpm);
  const windowLabel = rangeWindowLabel(range);
  const measured = loaded && hasWords && wpm != null && impact.timesFaster != null;
  const spoken = spokenMinutes(impact);

  const headingInfo = `Words you dictated at your ${wpm == null ? "estimated" : "measured"} pace, versus the same words typed at the ${TYPING_WPM} wpm average. The typing pace is a stated baseline, not a measurement of you.`;

  return (
    <section
      aria-label="How much faster dictation is than typing"
      className="tt-card"
      style={{
        padding: "32px 34px 28px",
        background: "var(--surface)",
        border: "1px solid var(--border)",
        boxShadow: "var(--shadow-3)",
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
      }}
    >
      <span className="tt-eyebrow" style={{ color: "var(--text-tertiary)", minWidth: 0 }}>{windowLabel}</span>

      <div style={{ display: "flex", alignItems: "baseline", gap: 12, flexWrap: "wrap", marginTop: 10, minWidth: 0 }}>
        <span
          aria-live="polite"
          title={headingInfo}
          style={{
            fontFamily: "var(--font-display)",
            fontSize: measured ? "clamp(58px, 8.4vw, 96px)" : "clamp(38px, 4.4vw, 52px)",
            fontWeight: 600,
            lineHeight: 0.92,
            letterSpacing: "var(--tracking-display)",
            color: measured ? "var(--text)" : "var(--text-tertiary)",
            whiteSpace: "nowrap",
            ...NUMERIC,
          }}
        >
          {measured ? `${impact.timesFaster}×` : "—"}
        </span>
        <span
          style={{
            fontFamily: "var(--font-display)",
            fontSize: "clamp(19px, 2.1vw, 25px)",
            fontWeight: 600,
            lineHeight: 1.15,
            letterSpacing: "-0.03em",
            color: "var(--text)",
            minWidth: 0,
          }}
        >
          faster than typing
        </span>
      </div>

      {loaded && hasWords ? (
        <p style={{ margin: "16px 0 0", fontSize: "var(--text-md)", color: "var(--text-secondary)", lineHeight: 1.6, maxWidth: 620, minWidth: 0, overflowWrap: "anywhere" }}>
          {data.totalWords.toLocaleString()} words dictated,{" "}
          <b style={{ color: "var(--text)", ...NUMERIC }}>{formatMinutes(impact.timeSavedMinutes)}</b> saved
          {wpm != null ? (
            <>
              {" "}
              versus the {TYPING_WPM} wpm typing baseline.
            </>
          ) : (
            " against the 150 wpm speaking estimate."
          )}
        </p>
      ) : (
        <p style={{ margin: "16px 0 0", fontSize: "var(--text-md)", color: "var(--text-secondary)", lineHeight: 1.6, maxWidth: 620, minWidth: 0, overflowWrap: "anywhere" }}>
          {!loaded
            ? "Reading your history from this Mac…"
            : <>
                No dictations in {windowLabel}.{" "}
                {range !== "lifetime" && (
                  <button
                    type="button"
                    onClick={onSeeAllTime}
                    style={{ background: "none", border: "none", padding: 0, color: "var(--accent)", fontWeight: 600, cursor: "pointer", fontSize: "inherit" }}
                  >
                    See all time
                  </button>
                )}
              </>}
        </p>
      )}

      {loaded && hasWords && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12, marginTop: 22, minWidth: 0 }}>
          <SpeedBar
            label="You, speaking"
            value={wpm == null ? "—" : `${wpm.toLocaleString()} wpm`}
            pct={wpm == null ? 0 : (wpm / scale) * 100}
            accent
            unmeasured={wpm == null}
          />
          <SpeedBar label="Typing average" value={`${TYPING_WPM} wpm`} pct={(TYPING_WPM / scale) * 100} />
          <div style={{ display: "flex", gap: 18, flexWrap: "wrap", fontSize: 12, color: "var(--text-tertiary)", minWidth: 0 }}>
            <span>
              Speaking time <b style={{ color: "var(--text-secondary)", ...NUMERIC }}>{formatMinutes(spoken.minutes)}</b>
              {spoken.estimated ? " (estimated)" : ""}
            </span>
            <span>
              Typing time <b style={{ color: "var(--text-secondary)", ...NUMERIC }}>{formatMinutes(impact.minutesTyped)}</b>
            </span>
          </div>
        </div>
      )}

      <p style={{ margin: "18px 0 0", fontSize: 12, color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
        {wpm != null
          ? `Your pace is measured over ${impact.ratedTakes.toLocaleString()} timed ${plural(impact.ratedTakes, "take", "takes")}${
              impact.skippedTakes > 0 ? `; ${impact.skippedTakes.toLocaleString()} were too short to time` : ""
            }. The typing figure is a stated average, not a measurement of you.`
          : "Your measured pace appears after a take longer than 1.2 seconds. Until then the multiplier is a 150 wpm speaking estimate."}
      </p>
    </section>
  );
}

/** Four numbers that answer "what did I get back?" without scrolling. */
function GlanceStrip({ data, range }: { data: Insights; range: Range }) {
  const pct = data.wordsChangePct;
  const tone: "up" | "down" | "flat" | undefined =
    pct == null ? undefined : Math.abs(pct) < 0.5 ? "flat" : pct > 0 ? "up" : "down";
  return (
    <div style={TILE_GRID}>
      <Tile
        icon="wave"
        label={`Words · ${rangeShortLabel(range)}`}
        value={data.totalWords.toLocaleString()}
        sub={changeLabel(pct)}
        tone={tone}
        info="Words in the selected window. The comparison is against the equal-length window immediately before it."
      />
      <Tile
        icon="clock"
        label="Time saved"
        value={formatMinutes(data.impact.timeSavedMinutes)}
        sub={`vs typing at ${TYPING_WPM} wpm`}
        info="Time saved is an estimate: the same words typed at 40 wpm, minus the time you actually spent speaking."
      />
      <Tile
        icon="calendar-check"
        label="Active days"
        value={`${data.activeDaysLast7}/7`}
        sub="days with a dictation, last 7 days"
        info="Always the rolling last 7 days, independent of the range above."
      />
      <Tile
        icon="chart-column"
        label="Per dictation"
        value={data.avgWordsPerDictation.toLocaleString()}
        sub={`avg words · ${data.totalDictations.toLocaleString()} dictations`}
        info="Words divided by dictations in the selected window."
      />
    </div>
  );
}

function ActivityCard({ data }: { data: Insights }) {
  const streak = data.streakDays;
  const longest = data.longestStreakDays;
  const hasStreak = streak > 0;
  return (
    <section
      aria-label="Your streak and daily habit"
      className="tt-card"
      style={{ padding: "24px 26px", boxShadow: "var(--shadow-2)", minWidth: 0 }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 16,
          flexWrap: "wrap",
          minWidth: 0,
        }}
      >
        <div style={{ display: "flex", alignItems: "baseline", gap: 12, flexWrap: "wrap", minWidth: 0 }}>
          <span
            style={{
              fontSize: "clamp(40px, 5.4vw, 58px)",
              fontWeight: 700,
              lineHeight: 0.98,
              letterSpacing: "var(--tracking-display)",
              color: hasStreak ? "var(--text)" : "var(--text-tertiary)",
              whiteSpace: "nowrap",
              ...NUMERIC,
            }}
          >
            {hasStreak ? streak.toLocaleString() : "—"}
          </span>
          <span
            style={{
              fontSize: "clamp(17px, 1.9vw, 22px)",
              fontWeight: 600,
              lineHeight: 1.15,
              letterSpacing: "var(--tracking-tight)",
              color: "var(--text)",
              minWidth: 0,
            }}
          >
            {hasStreak ? "days in a row" : "no streak yet"}
          </span>
        </div>
        <CardMeta label="All time" value={`${data.totalWords.toLocaleString()} words`} />      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(min(320px, 100%), 1fr))",
          gap: 24,
          alignItems: "center",
          marginTop: 18,
          minWidth: 0,
        }}
      >
        <Heatmap cells={data.heatmap} />
        <div style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
          {[
            { label: "Words today", value: data.wordsToday.toLocaleString() },
            { label: "Per active day", value: data.avgWordsPerDay.toLocaleString() },
            { label: "Best streak", value: longest.toLocaleString() },
          ].map((row, i) => (
            <div
              key={row.label}
              style={{
                display: "flex",
                alignItems: "baseline",
                justifyContent: "space-between",
                gap: 12,
                padding: "11px 0",
                borderTop: i === 0 ? undefined : "1px solid var(--border-subtle)",
                minWidth: 0,
              }}
            >
              <span style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", minWidth: 0, overflowWrap: "anywhere" }}>
                {row.label}
              </span>
              <span
                style={{
                  fontSize: "var(--text-lg)",
                  fontWeight: 650,
                  letterSpacing: "var(--tracking-tight)",
                  color: "var(--text)",
                  flexShrink: 0,
                  ...NUMERIC,
                }}
              >
                {row.value}
              </span>
            </div>
          ))}
        </div>
      </div>

      <p style={{ margin: "12px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
        Every square is a day you dictated. The grid and the streak are all time; the range above changes the numbers that follow it.
      </p>
    </section>
  );
}

/** The always-visible trend: 14 days of words plus the window-over-window
    comparison, so the dashboard answers "am I doing more of this?" */
function TrendCard({ data, range }: { data: Insights; range: Range }) {
  const hasData = data.totalWords > 0;
  const prior = data.priorWindowWords;
  return (
    <Card
      title="Words per day"
      icon="chart-column"
      action={<CardMeta label="Last 14 days" value={`${data.avgWordsPerDictation.toLocaleString()} words / dictation`} />}
    >
      {hasData ? (
        <BarChart daily={data.daily} />
      ) : (
        <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0 }}>
          <Icon name="chart-column" size={16} color="var(--text-secondary)" />
          No activity yet — your daily words will chart here.
        </p>
      )}
      <Hairline />
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 10, flexWrap: "wrap", minWidth: 0 }}>
        <span style={{ fontSize: "var(--text-sm)", color: "var(--text)" }}>
          {changeLabel(data.wordsChangePct)}
          {prior != null && (
            <span style={{ color: "var(--text-tertiary)" }}>
              {" "}
              ({prior.toLocaleString()} words before)
            </span>
          )}
        </span>
        <span className="tt-eyebrow" style={{ flexShrink: 0 }}>
          {rangeWindowLabel(range)}
        </span>
      </div>
      <p style={{ margin: "8px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
        The bars are always the last 14 days. The comparison follows the range above.
      </p>
    </Card>
  );
}

function QualityCard({
  data,
  range,
  onNavigate,
}: {
  data: Insights;
  range: Range;
  onNavigate?: (screen: string) => void;
}) {
  const learned = data.personalizationCorrections + data.dictionaryLearned;
  const editRate = data.learning?.editRate7d ?? null;
  const series = editRateSeries(data.learning?.daily);
  const accuracy = accuracyTrend(data.learning?.accuracy);
  const scope = rangeLearnedLabel(range);
  // The settle tallies are lifetime counters on the learned pairs, and they
  // only fill in as takes age past their 30-minute window, so a zero here
  // means "not yet observed" rather than "polish is wrong". Show the row once
  // at least one correction has been graded either way.
  const kept = data.learning?.keptCorrections ?? 0;
  const undone = data.learning?.undoneCorrections ?? 0;
  const graded = kept + undone;

  return (
    <Card
      title="Learning and cleanup"
      icon="wand"
      action={<CardMeta label="Scope" value={range === "lifetime" ? "All time" : rangeWindowLabel(range)} />}
    >
      {learned > 0 ? (
        <Headline
          value={learned.toLocaleString()}
          label={`things learned ${scope}`}
          labelInfo="Preferences Teletype picked up from your edits, plus dictionary words it learned after you fixed a mishearing. Words you add by hand are not counted here."
        />
      ) : (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          Fix a mishearing once and Teletype remembers it. Until then, filler removal and AutoText keep the text clean.
        </p>
      )}

      <div style={{ marginTop: 14, minWidth: 0 }}>
        <MetricRow
          icon="scissors"
          label="Filler words removed"
          scope="All time"
          value={data.fillerRemoved.toLocaleString()}
          info="Words like “um” and “uh” that Teletype removed before inserting the text. Only counted while filler removal is switched on; the counter has no timestamps, so it cannot be scoped to the selected range."
        />
        <MetricRow
          icon="autotext"
          label="AutoText expansions"
          scope="All time"
          value={data.autotextExpansions.toLocaleString()}
          info="Typed triggers and spoken snippets Teletype expanded for you. Like filler removal, this is a persisted all-time counter without timestamps."
        />
        {editRate != null && (
          <MetricRow
            icon="edit"
            label="Edits per 100 words"
            scope="Last 7 days"
            value={editRate.toFixed(1)}
            info="Words you changed per 100 dictated words over the last 7 days. Falling is improvement: the transcript needed less fixing."
          />
        )}
        {graded > 0 && (
          <MetricRow
            icon="check"
            label="Corrections you kept"
            scope="All time"
            value={`${kept.toLocaleString()} of ${graded.toLocaleString()}`}
            info="Changes AI Polish made that were still in the text 30 minutes later, out of all the changes it has had a chance to make. If you undo or rewrite one, it counts against Polish instead. Nothing is applied automatically from this yet."
          />
        )}
      </div>

      {accuracy && (
        <>
          <Hairline />
          <div style={{ minWidth: 0 }}>
            <div className="tt-eyebrow" style={{ marginBottom: 6 }}>Personal accuracy</div>
            <div style={{ display: "flex", alignItems: "baseline", gap: 8, flexWrap: "wrap", minWidth: 0 }}>
              <span style={{ fontSize: "var(--text-lg)", fontWeight: 650, ...NUMERIC }}>{accuracy.before.toFixed(1)}%</span>
              <span aria-hidden="true" style={{ color: "var(--text-tertiary)" }}>→</span>
              <span style={{ fontSize: "var(--text-lg)", fontWeight: 650, color: "var(--accent)", ...NUMERIC }}>{accuracy.after.toFixed(1)}%</span>
              <span style={{ fontSize: "var(--text-xs)", color: accuracy.deltaPts < 0 ? "var(--success)" : "var(--text-tertiary)", ...NUMERIC }}>
                {accuracy.deltaPts === 0
                  ? "no change"
                  : `${accuracy.deltaPts > 0 ? "+" : "−"}${Math.abs(accuracy.deltaPts).toFixed(1)} pts`}
              </span>
            </div>
            <p style={{ margin: "6px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
              Word error rate across {accuracy.points.toLocaleString()} replay {plural(accuracy.points, "run", "runs")} of your own recordings. Lower is better.
            </p>
          </div>
        </>
      )}

      {series.length >= 2 && (
        <>
          <Hairline />
          <div style={{ minWidth: 0 }}>
            <div className="tt-eyebrow" style={{ marginBottom: 7 }}>Edits per 100 words · last 30 days</div>
            <MiniBars items={series} />
            <p style={{ margin: "7px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
              Falling is better: the transcript needed fewer fixes.
            </p>
          </div>
        </>
      )}

      <Hairline />
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 12, flexWrap: "wrap", minWidth: 0 }}>
        <p style={{ flex: "1 1 180px", margin: 0, fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
          Polish runs before the text lands. When it changes something, the pill shows the before and after.
        </p>
        {onNavigate && (
          <Cta icon="sparkles" onClick={() => onNavigate("transforms")}>
            Tune AI Polish
          </Cta>
        )}
      </div>
    </Card>
  );
}

function PersonalCard({
  data,
  onNavigate,
}: {
  data: Insights;
  onNavigate?: (screen: string) => void;
}) {
  const terms = data.learning?.termsLearnedWeek ?? [];
  const topFix = data.learning?.topFixes?.[0];
  const hasWords = data.dictionaryWords > 0;

  return (
    <Card
      title="Your words"
      icon="personalization"
      action={<CardMeta label="Dictionary" value={`${data.dictionaryWords.toLocaleString()} ${plural(data.dictionaryWords, "word", "words")}`} />}
    >
      {hasWords ? (
        <>
          <Headline
            value={data.dictionaryWords.toLocaleString()}
            label="words spelled your way"
            labelInfo="All time: words you taught plus words Teletype learned from your corrections. The dictionary is applied before the text is inserted."
          />
          <p style={{ margin: "8px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
            {data.learnedWords > 0
              ? `${data.learnedWords.toLocaleString()} learned automatically from your fixes.`
              : "Every word here was added by you or shipped as a built-in."}
            {data.vocabularySize > 0 ? ` You have used ${data.vocabularySize.toLocaleString()} distinct words in this window.` : ""}
          </p>
        </>
      ) : (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          Teach Teletype one name or acronym. Fix a mishearing once and it spells it your way next time.
        </p>
      )}

      {terms.length > 0 && (
        <div style={{ marginTop: 14, minWidth: 0 }}>
          <div className="tt-eyebrow" style={{ marginBottom: 7 }}>Learned this week · {terms.length.toLocaleString()}</div>
          <div style={{ display: "flex", gap: 6, flexWrap: "wrap", minWidth: 0 }}>
            {terms.slice(0, RANK_CAP).map((t) => (
              <span
                key={t.term}
                title={t.aliases && t.aliases.length ? `Teletype now recognises "${t.term}" from "${t.aliases.join('", "')}"` : `Teletype now recognises "${t.term}"`}
                style={{
                  maxWidth: "100%",
                  padding: "3px 10px",
                  borderRadius: 999,
                  background: "var(--surface-2)",
                  border: "1px solid var(--border-subtle)",
                  fontSize: "var(--text-xs)",
                  fontWeight: 600,
                  color: "var(--text-secondary)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {t.term}
              </span>
            ))}
            {terms.length > RANK_CAP && (
              <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", alignSelf: "center" }}>+{terms.length - RANK_CAP} more</span>
            )}
          </div>
        </div>
      )}

      {topFix && (
        <div style={{ marginTop: 14, minWidth: 0 }}>
          <div className="tt-eyebrow" style={{ marginBottom: 6 }}>Your most-corrected phrase</div>
          <div
            title={`Heard "${topFix.from}", you wrote "${topFix.to}" — ${topFix.count.toLocaleString()} times`}
            style={{ display: "flex", alignItems: "baseline", gap: 8, fontSize: "var(--text-sm)", minWidth: 0 }}
          >
            <span style={{ color: "var(--text-tertiary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 }}>{topFix.from}</span>
            <span aria-hidden="true" style={{ color: "var(--text-tertiary)", flexShrink: 0 }}>→</span>
            <span style={{ fontWeight: 650, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 }}>{topFix.to}</span>
            <span style={{ marginLeft: "auto", color: "var(--text-secondary)", flexShrink: 0, fontWeight: 600, ...NUMERIC }}>{topFix.count.toLocaleString()}×</span>
          </div>
        </div>
      )}

      {onNavigate && (
        <>
          <Hairline />
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", minWidth: 0 }}>
            <Cta icon="dictionary" onClick={() => onNavigate("dictionary")}>
              {hasWords ? "Dictionary" : "Add a word"}
            </Cta>
            <Cta icon="autotext" onClick={() => onNavigate("autotext")}>
              AutoText
            </Cta>
            <Cta icon="style" onClick={() => onNavigate("style")}>
              Styles
            </Cta>
          </div>
        </>
      )}
    </Card>
  );
}

function AppUsageCard({
  apps,
  totalDictations,
  windowLabel,
}: {
  apps: RankedItem[];
  totalDictations: number;
  windowLabel: string;
}) {
  const rows = [...apps].sort((a, b) => b.count - a.count).slice(0, RANK_CAP);
  const appTotal = rows.reduce((s, a) => s + a.count, 0);
  const top = rows.length ? rows[0].count : 0;
  return (
    <Card title="Where you dictate" icon="dashboard" action={<CardMeta label="Scope" value={windowLabel} />}>
      {rows.length === 0 ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          No app attribution yet — dictate somewhere other than Teletype's own window.
        </p>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
          {rows.map((a) => {
            const pct = pctOf(a.count, appTotal);
            const width = top > 0 ? Math.max(6, Math.round((a.count / top) * 100)) : 0;
            return (
              <div key={a.label} style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
                <span
                  aria-hidden="true"
                  style={{
                    width: 26,
                    height: 26,
                    borderRadius: 8,
                    background: "var(--surface-2)",
                    color: "var(--text-secondary)",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: "var(--text-xs)",
                    fontWeight: 700,
                    flexShrink: 0,
                  }}
                >
                  {(a.label.trim().charAt(0) || "?").toUpperCase()}
                </span>
                <div style={{ flex: "1 1 0%", minWidth: 0 }}>
                  <div
                    title={a.label}
                    style={{
                      fontSize: "var(--text-xs)",
                      color: "var(--text-secondary)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      minWidth: 0,
                    }}
                  >
                    {a.label}
                  </div>
                  <div style={{ height: 7, background: "var(--surface-2)", borderRadius: 4, marginTop: 5, minWidth: 0 }}>
                    <div
                      title={`${a.label} — ${a.count.toLocaleString()} dictations, ${pct}% of the attributed dictations in this list. Bar scaled to the most-used app.`}
                      style={{ width: `${width}%`, height: "100%", borderRadius: 4, background: "var(--accent)" }}
                    />
                  </div>
                </div>
                <div style={{ flexShrink: 0, textAlign: "right" }}>
                  <div style={{ fontSize: "var(--text-xs)", fontWeight: 650, color: "var(--text)", ...NUMERIC }}>
                    {a.count.toLocaleString()}
                  </div>
                  <div style={{ fontSize: 10, color: "var(--text-tertiary)", ...NUMERIC }}>{pct}%</div>
                </div>
              </div>
            );
          })}
          <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", minWidth: 0, overflowWrap: "anywhere" }}>
            Percentages are of {appTotal.toLocaleString()} dictations with a detected app, out of {totalDictations.toLocaleString()} dictated in the selected range.
          </div>
        </div>
      )}
    </Card>
  );
}

/** Email / chat / code mix: the "what is this actually for?" view. */
function WorkMixCard({ data, windowLabel }: { data: Insights; windowLabel: string }) {
  return (
    <Card title="Work mix" icon="messages-square" action={<CardMeta label="Scope" value={windowLabel} />}>
      <RankedBars items={data.appCategories} unit="dictations" empty="No app attribution yet — this fills in once Teletype can see which app you dictated into." />
      {data.appCategories.length > 0 && (
        <p style={{ margin: "12px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
          Categories come from the app Teletype detected. Bars are scaled to your most-used category; dictations with no detected app are not counted.
        </p>
      )}
    </Card>
  );
}

/** Per-macro AutoText usage and the filler breakdown. Both counters are
    persisted all-time totals without timestamps, so they never follow the
    range and are labelled accordingly. */
function AutomationCard({
  data,
  onNavigate,
}: {
  data: Insights;
  onNavigate?: (screen: string) => void;
}) {
  const macros = data.topAutotext.slice(0, RANK_CAP);
  const fillers = data.topFillers.slice(0, RANK_CAP);
  const empty = macros.length === 0 && fillers.length === 0;
  return (
    <Card title="Automations" icon="autotext" action={<CardMeta label="Scope" value="All time" />}>
      {empty ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          No AutoText or filler activity yet. Add a snippet and it will show up here with how often it fires.
        </p>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
          <div style={{ minWidth: 0 }}>
            <div className="tt-eyebrow" style={{ marginBottom: 8 }}>AutoText macros</div>
            {macros.length === 0 ? (
              <p style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", margin: 0, minWidth: 0 }}>
                No snippet has fired yet.
              </p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
                {macros.map((m) => (
                  <div key={m.label} style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 10, minWidth: 0 }}>
                    <span style={{ fontSize: "var(--text-sm)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 }} title={m.label}>
                      {m.label}
                    </span>
                    <span style={{ fontSize: "var(--text-xs)", fontWeight: 600, color: "var(--text-secondary)", flexShrink: 0, ...NUMERIC }}>
                      {m.count.toLocaleString()}×
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
          <Hairline style={{ margin: 0 }} />
          <div style={{ minWidth: 0 }}>
            <div className="tt-eyebrow" style={{ marginBottom: 8 }}>Filler words removed</div>
            {fillers.length === 0 ? (
              <p style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", margin: 0, minWidth: 0 }}>
                No filler words removed yet.
              </p>
            ) : (
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap", minWidth: 0 }}>
                {fillers.map((f) => (
                  <span
                    key={f.label}
                    title={`“${f.label}” removed ${f.count.toLocaleString()} times`}
                    style={{
                      padding: "3px 10px",
                      borderRadius: 999,
                      background: "var(--surface-2)",
                      border: "1px solid var(--border-subtle)",
                      fontSize: "var(--text-xs)",
                      fontWeight: 600,
                      color: "var(--text-secondary)",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {f.label}
                    <span style={{ color: "var(--text-tertiary)", marginLeft: 4, ...NUMERIC }}>{f.count.toLocaleString()}</span>
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
      {onNavigate && (
        <p style={{ margin: "12px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0 }}>
          Both lists are all-time and cannot be windowed.{" "}
          <button
            type="button"
            onClick={() => onNavigate("autotext")}
            style={{ background: "none", border: "none", padding: 0, color: "var(--accent)", fontWeight: 600, cursor: "pointer", fontSize: "inherit" }}
          >
            Manage snippets
          </button>
        </p>
      )}
    </Card>
  );
}

function RhythmCard({ data, windowLabel }: { data: Insights; windowLabel: string }) {
  const bestDay = data.bestDay;
  const topDays = data.topDays.slice(0, 5);
  const busiestBand = [...data.hourBands].filter((b) => b.count > 0).sort((a, b) => b.count - a.count)[0];
  const hasData = data.totalWords > 0;

  return (
    <Card title="Your rhythm" icon="clock" action={<CardMeta label="Scope" value={windowLabel} />}>
      {!hasData ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          Dictate a few takes and your rhythm will appear here.
        </p>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
          {bestDay && (
            <div style={{ display: "flex", alignItems: "baseline", gap: 8, minWidth: 0, flexWrap: "wrap" }}>
              <span style={{ fontSize: "var(--text-md)", fontWeight: 650, color: "var(--text)", minWidth: 0, overflowWrap: "anywhere" }}>
                Your best day is {bestDay}
              </span>
              <InfoMark text="The weekday with the most words in the selected window. Counts are based on each dictation's timestamp." />
            </div>
          )}
          {topDays.length > 0 && (
            <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
              {topDays.map((d) => (
                <div key={d.label} style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
                  <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", width: 80, flexShrink: 0, textTransform: "capitalize" }}>
                    {d.label}
                  </span>
                  <div style={{ flex: "1 1 0%", height: 8, background: "var(--surface-2)", borderRadius: 4, minWidth: 0 }}>
                    <div style={{ height: "100%", width: `${Math.round(d.share * 100)}%`, background: "var(--accent)", borderRadius: 4 }} />
                  </div>
                  <span style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", fontWeight: 600, flexShrink: 0, ...NUMERIC }}>
                    {d.count.toLocaleString()}
                  </span>
                </div>
              ))}
            </div>
          )}
          {busiestBand && (
            <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", minWidth: 0, overflowWrap: "anywhere" }}>
              Most of your words land in the {busiestBand.label.toLowerCase()}.
            </div>
          )}
        </div>
      )}
    </Card>
  );
}

/** The five time-of-day bands, drawn rather than described. */
function HourBandsCard({ data, windowLabel }: { data: Insights; windowLabel: string }) {
  const bands = data.hourBands;
  const total = bands.reduce((s, b) => s + b.count, 0);
  const busiest = [...bands].filter((b) => b.count > 0).sort((a, b) => b.count - a.count)[0];
  const hour = data.busiestHour;
  return (
    <Card title="Time of day" icon="calendar-check" action={<CardMeta label="Scope" value={windowLabel} />}>
      {total === 0 ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          No words in this window yet.
        </p>
      ) : (
        <>
          <RankedBars items={bands} unit="words" empty="No words in this window yet." />
          <p style={{ margin: "12px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
            {busiest ? `Busiest band: ${busiest.label.toLowerCase()}.` : ""}
            {hour != null ? (
              <>
                {" "}
                Busiest single hour: <span style={{ whiteSpace: "nowrap" }}>{String(hour).padStart(2, "0")}:00 UTC</span>.
              </>
            ) : (
              ""
            )}
          </p>
        </>
      )}
    </Card>
  );
}

function PhraseList({ items }: { items: RankedItem[] }) {
  if (items.length === 0) {
    return (
      <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0 }}>
        <Icon name="sparkles" size={16} color="var(--text-secondary)" />
        Nothing here yet — say the same thing twice in this window and the pattern appears.
      </p>
    );
  }
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
      {items.map((item) => (
        <div key={item.label} style={{ minWidth: 0 }}>
          <div style={{ display: "flex", justifyContent: "space-between", gap: 8, fontSize: "var(--text-sm)", marginBottom: 5, minWidth: 0 }}>
            <span title={item.label} style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 }}>{item.label}</span>
            <span style={{ color: "var(--text-secondary)", flexShrink: 0, fontWeight: 600, ...NUMERIC }}>{item.count.toLocaleString()}×</span>
          </div>
          <div style={{ height: 8, background: "var(--surface-2)", borderRadius: 4, minWidth: 0 }}>
            <div style={{ height: "100%", width: `${Math.round(item.share * 100)}%`, background: "var(--accent)", borderRadius: 4 }} />
          </div>
        </div>
      ))}
    </div>
  );
}

function LanguageHabitCard({ data, windowLabel }: { data: Insights; windowLabel: string }) {
  const topWords = data.topWords.slice(0, RANK_CAP);
  const habit = data.habitWords;
  const hasWords = data.totalWords > 0;

  const trendLabel =
    habit.trend === 1 ? "slightly more than last period" :
    habit.trend === -1 ? "less than last period" : "steady";

  return (
    <Card title="Language habit" icon="wand" action={<CardMeta label="Scope" value={windowLabel} />}>
      {!hasWords ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          Dictate a few takes and your language habits will appear here.
        </p>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
          {topWords.length > 0 && (
            <div style={{ minWidth: 0 }}>
              <div className="tt-eyebrow" style={{ marginBottom: 8 }}>Your top words</div>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap", minWidth: 0 }}>
                {topWords.map((w) => (
                  <span
                    key={w.label}
                    title={`${w.count.toLocaleString()} occurrences in ${windowLabel}`}
                    style={{
                      maxWidth: "100%",
                      padding: "3px 10px",
                      borderRadius: 999,
                      background: "var(--surface-2)",
                      border: "1px solid var(--border-subtle)",
                      fontSize: "var(--text-xs)",
                      fontWeight: 600,
                      color: "var(--text-secondary)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {w.label}
                    <span style={{ color: "var(--text-tertiary)", marginLeft: 4, ...NUMERIC }}>{w.count.toLocaleString()}</span>
                  </span>
                ))}
              </div>
            </div>
          )}
          <div style={{ minWidth: 0 }}>
            <div className="tt-eyebrow" style={{ marginBottom: 6 }}>Word habits</div>
            {habit.total === 0 ? (
              <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
                No habit words detected — you're using clean, professional language.
              </p>
            ) : (
              <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere", lineHeight: 1.5 }}>
                <b style={{ color: "var(--text)", ...NUMERIC }}>{habit.per1kWords.toFixed(1)}</b> habit words per 1,000 words ({trendLabel}).
                {habit.items.length > 0 && (
                  <> Top: {habit.items.slice(0, 3).map((i) => i.label).join(", ")}.</>
                )}
              </p>
            )}
          </div>
        </div>
      )}
    </Card>
  );
}

/* ------------------------------------------------------------------ *
 * All-time
 * ------------------------------------------------------------------ */

function RecordsCard({ data }: { data: Insights }) {
  const tally = writtenTally(data.totalWords);
  return (
    <Card title="Records" icon="trophy" action={<CardMeta label="All time" value={`${data.totalDictations.toLocaleString()} dictations`} />}>
      {data.records.length === 0 ? (
        <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0 }}>
          <Icon name="trophy" size={16} color="var(--text-secondary)" />
          No records yet — your best dictations will show up here.
        </p>
      ) : (
        <>
          {tally && (
            <p style={{ fontSize: "var(--text-md)", fontWeight: 650, margin: "0 0 14px", color: "var(--text)", minWidth: 0, overflowWrap: "anywhere" }}>
              {tally}
            </p>
          )}
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(130px, 100%), 1fr))", gap: 14, minWidth: 0 }}>
            {data.records.map((r) => (
              <div key={r.label} style={{ minWidth: 0 }}>
                <div className="tt-eyebrow" style={{ marginBottom: 4 }}>{r.label}</div>
                <div
                  style={{
                    fontSize: r.value.length > 12 ? "var(--text-sm)" : "var(--text-lg)",
                    fontWeight: 650,
                    letterSpacing: "var(--tracking-tight)",
                    fontVariantNumeric: "tabular-nums",
                    whiteSpace: "normal",
                    overflowWrap: "anywhere",
                    minWidth: 0,
                  }}
                >
                  {r.value}
                </div>
              </div>
            ))}
          </div>
        </>
      )}
    </Card>
  );
}

function MilestonesCard({ data }: { data: Insights }) {
  return (
    <Card title="Milestones" icon="medal" action={<CardMeta label="All time" value="cumulative" />}>
      {data.milestones.length === 0 ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0 }}>
          No milestones yet.
        </p>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
          {data.milestones.map((row) => (
            <div key={row.category} style={{ minWidth: 0 }}>
              <div style={{ fontSize: "var(--text-sm)", fontWeight: 600, marginBottom: 7, minWidth: 0 }}>
                {row.category}
                <span style={{ color: "var(--text-secondary)", fontWeight: 400, marginLeft: 8, ...NUMERIC }}>{row.current.toLocaleString()}</span>
              </div>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap", minWidth: 0 }}>
                {row.items.map((m) => (
                  <div
                    key={m.label}
                    title={m.reached ? `Reached: ${m.label}` : `Not yet: ${m.threshold.toLocaleString()}`}
                    style={{
                      padding: "3px 10px",
                      borderRadius: 999,
                      fontSize: "var(--text-xs)",
                      fontWeight: 600,
                      whiteSpace: "nowrap",
                      maxWidth: "100%",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      background: m.reached ? "var(--accent-soft)" : "var(--surface-2)",
                      color: m.reached ? "var(--accent)" : "var(--text-secondary)",
                      border: `1px solid ${m.reached ? "var(--accent)" : "var(--border-subtle)"}`,
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
      )}
    </Card>
  );
}

/* ------------------------------------------------------------------ *
 * Screen
 * ------------------------------------------------------------------ */

export default function InsightsScreen({ onNavigate }: { onNavigate?: (screen: string) => void }) {
  const [data, setData] = useState<Insights>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [range, setRange] = useState<Range>("week");
  const [customFrom, setCustomFrom] = useState<string>("");
  const [customTo, setCustomTo] = useState<string>("");
  // First activity discovered from the backend; clamps the custom range.
  const [firstActivityAt, setFirstActivityAt] = useState<number | null>(null);
  // U2: per-stage latency percentiles (independent of the range filter: the
  // panel always shows the most recent takes, so it is fetched once and on
  // each dictation completion, not per range).
  const [latency, setLatency] = useState<LatencySummary | null>(null);

  const refresh = useCallback(() => {
    setError(null);
    const from = fromInputDate(customFrom);
    const to = fromInputDate(customTo);
    invoke<Insights>("get_insights", {
      range,
      customFrom: range === "custom" ? from : null,
      customTo: range === "custom" ? to : null,
    })
      .then((d) => {
        setData(d);
        setLoaded(true);
        if (d.firstActivityAt != null) setFirstActivityAt(d.firstActivityAt);
      })
      .catch((e) => {
        setError(errorMessage(e));
        setLoaded(true);
      });
  }, [range, customFrom, customTo]);

  useEffect(refresh, [refresh]);

  // U2: fetch the per-stage latency percentiles once and on each dictation
  // completion. Independent of the range filter (the panel always shows the
  // most recent takes), so it does not re-run on range changes.
  const refreshLatency = useCallback(() => {
    invoke<LatencySummary>("get_latency_summary")
      .then(setLatency)
      .catch(() => setLatency(null));
  }, []);
  useEffect(refreshLatency, [refreshLatency]);

  // Refresh in real-time when a dictation completes.
  useTauriEvent<{ phase: string }>("dictation-state", ({ payload }) => {
    if (payload.phase === "idle") {
      refresh();
      refreshLatency();
    }
  });

  // Re-fetch when the window becomes visible again (dictations and typed
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

  const chooseRange = (id: Range) => {
    if (id === "custom") {
      const today = toInputDate(Date.now());
      if (!customTo) setCustomTo(today);
      if (!customFrom) {
        // Start at the first day with activity, so the first custom view is
        // the whole story rather than an empty one-day window.
        setCustomFrom(toInputDate(firstActivityAt ?? Date.now() - 30 * 86_400_000));
      }
    }
    setRange(id);
  };

  const customMin = firstActivityAt ? toInputDate(firstActivityAt) : undefined;
  const customMax = toInputDate(Date.now());
  const windowLabel = rangeWindowLabel(range);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, width: "100%", minWidth: 0, overflowX: "clip" }}>
      {/* Header: title left, the window control right. */}
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 16, flexWrap: "wrap", minWidth: 0 }}>
        <div style={{ minWidth: 0 }}>
          <h2 className="tt-screen-title">Insights</h2>
          <p className="tt-screen-sub" style={{ margin: 0, maxWidth: 620 }}>
            What you got back from talking instead of typing — and how the writing is getting better. Everything here is computed on this Mac.
          </p>
        </div>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 6, minWidth: 0, maxWidth: "100%" }}>
          <div
            role="group"
            aria-label="Time range"
            style={{ display: "flex", flexWrap: "wrap", gap: 2, justifyContent: "flex-end", background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 999, padding: 4, boxShadow: "var(--shadow-1)", maxWidth: "100%", minWidth: 0 }}
          >
            {RANGES.map((r) => (
              <button
                key={r.id}
                type="button"
                onClick={() => chooseRange(r.id)}
                aria-pressed={range === r.id}
                title="Sets the window for impact, work mix, rhythm and language. The heatmap, daily chart and records are all time."
                style={{
                  border: "none",
                  background: range === r.id ? "var(--action)" : "transparent",
                  color: range === r.id ? "var(--on-action)" : "var(--text-secondary)",
                  fontSize: "var(--text-sm)",
                  fontWeight: 600,
                  padding: "6px 13px",
                  borderRadius: 999,
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                  transition: "background var(--dur) var(--ease), color var(--dur) var(--ease)",
                }}
              >
                {r.label}
              </button>
            ))}
          </div>
          {range === "custom" ? (
            <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap", justifyContent: "flex-end", minWidth: 0 }}>
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
                  fontSize: "var(--text-xs)",
                  background: "var(--surface)",
                  color: "var(--text)",
                  minWidth: 0,
                }}
              />
              <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>to</span>
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
                  fontSize: "var(--text-xs)",
                  background: "var(--surface)",
                  color: "var(--text)",
                  minWidth: 0,
                }}
              />
              {firstActivityAt && (
                <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>
                  (earliest activity {shortDate(firstActivityAt)})
                </span>
              )}
            </div>
          ) : (
            <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", textAlign: "right" }}>
              Impact, work mix and language follow this window; habits and records stay all-time
            </span>
          )}
        </div>
      </div>

      {error && (
        <div
          role="alert"
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "10px 14px",
            borderRadius: "var(--radius-sm)",
            background: "var(--warning-soft)",
            color: "var(--warning)",
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            minWidth: 0,
          }}
        >
          <Icon name="alert" size={16} color="var(--warning)" />
          <span style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>Could not read your history: {error}</span>
          <button type="button" className="tt-btn tt-btn-sm" onClick={refresh} style={{ background: "transparent", borderColor: "var(--warning)", color: "var(--warning)" }}>
            Retry
          </button>
        </div>
      )}

      {data.polishStatus && (
        <div
          role="status"
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "10px 14px",
            borderRadius: "var(--radius-sm)",
            background: "var(--warning-soft)",
            color: "var(--warning)",
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            minWidth: 0,
          }}
        >
          <Icon name="alert" size={16} color="var(--warning)" />
          <span style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{data.polishStatus}</span>
          {onNavigate && (
            <button type="button" className="tt-btn tt-btn-sm" onClick={() => onNavigate("models")} style={{ background: "transparent", borderColor: "var(--warning)", color: "var(--warning)" }}>
              Open Models
            </button>
          )}
        </div>
      )}

      <Hero data={data} loaded={loaded} range={range} onSeeAllTime={() => chooseRange("lifetime")} />

      <GlanceStrip data={data} range={range} />

      <SectionHeader title="Habit" hint="streaks, consistency and the daily trend" />
      <ActivityCard data={data} />
      <TrendCard data={data} range={range} />

      <SectionHeader title="Quality" hint="what the app learned from you" />
      <div style={CARD_GRID}>
        <QualityCard data={data} range={range} onNavigate={onNavigate} />
        <PersonalCard data={data} onNavigate={onNavigate} />
      </div>

      <SectionHeader title="Latency" hint="how long each stage takes, most recent takes" />
      <Card title="Release to paste, by stage" icon="clock" action={<CardMeta label="Window" value={latency ? `${latency.window} takes` : "—"} />}
        >
        <LatencyPanel summary={latency} />
      </Card>

      <SectionHeader title="Where your words go" hint={windowLabel} />
      <div style={CARD_GRID}>
        <AppUsageCard apps={data.topApps} totalDictations={data.totalDictations} windowLabel={windowLabel} />
        <WorkMixCard data={data} windowLabel={windowLabel} />
        <AutomationCard data={data} onNavigate={onNavigate} />
      </div>

      <SectionHeader title="Rhythm" hint="when you do your best talking" />
      <div style={CARD_GRID}>
        <RhythmCard data={data} windowLabel={windowLabel} />
        <HourBandsCard data={data} windowLabel={windowLabel} />
      </div>

      <SectionHeader title="Language" hint={windowLabel} />
      <div style={CARD_GRID}>
        <Card quiet title="Phrases you repeat" icon="sparkles" action={<CardMeta label="Scope" value={windowLabel} />}>
          <PhraseList items={data.topPhrases} />
        </Card>
        <LanguageHabitCard data={data} windowLabel={windowLabel} />
      </div>

      <SectionHeader title="All-time records" hint="never filtered by the range above" />
      <div style={CARD_GRID}>
        <RecordsCard data={data} />
        <MilestonesCard data={data} />
      </div>

      <p style={{ margin: "4px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.6, textAlign: "center", minWidth: 0, overflowWrap: "anywhere" }}>
        Every number on this screen is computed on this Mac from your own dictation history. Nothing is uploaded, and there is no account.
      </p>
    </div>
  );
}
