import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

type Screen =
  | "home"
  | "dictation"
  | "transforms"
  | "autotext"
  | "personalization"
  | "models"
  | "settings";

interface HistoryEntry {
  id: string;
  createdAt: number;
  text: string;
  appName: string;
  appType: string;
}

interface Insights {
  totalWords: number;
  totalDictations: number;
  streakDays: number;
  impact: { wordsPerMinute: number; timeSavedLabel: string };
  wordsLast7Days: number;
  avgWordsPerDay: number;
}

const ACCENT = "#2563eb";

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

function dayLabel(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
  if (sameDay(d, today)) return "TODAY";
  if (sameDay(d, yesterday)) return "YESTERDAY";
  return d.toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric" }).toUpperCase();
}

function exactNumber(n: number): string {
  return Math.round(n).toLocaleString("en-US");
}

export default function HomeScreen({
  onNavigate,
  listening,
}: {
  onNavigate: (s: Screen) => void;
  listening: boolean;
}) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [insights, setInsights] = useState<Insights | null>(null);

  const refresh = useCallback(() => {
    invoke<HistoryEntry[]>("list_dictation_history").then(setEntries).catch(() => {});
    invoke<Insights>("get_insights").then(setInsights).catch(() => {});
  }, []);

  useEffect(refresh, [refresh, listening]);

  const grouped = useMemo(() => {
    const groups: { label: string; items: HistoryEntry[] }[] = [];
    for (const e of entries) {
      const label = dayLabel(e.createdAt);
      const last = groups[groups.length - 1];
      if (last && last.label === label) last.items.push(e);
      else groups.push({ label, items: [e] });
    }
    return groups;
  }, [entries]);

  const totalWords = insights?.totalWords ?? 0;
  const wpm = insights?.impact.wordsPerMinute ?? 0;
  const streak = insights?.streakDays ?? 0;
  const wordsLast7 = insights?.wordsLast7Days ?? 0;
  const weekGoal = Math.max(1000, Math.round(((insights?.avgWordsPerDay ?? 0) * 7 * 2) / 100) * 100);
  const weekPct = Math.min(100, Math.round((wordsLast7 / weekGoal) * 100));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <h1 style={{ fontSize: 26, fontWeight: 700, letterSpacing: -0.5 }}>
        Welcome back
      </h1>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.5fr) minmax(280px, 1fr)", gap: 20 }}>
        {/* Left column: hero + history */}
        <div style={{ display: "flex", flexDirection: "column", gap: 20, minWidth: 0 }}>
          {/* Hero banner */}
          <div
            style={{
              borderRadius: "var(--radius)",
              overflow: "hidden",
              padding: "28px 30px",
              color: "#fff",
              position: "relative",
              minHeight: 190,
            }}
          >
            <img
              src="/home-hero.jpeg"
              alt=""
              aria-hidden
              style={{
                position: "absolute",
                inset: 0,
                width: "100%",
                height: "100%",
                objectFit: "cover",
                objectPosition: "right center",
                pointerEvents: "none",
                zIndex: 0,
              }}
            />
            <div
              style={{
                position: "absolute",
                inset: 0,
                background: "linear-gradient(90deg, rgba(9,12,22,0.92) 0%, rgba(9,12,22,0.72) 38%, rgba(9,12,22,0.15) 66%, rgba(9,12,22,0) 100%)",
                zIndex: 1,
              }}
            />
            <div style={{ position: "relative", zIndex: 2 }}>
              <h2 style={{ fontSize: 22, fontWeight: 700, marginBottom: 8, fontFamily: "Georgia, serif" }}>
                Transform works anywhere you write
              </h2>
              <p style={{ fontSize: 14, opacity: 0.85, maxWidth: 340, marginBottom: 20, lineHeight: 1.5 }}>
                Apply a Transform to rewrite, clean up, or restructure text after you dictate.
              </p>
              <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                <button
                  onClick={() => onNavigate("transforms")}
                  style={{
                    background: "rgba(255,255,255,0.15)",
                    border: "1px solid rgba(255,255,255,0.25)",
                    borderRadius: 8,
                    color: "#fff",
                    fontSize: 13,
                    fontWeight: 600,
                    padding: "8px 18px",
                    cursor: "pointer",
                    backdropFilter: "blur(8px)",
                  }}
                >
                  Try it out
                </button>
                <button
                  onClick={() => onNavigate("transforms")}
                  style={{
                    background: "none",
                    border: "none",
                    color: "rgba(255,255,255,0.8)",
                    fontSize: 13,
                    fontWeight: 500,
                    cursor: "pointer",
                    padding: "8px 4px",
                  }}
                >
                  How it works
                </button>
              </div>
            </div>
          </div>

          {/* History list */}
          {grouped.length === 0 ? (
            <div style={{ padding: 40, textAlign: "center", color: "var(--text-secondary)", fontSize: 14 }}>
              <Icon name="mic" size={28} color="var(--text-secondary)" style={{ marginBottom: 12 }} />
              <p>No dictations yet. Hold your hotkey and start talking.</p>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
              {grouped.map((group) => (
                <div key={group.label}>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      marginBottom: 8,
                    }}
                  >
                    <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: 0.8, color: "var(--text-secondary)" }}>
                      {group.label}
                    </span>
                    <Icon name="search" size={14} color="var(--text-secondary)" />
                  </div>
                  <div
                    style={{
                      background: "var(--surface)",
                      border: "1px solid var(--border)",
                      borderRadius: "var(--radius)",
                      overflow: "hidden",
                    }}
                  >
                    {group.items.map((e, i) => (
                      <div
                        key={e.id}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 14,
                          padding: "12px 16px",
                          borderBottom: i < group.items.length - 1 ? "1px solid var(--border)" : "none",
                        }}
                      >
                        <span style={{ fontSize: 12, color: "var(--text-secondary)", width: 56, flexShrink: 0, fontVariantNumeric: "tabular-nums" }}>
                          {formatTime(e.createdAt)}
                        </span>
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            fontSize: 13,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                            color: "var(--text)",
                          }}
                        >
                          {e.text}
                        </span>
                        <div style={{ display: "flex", gap: 6, flexShrink: 0, opacity: 0.55 }}>
                          <button
                            type="button"
                            title="Copy"
                            onClick={(ev) => {
                              ev.stopPropagation();
                              navigator.clipboard.writeText(e.text);
                            }}
                            style={{ background: "none", border: "none", cursor: "pointer", padding: 4, display: "flex", color: "inherit" }}
                          >
                            <Icon name="copy" size={15} color="var(--text-secondary)" />
                          </button>
                          <button
                            type="button"
                            title="Delete"
                            onClick={(ev) => {
                              ev.stopPropagation();
                              invoke("delete_dictation_entry", { id: e.id }).then(refresh).catch(() => {});
                            }}
                            style={{ background: "none", border: "none", cursor: "pointer", padding: 4, display: "flex", color: "inherit" }}
                          >
                            <Icon name="trash" size={15} color="var(--text-secondary)" />
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Right sidebar */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          {/* Stats card */}
          <div
            style={{
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              padding: "20px 22px",
            }}
          >
            <StatRow value={exactNumber(totalWords)} label="total words" />
            <StatRow value={wpm ? `${wpm}` : "—"} label="wpm" />
            <StatRow value={`${streak}`} label={`day streak${streak === 1 ? "" : "s"}`} />
          </div>

          {/* Weekly goal */}
          <div
            style={{
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              padding: "20px 22px",
            }}
          >
            <div style={{ display: "flex", alignItems: "baseline", gap: 6, marginBottom: 6 }}>
              <span style={{ fontSize: 18, fontWeight: 700, color: ACCENT }}>
                {exactNumber(Math.max(0, weekGoal - wordsLast7))}
              </span>
              <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>words left</span>
              <Icon name="info" size={13} color="var(--text-secondary)" />
            </div>
            <p style={{ fontSize: 12, color: "var(--text-secondary)", lineHeight: 1.5, marginBottom: 10 }}>
              You're close to your weekly dictation limit. Keep going!
            </p>
            <div style={{ height: 6, background: "var(--surface-2)", borderRadius: 3, overflow: "hidden" }}>
              <div
                style={{
                  height: "100%",
                  width: `${weekPct}%`,
                  background: ACCENT,
                  borderRadius: 3,
                  transition: "width 0.4s ease",
                }}
              />
            </div>
            <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 6 }}>
              {wordsLast7.toLocaleString()} / {weekGoal.toLocaleString()} words this week
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function StatRow({ value, label }: { value: string; label: string }) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 8, marginBottom: 12 }}>
      <span style={{ fontSize: 28, fontWeight: 800, fontVariantNumeric: "tabular-nums" }}>{value}</span>
      <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>{label}</span>
    </div>
  );
}
