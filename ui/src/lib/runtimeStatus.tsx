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

type Tone = "ok" | "busy" | "bad";

const TONE: Record<ComponentRuntime["state"], Tone> = {
  loaded: "ok",
  ready: "ok",
  loading: "busy",
  error: "bad",
  missing: "bad",
  none: "bad",
};

const TONE_COLOR: Record<Tone, string> = {
  ok: "var(--success)",
  busy: "var(--accent)",
  bad: "var(--danger)",
};

const TONE_ICON: Record<Tone, IconName> = {
  ok: "check",
  busy: "refresh",
  bad: "x",
};

function runtimeTone(component: ComponentRuntime): Tone {
  return TONE[component.state] ?? "bad";
}

function runtimeStateLabel(component: ComponentRuntime): string {
  const tone = runtimeTone(component);
  if (tone === "busy") return "Loading";
  if (tone === "bad") return "Not loaded";
  return component.state === "ready" ? "Ready" : "Loaded";
}

/// The model name to show in the readiness row. Speech labels are already the
/// catalogue's friendly name; local transform labels are generic ("Local
/// transform"), so use the selected model id there instead.
function modelName(component: ComponentRuntime, role?: string): string {
  if (!component.modelId) {
    return component.state === "none" ? "No model selected" : component.label;
  }
  if (role === "Speech Model") return component.label || component.modelId;
  return role ? component.modelId : component.label;
}

/**
 * One model's readiness: a state icon, the role and model name, and the state
 * in words. The icon is the at-a-glance answer (green check = ready, red x =
 * unavailable, spinner = loading); the words make it unambiguous.
 */
export function StatusRow({
  component,
  compact = false,
  role,
}: {
  component: ComponentRuntime;
  compact?: boolean;
  role?: string;
}) {
  const tone = runtimeTone(component);
  const color = TONE_COLOR[tone];
  const name = modelName(component, role);
  const state = runtimeStateLabel(component);
  return (
    <div
      title={component.detail}
      style={{
        display: "flex",
        alignItems: compact ? "center" : "flex-start",
        gap: compact ? 8 : 10,
        minWidth: 0,
        padding: compact ? "4px 0" : "9px 0",
      }}
    >
      <span
        aria-hidden
        style={{
          display: "grid",
          placeItems: "center",
          width: compact ? 20 : 22,
          height: compact ? 20 : 22,
          borderRadius: 999,
          flex: "0 0 auto",
          marginTop: compact ? 0 : 1,
          color,
          background: `color-mix(in srgb, ${color} 13%, transparent)`,
        }}
      >
        <Icon
          name={TONE_ICON[tone]}
          size={compact ? 12 : 13}
          style={tone === "busy" ? { animation: "tt-spin 1.1s linear infinite" } : undefined}
        />
      </span>
      <span style={{ minWidth: 0, flex: 1, display: "flex", flexDirection: "column", gap: 1 }}>
        {role ? (
          <span
            style={{
              fontSize: 11.5,
              fontWeight: 600,
              color: "var(--text-tertiary)",
              letterSpacing: "0.01em",
            }}
          >
            {role}
          </span>
        ) : null}
        <span
          style={{
            fontSize: compact ? 13 : 13.5,
            fontWeight: 600,
            letterSpacing: "-0.1px",
            color: "var(--text)",
            minWidth: 0,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {name}
        </span>
        {!compact && (
          <span style={{ fontSize: 12.5, color: "var(--text-secondary)", lineHeight: 1.45 }}>
            {component.detail}
          </span>
        )}
      </span>
      <span
        style={{
          flex: "0 0 auto",
          fontSize: 11.5,
          fontWeight: 600,
          color,
          whiteSpace: "nowrap",
        }}
      >
        {state}
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
        <div style={{ fontSize: 13, color: "var(--text-secondary)", marginTop: 4 }}>
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
          marginBottom: 8,
        }}
      >
        <div className="tt-eyebrow">Status</div>
        {blocked && onFix && (
          <button className="tt-btn tt-btn-primary tt-btn-sm" onClick={onFix}>
            Fix this
          </button>
        )}
      </div>
      {/* Two columns, each naming the role and the selected model. The state
          icon and word are deliberately redundant: the icon answers at a
          glance, the word survives colour-blindness and screenshots. The
          detail sentence lives in the tooltip and on the Models tab. */}
      <div style={{ display: "flex", alignItems: "stretch", gap: 12 }}>
        <div style={{ flex: "1 1 0", minWidth: 0 }}>
          <StatusRow component={status.speech} compact role="Speech Model" />
        </div>
        <div style={{ width: 1, alignSelf: "stretch", background: "var(--border-subtle)", margin: "2px 0" }} />
        <div style={{ flex: "1 1 0", minWidth: 0 }}>
          <StatusRow component={status.llm} compact role="Polish Model" />
        </div>
      </div>
    </div>
  );
}
