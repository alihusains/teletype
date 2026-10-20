import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "../lib/useTauriEvent";
import { Icon } from "../components/Icon";
import { Toggle } from "../settings/primitives";

interface PipelineTrace {
  raw_input: string;
  intent: string | null;
  command: { tool: string; arguments: Record<string, unknown> } | null;
  transformed: boolean;
  final_text: string;
  pipeline_ms: number;
  llm_ms: number;
}

interface TraceEntry extends PipelineTrace {
  id: number;
  timestamp: string;
}

const MONO = "ui-monospace, SFMono-Regular, Menlo, monospace";

export default function CommandModeScreen() {
  const [enabled, setEnabled] = useState<boolean>(false);
  const [traces, setTraces] = useState<TraceEntry[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    invoke<{ commandModeEnabled?: boolean }>("get_settings")
      .then((s) => {
        setEnabled(!!s.commandModeEnabled);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  useTauriEvent<void>("settings-changed", () => {
    invoke<{ commandModeEnabled?: boolean }>("get_settings")
      .then((s) => setEnabled(!!s.commandModeEnabled))
      .catch(() => {});
  });

  useTauriEvent<PipelineTrace>("pipeline-trace", ({ payload }) => {
    const entry: TraceEntry = {
      ...payload,
      id: Date.now(),
      timestamp: new Date().toLocaleTimeString(),
    };
    setTraces((prev) => [entry, ...prev].slice(0, 20));
  });

  const toggle = async (value: boolean) => {
    setEnabled(value);
    try {
      const settings = await invoke<Record<string, unknown>>("get_settings");
      settings.commandModeEnabled = value;
      await invoke("save_settings", { settings });
    } catch (e) {
      console.error("Failed to save command mode setting:", e);
      setEnabled(!value);
    }
  };

  if (loading) {
    return (
      <div style={{ width: "100%", maxWidth: 860 }}>
        <div
          style={{
            height: 24,
            width: 180,
            borderRadius: 6,
            background: "var(--surface-2)",
            marginBottom: 12,
          }}
        />
        <div
          style={{
            height: 88,
            width: "100%",
            borderRadius: "var(--radius)",
            background: "var(--surface-2)",
          }}
        />
      </div>
    );
  }

  return (
    <div style={{ width: "100%", maxWidth: 860 }}>
      {/* Header */}
      <div style={{ marginBottom: 24 }}>
        <h1
          style={{
            margin: 0,
            fontSize: 22,
            fontWeight: 650,
            letterSpacing: "var(--tracking-tight)",
          }}
        >
          Command Mode
          <sup
            style={{
              fontSize: "var(--text-xs)",
              color: "var(--accent)",
              marginLeft: 6,
              fontWeight: 600,
            }}
          >
            beta
          </sup>
        </h1>
        <p
          style={{
            margin: "6px 0 0",
            color: "var(--text-secondary)",
            fontSize: "var(--text-sm)",
            lineHeight: 1.5,
            maxWidth: "56ch",
          }}
        >
          When enabled, spoken commands like "open Safari" or "set brightness to 50%" are
          executed directly instead of being typed into the active field.
        </p>
      </div>

      {/* Toggle */}
      <div className="tt-card" style={{ marginBottom: 24, overflow: "hidden" }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 16,
            padding: "14px 16px",
          }}
        >
          <div>
            <div style={{ fontWeight: 600, fontSize: "var(--text-md)" }}>Enable Command Mode</div>
            <div
              style={{
                fontSize: "var(--text-sm)",
                color: "var(--text-tertiary)",
                marginTop: 4,
                maxWidth: "56ch",
              }}
            >
              Voice commands are classified and executed on-device.
              Dictation is unaffected and always polished.
            </div>
          </div>
          <Toggle label="Enable Command Mode" checked={enabled} onChange={toggle} />
        </div>
      </div>

      {/* Pipeline Visualizer */}
      <section>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: "var(--text-md)",
            fontWeight: 650,
            letterSpacing: "var(--tracking-tight)",
            marginBottom: 4,
          }}
        >
          <Icon name="zap" size={16} style={{ display: "inline", verticalAlign: "middle" }} />
          Live Pipeline
        </div>
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-tertiary)", marginBottom: 12 }}>
          Real-time view of each dictation: what you said, how it was classified, and what happened.
        </p>

        {traces.length === 0 ? (
          <div
            style={{
              padding: 32,
              textAlign: "center",
              color: "var(--text-tertiary)",
              fontSize: "var(--text-md)",
              background: "var(--surface-2)",
              borderRadius: "var(--radius)",
            }}
          >
            No dictations yet. Speak something to see the pipeline in action.
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {traces.map((t) => (
              <TraceCard key={t.id} trace={t} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function TraceCard({ trace }: { trace: TraceEntry }) {
  const isCommand = trace.intent === "command";
  const isDictation = trace.intent === "dictation";

  const chip = (label: string, color: string) => (
    <span
      style={{
        padding: "1px 8px",
        borderRadius: "var(--radius-pill, 999px)",
        fontSize: "var(--text-xs)",
        fontWeight: 600,
        border: `1px solid ${color}`,
        color,
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </span>
  );

  return (
    <div className="tt-card" style={{ padding: "12px 16px", fontSize: "var(--text-sm)" }}>
      {/* Row 1: timestamp + intent badge */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
        <span
          style={{
            color: "var(--text-tertiary)",
            fontSize: "var(--text-xs)",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {trace.timestamp}
        </span>
        {isCommand && chip("COMMAND", "var(--accent-ink)")}
        {isDictation && chip("DICTATION", "var(--success)")}
        {!trace.intent && chip("NO ROUTING", "var(--text-tertiary)")}
        <span
          style={{
            marginLeft: "auto",
            color: "var(--text-tertiary)",
            fontSize: "var(--text-xs)",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {trace.pipeline_ms} ms
        </span>
      </div>

      {/* Row 2: what was said */}
      <div style={{ marginBottom: 6 }}>
        <span
          style={{
            color: "var(--text-tertiary)",
            fontSize: "var(--text-xs)",
            display: "block",
            marginBottom: 2,
          }}
        >
          You said
        </span>
        <span style={{ fontWeight: 500, fontSize: "var(--text-md)" }}>"{trace.raw_input}"</span>
      </div>

      {/* Row 3: what happened */}
      {isCommand && trace.command ? (
        <div style={{ marginTop: 6 }}>
          <span
            style={{
              color: "var(--text-tertiary)",
              fontSize: "var(--text-xs)",
              display: "block",
              marginBottom: 2,
            }}
          >
            Executed
          </span>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "6px 10px",
              background: "var(--surface-2)",
              borderRadius: "var(--radius-xs, 6px)",
              boxShadow: "var(--shadow-1)",
              fontFamily: MONO,
              fontSize: "var(--text-xs)",
              overflowWrap: "anywhere",
            }}
          >
            <Icon name="terminal" size={14} />
            <span style={{ fontWeight: 600 }}>{trace.command.tool}</span>
            <span style={{ color: "var(--text-tertiary)" }}>
              {JSON.stringify(trace.command.arguments)}
            </span>
          </div>
        </div>
      ) : isDictation ? (
        <div style={{ marginTop: 6 }}>
          <span
            style={{
              color: "var(--text-tertiary)",
              fontSize: "var(--text-xs)",
              display: "block",
              marginBottom: 2,
            }}
          >
            {trace.transformed ? "Polished & injected" : "Injected as-is"}
          </span>
          <span style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>
            "{trace.final_text}"
          </span>
        </div>
      ) : null}
    </div>
  );
}
