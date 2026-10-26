import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";
import { useNow } from "./useNow";
import { useTauriEvent } from "./useTauriEvent";

/** One model's runtime state, mirroring `ComponentRuntime` in commands.rs. */
export interface ComponentRuntime {
  state: "none" | "missing" | "loading" | "ready" | "loaded" | "error";
  label: string;
  modelId: string;
  detail: string;
  blocking: boolean;
}

export interface RuntimeStatus {
  speech: ComponentRuntime;
  llm: ComponentRuntime;
}

export interface RuntimeStatusInfo {
  status: RuntimeStatus | null;
  /** Epoch millis of the last successful read (poll or event), or null. */
  lastChecked: number | null;
  /** True when the most recent read rejected; the previous value is kept. */
  stale: boolean;
}

/**
 * Reads the app's real runtime state.
 *
 * This exists as a hook rather than a one-shot fetch because the answer
 * changes on its own: a model unloads after an idle timeout, a download
 * finishes, a llama-server dies. A snapshot taken when a screen mounted is
 * stale the moment any of those happen, which is how the previous UI came to
 * show "Ready" next to a model that had been unloaded.
 *
 * The backend pushes a `runtime-status` event at every point that can change
 * the answer (model select/download, engine spawn/stop, startup restore), and
 * the event payload serialises to the identical shape the
 * `get_runtime_status` command returns. An event applies the payload
 * immediately and resets the poll timer, so a change is visible without
 * waiting a full `POLL_MS`. The 5 s poll stays as the safety net: an engine
 * death has no observation point in Rust, so the next poll is what notices it.
 * Polling is deliberately coarse and pauses while the tab is hidden: the
 * state that changes fast is the dictation phase, and that already arrives as
 * its own event. Nothing here is on the dictation hot path.
 */
const POLL_MS = 5000;

/** "Checked Ns ago" copy for a timestamp, clamped so the counter never says 0s. */
export function checkedAgoLabel(lastChecked: number | null, now: number): string {
  if (lastChecked === null) return "Not checked yet";
  const seconds = Math.max(0, Math.floor((now - lastChecked) / 1000));
  return `Checked ${Math.max(seconds, 1)}s ago`;
}

export function useRuntimeStatus(active = true): RuntimeStatusInfo {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [lastChecked, setLastChecked] = useState<number | null>(null);
  const [stale, setStale] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  // The event path resets the poll, so it needs the current `read` without
  // re-subscribing on every render.
  const readRef = useRef<() => void>(() => {});

  useEffect(() => {
    if (!active) return;
    let cancelled = false;

    const schedule = (delay: number) => {
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void read(), delay);
    };

    const read = async () => {
      try {
        const s = await invoke<RuntimeStatus>("get_runtime_status");
        if (cancelled) return;
        setStatus(s);
        setStale(false);
        setLastChecked(Date.now());
      } catch {
        // The command is unavailable (older backend, or state not ready) or the
        // read failed: keep the previous value, mark it stale so the surface
        // can say "Not updating", and let the next poll retry.
        if (!cancelled) setStale(true);
      }
      if (!cancelled) schedule(POLL_MS);
    };
    readRef.current = () => void read();

    const onVisible = () => {
      if (document.visibilityState === "visible") {
        schedule(0);
      }
    };

    void read();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      window.clearTimeout(timer.current);
      document.removeEventListener("visibilitychange", onVisible);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // Event path: apply immediately and reset the poll so the next read is not
  // a duplicate of the value that just arrived.
  useTauriEvent<RuntimeStatus>("runtime-status", ({ payload }) => {
    setStatus(payload);
    setStale(false);
    setLastChecked(Date.now());
    // Restart the safety-net poll from now, so it cannot fire a redundant read
    // moments after the event already delivered the same answer.
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => readRef.current(), POLL_MS);
  });

  return { status, lastChecked, stale };
}

type Tone = "ok" | "busy" | "warn" | "idle";

const TONE: Record<ComponentRuntime["state"], Tone> = {
  loaded: "ok",
  ready: "ok",
  loading: "busy",
  error: "warn",
  missing: "warn",
  none: "idle",
};

const TONE_COLOR: Record<Tone, string> = {
  ok: "var(--success)",
  busy: "var(--accent)",
  warn: "var(--warning)",
  idle: "var(--text-tertiary)",
};

