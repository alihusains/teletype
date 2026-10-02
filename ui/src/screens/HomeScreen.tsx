import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { PrivacyBadge, ShieldIcon } from "../lib/PrivacyBadge";
import { useTauriEvent } from "../lib/useTauriEvent";

const PRIVACY_DISMISSED_KEY = "teletype.privacyRowDismissed";

function loadPrivacyDismissed(): boolean {
  try {
    return localStorage.getItem(PRIVACY_DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

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
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [insights, setInsights] = useState<Insights | null>(null);
  const [modelReady, setModelReady] = useState<boolean | null>(null);
  const [privacyDismissed, setPrivacyDismissed] = useState(loadPrivacyDismissed);

  const dismissPrivacy = () => {
    setPrivacyDismissed(true);
    try {
      localStorage.setItem(PRIVACY_DISMISSED_KEY, "1");
    } catch {
      // ignore quota / private mode
    }
  };

  const refresh = useCallback(() => {
    invoke<HistoryEntry[]>("list_dictation_history").then(setEntries).catch(() => {});
    invoke<Insights>("get_insights", { range: "week" }).then(setInsights).catch(() => {});
    invoke<{ downloaded: boolean }[]>("list_speech_models")
      .then((models) => setModelReady(models.some((m) => m.downloaded)))
      .catch(() => setModelReady(null));
  }, []);

  useEffect(refresh, [refresh]);

  // Refresh metrics in real-time when a dictation completes (phase returns
  // to idle after transcribe + transform + inject).
  useTauriEvent<{ phase: string }>("dictation-state", ({ payload }) => {
    if (payload.phase === "idle") refresh();
  });

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
  const timeSavedLabel = insights?.impact.timeSavedLabel ?? "0 min";
  const wordsLast7 = insights?.wordsLast7Days ?? 0;
  const weekGoal = Math.max(1000, Math.round(((insights?.avgWordsPerDay ?? 0) * 7 * 2) / 100) * 100);
  const weekPct = Math.min(100, Math.round((wordsLast7 / weekGoal) * 100));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <h1 style={{ fontSize: 26, fontWeight: 700, letterSpacing: -0.5 }}>
        Welcome back
      </h1>

      {modelReady === false && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            padding: "12px 16px",
            borderRadius: "var(--radius)",
            background: "color-mix(in srgb, var(--accent) 10%, var(--surface))",
            border: "1px solid color-mix(in srgb, var(--accent) 25%, transparent)",
          }}
        >
          <span style={{ fontSize: 14, fontWeight: 500 }}>
            No speech model downloaded yet. You need one to start dictating.
          </span>
          <button
            onClick={() => onNavigate?.("models")}
            style={{
              padding: "6px 14px",
              borderRadius: 6,
              border: "none",
              background: "var(--accent)",
              color: "#fff",
              fontSize: 13,
              fontWeight: 600,
              cursor: "pointer",
              whiteSpace: "nowrap",
            }}
          >
            Get a model
          </button>
        </div>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.5fr) minmax(280px, 1fr)", gap: 20 }}>
        {/* Left column: hero + history */}
        <div style={{ display: "flex", flexDirection: "column", gap: 20, minWidth: 0 }}>
          {/* Hero banner */}
          <div
            style={{
              borderRadius: "var(--radius)",
              overflow: "hidden",
              lineHeight: 0,
            }}
          >
            <img
              src="/home-card.png"
              alt=""
              draggable={false}
              style={{ width: "100%", display: "block", borderRadius: "var(--radius)" }}
            />
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
          {/* On-device privacy row */}
          {!privacyDismissed && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                background: "var(--accent-soft)",
                border: "1px solid rgba(22,163,74,0.2)",
                borderRadius: "var(--radius)",
                padding: "14px 16px",
              }}
            >
              <div
                style={{
                  width: 34,
                  height: 34,
                  borderRadius: 10,
                  background: "var(--surface)",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                }}
              >
                <ShieldIcon size={18} color="var(--success)" />
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 3 }}>
                  <span style={{ fontSize: 13, fontWeight: 700 }}>On-device · nothing uploaded</span>
                </div>
                <PrivacyBadge />
              </div>
              <button
                type="button"
                title="Dismiss"
                onClick={dismissPrivacy}
                style={{
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  padding: 4,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  borderRadius: 6,
                  color: "var(--text-secondary)",
                  flexShrink: 0,
                }}
              >
                <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </div>
          )}

          {/* Time saved — the hero impact number, leads the sidebar */}
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
                Time saved vs typing
              </h3>
            </div>
            <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
              <span style={{ fontSize: 36, fontWeight: 800, lineHeight: 1, fontVariantNumeric: "tabular-nums" }}>{timeSavedLabel}</span>
              {wpm > 0 && (
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
                  {wpm ? `${Math.round(wpm / 40)}×` : ""} faster
                </span>
              )}
            </div>
            <div style={{ fontSize: 12, opacity: 0.85, marginTop: 8 }}>
              Across {exactNumber(totalWords)} words · you speak at {wpm || "—"} wpm
            </div>
          </div>

          {/* Impact grid — the key metrics at a glance */}
          <div
            style={{
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              padding: "18px 20px",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 14 }}>
              <Icon name="zap" size={15} color={ACCENT} />
              <h3 style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.5, color: "var(--text-secondary)", margin: 0 }}>
                Your impact
              </h3>
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <MiniStat icon="messages-square" value={exactNumber(totalWords)} label="words" />
              <MiniStat icon="zap" value={wpm ? `${wpm}` : "—"} label="wpm" />
              <MiniStat icon="flame" value={`${streak}`} label={streak === 1 ? "day streak" : "day streaks"} />
              <MiniStat icon="calendar-check" value={exactNumber(wordsLast7)} label="words / 7d" />
            </div>
          </div>

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

