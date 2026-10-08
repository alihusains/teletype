import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";

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

/**
 * Reads the app's real runtime state.
 *
 * This exists as a hook rather than a one-shot fetch because the answer
 * changes on its own: a model unloads after an idle timeout, a download
 * finishes, a llama-server dies. A snapshot taken when a screen mounted is
 * stale the moment any of those happen, which is how the previous UI came to
 * show "Ready" next to a model that had been unloaded.
 *
 * Polling is deliberately coarse (POLL_MS) and pauses while the tab is
 * hidden: the state that changes fast is the dictation phase, and that already
 * arrives as an event. Nothing here is on the dictation hot path, so a 5 s
 * cadence cannot cost the user latency.
 */
const POLL_MS = 5000;

export function useRuntimeStatus(active = true): RuntimeStatus | null {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let timer: number | undefined;

    const read = async () => {
      try {
        const s = await invoke<RuntimeStatus>("get_runtime_status");
        if (!cancelled) setStatus(s);
      } catch {
        // The command is unavailable (older backend, or state not ready).
        // Leave the previous value; the surface renders its own unknown state.
      }
      if (!cancelled) timer = window.setTimeout(read, POLL_MS);
    };

    const onVisible = () => {
      if (document.visibilityState === "visible") {
        window.clearTimeout(timer);
        timer = undefined;
        void read();
      }
    };

    read();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [active]);

  return status;
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
  // Null means the backend has not answered yet. Say so rather than implying
  // a healthy default.
  if (!status) {
    return (
      <div className="tt-card" style={{ padding: "14px 16px" }}>
        <div className="tt-eyebrow">Status</div>
        <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
          Checking what is loaded…
        </div>
      </div>
    );
  }

  const blocked = status.speech.blocking;

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
        <div className="tt-eyebrow">Status</div>
        {blocked && onFix && (
          <button className="tt-btn tt-btn-primary tt-btn-sm" onClick={onFix}>
            Fix this
          </button>
        )}
      </div>
      {/* One line: transcription model left, polish model right. The detail
          sentence is redundant here — the check mark already says loaded,
          and the Models tab has the full state. */}
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <div style={{ flex: "1 1 0", minWidth: 0 }}>
          <StatusRow component={status.speech} compact />
        </div>
        <div style={{ width: 1, alignSelf: "stretch", background: "var(--border-subtle)", margin: "2px 0" }} />
        <div style={{ flex: "1 1 0", minWidth: 0 }}>
          <StatusRow component={status.llm} compact />
        </div>
      </div>
    </div>
  );
}
