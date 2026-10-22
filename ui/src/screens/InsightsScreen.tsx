import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";
import { useTauriEvent } from "../lib/useTauriEvent";

interface RankedItem {
  label: string;
  count: number;
  /** Share of the *most used* item, not of the total — the backend scales
      the ranking bar to the leader. Percentages of the whole are computed
      in this file from the counts, never read from here. */
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

/** One replay run's error rate before and after the personalization boost. */
interface AccuracyPoint {
  ts: number;
  werBefore: number;
  werAfter: number;
}

interface LearningSummary {
  topFixes?: TrackedFix[];
  termsLearnedWeek?: LearnedTerm[];
  /** Edited words per 100 dictated words over the last 7 days. */
  editRate7d?: number | null;
  /** Oldest first, at most 10 points. Empty until a replay run has been stored. */
  accuracy?: AccuracyPoint[];
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
  /** The backend sends an all-empty object when there is no learning store;
      the dev preview and the test harness send null. Both are "nothing yet". */
  learning?: LearningSummary | null;
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
  learning: null,
};

const ACCENT = "#2563eb";
// Heatmap ramp: from the page background into the brand blue, so the
// "empty" cells read as part of the surface rather than a grey block.
const APP_COLORS = ["#2563eb", "#7c3aed", "#0891b2", "#ea580c", "#059669", "#db2777", "#4f46e5", "#ca8a04"];

/* The old ramp was five literal greys: level 0 (#eef1f6) sat on a white card
   with almost no delta, so 26 weeks of quiet days read as one blank slab.
   Mixing the accent into the *card surface* keeps every step visible and lets
   the same five stops work in dark mode, where a literal light grey would be
   the loudest thing on the screen. */
const HEAT_RAMP = [
  "color-mix(in srgb, var(--accent) 10%, var(--surface))",
  "color-mix(in srgb, var(--accent) 26%, var(--surface))",
  "color-mix(in srgb, var(--accent) 46%, var(--surface))",
  "color-mix(in srgb, var(--accent) 72%, var(--surface))",
  "var(--accent)",
];

function heatColor(level: number): string {
  return HEAT_RAMP[Math.max(0, Math.min(4, level))];
}

/** The dial's domain. Deliberately labelled at both ends on the dial itself:
    an unlabelled gauge invites the reader to invent a scale. */
const GAUGE_MAX_WPM = 200;
/** crates/teletype-core/src/insights.rs TYPING_WPM — the baseline every
    "times faster"/"time saved" figure in the backend is measured against. */
const TYPING_WPM = 40;
/** crates/teletype-core/src/insights.rs MAX_ITEMS — the backend truncates the
    ranked lists to this many entries, so the UI must not imply it sees more. */
const RANK_CAP = 8;

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

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

function plural(n: number, one: string, many: string): string {
  return n === 1 ? one : many;
}

/** Percentage of `total` that `part` is, rounded for display. */
function pctOf(part: number, total: number): number {
  return total > 0 ? Math.round((part / total) * 100) : 0;
}

const NUMERIC: React.CSSProperties = {
  fontVariantNumeric: "tabular-nums",
  whiteSpace: "nowrap",
};

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

/** The reference puts a small "?" disc next to every claim that needs a
    definition. Ours carries the exact counting rule in a native tooltip,
    because these four numbers are easy to over-read. */
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

/** Big number + uppercase label underneath: the reference's headline unit.
    Shared by all three row-1 cards so they read as one tier. */
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
        {/* nowrap + the scaling helper: a counter must never break mid-number. */}
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

function Card({
  title,
  icon,
  children,
  style,
  action,
  quiet,
}: {
  title?: string;
  icon?: IconName;
  children: React.ReactNode;
  style?: React.CSSProperties;
  action?: React.ReactNode;
  /** Rows below the headline tier are deliberately lighter: less padding, one
      step down on elevation, no icon tile. Same card, lower voice. */
  quiet?: boolean;
}) {
  return (
    <div
      className="tt-card"
      style={{
        padding: quiet ? "16px 18px" : "20px 22px",
        boxShadow: quiet ? "var(--shadow-1)" : "var(--shadow-2)",
        minWidth: 0,
        ...style,
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

/** Right-aligned "LABEL | value" meta in a card header, as in the reference. */
function CardMeta({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <span
      className="tt-eyebrow"
      title={title}
      style={{ display: "inline-flex", alignItems: "baseline", gap: 6, whiteSpace: "nowrap", flexShrink: 0 }}
    >
      <span>{label}</span>
      <span style={{ color: "var(--text)" }}>|</span>
      <span style={{ color: "var(--text)", ...NUMERIC }}>{value}</span>
    </span>
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

export function statFontSize(value: string): number {
  const n = value.length;
  if (n <= 7) return 30;
  if (n <= 10) return 24;
  if (n <= 13) return 20;
  return 17;
}

function BarChart({ daily }: { daily: DayStat[] }) {
  const bars = bucketDaily(daily, 22);
  const showNumbers = bars.length <= 12;
  const max = Math.max(1, ...bars.map((d) => d.words));
  // Naming every bar collides once the columns get narrow, so a month is
  // written once, at the bar where it starts. The exact day stays in the title.
  const monthOf = (d: DayStat) => d.label.split(" ")[0];
  const monthStarts = new Set(
    bars.filter((d, i) => i === 0 || monthOf(d) !== monthOf(bars[i - 1])).map((d) => d.date)
  );
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 6, height: 158, paddingTop: 4, width: "100%", minWidth: 0 }}>
      {bars.map((d) => (
        <div
          key={d.date}
          style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", alignItems: "center", gap: 7, height: "100%", justifyContent: "flex-end" }}
          title={`${d.label}: ${d.words} words, ${d.dictations} dictations`}
        >
          <div style={{ fontSize: "var(--text-xs)", height: 14, color: "var(--text-tertiary)", fontWeight: 600, ...NUMERIC }}>{showNumbers && d.words > 0 ? d.words : ""}</div>
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
          <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", whiteSpace: "nowrap", fontWeight: 500 }}>
            {monthStarts.has(d.date) ? monthOf(d) : ""}
          </div>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ *
 * The headline tier
 * ------------------------------------------------------------------ */

/** A semicircular dial: 0 → GAUGE_MAX_WPM, with a notch at the typing average
    the backend compares against. Both ends are printed, so the reader knows
    what the arc is a fraction *of* instead of guessing. */
function PaceDial({ wpm }: { wpm: number | null }) {
  const cx = 100;
  const cy = 96;
  const r = 78;
  const stroke = 15;
  const frac = wpm == null ? 0 : Math.max(0, Math.min(1, wpm / GAUGE_MAX_WPM));
  // Angles in SVG space (y grows downward): 180° is due left, 360° due right,
  // and the sweep between them, increasing, passes over the top.
  const at = (deg: number, radius = r) => {
    const rad = (deg * Math.PI) / 180;
    return [cx + radius * Math.cos(rad), cy + radius * Math.sin(rad)] as const;
  };
  const arc = (from: number, to: number) => {
    const [x0, y0] = at(from);
    const [x1, y1] = at(to);
    const large = Math.abs(to - from) > 180 ? 1 : 0;
    return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
  };
  const baseAngle = 180 + (TYPING_WPM / GAUGE_MAX_WPM) * 180;
  const [tx0, ty0] = at(baseAngle, r - stroke / 2 - 2);
  const [tx1, ty1] = at(baseAngle, r + stroke / 2 + 2);
  return (
    <svg
      viewBox="0 0 200 120"
      role="img"
      aria-label={
        wpm == null
          ? "Speaking pace dial, no measurement yet"
          : `Speaking pace ${wpm} words per minute on a 0 to ${GAUGE_MAX_WPM} scale, against a ${TYPING_WPM} words per minute typing average`
      }
      style={{ width: "100%", maxWidth: 190, height: "auto", display: "block", margin: "2px auto 0" }}
    >
      {/* Strokes and fills are set through `style`, not presentation attributes:
          var()/color-mix() substitution is a CSS feature and an attribute value
          is the one place a browser is allowed to treat it as invalid. */}
      <path
        d={arc(180, 360)}
        fill="none"
        style={{ stroke: "color-mix(in srgb, var(--accent) 12%, var(--surface))" }}
        strokeWidth={stroke}
        strokeLinecap="round"
      />
      {frac > 0 && (
        <path d={arc(180, 180 + 180 * frac)} fill="none" style={{ stroke: "var(--accent)" }} strokeWidth={stroke} strokeLinecap="round" />
      )}
      <line x1={tx0} y1={ty0} x2={tx1} y2={ty1} style={{ stroke: "var(--surface)" }} strokeWidth={2} />
      <text x={12} y={116} fontSize={9} style={{ fill: "var(--text-tertiary)" }}>0</text>
      <text x={188} y={116} fontSize={9} textAnchor="end" style={{ fill: "var(--text-tertiary)" }}>
        {GAUGE_MAX_WPM} wpm
      </text>
    </svg>
  );
}

function PaceCard({ impact }: { impact: Impact }) {
  const wpm = impact.wordsPerMinute;
  const value = wpm == null ? "—" : wpm.toLocaleString();
  const faster = impact.timesFaster;
  return (
    <Card>
      <Headline
        value={value}
        label="Words per minute"
        labelInfo="Your average speaking pace, measured only over dictations long enough to time reliably. Teletype has no population data, so this is compared with the 40 wpm average typing pace and nothing else."
        action={
          faster != null && faster > 0 ? (
            <span
              title={`You speak ${wpm} wpm; the average typing pace used here is ${TYPING_WPM} wpm.`}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                border: "1px solid var(--border-subtle)",
                background: "var(--surface-2)",
                color: "var(--text-secondary)",
                borderRadius: 999,
                padding: "4px 10px",
                fontSize: "var(--text-xs)",
                fontWeight: 600,
                whiteSpace: "nowrap",
                flexShrink: 0,
                ...NUMERIC,
              }}
            >
              <Icon name="trending-up" size={13} color="var(--text-secondary)" />
              {faster}× typing
            </span>
          ) : undefined
        }
      />
      <div style={{ marginTop: 10 }}>
        <PaceDial wpm={wpm} />
      </div>
      <p style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", margin: "8px 0 0", lineHeight: 1.45, minWidth: 0, overflowWrap: "anywhere" }}>
        {wpm == null
          ? "No pace yet — Teletype only measures words per minute on takes long enough to time, and never estimates one."
          : `Against the ${TYPING_WPM} wpm average typing pace (the notch on the dial), from ${impact.ratedTakes.toLocaleString()} timed ${plural(impact.ratedTakes, "take", "takes")}.`}
      </p>
    </Card>
  );
}

function FixRow({ value, label, info }: { value: number; label: string; info: string }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10, minWidth: 0, padding: "5px 0" }}>
      <span style={{ fontSize: "var(--text-sm)", minWidth: 0, overflowWrap: "anywhere" }}>
        <b style={{ fontWeight: 650, color: "var(--text)", ...NUMERIC }}>{value.toLocaleString()}</b>{" "}
        <span style={{ color: "var(--text-secondary)" }}>{label}</span>
      </span>
      <InfoMark text={info} />
    </div>
  );
}

/**
 * "Fixes made by Teletype" — the two counters that are genuinely a fix the app
 * performed on the user's behalf. Filler removal and AutoText expansions are
 * also fixes in a looser sense, but they are a cleanup pass and a shortcut
 * trigger rather than something learned about this user, so they live in the
 * detail tier instead of being summed into this headline.
 */
function FixesCardHeadline({ data }: { data: Insights }) {
  const corrections = data.personalizationCorrections;
  const dictionary = data.dictionaryLearned;
  const total = corrections + dictionary;
  return (
    <Card>
      <Headline
        value={total.toLocaleString()}
        label="Fixes made by Teletype"
        labelInfo="Corrections Teletype applied on its own in this window: personalization preferences it picked up from your edits, and dictionary words it learned after you fixed a mishearing. The total is the two rows below added together."
      />
      <Hairline />
      {total === 0 ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          Nothing yet — Teletype makes its first fix the first time you correct a mishearing.
        </p>
      ) : (
        <div style={{ minWidth: 0 }}>
          <FixRow
            value={corrections}
            label="words corrected"
            info="Personalization corrections: dictations in this window where Teletype rewrote wording to match a preference it learned from your earlier edits (not a rule you wrote yourself)."
          />
          <FixRow
            value={dictionary}
            label="dictionary fixes"
            info="Dictionary fixes: terms Teletype added to your dictionary automatically, in this window, after you corrected how one of them was heard. Words you add by hand are counted in the dictionary total, not here."
          />
        </div>
      )}
    </Card>
  );
}

/** Horizontal stacked split bar + legend. The reference splits Desktop vs
    Mobile; Teletype is desktop-only, so the honest equivalent is which apps
    the dictation landed in. */
function SplitBar({ segments, caption }: { segments: { label: string; count: number; color: string; light?: boolean }[]; caption: string }) {
  const total = segments.reduce((s, x) => s + x.count, 0);
  if (total <= 0) return null;
  return (
    <div style={{ minWidth: 0 }}>
      <div style={{ display: "flex", width: "100%", minWidth: 0, height: 24, borderRadius: 7, overflow: "hidden", background: "var(--surface-2)" }}>
        {segments.map((s) => {
          const pct = (s.count / total) * 100;
          if (pct <= 0) return null;
          return (
            <div
              key={s.label}
              title={`${s.label} — ${s.count.toLocaleString()} dictations (${Math.round(pct)}% of ${total.toLocaleString()} attributed)`}
              style={{
                width: `${pct}%`,
                minWidth: pct > 0 ? 3 : 0,
                height: "100%",
                background: s.color,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                padding: "0 8px",
                boxSizing: "border-box",
                overflow: "hidden",
                flexShrink: 0,
              }}
            >
              {/* The label only goes inside a segment wide enough to hold it and
                  dark enough to carry text on it; the rest are named in the
                  legend below. */}
              {pct >= 24 && !s.light && (
                <span style={{ fontSize: "var(--text-xs)", fontWeight: 600, color: "var(--on-accent)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  {s.label}
                </span>
              )}
            </div>
          );
        })}
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 14px", marginTop: 10, minWidth: 0 }}>
        {segments.map((s) => (
          <span
            key={s.label}
            title={`${s.label} — ${s.count.toLocaleString()} dictations`}
            style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: "var(--text-xs)", color: "var(--text-secondary)", minWidth: 0 }}
          >
            <span style={{ width: 9, height: 9, borderRadius: 3, background: s.color, flexShrink: 0 }} />
            <span style={{ maxWidth: 140, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.label}</span>
            <span style={{ color: "var(--text)", fontWeight: 600, ...NUMERIC }}>{pctOf(s.count, total)}%</span>
          </span>
        ))}
      </div>
      <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", marginTop: 6, minWidth: 0, overflowWrap: "anywhere" }}>{caption}</div>
    </div>
  );
}

function WordsCard({ data }: { data: Insights }) {
  const value = data.totalWords.toLocaleString();
  const daily = data.daily;
  // `daily` is the last 14 days oldest-first, so the two halves are a genuine
  // period-over-period comparison. Anything shorter and the baseline would be
  // partial, so the chip is dropped rather than extrapolated.
  const prev7 = daily.length >= 14 ? daily.slice(-14, -7).reduce((s, d) => s + d.words, 0) : null;
  const cur7 = data.wordsLast7Days;
  const delta = prev7 && prev7 > 0 ? Math.round(((cur7 - prev7) / prev7) * 100) : null;
  const tally = writtenTally(data.totalWords);

  const apps = data.topApps.slice(0, RANK_CAP);
  const appTotal = apps.reduce((s, a) => s + a.count, 0);
  const head = apps.slice(0, 3);
  const restCount = apps.slice(3).reduce((s, a) => s + a.count, 0);
  const segments = [
    ...head.map((a, i) => ({ label: a.label, count: a.count, color: APP_COLORS[i % APP_COLORS.length] })),
    ...(restCount > 0 ? [{ label: "Other apps", count: restCount, color: "var(--border)", light: true }] : []),
  ];

  return (
    <Card>
      <Headline
        value={value}
        label="Total words dictated"
        labelInfo="Words in the dictations Teletype pasted for you, in the selected range."
        action={
          delta === null ? undefined : (
            <span
              title={`Last 7 days: ${cur7.toLocaleString()} words. The 7 days before that: ${prev7?.toLocaleString()} words. Both from your daily history.`}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                border: "1px solid var(--border-subtle)",
                background: "var(--surface-2)",
                color: "var(--text-secondary)",
                borderRadius: 999,
                padding: "4px 10px",
                fontSize: "var(--text-xs)",
                fontWeight: 600,
                flexShrink: 0,
                ...NUMERIC,
              }}
            >
              <span style={{ display: "inline-flex", transform: delta < 0 ? "scaleY(-1)" : undefined }}>
                <Icon name="trending-up" size={13} color="var(--text-secondary)" />
              </span>
              {delta > 0 ? "+" : ""}
              {delta}% vs prev 7 days
            </span>
          )
        }
      />
      <Hairline />
      {tally && (
        <p title={`${data.totalWords.toLocaleString()} words dictated in total`} style={{ fontSize: "var(--text-md)", margin: "0 0 14px", minWidth: 0, overflowWrap: "anywhere" }}>
          {tally}
        </p>
      )}
      {appTotal > 0 ? (
        <SplitBar
          segments={segments}
          caption={`Where your dictations went — ${appTotal.toLocaleString()} of ${data.totalDictations.toLocaleString()} dictations had a detectable app.`}
        />
      ) : (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          No app attribution yet — Teletype records which app a dictation landed in.
        </p>
      )}
    </Card>
  );
}

/**
 * The reference's "Desktop usage" card. The percentage inside the bar is the
 * share of attributed dictations; the bar length is scaled to the most-used
 * app (that is what `share` from the backend is), which is why the exact
 * counts are repeated in every row's tooltip.
 */
function AppUsageCard({ apps, appTotal, totalDictations }: { apps: RankedItem[]; appTotal: number; totalDictations: number }) {
  // The backend already returns this sorted, but the bar scaling below reads
  // rows[0] as the maximum, so the UI re-sorts rather than trusting it.
  const rows = [...apps].sort((a, b) => b.count - a.count).slice(0, RANK_CAP);
  const top = rows.length ? rows[0].count : 0;
  return (
    <Card
      title="App usage"
      action={
        <CardMeta
          label="Apps ranked"
          value={String(rows.length)}
          title={`Teletype keeps the ${RANK_CAP} most-used apps for this window. A distinct-app total is not tracked, so this counts the apps listed, not every app you have dictated into.`}
        />
      }
    >
      {rows.length === 0 ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          No app data yet — dictate somewhere other than Teletype's own window.
        </p>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
          {rows.map((a) => {
            const pct = pctOf(a.count, appTotal);
            // Scaled to the leader so the busiest app fills the track, which is
            // how the reference reads; the number itself is the real share.
            const width = top > 0 ? Math.max(12, Math.round((a.count / top) * 100)) : 0;
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
                <div style={{ flex: "1 1 0%", minWidth: 0, height: 28, borderRadius: 7, background: "var(--surface-2)", display: "flex", overflow: "hidden" }}>
                  <div
                    title={`${a.label} — ${a.count.toLocaleString()} dictations, ${pct}% of the ${appTotal.toLocaleString()} attributed in this list. Bar scaled to the most-used app.`}
                    style={{
                      width: `${width}%`,
                      minWidth: 46,
                      maxWidth: "100%",
                      height: "100%",
                      borderRadius: 7,
                      background: "var(--accent)",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      padding: "0 8px",
                      boxSizing: "border-box",
                      overflow: "hidden",
                      flexShrink: 0,
                    }}
                  >
                    <span style={{ fontSize: "var(--text-xs)", fontWeight: 650, color: "var(--on-accent)", ...NUMERIC }}>{pct}%</span>
                  </div>
                </div>
                <div
                  title={`${a.count.toLocaleString()} dictations in ${a.label}`}
                  style={{
                    flex: "0 1 auto",
                    minWidth: 0,
                    // Capped so the bar always keeps a usable share of the row;
                    // the full app name is one hover away.
                    maxWidth: "45%",
                    fontSize: "var(--text-xs)",
                    fontWeight: 600,
                    letterSpacing: 0.3,
                    textTransform: "uppercase",
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  <span style={{ ...NUMERIC, textTransform: "none" }}>{a.count.toLocaleString()}</span> {a.label}
                </div>
              </div>
            );
          })}
          {apps.length > rows.length && (
            <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>
              +{apps.length - rows.length} more apps not shown
            </div>
          )}
          <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", minWidth: 0, overflowWrap: "anywhere" }}>
            Percentages are of {appTotal.toLocaleString()} dictations with a detected app, out of {totalDictations.toLocaleString()} in this window.
          </div>
        </div>
      )}
    </Card>
  );
}

/* ------------------------------------------------------------------ *
 * Habit grid
 * ------------------------------------------------------------------ */

/**
 * 26 weeks of daily activity. Two things made the old version unreadable: the
 * tracks were `1fr`, so 26 columns of mostly-zero days stretched into one wide
 * blank slab, and level 0 was a near-white grey. Cells are now fixed squares
 * (clamped down so they always fit the card, so no horizontal scrollbar is
 * ever needed), columns are real calendar weeks aligned to Sunday, and the
 * month labels sit at the week column where each month first appears.
 */
function Heatmap({ cells }: { cells: HeatCell[] }) {
  if (cells.length === 0) {
    return (
      <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
        No activity yet — the grid fills in one square per day.
      </p>
    );
  }
  const gap = 3;
  // Leading blanks so that column 0 starts on a Sunday and every row is a
  // fixed weekday, matching the reference's Sun…Sat rows.
  const lead = new Date(cells[0].date).getDay();
  const weeks = Math.ceil((lead + cells.length) / 7);
  // `100%` here is the grid container's own inline size, so the cell shrinks
  // to fit rather than overflowing. 13px is the cap; the 5px floor keeps 27
  // week columns inside ~215px, which is narrower than the card can get.
  const cell = `clamp(5px, calc((100% - ${(weeks - 1) * gap}px) / ${weeks}), 13px)`;
  const tracks: React.CSSProperties = {
    display: "grid",
    gridTemplateColumns: `repeat(${weeks}, ${cell})`,
    columnGap: gap,
    rowGap: gap,
    minWidth: 0,
    width: "100%",
  };

  // Month label per week column: named on the column where the month first
  // appears, then left blank until the next one.
  let lastMonth = -1;
  const labels: string[] = [];
  for (let w = 0; w < weeks; w++) {
    const firstIndex = Math.max(0, w * 7 - lead);
    const month = new Date(cells[Math.min(firstIndex, cells.length - 1)].date).getMonth();
    labels.push(month === lastMonth ? "" : MONTHS[month]);
    lastMonth = month;
  }

  const cellsRendered: React.ReactNode[] = [];
  for (let i = 0; i < lead; i++) {
    cellsRendered.push(<div key={`pad-${i}`} style={{ borderRadius: 2 }} />);
  }
  for (const cellData of cells) {
    const d = new Date(cellData.date);
    const label = `${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()}`;
    cellsRendered.push(
      <div
        key={cellData.date}
        title={`${label}: ${cellData.words.toLocaleString()} words`}
        style={{
          aspectRatio: "1",
          width: "100%",
          minWidth: 0,
          borderRadius: 2,
          background: heatColor(cellData.level),
          // Level 0 has to still read as a square, not as a hole in the card.
          boxShadow: cellData.level === 0 ? "inset 0 0 0 1px var(--border)" : undefined,
        }}
      />
    );
  }

  return (
    <div style={{ minWidth: 0, width: "100%" }}>
      <div style={{ ...tracks, rowGap: 0 }} aria-hidden="true">
        {labels.map((m, w) =>
          m ? (
            <div
              key={`m-${w}`}
              style={{
                gridColumn: `${w + 1} / span ${Math.min(3, weeks - w)}`,
                fontSize: "var(--text-xs)",
                color: "var(--text-tertiary)",
                height: 15,
                minWidth: 0,
                overflow: "hidden",
                whiteSpace: "nowrap",
              }}
            >
              {m}
            </div>
          ) : null
        )}
      </div>
      <div style={{ ...tracks, gridTemplateRows: "repeat(7, auto)", gridAutoFlow: "column", marginTop: 4 }}>
        {cellsRendered}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 10, fontSize: "var(--text-xs)", color: "var(--text-secondary)", flexWrap: "wrap", minWidth: 0 }}>
        <span>Less</span>
        {HEAT_RAMP.map((c, i) => (
          <div
            key={c}
            title={`${i === 0 ? "No words" : `Level ${i} of 4`} — ${["0 words", "1-49 words", "50-149 words", "150-399 words", "400+ words"][i]}`}
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

function StreakCard({ data }: { data: Insights }) {
  const streak = data.streakDays;
  const longest = data.longestStreakDays;
  return (
    <Card
      title={`${streak.toLocaleString()}-day streak`}
      action={
        <CardMeta
          label="Longest streak"
          value={`${longest.toLocaleString()} ${plural(longest, "day", "days")}`}
          title={`Your best run of consecutive days with at least one dictation: ${longest.toLocaleString()} days. The current run is ${streak.toLocaleString()}.`}
        />
      }
    >
      <div className="tt-eyebrow" style={{ marginBottom: 8 }}>
        Your habit · last 26 weeks
      </div>
      <Heatmap cells={data.heatmap} />
    </Card>
  );
}

/* ------------------------------------------------------------------ *
 * The detail tier
 * ------------------------------------------------------------------ */

function StatCard({ label, value, sub, spark, icon, info }: { label: string; value: string; sub?: string; spark?: number[]; icon?: IconName; info?: string }) {
  return (
    <div
      title={info}
      style={{
        background: "var(--surface)",
        border: "1px solid var(--border-subtle)",
        borderRadius: "var(--radius)",
        padding: "14px 16px",
        boxShadow: "var(--shadow-1)",
        minWidth: 0,
        display: "flex",
        flexDirection: "column",
        gap: 8,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
        {icon && (
          <span
            style={{
              width: 24,
              height: 24,
              borderRadius: 7,
              background: "var(--accent-soft)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <Icon name={icon} size={13} color="var(--accent)" />
          </span>
        )}
        <span className="tt-eyebrow" style={{ minWidth: 0 }}>
          {label}
        </span>
      </div>
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 8, minWidth: 0, flexWrap: "wrap" }}>
        {/* A counter must never split mid-number, so it keeps one line and the
            sparkline drops below it when the value is too long to share the row. */}
        <span style={{ fontSize: statFontSize(value), fontWeight: 650, lineHeight: 1, letterSpacing: "var(--tracking-tight)", color: "var(--text)", minWidth: 0, ...NUMERIC }}>{value}</span>
        {spark && spark.length >= 2 && <Sparkline values={spark} width={76} height={32} />}
      </div>
      {sub && <div style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", overflowWrap: "anywhere", minWidth: 0 }}>{sub}</div>}
    </div>
  );
}

function PhraseList({ items }: { items: RankedItem[] }) {
  if (items.length === 0) {
    return (
      <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0 }}>
        <Icon name="sparkles" size={16} color="var(--text-secondary)" />
        Nothing here yet — dictate a few times and patterns will appear.
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
            <div style={{ height: "100%", width: `${Math.round(item.share * 100)}%`, background: "linear-gradient(90deg, #60a5fa 0%, #2563eb 100%)", borderRadius: 4 }} />
          </div>
        </div>
      ))}
    </div>
  );
}

function LearningCard({ learning }: { learning?: LearningSummary | null }) {
  const fixes = learning?.topFixes ?? [];
  const terms = learning?.termsLearnedWeek ?? [];
  const editRate = learning?.editRate7d ?? null;
  const accuracy = learning?.accuracy ?? [];
  const latest = accuracy.length ? accuracy[accuracy.length - 1] : null;
  const first = accuracy.length ? accuracy[0] : null;
  const improved = latest && first ? first.werAfter - latest.werAfter : 0;
  return (
    <Card quiet title="What Teletype is learning" icon="wand">
      {fixes.length === 0 && terms.length === 0 && editRate == null && accuracy.length < 2 ? (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
          Nothing learned yet. Teletype starts learning the first time you correct a mishearing — everything it picks up stays on this device.
        </p>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
          {(editRate != null || latest) && (
            <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 12, flexWrap: "wrap", minWidth: 0 }}>
              <div style={{ display: "flex", gap: 20, minWidth: 0 }}>
                {editRate != null && (
                  <div
                    style={{ minWidth: 0 }}
                    title="Words you changed per 100 dictated words, last 7 days. Falling is improvement — it means Teletype's transcript needed less fixing."
                  >
                    <div className="tt-eyebrow" style={{ marginBottom: 4 }}>Edit rate · 7 days</div>
                    <div style={{ fontSize: "var(--text-lg)", fontWeight: 700, ...NUMERIC }}>
                      {editRate.toFixed(1)}
                      <span style={{ fontSize: "var(--text-xs)", fontWeight: 500, color: "var(--text-secondary)", marginLeft: 4 }}>per 100 words</span>
                    </div>
                  </div>
                )}
                {latest && (
                  <div
                    style={{ minWidth: 0 }}
                    title={`Personal word error rate from the latest replay check: ${(latest.werAfter * 100).toFixed(1)}% after the boost, ${(latest.werBefore * 100).toFixed(1)}% before. Computed on this device.`}
                  >
                    <div className="tt-eyebrow" style={{ marginBottom: 4 }}>Error rate · check</div>
                    <div style={{ fontSize: "var(--text-lg)", fontWeight: 700, ...NUMERIC }}>
                      {(latest.werAfter * 100).toFixed(1)}%
                      {improved > 0.0005 && (
                        <span style={{ fontSize: "var(--text-xs)", fontWeight: 600, color: "var(--success)", marginLeft: 6 }}>
                          −{(improved * 100).toFixed(1)} pts
                        </span>
                      )}
                    </div>
                  </div>
                )}
              </div>
              {accuracy.length >= 2 && (
                <Sparkline values={accuracy.map((p) => p.werAfter)} width={104} height={32} />
              )}
            </div>
          )}
          {fixes.length > 0 && (
            <div style={{ minWidth: 0 }}>
              <div className="tt-eyebrow" style={{ marginBottom: 8 }}>Most often corrected</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
                {fixes.slice(0, RANK_CAP).map((f) => (
                  <div
                    key={`${f.from}|${f.to}`}
                    title={`Heard "${f.from}", you wrote "${f.to}" — ${f.count.toLocaleString()} times`}
                    style={{ display: "flex", alignItems: "baseline", gap: 8, fontSize: "var(--text-sm)", minWidth: 0 }}
                  >
                    <span style={{ color: "var(--text-tertiary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 }}>{f.from}</span>
                    <span aria-hidden="true" style={{ color: "var(--text-tertiary)", flexShrink: 0 }}>→</span>
                    <span style={{ fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 }}>{f.to}</span>
                    <span style={{ marginLeft: "auto", color: "var(--text-secondary)", flexShrink: 0, fontWeight: 600, ...NUMERIC }}>{f.count.toLocaleString()}×</span>
                  </div>
                ))}
              </div>
            </div>
          )}
          {terms.length > 0 && (
            <div style={{ minWidth: 0 }}>
              <div className="tt-eyebrow" style={{ marginBottom: 8 }}>Learned this week · {terms.length.toLocaleString()}</div>
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
              </div>
            </div>
          )}
        </div>
      )}
    </Card>
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
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

/** Every grid on this screen uses the same collapse-safe track: a card never
    forces a horizontal scrollbar, it just drops to one column. */
const CARD_GRID: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(min(260px, 100%), 1fr))",
  gap: 16,
  width: "100%",
  minWidth: 0,
};

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
  const last7words = data.daily.slice(-7).map((d) => d.words);

  const customMin = firstActivityAt ? toInputDate(firstActivityAt) : undefined;
  const customMax = toInputDate(Date.now());

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, width: "100%", minWidth: 0, overflowX: "clip" }}>
      {/* Header: title left, the window control right. */}
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 16, flexWrap: "wrap", minWidth: 0 }}>
        <div style={{ minWidth: 0 }}>
          <h2 className="tt-screen-title">Insights</h2>
          <p className="tt-screen-sub" style={{ margin: 0, maxWidth: 520 }}>
            What you dictated, and what Teletype fixed for you — computed locally, on this device.
          </p>
        </div>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 6, minWidth: 0, maxWidth: "100%" }}>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 2, justifyContent: "flex-end", background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 999, padding: 4, boxShadow: "var(--shadow-1)", maxWidth: "100%", minWidth: 0 }}>
            {RANGES.map((r) => (
              <button
                key={r.id}
                onClick={() => setRange(r.id)}
                title="Sets the window for totals, apps, phrases and pace. Habits, records and milestones stay all-time."
                style={{
                  border: "none",
                  background: range === r.id ? "var(--accent)" : "transparent",
                  color: range === r.id ? "var(--on-accent)" : "var(--text-secondary)",
                  fontSize: "var(--text-sm)",
                  fontWeight: 600,
                  padding: "6px 14px",
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
              Totals follow this window; habits, records and milestones are all-time
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
            borderRadius: "var(--radius-sm)",
            background: "var(--warning-soft)",
            color: "var(--warning)",
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            minWidth: 0,
          }}
        >
          <Icon name="info" size={16} color="var(--warning)" />
          <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>{data.polishStatus}</span>
        </div>
      )}

      {/* Row 1 — the three headline numbers. */}
      <div style={CARD_GRID}>
        <PaceCard impact={impact} />
        <FixesCardHeadline data={data} />
        <WordsCard data={data} />
      </div>

      {/* Row 2 — usage and habit. */}
      <div style={CARD_GRID}>
        <AppUsageCard apps={data.topApps} appTotal={data.topApps.reduce((s, a) => s + a.count, 0)} totalDictations={data.totalDictations} />
        <StreakCard data={data} />
      </div>

      {/* Detail tier — everything that was here before, one voice down. */}
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
          <span className="tt-eyebrow">More detail</span>
          <span style={{ flex: 1, height: 1, background: "var(--border-subtle)", minWidth: 0 }} />
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(160px, 100%), 1fr))", gap: 12, width: "100%", minWidth: 0 }}>
          <StatCard label="Words today" value={data.wordsToday.toLocaleString()} spark={last7words} icon="calendar-check" />
          <StatCard label="Words / dictation" value={data.avgWordsPerDictation.toLocaleString()} icon="messages-square" />
          <StatCard label="Dictations" value={data.totalDictations.toLocaleString()} icon="mic" />
          <StatCard
            label="Time saved"
            value={hasData ? impact.timeSavedLabel : "—"}
            sub={impact.timesFaster != null && impact.timesFaster > 0 ? `${impact.timesFaster}× faster than typing at ${TYPING_WPM} wpm` : `vs typing the same words at ${TYPING_WPM} wpm`}
            icon="clock"
            info="Minutes saved = the words you dictated, priced at the 40 wpm average typing pace, minus the time you actually spent speaking."
          />
          <StatCard label="Busiest hour" value={hourLabel(data.busiestHour)} icon="zap" />
          <StatCard label="Vocabulary" value={data.vocabularySize.toLocaleString()} sub="distinct words" icon="library" />
          <StatCard label="Filler removed" value={data.fillerRemoved.toLocaleString()} sub="words cleaned, all time" icon="scissors" />
          <StatCard label="AutoText" value={data.autotextExpansions.toLocaleString()} sub="expansions, all time" icon="keyboard" />
          <StatCard label="Timed takes" value={impact.ratedTakes.toLocaleString()} sub={`${impact.skippedTakes.toLocaleString()} too short to time`} icon="target" />
          {(data.dictionaryWords ?? 0) > 0 && (
            <StatCard label="Dictionary" value={data.dictionaryWords.toLocaleString()} sub={`${data.learnedWords ?? 0} auto-learned`} icon="dictionary" />
          )}
        </div>

        <div style={CARD_GRID}>
          <Card quiet title="Words per day" icon="chart-column">
            {hasData ? (
              <BarChart daily={data.daily} />
            ) : (
              <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0 }}>
                <Icon name="chart-column" size={16} color="var(--text-secondary)" />
                No activity yet — your daily words will chart here.
              </p>
            )}
          </Card>
          <Card quiet title="Top phrases" icon="sparkles">
            <PhraseList items={data.topPhrases} />
          </Card>
        </div>

        <div style={CARD_GRID}>
          <Card quiet title="Your records · all time" icon="trophy">
            {!hasData ? (
              <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0 }}>
                <Icon name="trophy" size={16} color="var(--text-secondary)" />
                No records yet — your best dictations will show up here.
              </p>
            ) : (
              <div style={{ display: "flex", gap: 14, flexWrap: "wrap", minWidth: 0 }}>
                {data.records.map((r) => (
                  <div key={r.label} style={{ flex: "1 1 160px", minWidth: 0, display: "flex", alignItems: "center", gap: 10 }}>
                    <div style={{ width: 32, height: 32, borderRadius: 9, background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
                      <Icon name="trophy" size={17} color="var(--accent)" />
                    </div>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={r.label}>{r.label}</div>
                      <div style={{ fontSize: "var(--text-lg)", fontWeight: 650, marginTop: 1, letterSpacing: "var(--tracking-tight)", ...NUMERIC }}>{r.value}</div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </Card>
          <Card quiet title="Milestones · all time" icon="medal">
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
          </Card>
          <LearningCard learning={data.learning} />
        </div>
      </div>
    </div>
  );
}
