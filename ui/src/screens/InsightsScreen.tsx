import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";

interface RankedItem {
  label: string;
  count: number;
  share: number;
}

interface Impact {
  wordsPerMinute: number;
  timesFaster: number;
  timeSavedMinutes: number;
  timeSavedLabel: string;
  minutesSpoken: number;
  minutesTyped: number;
  essays: number;
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
}

const EMPTY: Insights = {
  topPhrases: [],
  topApps: [],
  avgWordsPerDictation: 0,
  busiestHour: null,
  vocabularySize: 0,
  totalWords: 0,
  totalDictations: 0,
  streakDays: 0,
  longestStreakDays: 0,
  impact: {
    wordsPerMinute: 0,
    timesFaster: 0,
    timeSavedMinutes: 0,
    timeSavedLabel: "0 min",
    minutesSpoken: 0,
    minutesTyped: 0,
    essays: 0,
  },
  records: [],
  milestones: [],
  heatmap: [],
  daily: [],
  wordsToday: 0,
  wordsLast7Days: 0,
  avgWordsPerDay: 0,
};

const ACCENT = "#2563eb";
const ACCENT_HOVER = "#1d4ed8";
const HEAT_COLORS = ["#eef1f6", "#dbe6fd", "#b3ccfb", "#7ba3f7", "#2563eb"];
const APP_COLORS = ["#2563eb", "#16a34a", "#d97706", "#9333ea", "#0284c7", "#e11d48", "#65a30d", "#db2777"];

function heatColor(level: number): string {
  return HEAT_COLORS[Math.max(0, Math.min(4, level))];
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
}: {
  title?: string;
  icon?: IconName;
  children: React.ReactNode;
  style?: React.CSSProperties;
  span?: number;
}) {
  return (
    <div
      style={{
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: "18px 20px",
        gridColumn: span ? `span ${span}` : undefined,
        ...style,
      }}
    >
      {title && (
        <h3 style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 16, letterSpacing: 0.4 }}>
          {icon && <Icon name={icon} size={16} color={ACCENT} />}
          {title}
        </h3>
      )}
      {children}
    </div>
  );
}

function Ring({
  pct,
  size = 120,
  stroke = 12,
  color = ACCENT,
  track = "#e6e9ef",
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
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={offset}
          style={{ transition: "stroke-dashoffset 0.5s ease" }}
        />
      </svg>
      <div
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {children}
      </div>
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
                  key={item.label}
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
    <div style={{ display: "flex", alignItems: "flex-end", gap: 6, height: 150 }}>
      {daily.map((d) => (
        <div
          key={d.date}
          style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", gap: 6, height: "100%", justifyContent: "flex-end" }}
          title={`${d.label}: ${d.words} words, ${d.dictations} dictations`}
        >
          <div style={{ fontSize: 10, color: "var(--text-secondary)", height: 12 }}>{d.words > 0 ? d.words : ""}</div>
          <div
            style={{
              width: "100%",
              maxWidth: 30,
              height: `${Math.max(3, (d.words / max) * 100)}%`,
              borderRadius: 4,
              background: d.words > 0 ? ACCENT : "var(--border)",
              transition: "height 0.3s",
            }}
          />
          <div style={{ fontSize: 10, color: "var(--text-secondary)", whiteSpace: "nowrap" }}>{d.label.split(" ")[0]}</div>
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
    <div style={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: "var(--radius)", padding: "16px 18px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: 0.4 }}>
        {icon && <Icon name={icon} size={14} color={ACCENT} />}
        {label}
      </div>
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 8 }}>
        <div style={{ fontSize: 28, fontWeight: 800, marginTop: 4, lineHeight: 1 }}>{value}</div>
        {spark && spark.length >= 2 && <Sparkline values={spark} width={72} height={34} />}
      </div>
      {sub && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4 }}>{sub}</div>}
    </div>
  );
}

