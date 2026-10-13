import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "../lib/useTauriEvent";
import { Icon } from "../components/Icon";

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
    return <div style={{ padding: 28, color: "var(--text-tertiary)" }}>Loading…</div>;
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 24 }}>
      {/* Header */}
      <div>
        <h1 style={{ margin: 0, fontSize: 22, fontWeight: 600 }}>
          Command Mode
          <sup style={{ fontSize: 11, color: "var(--accent)", marginLeft: 4, fontWeight: 500 }}>
            beta
          </sup>
        </h1>
        <p style={{ margin: "8px 0 0", color: "var(--text-secondary)", fontSize: 14, lineHeight: 1.5 }}>
          When enabled, spoken commands like "open Safari" or "set brightness to 50%" are
          executed directly instead of being typed into the active field.
        </p>
      </div>

      {/* Toggle */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "16px 20px",
          background: "var(--surface)",
          borderRadius: 12,
          border: "1px solid var(--border)",
        }}
      >
        <div>
          <div style={{ fontWeight: 500, fontSize: 15 }}>Enable Command Mode</div>
          <div style={{ fontSize: 13, color: "var(--text-tertiary)", marginTop: 4 }}>
            Voice commands are classified and executed on-device.
            Dictation is unaffected and always polished.
          </div>
        </div>
        <button
          onClick={() => toggle(!enabled)}
          style={{
            width: 48,
            height: 28,
            borderRadius: 14,
            border: "none",
            cursor: "pointer",
            position: "relative",
            background: enabled ? "var(--accent)" : "var(--border)",
            transition: "background 0.2s",
          }}
        >
          <span
            style={{
              position: "absolute",
              top: 3,
              left: enabled ? 23 : 3,
              width: 22,
              height: 22,
              borderRadius: "50%",
              background: "white",
              transition: "left 0.2s",
              boxShadow: "0 1px 3px rgba(0,0,0,0.2)",
            }}
          />
        </button>
      </div>

      {/* Pipeline Visualizer */}
      <div>
        <h2 style={{ fontSize: 16, fontWeight: 600, marginBottom: 12 }}>
          <Icon name="zap" size={16} style={{ display: "inline", verticalAlign: "middle", marginRight: 6 }} />
          Live Pipeline
        </h2>
        <p style={{ fontSize: 13, color: "var(--text-tertiary)", marginBottom: 12 }}>
          Real-time view of each dictation: what you said, how it was classified, and what happened.
        </p>

        {traces.length === 0 ? (
          <div
            style={{
              padding: 32,
              textAlign: "center",
              color: "var(--text-tertiary)",
              fontSize: 14,
              border: "1px dashed var(--border)",
              borderRadius: 12,
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
      </div>
    </div>
  );
}

function TraceCard({ trace }: { trace: TraceEntry }) {
  const isCommand = trace.intent === "command";
  const isDictation = trace.intent === "dictation";

  return (
    <div
      style={{
        padding: "14px 16px",
        background: "var(--surface)",
        borderRadius: 10,
        border: "1px solid var(--border)",
        fontSize: 13,
      }}
    >
      {/* Row 1: timestamp + intent badge */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
        <span style={{ color: "var(--text-tertiary)", fontSize: 11 }}>{trace.timestamp}</span>
        {isCommand && (
          <span
            style={{
              padding: "2px 8px",
              borderRadius: 4,
              fontSize: 11,
              fontWeight: 600,
              background: "var(--accent)",
              color: "white",
            }}
          >
            COMMAND
          </span>
        )}
        {isDictation && (
          <span
            style={{
              padding: "2px 8px",
              borderRadius: 4,
              fontSize: 11,
              fontWeight: 600,
              background: "var(--success)",
              color: "white",
            }}
          >
            DICTATION
          </span>
        )}
        {!trace.intent && (
          <span
            style={{
              padding: "2px 8px",
              borderRadius: 4,
              fontSize: 11,
              fontWeight: 600,
              background: "var(--border)",
              color: "var(--text-secondary)",
            }}
          >
            NO ROUTING
          </span>
        )}
        <span style={{ marginLeft: "auto", color: "var(--text-tertiary)", fontSize: 11 }}>
          {trace.pipeline_ms} ms
        </span>
      </div>

      {/* Row 2: what was said */}
      <div style={{ marginBottom: 6 }}>
        <span style={{ color: "var(--text-tertiary)", fontSize: 11, display: "block", marginBottom: 2 }}>
          You said
        </span>
        <span style={{ fontWeight: 500, fontSize: 14 }}>"{trace.raw_input}"</span>
      </div>

      {/* Row 3: what happened */}
      {isCommand && trace.command ? (
        <div style={{ marginTop: 6 }}>
          <span style={{ color: "var(--text-tertiary)", fontSize: 11, display: "block", marginBottom: 2 }}>
            Executed
          </span>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "6px 10px",
              background: "var(--surface-hover)",
              borderRadius: 6,
              fontFamily: "var(--font-mono, monospace)",
              fontSize: 12,
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
          <span style={{ color: "var(--text-tertiary)", fontSize: 11, display: "block", marginBottom: 2 }}>
            {trace.transformed ? "Polished & injected" : "Injected as-is"}
          </span>
          <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>
            "{trace.final_text}"
          </span>
        </div>
      ) : null}
    </div>
  );
}