const TONE_ICON: Record<Tone, IconName> = {
  ok: "check",
  busy: "refresh",
  warn: "alert",
  idle: "minus",
};

/**
 * One row of the readiness panel: an icon, a name, and a sentence.
 *
 * The sentence comes from Rust rather than being assembled here. That is the
 * whole contract: the backend knows what "on disk but not loaded" means, and a
 * UI that rebuilt that phrase from a boolean is how it came to read "not
 * loaded" under a heading saying "Active".
 */
export function StatusRow({
  component,
  compact = false,
}: {
  component: ComponentRuntime;
  compact?: boolean;
}) {
  const tone = TONE[component.state] ?? "idle";
  const color = TONE_COLOR[tone];
  return (
    <div
      style={{
        display: "flex",
        alignItems: compact ? "center" : "flex-start",
        gap: 8,
        padding: compact ? "4px 0" : "9px 0",
      }}
    >
      <span
        aria-hidden
        style={{
          display: "grid",
          placeItems: "center",
          width: compact ? 18 : 22,
          height: compact ? 18 : 22,
          borderRadius: 999,
          flex: "0 0 auto",
          marginTop: compact ? 0 : 1,
          color,
          background: `color-mix(in srgb, ${color} 14%, transparent)`,
        }}
      >
        <Icon name={TONE_ICON[tone]} size={compact ? 11 : 13} />
      </span>
      <span style={{ minWidth: 0, display: "flex", flexDirection: "column", gap: 1 }}>
        <span
          style={{
            fontSize: compact ? 12.5 : 13,
            fontWeight: 600,
            letterSpacing: "-0.1px",
            color: "var(--text)",
          }}
        >
          {component.label}
        </span>
        {!compact && (
          <span style={{ fontSize: 12.5, color: "var(--text-secondary)", lineHeight: 1.45 }}>
            {component.detail}
          </span>
        )}
      </span>
    </div>
  );
}

/**
 * The always-visible answer to "is this thing actually working?".
 *
 * Deliberately not a spinner or a percentage. A model that is downloaded but
 * not resident is fully working, and rendering that as "pending" would train
 * the user to ignore the one indicator that does mean something.
 */
export function ReadinessPanel({
  status,
  onFix,
}: {
  status: RuntimeStatus | null;
  onFix?: () => void;
}) {
  const info = useRuntimeStatus();
  const now = useNow(1000, info.status !== null && !info.stale);
  // Prefer the caller's value (a screen may pass its own snapshot); fall back
  // to this panel's own read so the checked-ago line is always live.
  const current = status ?? info.status;

  // Null means the backend has not answered yet. Say so rather than implying
  // a healthy default.
  if (!current) {
    return (
      <div className="tt-card" style={{ padding: "14px 16px" }}>
        <div className="tt-eyebrow">Status</div>
        <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
          Checking what is loaded…
        </div>
      </div>
    );
  }

  const blocked = current.speech.blocking;

  return (
    <div className="tt-card" style={{ padding: "12px 16px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          marginBottom: 6,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <div className="tt-eyebrow">Status</div>
          {info.stale && (
            <span
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: "var(--warning)",
                background: "color-mix(in srgb, var(--warning) 14%, transparent)",
                borderRadius: 999,
                padding: "1px 8px",
                whiteSpace: "nowrap",
              }}
            >
              Not updating
            </span>
          )}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span
            aria-live="polite"
            style={{ fontSize: 11, color: "var(--text-tertiary)", whiteSpace: "nowrap" }}
          >
            {checkedAgoLabel(info.lastChecked, now)}
          </span>
          {blocked && onFix && (
            <button className="tt-btn tt-btn-primary tt-btn-sm" onClick={onFix}>
              Fix this
            </button>
          )}
        </div>
      </div>
      {/* One line: transcription model left, polish model right. The detail
          sentence is redundant here — the check mark already says loaded,
          and the Models tab has the full state. */}
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <div style={{ flex: "1 1 0", minWidth: 0 }}>
          <StatusRow component={current.speech} compact />
        </div>
        <div
          style={{
            width: 1,
            alignSelf: "stretch",
            background: "var(--border-subtle)",
            margin: "2px 0",
          }}
        />
        <div style={{ flex: "1 1 0", minWidth: 0 }}>
          <StatusRow component={current.llm} compact />
        </div>
      </div>
    </div>
  );
}
