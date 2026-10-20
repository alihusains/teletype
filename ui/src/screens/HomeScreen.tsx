import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";
import ImpactCard from "../components/ImpactCard";
import { PrivacyBadge, ShieldIcon } from "../lib/PrivacyBadge";
import { useTauriEvent } from "../lib/useTauriEvent";
import { ReadinessPanel, useRuntimeStatus } from "../lib/runtimeStatus";

const PRIVACY_DISMISSED_KEY = "teletype.privacyRowDismissed";

// Decorative app tiles in the hero banner. The old home-card artwork used
// real third-party app logos; these are generic glyphs from our own icon set
// instead, so the banner carries no brand marks.
const HERO_APPS: { icon: IconName; accent?: boolean }[] = [
  { icon: "messages-square" },
  { icon: "calendar-check" },
  { icon: "keyboard" },
  { icon: "terminal" },
  { icon: "library" },
  { icon: "wand", accent: true },
];

function HeroAppTile({ app }: { app: { icon: IconName; accent?: boolean } }) {
  return (
    <span
      style={{
        width: 40,
        height: 40,
        borderRadius: "50%",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: app.accent ? "var(--accent-soft)" : "var(--banner-tile)",
        border: `1px solid ${app.accent ? "transparent" : "var(--banner-tile-border)"}`,
        color: app.accent ? "var(--accent)" : "var(--banner-text-secondary)",
        flexShrink: 0,
      }}
    >
      <Icon name={app.icon} size={18} />
    </span>
  );
}

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
  impact: {
    wordsPerMinute: number | null;
    timesFaster: number | null;
    timeSavedLabel: string;
    timeSavedMinutes: number;
    ratedTakes: number;
  };
  wordsLast7Days: number;
  avgWordsPerDay: number;
}

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

