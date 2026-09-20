import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface DayStat {
  label: string;
  date: number;
  words: number;
  dictations: number;
}

interface DashboardStats {
  totalWords: number;
  totalDictations: number;
  streakDays: number;
  avgWordsPerDay: number;
  wordsToday: number;
  wordsLast7Days: number;
  daily: DayStat[];
  estMinutesSpoken: number;
}

const EMPTY: DashboardStats = {
  totalWords: 0,
  totalDictations: 0,
  streakDays: 0,
  avgWordsPerDay: 0,
  wordsToday: 0,
  wordsLast7Days: 0,
  daily: [],
  estMinutesSpoken: 0,
};

function StatCard({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div
      style={{
        flex: 1,
        minWidth: 140,
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: "16px 18px",
      }}
    >
      <div style={{ fontSize: 12, color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: 0.4 }}>
        {label}
      </div>
      <div style={{ fontSize: 28, fontWeight: 700, marginTop: 4 }}>{value}</div>
      {hint && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>{hint}</div>}
    </div>
  );
}

function BarChart({ daily }: { daily: DayStat[] }) {
  const max = Math.max(1, ...daily.map((d) => d.words));
  return (
    <div
      style={{
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: "18px 20px",
      }}
    >
      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 16 }}>
        Words per day · last 14 days
      </h3>
      <div style={{ display: "flex", alignItems: "flex-end", gap: 8, height: 140 }}>
        {daily.map((d) => (
          <div key={d.date} style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", gap: 6, height: "100%", justifyContent: "flex-end" }} title={`${d.label}: ${d.words} words, ${d.dictations} dictations`}>
            <div style={{ fontSize: 10, color: "var(--text-secondary)" }}>{d.words > 0 ? d.words : ""}</div>
            <div
              style={{
                width: "100%",
                maxWidth: 34,
                height: `${Math.max(2, (d.words / max) * 100)}%`,
                borderRadius: 4,
                background: d.words > 0 ? "var(--accent)" : "var(--border)",
                transition: "height 0.3s",
              }}
            />
            <div style={{ fontSize: 10, color: "var(--text-secondary)", whiteSpace: "nowrap" }}>{d.label.split(" ")[0]}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

export default function DashboardScreen() {
  const [stats, setStats] = useState<DashboardStats>(EMPTY);

  const refresh = useCallback(() => {
    invoke<DashboardStats>("get_dashboard_stats")
      .then(setStats)
      .catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, maxWidth: 860 }}>
      <div>
        <h2 style={{ fontSize: 20, fontWeight: 700 }}>Dashboard</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>Your dictation activity, computed locally.</p>
      </div>

      <div style={{ display: "flex", gap: 14, flexWrap: "wrap" }}>
        <StatCard label="Words today" value={stats.wordsToday.toLocaleString()} />
        <StatCard label="Last 7 days" value={stats.wordsLast7Days.toLocaleString()} />
        <StatCard label="Total words" value={stats.totalWords.toLocaleString()} hint={`${stats.totalDictations.toLocaleString()} dictations`} />
        <StatCard label="Streak" value={`${stats.streakDays} day${stats.streakDays === 1 ? "" : "s"}`} />
        <StatCard label="Avg / active day" value={stats.avgWordsPerDay.toLocaleString()} hint="last 7 days" />
        <StatCard label="Est. time spoken" value={`${stats.estMinutesSpoken} min`} />
      </div>

      <BarChart daily={stats.daily} />
    </div>
  );
}