export default function InsightsScreen() {
  const [data, setData] = useState<Insights>(EMPTY);

  const refresh = useCallback(() => {
    invoke<Insights>("get_insights").then(setData).catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  const impact = data.impact;
  const hasData = data.totalWords > 0;
  const weekGoal = Math.max(1000, Math.round((data.avgWordsPerDay * 7 * 2) / 100) * 100);
  const weekPct = data.wordsLast7Days / weekGoal;
  const last7words = data.daily.slice(-7).map((d) => d.words);
  const last14words = data.daily.map((d) => d.words);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 980 }}>
      <div>
        <h2 style={{ fontSize: 22, fontWeight: 800 }}>Insights</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>
          Your impact, habit, and records — computed locally from your dictation history.
        </p>
      </div>

      {/* Hero: impact + weekly goal ring */}
      <div
        style={{
          display: "flex",
          gap: 20,
          flexWrap: "wrap",
          background: "linear-gradient(135deg, #1d4ed8 0%, #2563eb 55%, #3b82f6 100%)",
          borderRadius: "var(--radius)",
          padding: "22px 24px",
          color: "#fff",
        }}
      >
        <div style={{ flex: "1 1 320px", display: "flex", flexDirection: "column" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, textTransform: "uppercase", letterSpacing: 0.5, opacity: 0.85 }}>
            <Icon name="zap" size={15} color="#fff" />
            Time saved vs typing
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 14, marginTop: 6 }}>
            <div style={{ fontSize: 42, fontWeight: 800, lineHeight: 1 }}>{hasData ? impact.timeSavedLabel : "—"}</div>
            {hasData && impact.timesFaster > 0 && (
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
                {impact.timesFaster}× faster
              </div>
            )}
          </div>
          <div style={{ fontSize: 13, opacity: 0.9, marginTop: 8 }}>
            {hasData ? (
              <>
                You speak at <b>{impact.wordsPerMinute} wpm</b> vs a 40 wpm typing pace.
              </>
            ) : (
              "Dictate your first message to start measuring the time you save."
            )}
          </div>
          <div style={{ display: "flex", gap: 24, marginTop: 16, flexWrap: "wrap" }}>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 22, fontWeight: 800 }}>
                {data.totalWords.toLocaleString()}
              </div>
              <div style={{ fontSize: 12, opacity: 0.85 }}>total words</div>
            </div>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 22, fontWeight: 800 }}>
                <Icon name="messages-square" size={16} color="rgba(255,255,255,0.9)" />
                {data.totalDictations.toLocaleString()}
              </div>
              <div style={{ fontSize: 12, opacity: 0.85 }}>dictations</div>
            </div>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 22, fontWeight: 800 }}>
                <Icon name="zap" size={16} color="rgba(255,255,255,0.9)" />
                {impact.wordsPerMinute}
              </div>
              <div style={{ fontSize: 12, opacity: 0.85 }}>wpm speaking</div>
            </div>
          </div>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 16, flex: "0 0 auto" }}>
          <Ring pct={weekPct} size={128} stroke={12} color="#ffffff" track="rgba(255,255,255,0.25)">
            <div style={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
              <Icon name="target" size={16} color="rgba(255,255,255,0.9)" />
              <div style={{ fontSize: 24, fontWeight: 800, marginTop: 2 }}>{Math.round(weekPct * 100)}%</div>
              <div style={{ fontSize: 11, opacity: 0.85 }}>weekly goal</div>
            </div>
          </Ring>
          <div>
            <div style={{ fontSize: 13, opacity: 0.9 }}>{data.wordsLast7Days.toLocaleString()} / {weekGoal.toLocaleString()} words</div>
            <div style={{ fontSize: 12, opacity: 0.7, marginTop: 2 }}>last 7 days</div>
          </div>
        </div>
      </div>

      {/* Key metrics */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 14 }}>
        <StatCard label="Words today" value={data.wordsToday.toLocaleString()} spark={last7words} icon="calendar-check" />
        <StatCard label="Last 7 days" value={data.wordsLast7Days.toLocaleString()} sub={`avg ${data.avgWordsPerDay}/active day`} icon="trending-up" />
        <StatCard label="Streak" value={`${data.streakDays}`} sub={`best ${data.longestStreakDays} days`} icon="flame" />
        <StatCard label="Vocabulary" value={data.vocabularySize.toLocaleString()} sub="distinct words" icon="library" />
      </div>

      {/* Activity: bar chart + heatmap */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))", gap: 14 }}>
        <Card title="Words per day · last 14 days" icon="chart-column">
          {hasData ? (
            <BarChart daily={data.daily} />
          ) : (
            <p style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--text-secondary)" }}>
              <Icon name="chart-column" size={16} color="var(--text-secondary)" />
              No activity yet — your daily words will chart here.
            </p>
          )}
        </Card>
        <Card title="Your habit · last 12 weeks" icon="flame">
          <Heatmap cells={data.heatmap} />
        </Card>
      </div>

      {/* Where you dictate + speaking speed */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))", gap: 14 }}>
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
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              {data.topPhrases.map((item) => (
                <div key={item.label}>
                  <div style={{ display: "flex", justifyContent: "space-between", fontSize: 13, marginBottom: 3 }}>
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{item.label}</span>
                    <span style={{ color: "var(--text-secondary)", flexShrink: 0, marginLeft: 8 }}>{item.count}×</span>
                  </div>
                  <div style={{ height: 6, background: "var(--surface-2)", borderRadius: 3 }}>
                    <div style={{ height: "100%", width: `${Math.round(item.share * 100)}%`, background: ACCENT, borderRadius: 3 }} />
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>

      {/* Records */}
      <Card title="Your records" icon="trophy">
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
                  <div style={{ fontSize: 19, fontWeight: 800, marginTop: 1 }}>{r.value}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>

      {/* Milestones */}
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

      {/* Writing habits */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 14 }}>
        <StatCard label="Avg words / dictation" value={data.avgWordsPerDictation.toLocaleString()} icon="messages-square" />
        <StatCard label="Busiest hour" value={hourLabel(data.busiestHour)} icon="clock" />
        <StatCard label="14-day trend" value={`${data.daily.reduce((s, d) => s + d.words, 0).toLocaleString()}`} sub="words" spark={last14words} icon="trending-up" />
      </div>
    </div>
  );
}