export default function HomeScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [insights, setInsights] = useState<Insights | null>(null);
  const [privacyDismissed, setPrivacyDismissed] = useState(loadPrivacyDismissed);
  const [showHow, setShowHow] = useState(false);

  // Real runtime state, polled on a coarse cadence. This replaces the old
  // `modelReady` boolean, which was computed as
  // `list_speech_models().some(m => m.downloaded)` — "is *any* model on
  // disk". Dictation uses the *selected* model, so that boolean said "ready"
  // to a user who had downloaded a small model and then selected a different
  // one that was absent, and their first dictation failed. It also could not
  // distinguish downloaded from loaded, which is the question being asked.
  const runtime = useRuntimeStatus();

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
    invoke<Insights>("get_insights", { range: "lifetime" }).then(setInsights).catch(() => {});
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
  // (wpm / streak / time-saved / weekly-goal are all rendered by the shared
  // ImpactCard now, so they are computed there from the same backend object.)

  return (
    <div className="tt-page" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div className="tt-page-header" style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 16, flexWrap: "wrap" }}>
        <div>
          <div className="tt-page-title">Welcome back</div>
        </div>
      </div>

      {/* Only when the *selected* model cannot be used. Previously this was
          driven by "no model file exists anywhere", so it hid itself for
          users who were one selection away from a broken first dictation. */}
      {runtime?.speech.blocking === true && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            padding: "12px 16px",
            borderRadius: "var(--radius)",
            background: "var(--warning-soft)",
          }}
        >
          <span style={{ fontSize: 14, fontWeight: 500, display: "flex", alignItems: "center", gap: 9 }}>
            <span style={{ color: "var(--warning)", display: "grid", placeItems: "center" }}>
              <Icon name="alert" size={16} />
            </span>
            {runtime.speech.detail}
          </span>
          <button
            className="tt-btn tt-btn-primary tt-btn-sm"
            onClick={() => onNavigate?.("models")}
            style={{ flex: "0 0 auto" }}
          >
            Get a model
          </button>
        </div>
      )}

      <ReadinessPanel status={runtime} onFix={() => onNavigate?.("models")} />

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.5fr) minmax(280px, 1fr)", gap: 20, alignItems: "start" }}>
        {/* Left column: hero + history */}
        <div style={{ display: "flex", flexDirection: "column", gap: 20, minWidth: 0 }}>
          {/* Hero banner. The old home-card.png baked its copy and both
              actions into the artwork, so "Try it out" and "How it works"
              were not clickable. It is replaced by real DOM below (the PNG
              stays in public/, just unreferenced): one button opens the
              AI Polish screen, the other expands the how-it-works note. */}
          <div
            className="tt-card"
            style={{
              background: "var(--banner)",
              padding: "20px 22px",
              display: "flex",
              alignItems: "center",
              gap: 20,
              overflow: "hidden",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <Icon name="wand" size={16} color="var(--accent)" />
                <h2
                  style={{
                    margin: 0,
                    fontSize: 19,
                    fontWeight: 650,
                    letterSpacing: "var(--tracking-tight)",
                    color: "var(--banner-text)",
                  }}
                >
                  Transform works anywhere you write
                </h2>
              </div>
              <p
                style={{
                  margin: "6px 0 0",
                  fontSize: 12.5,
                  lineHeight: 1.5,
                  color: "var(--banner-text-secondary)",
                  maxWidth: "46ch",
                }}
              >
                Apply a Transform to rewrite, clean up, or restructure text after you dictate.
              </p>
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 14 }}>
                <button
                  type="button"
                  className="tt-btn tt-btn-sm tt-btn-on-dark"
                  onClick={() => onNavigate?.("transforms")}
                >
                  Try it out
                </button>
                <button
                  type="button"
                  className="tt-btn tt-btn-sm tt-btn-on-dark-outline"
                  aria-expanded={showHow}
                  aria-controls="home-banner-how"
                  onClick={() => setShowHow((v) => !v)}
                >
                  How it works
                </button>
              </div>
              {showHow ? (
                <p
                  id="home-banner-how"
                  style={{
                    margin: "12px 0 0",
                    paddingTop: 12,
                    borderTop: "1px solid var(--banner-border)",
                    fontSize: 12.5,
                    lineHeight: 1.55,
                    color: "var(--banner-text-secondary)",
                    maxWidth: "62ch",
                  }}
                >
                  Select text in any app, press its shortcut, and the Transform rewrites it in
                  place. Rules live under AI Polish; shortcuts under Keybinds.
                </p>
              ) : null}
            </div>
            <div className="tt-hero-art" aria-hidden="true">
              <div style={{ display: "flex", gap: 10 }}>
                {HERO_APPS.slice(0, 3).map((app) => (
                  <HeroAppTile key={app.icon} app={app} />
                ))}
              </div>
              <div style={{ display: "flex", gap: 10, marginLeft: 20 }}>
                {HERO_APPS.slice(3).map((app) => (
                  <HeroAppTile key={app.icon} app={app} />
                ))}
              </div>
            </div>
          </div>

          {/* History list */}
          {grouped.length === 0 ? (
            <div className="tt-card" style={{ padding: 48, textAlign: "center", color: "var(--text-secondary)", fontSize: 14 }}>
              <Icon name="mic" size={30} color="var(--text-secondary)" style={{ marginBottom: 12 }} />
              <p style={{ margin: 0, fontWeight: 600, color: "var(--text)" }}>No dictations yet</p>
              <p style={{ margin: "4px 0 0" }}>Hold your hotkey and start talking.</p>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 22, minWidth: 0 }}>
              {grouped.map((group) => (
                <div key={group.label} style={{ minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 8 }}>
                    <span style={{ fontSize: 15, fontWeight: 650, letterSpacing: "var(--tracking-tight)", color: "var(--text)" }}>
                      {group.label}
                    </span>
                    <span style={{ fontSize: 11.5, color: "var(--text-tertiary)", fontVariantNumeric: "tabular-nums" }}>
                      {group.items.length}
                    </span>
                  </div>
                  <div className="tt-card" style={{ overflow: "hidden", minWidth: 0 }}>
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
              className="tt-card"
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "14px 16px",
              }}
            >
              <div
                style={{
                  width: 34,
                  height: 34,
                  borderRadius: 10,
                  background: "var(--accent-soft)",
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
                className="tt-btn tt-btn-ghost"
                style={{
                  padding: 6,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  borderRadius: 6,
                  flexShrink: 0,
                }}
              >
                <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </div>
          )}

          {/* Impact — one card: the hero time-saved number leads, then the
              supporting metrics, then the weekly goal as a compact row.
              Previously this was three separate cards that repeated wpm and
              words/7d between them. */}
          {insights && (
            <ImpactCard
              variant="compact"
              hasData={totalWords > 0}
              impact={{
                wordsPerMinute: insights.impact.wordsPerMinute,
                timesFaster: insights.impact.timesFaster,
                timeSavedLabel: insights.impact.timeSavedLabel,
                totalWords: insights.totalWords,
                totalDictations: insights.totalDictations,
                ratedTakes: insights.impact.ratedTakes,
                streakDays: insights.streakDays,
                wordsLast7Days: insights.wordsLast7Days,
                avgWordsPerDay: insights.avgWordsPerDay,
              }}
            />
          )}
        </div>
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
        borderBottom: isLast ? "none" : "1px solid var(--border-subtle)",
        background: hover ? "var(--surface-hover)" : "transparent",
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