function MiniStat({
  value,
  label,
  icon,
}: {
  value: string;
  label: string;
  icon: import("../components/Icon").IconName;
}) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
      <div
        style={{
          width: 34,
          height: 34,
          borderRadius: 9,
          background: "var(--accent-soft)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        <Icon name={icon} size={17} color={ACCENT} />
      </div>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 20, fontWeight: 800, lineHeight: 1.1, fontVariantNumeric: "tabular-nums" }}>{value}</div>
        <div style={{ fontSize: 11.5, color: "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</div>
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
  const [copied, setCopied] = useState(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        padding: "12px 16px",
        borderBottom: isLast ? "none" : "1px solid var(--border)",
        background: hover ? "var(--surface-2)" : "transparent",
        transition: "background 0.12s ease",
      }}
    >
      <span
        style={{
          fontSize: 12,
          color: "var(--text-secondary)",
          width: 56,
          flexShrink: 0,
          fontVariantNumeric: "tabular-nums",
          paddingTop: 2,
        }}
      >
        {formatTime(entry.createdAt)}
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontSize: 13.5,
            lineHeight: 1.5,
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            color: "var(--text)",
          }}
          title={entry.text}
        >
          {entry.text}
        </div>
        {entry.appName && (
          <div style={{ marginTop: 6 }}>
            <span
              style={{
                fontSize: 11,
                color: "var(--text-secondary)",
                background: "var(--surface-2)",
                borderRadius: 6,
                padding: "1px 8px",
                maxWidth: 120,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                display: "inline-block",
              }}
            >
              {entry.appName}
            </span>
          </div>
        )}
      </div>
      <div
        style={{
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          gap: 2,
          flexShrink: 0,
          opacity: hover ? 1 : 0.4,
          transition: "opacity 0.12s ease",
          paddingTop: 2,
        }}
      >
        <button
          type="button"
          title={copied ? "Copied!" : "Copy to clipboard"}
          aria-label={copied ? "Copied" : "Copy to clipboard"}
          onClick={(ev) => {
            ev.stopPropagation();
            navigator.clipboard.writeText(entry.text);
            setCopied(true);
            setTimeout(() => setCopied(false), 1200);
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
          <Icon name={copied ? "check" : "copy"} size={15} color={copied ? "var(--success)" : "var(--text-secondary)"} />
        </button>
        <button
          type="button"
          title="Delete"
          aria-label="Delete entry"
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
