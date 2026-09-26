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
  impact: { wordsPerMinute: number; timeSavedLabel: string; timeSavedMinutes: number };
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
  const timeSaved = insights?.impact.timeSavedMinutes ?? 0;
  const timeSavedLabel = insights?.impact.timeSavedLabel ?? "0 min";
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
              padding: "26px 30px",
              color: "#fff",
              position: "relative",
              minHeight: 168,
              background: "linear-gradient(120deg, #0f172a 0%, #1e3a8a 55%, #2563eb 100%)",
            }}
          >
            {/* Soft decorative glow */}
            <div
              style={{
                position: "absolute",
                right: -40,
                top: -60,
                width: 220,
                height: 220,
                borderRadius: "50%",
                background: "radial-gradient(circle, rgba(96,165,250,0.35) 0%, rgba(96,165,250,0) 70%)",
                pointerEvents: "none",
              }}
            />
            <div
              style={{
                position: "absolute",
                right: 28,
                bottom: -30,
                width: 150,
                height: 150,
                borderRadius: "50%",
                background: "radial-gradient(circle, rgba(59,130,246,0.3) 0%, rgba(59,130,246,0) 70%)",
                pointerEvents: "none",
              }}
            />
            <div style={{ position: "relative", zIndex: 2 }}>
              <div
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 6,
                  background: "rgba(255,255,255,0.12)",
                  borderRadius: 999,
                  padding: "4px 12px",
                  fontSize: 11,
                  fontWeight: 600,
                  letterSpacing: 0.3,
                  marginBottom: 14,
                }}
              >
                <Icon name="wand" size={12} color="#93c5fd" />
                Transform
              </div>
              <h2 style={{ fontSize: 21, fontWeight: 700, marginBottom: 6, letterSpacing: -0.3, maxWidth: 400 }}>
                Rewrite, clean up, or restructure after you dictate
              </h2>
              <p style={{ fontSize: 13.5, opacity: 0.82, maxWidth: 380, marginBottom: 18, lineHeight: 1.5, margin: 0 }}>
                Apply a Transform to polish your words in any app you write in.
              </p>
              <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                <button
                  onClick={() => onNavigate("transforms")}
                  style={{
                    background: "#fff",
                    border: "none",
                    borderRadius: 8,
                    color: "#1e3a8a",
                    fontSize: 13,
                    fontWeight: 700,
                    padding: "9px 18px",
                    cursor: "pointer",
                  }}
                >
                  Try it out
                </button>
                <button
                  onClick={() => onNavigate("transforms")}
                  style={{
                    background: "none",
                    border: "none",
                    color: "rgba(255,255,255,0.85)",
                    fontSize: 13,
                    fontWeight: 500,
                    cursor: "pointer",
                    padding: "9px 6px",
                  }}
                >
                  How it works
                </button>
              </div>
            </div>
          </div>

          {/* History list */}
          {grouped.length === 0 ? (
            <div style={{ padding: 48, textAlign: "center", color: "var(--text-secondary)", fontSize: 14, background: "var(--surface)", border: "1px dashed var(--border)", borderRadius: "var(--radius)" }}>
              <Icon name="mic" size={30} color="var(--text-secondary)" style={{ marginBottom: 12 }} />
              <p style={{ margin: 0, fontWeight: 600, color: "var(--text)" }}>No dictations yet</p>
              <p style={{ margin: "4px 0 0" }}>Hold your hotkey and start talking.</p>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 22, minWidth: 0 }}>
              {grouped.map((group) => (
                <div key={group.label} style={{ minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8 }}>
                    <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: 0.8, color: "var(--text-secondary)" }}>
                      {group.label}
                    </span>
                    <span style={{ flex: 1, height: 1, background: "var(--border)" }} />
                    <span style={{ fontSize: 11, color: "var(--text-secondary)", fontVariantNumeric: "tabular-nums" }}>
                      {group.items.length}
                    </span>
                  </div>
                  <div
                    style={{
                      background: "var(--surface)",
                      border: "1px solid var(--border)",
                      borderRadius: "var(--radius)",
                      overflow: "hidden",
                      minWidth: 0,
                    }}
                  >
                    {group.items.map((e, i) => (
                      <HistoryRow key={e.id} entry={e} isLast={i === group.items.length - 1} onChanged={refresh} />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Right sidebar */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          {/* Your impact card */}
          <div
            style={{
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              padding: "20px 22px",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16 }}>
              <Icon name="zap" size={15} color={ACCENT} />
              <h3 style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.5, color: "var(--text-secondary)", margin: 0 }}>
                Your impact
              </h3>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <StatRow icon="messages-square" value={exactNumber(totalWords)} label="total words" />
              <StatRow icon="zap" value={wpm ? `${wpm}` : "—"} label="wpm speaking" divider />
              <StatRow icon="flame" value={`${streak}`} label={`day streak${streak === 1 ? "" : "s"}`} />
            </div>
          </div>

          {/* Time saved card */}
          {timeSaved > 0 && (
            <div
              style={{
                background: "linear-gradient(135deg, #065f46 0%, #059669 60%, #10b981 100%)",
                borderRadius: "var(--radius)",
                padding: "20px 22px",
                color: "#fff",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
                <Icon name="clock" size={15} color="#fff" />
                <h3 style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.5, opacity: 0.9, margin: 0 }}>
                  Time saved
                </h3>
              </div>
              <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                <span style={{ fontSize: 32, fontWeight: 800, lineHeight: 1, fontVariantNumeric: "tabular-nums" }}>{timeSavedLabel}</span>
              </div>
              <div style={{ fontSize: 12, opacity: 0.85, marginTop: 8 }}>
                vs typing {exactNumber(totalWords)} words
              </div>
            </div>
          )}

          {/* Weekly goal */}
          <div
            style={{
              background: "linear-gradient(135deg, #1d4ed8 0%, #2563eb 60%, #3b82f6 100%)",
              borderRadius: "var(--radius)",
              padding: "20px 22px",
              color: "#fff",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 14 }}>
              <Icon name="target" size={15} color="#fff" />
              <h3 style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.5, opacity: 0.9, margin: 0 }}>
                Weekly goal
              </h3>
            </div>
            <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
              <span style={{ fontSize: 32, fontWeight: 800, lineHeight: 1, fontVariantNumeric: "tabular-nums" }}>{weekPct}%</span>
              <span style={{ fontSize: 13, opacity: 0.85 }}>
                {weekPct >= 100 ? "goal reached!" : `${exactNumber(Math.max(0, weekGoal - wordsLast7))} words to go`}
              </span>
            </div>
            <div style={{ height: 8, background: "rgba(255,255,255,0.25)", borderRadius: 4, overflow: "hidden", marginTop: 14 }}>
              <div
                style={{
                  height: "100%",
                  width: `${weekPct}%`,
                  background: "#fff",
                  borderRadius: 4,
                  transition: "width 0.5s ease",
                }}
              />
            </div>
            <div style={{ fontSize: 12, opacity: 0.85, marginTop: 8, fontVariantNumeric: "tabular-nums" }}>
              {wordsLast7.toLocaleString()} / {weekGoal.toLocaleString()} words this week
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function StatRow({
  value,
  label,
  icon,
  divider,
}: {
  value: string;
  label: string;
  icon: import("../components/Icon").IconName;
  divider?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 0",
        borderTop: divider ? "1px solid var(--border)" : "none",
      }}
    >
      <div
        style={{
          width: 36,
          height: 36,
          borderRadius: 9,
          background: "var(--accent-soft)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        <Icon name={icon} size={18} color={ACCENT} />
      </div>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span style={{ fontSize: 24, fontWeight: 800, fontVariantNumeric: "tabular-nums" }}>{value}</span>
        <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>{label}</span>
      </div>
    </div>
  );
}

