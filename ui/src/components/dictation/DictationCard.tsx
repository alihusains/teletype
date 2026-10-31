import { useEffect, useId, useState } from "react";
import { Icon } from "../Icon";
import { WriteFailed } from "../StateRows";
import { Toggle } from "../../settings/primitives";
import { AudioPlayer } from "./AudioPlayer";
import { InlineDiff, changeSummary, srOnly } from "./InlineDiff";
import type { HistoryEntry } from "./types";

/** localStorage key for the "Show changes" switch (per viewer). */
export const SHOW_CHANGES_KEY = "tt.dictation.showChanges";

function readShowChanges(): boolean {
  try {
    return localStorage.getItem(SHOW_CHANGES_KEY) !== "0";
  } catch {
    return true;
  }
}

function writeShowChanges(on: boolean) {
  try {
    localStorage.setItem(SHOW_CHANGES_KEY, on ? "1" : "0");
  } catch {
    // Storage blocked: the switch still works for this session.
  }
}

export function timeLabel(ts: number): string {
  return new Date(ts).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

export function elapsedLabel(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m ${s % 60}s`;
}

function prefersReducedMotion(): boolean {
  try {
    return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
  } catch {
    return false;
  }
}

const outlineButton: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 8,
  padding: "8px 16px",
  fontSize: 14,
  fontWeight: 500,
  borderRadius: 12,
  border: "1px solid var(--border-strong)",
  background: "var(--surface)",
  color: "var(--text)",
  cursor: "pointer",
};

const COPY_LABELS = ["Copy", "Copied", "Copy failed"] as const;

export interface CardActions {
  copyState: "idle" | "copied" | "failed";
  onCopy: () => void;
  onReveal: () => void;
  onDelete: () => void;
  deleting: boolean;
  deleteError: string | null;
  onDismissError: () => void;
}

/**
 * One dictation. Collapsed: the first line and a meta row (time, words,
 * duration, app). Expanded: the full text with every stage's edit shown
 * inline, the recording, and the actions.
 */
export function DictationCard({ entry, actions }: { entry: HistoryEntry; actions: CardActions }) {
  const [open, setOpen] = useState(false);
  const [showChanges, setShowChanges] = useState(readShowChanges);
  const [reduced] = useState(prefersReducedMotion);
  const panelId = useId();
  const headerId = useId();
  const switchId = useId();
  const isWindows = /Windows/i.test(navigator.userAgent);

  // Another card flipping the switch updates this one too.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === SHOW_CHANGES_KEY) setShowChanges(readShowChanges());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const summary = changeSummary(entry.edits, entry.polishMs);
  const words = entry.wordCount;
  const toggleChanges = (next: boolean) => {
    setShowChanges(next);
    writeShowChanges(next);
  };
  const copyLabel =
    actions.copyState === "copied" ? "Copied" : actions.copyState === "failed" ? "Copy failed" : "Copy";

  const modelLine = [
    entry.asrModel,
    entry.polishModel ? `Polished by ${entry.polishModel}` : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div>
      <div
        className="tt-dictation-card"
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          minWidth: 0,
          position: "relative",
        }}
      >
        <button
          type="button"
          id={headerId}
          className="tt-dictation-head"
          aria-expanded={open}
          aria-controls={panelId}
          onClick={() => setOpen((o) => !o)}
          style={{
            display: "flex",
            alignItems: open ? "flex-start" : "center",
            gap: 16,
            width: "100%",
            padding: "16px 20px",
            paddingBottom: open ? 8 : 16,
            // Room for the copy icon on the right.
            paddingRight: 56,
            background: "none",
            border: "none",
            borderRadius: "var(--radius)",
            textAlign: "left",
            cursor: "pointer",
            color: "var(--text)",
            font: "inherit",
          }}
        >
          <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 6 }}>
            {!open && (
              <span
                title={entry.text}
                style={{
                  fontSize: 15,
                  color: "var(--text)",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {entry.text}
              </span>
            )}
            <span
              style={{
                display: "flex",
                alignItems: "center",
                flexWrap: "wrap",
                columnGap: 20,
                rowGap: 4,
                fontSize: 13,
                color: "var(--text-secondary)",
                fontVariantNumeric: "tabular-nums",
                minWidth: 0,
              }}
            >
              <Meta icon="clock">{timeLabel(entry.createdAt)}</Meta>
              <Meta icon="type">{words === 1 ? "1 word" : `${words} words`}</Meta>
              {entry.durationMs !== null && (
                <Meta icon="timer">
                  {entry.durationMs < 1000 ? "<1s" : elapsedLabel(entry.durationMs)}
                </Meta>
              )}
              {entry.appName && (
                <span
                  title={entry.appName}
                  style={{
                    fontSize: 11,
                    color: "var(--text-secondary)",
                    background: "var(--surface-2)",
                    borderRadius: 6,
                    padding: "1px 8px",
                    maxWidth: "min(100%, 320px)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {entry.appName}
                </span>
              )}
            </span>
            <span style={srOnly}>{open ? "Hide details" : "Show details"}</span>
          </span>
          <Icon
            name="chevron-down"
            size={18}
            color="var(--text-secondary)"
            data-chevron=""
            style={{
              flex: "none",
              transform: open ? "rotate(180deg)" : "none",
              transition: reduced ? "none" : "transform 150ms ease",
              marginTop: open ? 2 : 0,
            }}
          />
        </button>
        {!open && (
          <span
            className="tt-card-copy-slot"
            // Sits over the header's right edge (the header keeps room for it
            // with its padding-right): the card is position:relative. Its own
            // click never toggles the card, because it is a sibling of the
            // header button, not inside it. Collapsed only: the expanded card
            // has its own Copy button, so the icon would be redundant there.
            style={{
              position: "absolute",
              // The header's own box: the icon is centred on the meta row,
              // the same row the chevron sits on, in both states.
              top: 8,
              right: 12,
              bottom: 8,
              display: "flex",
              alignItems: "center",
              pointerEvents: "none",
            }}
          >
            <button
              type="button"
              className="tt-card-copy"
              aria-label="Copy dictation"
              title={
                actions.copyState === "copied"
                  ? "Copied!"
                  : actions.copyState === "failed"
                    ? "Copy failed"
                    : "Copy to clipboard"
              }
              onClick={actions.onCopy}
              style={{
                display: "grid",
                placeItems: "center",
                width: 28,
                height: 28,
                borderRadius: 8,
                border: "none",
                background: "none",
                color:
                  actions.copyState === "copied"
                    ? "var(--success)"
                    : actions.copyState === "failed"
                      ? "var(--danger)"
                      : "var(--text-tertiary)",
                cursor: "pointer",
                pointerEvents: "auto",
              }}
            >
              {actions.copyState === "copied" ? (
                <Icon name="check" size={16} color="var(--success)" />
              ) : (
                <Icon
                  name="copy"
                  size={16}
                  color={
                    actions.copyState === "failed"
                      ? "var(--danger)"
                      : "var(--text-tertiary)"
                  }
                />
              )}
            </button>
          </span>
        )}

        {open && (
          <div
            id={panelId}
            role="region"
            aria-labelledby={headerId}
            className={reduced ? undefined : "tt-dictation-panel"}
            style={{ padding: "0 20px 16px", display: "flex", flexDirection: "column", gap: 16 }}
          >
            <div
              style={{
                fontSize: 15,
                lineHeight: 1.6,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                color: "var(--text)",
                marginTop: 8,
              }}
            >
              <InlineDiff text={entry.text} edits={entry.edits} showChanges={showChanges} />
            </div>

            {summary && (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 12,
                  fontSize: 12,
                  color: "var(--text-secondary)",
                  minWidth: 0,
                }}
              >
                <span style={{ minWidth: 0 }}>{summary}</span>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 8, flex: "none" }}>
                  {/* The app's own switch; the words beside it toggle it too. */}
                  <label htmlFor={switchId} style={{ cursor: "pointer", userSelect: "none" }}>
                    Show changes
                  </label>
                  <Toggle id={switchId} label="Show changes" checked={showChanges} onChange={toggleChanges} />
                </span>
              </div>
            )}

            {entry.hasRecording && <AudioPlayer id={entry.id} />}

            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 12,
                flexWrap: "wrap",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
                <button
                  type="button"
                  className="tt-card-action"
                  onClick={actions.onCopy}
                  title={
                    actions.copyState === "copied"
                      ? "Copied!"
                      : actions.copyState === "failed"
                        ? "Copy failed"
                        : "Copy to clipboard"
                  }
                  style={{
                    ...outlineButton,
                    color: actions.copyState === "failed" ? "var(--danger)" : "var(--text)",
                  }}
                >
                  {actions.copyState === "copied" ? (
                    <Icon name="check" size={16} color="var(--success)" />
                  ) : (
                    <Icon
                      name="copy"
                      size={16}
                      color={actions.copyState === "failed" ? "var(--danger)" : "currentColor"}
                    />
                  )}
                  {/* Every label shares one grid cell, so the button is as wide
                      as the longest one and the buttons after it never move. */}
                  <span style={{ display: "inline-grid", textAlign: "left" }}>
                    {COPY_LABELS.map((l) => (
                      <span key={l} style={{ gridArea: "1 / 1", visibility: l === copyLabel ? "visible" : "hidden" }}>
                        {l}
                      </span>
                    ))}
                  </span>
                </button>
                {entry.hasRecording && (
                  <button type="button" className="tt-card-action" onClick={actions.onReveal} style={outlineButton}>
                    <Icon name="folder" size={16} />
                    {isWindows ? "Show in Explorer" : "Show in Finder"}
                  </button>
                )}
                <button
                  type="button"
                  className="tt-card-delete"
                  onClick={actions.onDelete}
                  disabled={actions.deleting}
                  style={{
                    ...outlineButton,
                    border: "1px solid transparent",
                    background: "none",
                    color: "var(--danger)",
                    padding: "8px 8px",
                    opacity: actions.deleting ? 0.6 : 1,
                  }}
                >
                  <Icon name="trash" size={16} />
                  {actions.deleting ? "Deleting…" : "Delete"}
                </button>
              </div>
              {modelLine && (
                <span
                  title={modelLine}
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 6,
                    fontSize: 13,
                    color: "var(--text-tertiary)",
                    minWidth: 0,
                    maxWidth: "100%",
                    // Stays right-aligned when it wraps under the buttons.
                    marginLeft: "auto",
                  }}
                >
                  <Icon name="cpu" size={15} style={{ flex: "none" }} />
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {modelLine}
                  </span>
                </span>
              )}
            </div>
          </div>
        )}
      </div>
      {actions.deleteError && (
        <div style={{ marginTop: 6 }}>
          <WriteFailed
            message={actions.deleteError}
            label="This entry"
            onRetry={actions.onDelete}
            onDismiss={actions.onDismissError}
          />
        </div>
      )}
    </div>
  );
}

function Meta({ icon, children }: { icon: "clock" | "type" | "timer"; children: React.ReactNode }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6, whiteSpace: "nowrap" }}>
      <Icon name={icon} size={14} color="var(--text-tertiary)" />
      {children}
    </span>
  );
}
