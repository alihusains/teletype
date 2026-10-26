import type { CSSProperties } from "react";

/**
 * The two rows every list screen needs and most of them invent differently:
 * `LoadFailed` for a read that rejected, `BusyRow` for a write in flight.
 *
 * A failed read must not render as the empty state. "No dictations yet, go
 * record one" is a sentence that sends the user off to do work that changes
 * nothing when the real cause is a locked file.
 */

const shell: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  padding: "10px 12px",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius)",
  background: "var(--surface)",
};

export function LoadFailed({
  what,
  detail,
  onRetry,
  retrying,
}: {
  /** The noun the user was looking for, e.g. "Your dictation history". */
  what: string;
  detail?: string;
  onRetry: () => void;
  retrying?: boolean;
}) {
  return (
    <div role="alert" style={{ ...shell, borderColor: "var(--danger)", background: "var(--danger-soft)" }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13.5, color: "var(--text)", lineHeight: 1.4 }}>
          {what} could not be read.
        </div>
        {detail ? (
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>{detail}</div>
        ) : (
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>
            Nothing was lost. This is the read failing, not the list being empty.
          </div>
        )}
      </div>
      <button onClick={onRetry} disabled={retrying} style={{ whiteSpace: "nowrap" }}>
        {retrying ? "Trying…" : "Try again"}
      </button>
    </div>
  );
}

/** A short inline line for a write that failed under a row. It stays until the
 *  user retries or dismisses, so a silent failure cannot scroll out of view. */
export function WriteFailed({
  message,
  onRetry,
  onDismiss,
  label = "the change",
}: {
  message: string;
  onRetry?: () => void;
  onDismiss?: () => void;
  label?: string;
}) {
  return (
    <div
      role="alert"
      style={{
        ...shell,
        padding: "6px 10px",
        borderColor: "var(--danger)",
        background: "var(--danger-soft)",
      }}
    >
      <span style={{ flex: 1, minWidth: 0, fontSize: 12.5, lineHeight: 1.4 }}>
        {label} did not save: {message}
      </span>
      {onRetry ? (
        <button onClick={onRetry} style={{ fontSize: 12 }}>
          Retry
        </button>
      ) : null}
      {onDismiss ? (
        <button onClick={onDismiss} style={{ fontSize: 12 }} aria-label="Dismiss this error">
          Dismiss
        </button>
      ) : null}
    </div>
  );
}

/** Reading state for a list that has not answered yet. Rendered instead of the
 *  empty state so "loading" never flashes as "you have nothing". */
export function LoadingRow({ label = "Loading…" }: { label?: string }) {
  return (
    <div role="status" aria-live="polite" style={{ ...shell, background: "var(--surface-2)" }}>
      <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>{label}</span>
    </div>
  );
}
