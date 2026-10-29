import { useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import ImpactCard from "../components/ImpactCard";
import { LoadFailed, LoadingRow } from "../components/StateRows";
import { ShieldIcon } from "../lib/PrivacyBadge";
import { useTauriEvent } from "../lib/useTauriEvent";
import { useResource } from "../lib/writeGuard";
import { ReadinessPanel, useRuntimeStatus } from "../lib/runtimeStatus";

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
  const [privacyDismissed, setPrivacyDismissed] = useState(loadPrivacyDismissed);

  // Real runtime state, polled on a coarse cadence. This replaces the old
  // `modelReady` boolean, which was computed as
  // `list_speech_models().some(m => m.downloaded)` — "is *any* model on
  // disk". Dictation uses the *selected* model, so that boolean said "ready"
  // to a user who had downloaded a small model and then selected a different
  // one that was absent, and their first dictation failed. It also could not
  // distinguish downloaded from loaded, which is the question being asked.
  const runtime = useRuntimeStatus().status;

  const dismissPrivacy = () => {
    setPrivacyDismissed(true);
    try {
      localStorage.setItem(PRIVACY_DISMISSED_KEY, "1");
    } catch {
      // ignore quota / private mode
    }
  };

  // Each read is its own resource. They used to share one `refresh` and one
  // `.catch(() => {})`, so a rejected `get_insights` left the history column
  // blank with no explanation, and a rejected history read rendered as "No
  // dictations yet". Now each failure names itself and retries on its own.
  const history = useResource<HistoryEntry[]>(() => invoke("list_dictation_history"));
  const insights = useResource<Insights>(() => invoke("get_insights", { range: "lifetime" }));

  const entries = history.data ?? [];

  const refresh = useCallback(() => {
    history.reload();
    insights.reload();
  }, [history.reload, insights.reload]);

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

  const totalWords = insights.data?.totalWords ?? 0;
  // (wpm / streak / time-saved / weekly-goal are all rendered by the shared
  // ImpactCard now, so they are computed there from the same backend object.)

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 24 }}>
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 16, flexWrap: "wrap" }}>
        <div>
          <div className="tt-eyebrow">
            {new Date().toLocaleDateString("en-US", { weekday: "long", month: "long", day: "numeric" })}
          </div>
          <div className="tt-screen-title" style={{ marginTop: 7 }}>Welcome back</div>
        </div>
        <div
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 7,
            padding: "5px 12px",
            borderRadius: "var(--radius-pill)",
            border: "1px solid var(--border)",
            background: "var(--surface)",
            fontSize: 12,
            fontWeight: 500,
            color: "var(--text-secondary)",
            whiteSpace: "nowrap",
          }}
        >
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: "50%",
              background: runtime?.speech.blocking ? "var(--warning)" : "var(--success)",
            }}
          />
          {runtime?.speech.blocking ? "Setup needed" : "On-device · private"}
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
            padding: "12px 18px",
            borderRadius: "var(--radius)",
            background: "var(--warning-soft)",
            border: "1px solid color-mix(in srgb, var(--warning) 26%, transparent)",
          }}
        >
          <span style={{ fontSize: "var(--text-md)", fontWeight: 500, display: "flex", alignItems: "center", gap: 9 }}>
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

      {/* The one atmospheric band in the app. The gradient sets the clinical,
          open-sky tone; every section below stays pure white. It spans the
          content width so the headline has room to set in two lines. */}
      <section className="tt-hero" aria-label="Dictation">
        <div className="tt-hero-copy">
          <div className="tt-hero-eyebrow">Teletype · On-device dictation</div>
          <h1 className="tt-hero-title">
            Speak naturally.
            <br />
            It’s already written.
          </h1>
          <p className="tt-hero-sub">
            Hold your hotkey and talk. Clean, formatted text lands at your
            cursor — transcribed and polished entirely on this Mac.
          </p>
          <div className="tt-hero-actions">
            <button
              className="tt-btn tt-btn-primary"
              onClick={() => onNavigate?.("dictation")}
            >
              Start dictating
            </button>
            <button
              className="tt-btn tt-hero-ghost"
              onClick={() => onNavigate?.("transforms")}
            >
              Try a transform
            </button>
          </div>
        </div>
        <div className="tt-hero-art" aria-hidden="true">
          <div className="tt-hero-card-back" />
          <div className="tt-hero-card">
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span
                style={{
                  width: 28,
                  height: 28,
                  borderRadius: 8,
                  background: "var(--accent-soft)",
                  display: "grid",
                  placeItems: "center",
                }}
              >
                <Icon name="mic" size={14} color="var(--accent)" />
              </span>
              <span
                style={{
                  fontSize: 10.5,
                  fontWeight: 600,
                  letterSpacing: "0.1em",
                  textTransform: "uppercase",
                  color: "var(--color-charcoal)",
                }}
              >
                Listening
              </span>
              <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--color-slate)" }}>
                0:04
              </span>
            </div>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 3,
                height: 34,
                margin: "14px 0 15px",
              }}
            >
              {[10, 18, 27, 14, 32, 21, 12, 26, 31, 17, 9, 23, 29, 15, 25, 11, 19, 28, 16, 8, 22, 30, 13, 24].map(
                (h, i) => (
                  <span
                    key={i}
                    style={{
                      flex: 1,
                      height: h,
                      borderRadius: 2,
                      background: "var(--accent)",
                      opacity: 0.35 + (h / 32) * 0.65,
                    }}
                  />
                ),
              )}
            </div>
            <div
              style={{
                fontFamily: "var(--font-display)",
                fontSize: 15,
                lineHeight: 1.36,
                letterSpacing: "-0.01em",
                color: "var(--color-obsidian)",
              }}
            >
              The quarterly report is ready to send to the team by end of day.
            </div>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                marginTop: 12,
                fontSize: 11,
                color: "var(--color-slate)",
              }}
            >
              <Icon name="check" size={12} color="var(--success)" />
              Inserted in Mail
            </div>
          </div>
        </div>
      </section>

      {/* The rail is a bounded column, not a share of the page. It used to be
          `minmax(280px, 1fr)` against a 1.5fr main column, which at a normal
          window width handed the sidebar 40% of the screen and left the
          history list reading as a narrow newspaper column. */}
      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) clamp(260px, 26vw, 320px)", gap: 24, alignItems: "start" }}>
        {/* Left column: history */}
        <div style={{ display: "flex", flexDirection: "column", gap: 22, minWidth: 0 }}>
          {/* History list */}
          {history.status === "error" ? (
            <LoadFailed
              what="Recent dictations"
              detail={history.error ?? undefined}
              onRetry={history.reload}
            />
          ) : history.status === "loading" ? (
            <LoadingRow label="Loading your dictations…" />
          ) : grouped.length === 0 ? (
            <div style={{ padding: 52, textAlign: "center", color: "var(--text-secondary)", fontSize: "var(--text-md)", background: "var(--surface)", border: "1px dashed var(--border-strong)", borderRadius: "var(--radius)" }}>
              <span
                style={{
                  width: 46,
                  height: 46,
                  borderRadius: 14,
                  background: "var(--accent-soft)",
                  display: "inline-grid",
                  placeItems: "center",
                  marginBottom: 14,
                }}
              >
                <Icon name="mic" size={22} color="var(--accent)" />
              </span>
              <p style={{ margin: 0, fontWeight: 600, color: "var(--text)", fontSize: "var(--text-lg)", fontFamily: "var(--font-display)", letterSpacing: "-0.03em" }}>No dictations yet</p>
              <p style={{ margin: "6px 0 0" }}>Hold your hotkey and start talking.</p>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 22, minWidth: 0 }}>
              {grouped.map((group) => (
                <div key={group.label} style={{ minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 10 }}>
                    <span style={{ fontSize: 10.5, fontWeight: 600, letterSpacing: "0.1em", color: "var(--text-tertiary)" }}>
                      {group.label}
                    </span>
                    <span style={{ flex: 1, height: 1, background: "var(--border-subtle)" }} />
                    <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", fontVariantNumeric: "tabular-nums" }}>
                      {group.items.length}
                    </span>
                  </div>
                  <div
                    style={{
                      background: "var(--surface)",
                      border: "1px solid var(--border)",
                      borderRadius: "var(--radius)",
                      boxShadow: "var(--shadow-1)",
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
                background: "var(--surface-2)",
                border: "1px solid var(--border-subtle)",
                borderRadius: "var(--radius)",
                padding: "15px 16px",
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
                  boxShadow: "var(--shadow-1)",
                }}
              >
                <ShieldIcon size={18} color="var(--success)" />
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 13, fontWeight: 600 }}>Private by default</div>
                <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2, lineHeight: 1.45 }}>
                  Audio, transcripts and history never leave this Mac.
                </div>
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

          {/* Impact — one card: the hero time-saved number leads, then the
              supporting metrics, then the weekly goal as a compact row.
              Previously this was three separate cards that repeated wpm and
              words/7d between them. */}
          {insights.status === "error" && (
            <LoadFailed
              what="Your insights"
              detail={insights.error ?? undefined}
              onRetry={insights.reload}
            />
          )}
          {insights.data && (
            <ImpactCard
              variant="compact"
              hasData={totalWords > 0}
              impact={{
                wordsPerMinute: insights.data.impact.wordsPerMinute,
                timesFaster: insights.data.impact.timesFaster,
                timeSavedLabel: insights.data.impact.timeSavedLabel,
                totalWords: insights.data.totalWords,
                totalDictations: insights.data.totalDictations,
                ratedTakes: insights.data.impact.ratedTakes,
                streakDays: insights.data.streakDays,
                wordsLast7Days: insights.data.wordsLast7Days,
                avgWordsPerDay: insights.data.avgWordsPerDay,
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
        padding: "16px 20px",
        borderBottom: isLast ? "none" : "1px solid var(--border-subtle)",
        background: hover ? "var(--surface-2)" : "transparent",
        transition: "background 0.12s ease",
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontSize: "var(--text-md)",
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
          <div style={{ marginTop: 8 }}>
            <span
              style={{
                fontSize: 11.5,
                fontWeight: 500,
                color: "var(--text-secondary)",
                background: "var(--surface-2)",
                borderRadius: "var(--radius-pill)",
                padding: "3px 9px",
                display: "inline-block",
                maxWidth: "100%",
                overflowWrap: "anywhere",
              }}
            title={entry.appName}
            >
              {entry.appName}
            </span>
          </div>
        )}
      </div>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-end",
          gap: 6,
          flexShrink: 0,
          paddingTop: 2,
        }}
      >
        <span
          style={{
            fontSize: "var(--text-sm)",
            color: "var(--text-secondary)",
            fontVariantNumeric: "tabular-nums",
            whiteSpace: "nowrap",
          }}
        >
          {formatTime(entry.createdAt)}
        </span>
        <div
          style={{
            display: "flex",
            flexDirection: "row",
            alignItems: "center",
            gap: 2,
            opacity: hover ? 1 : 0.4,
            transition: "opacity 0.12s ease",
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
    </div>
  );
}