function HistoryRow({
  entry,
  isLast,
  onChanged,
}: {
  entry: HistoryEntry;
  isLast: boolean;
  onChanged: () => void;
}) {
  const [hover, setHover] = useState(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 16px",
        borderBottom: isLast ? "none" : "1px solid var(--border)",
        background: hover ? "var(--surface-2)" : "transparent",
        transition: "background 0.12s ease",
      }}
    >
      <span
        style={{
          fontSize: 12,
          color: "var(--text-secondary)",
          width: 54,
          flexShrink: 0,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {formatTime(entry.createdAt)}
      </span>
      <span
        style={{
          flex: 1,
          minWidth: 0,
          fontSize: 13,
          lineHeight: 1.4,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--text)",
        }}
      >
        {entry.text}
      </span>
      {entry.appName && (
        <span
          style={{
            fontSize: 11,
            color: "var(--text-secondary)",
            background: "var(--surface-2)",
            borderRadius: 6,
            padding: "2px 8px",
            flexShrink: 0,
            maxWidth: 96,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {entry.appName}
        </span>
      )}
      <div
        style={{
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          gap: 2,
          flexShrink: 0,
          opacity: hover ? 1 : 0.4,
          transition: "opacity 0.12s ease",
        }}
      >
        <button
          type="button"
          title="Copy"
          onClick={(ev) => {
            ev.stopPropagation();
            navigator.clipboard.writeText(entry.text);
          }}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            padding: 5,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: 6,
          }}
        >
          <Icon name="copy" size={15} color="var(--text-secondary)" />
        </button>
        <button
          type="button"
          title="Delete"
          onClick={(ev) => {
            ev.stopPropagation();
            invoke("delete_dictation_entry", { id: entry.id }).then(onChanged).catch(() => {});
          }}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            padding: 5,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: 6,
          }}
        >
          <Icon name="trash" size={15} color="var(--text-secondary)" />
        </button>
      </div>
    </div>
  );
}
