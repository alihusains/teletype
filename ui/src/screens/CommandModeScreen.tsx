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

/** One row from `list_command_deciders`: a decider the app can offer. */
interface DeciderRow {
  id: string;
  name: string;
  installed: boolean;
  available: boolean;
  reason: string | null;
}

interface ToolRow {
  tool: string;
  summary: string;
  example: string;
  /** What happens when the command runs. */
  outcome: string;
  /** One extra truth the user needs, when the tool has a caveat. */
  note?: string;
}

// Mirrors the 14 tools in `crates/teletype-desktop/src/command_executor.rs`
// and the schemas in `crates/teletype-inference/src/needle.rs`. The outcome
// column is the behaviour in the executor, not a promise we add here.
const TOOLS: ToolRow[] = [
  {
    tool: "open_app",
    summary: "Opens an app by name.",
    example: "Open Safari",
    outcome: "Acts immediately",
  },
  {
    tool: "open_url",
    summary: "Opens a website, or searches the web.",
    example: "Go to github.com",
    outcome: "Acts immediately",
  },
  {
    tool: "open_file",
    summary: "Opens a file or folder.",
    example: "Open my downloads",
    outcome: "Acts immediately",
  },
  {
    tool: "open_settings",
    summary: "Opens System Settings, or one pane.",
    example: "Open display settings",
    outcome: "Acts immediately",
  },
  {
    tool: "keyboard_shortcut",
    summary: "Presses a shortcut or runs a system action.",
    example: "Copy this",
    outcome: "Acts immediately",
    note: "No secure-field check",
  },
  {
    tool: "type_text",
    summary: "Types text where the cursor is.",
    example: "Type see you at six",
    outcome: "Acts immediately",
    note: "Refuses when the field is secure",
  },
  {
    tool: "compose_email",
    summary: "Opens a Gmail draft.",
    example: "Email sam at example dot com about lunch",
    outcome: "Opens a draft",
    note: "Nothing is sent",
  },
  {
    tool: "send_message",
    summary: "Sends a message through Messages.",
    example: "Text Sam that I am running late",
    outcome: "Sends immediately",
    note: "No confirmation step",
  },
  {
    tool: "create_calendar_event",
    summary: "Opens a Google Calendar draft.",
    example: "Meeting with Sam tomorrow at three",
    outcome: "Opens a draft",
    note: "Nothing is saved",
  },
  {
    tool: "set_timer",
    summary: "Opens the Clock app.",
    example: "Set a timer for five minutes",
    outcome: "Acts immediately",
    note: "The timer is not started for you",
  },
  {
    tool: "set_volume",
    summary: "Changes the system volume.",
    example: "Set volume to 30 percent",
    outcome: "Acts immediately",
  },
  {
    tool: "mute_speaker",
    summary: "Mutes or unmutes the speaker.",
    example: "Mute the speaker",
    outcome: "Acts immediately",
  },
  {
    tool: "set_brightness",
    summary: "Changes the screen brightness.",
    example: "Dim the screen",
    outcome: "Acts immediately",
  },
  {
    tool: "play_media",
    summary: "Plays, pauses or skips the current media.",
    example: "Pause the music",
    outcome: "Acts immediately",
  },
];

export default function CommandModeScreen() {
  const [enabled, setEnabled] = useState<boolean>(false);
  const [traces, setTraces] = useState<TraceEntry[]>([]);
  const [commandHotkey, setCommandHotkey] = useState<string>("");
  const [deciderModel, setDeciderModel] = useState<string>("neudecide-q4");
  const [deciderRows, setDeciderRows] = useState<DeciderRow[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    invoke<{
      commandModeEnabled?: boolean;
      commandHotkey?: string;
      commandDeciderModel?: string;
    }>("get_settings")
      .then((s) => {
        setEnabled(!!s.commandModeEnabled);
        setCommandHotkey(s.commandHotkey ?? "");
        setDeciderModel(s.commandDeciderModel ?? "neudecide-q4");
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  // The picker's options come from the backend's decider inventory, which
  // reports each row's live availability (installed + runnable). The UI
  // never invents a model the backend cannot build.
  useEffect(() => {
    invoke<DeciderRow[]>("list_command_deciders")
      .then(setDeciderRows)
      .catch(() => setDeciderRows([]));
  }, []);

  useTauriEvent<void>("settings-changed", () => {
    invoke<{
      commandModeEnabled?: boolean;
      commandHotkey?: string;
      commandDeciderModel?: string;
    }>("get_settings")
      .then((s) => {
        setEnabled(!!s.commandModeEnabled);
        setCommandHotkey(s.commandHotkey ?? "");
        setDeciderModel(s.commandDeciderModel ?? "neudecide-q4");
      })
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

  const pickDeciderModel = async (id: string) => {
    const prev = deciderModel;
    setDeciderModel(id);
    try {
      const settings = await invoke<Record<string, unknown>>("get_settings");
      settings.commandDeciderModel = id;
      await invoke("save_settings", { settings });
    } catch (e) {
      // The picker shows the backend's value; a rejected save must put the
      // previous model back, the same way the toggle above restores on error.
      console.error("Failed to save decider model:", e);
      setDeciderModel(prev);
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

  const selectedDecider = deciderRows.find((r) => r.id === deciderModel);
  const deciderUnavailable =
    deciderRows.length > 0 && (!selectedDecider || !selectedDecider.available);
  const noDeciderAvailable =
    deciderRows.length > 0 && !deciderRows.some((r) => r.available);

  return (
    <div style={{ width: "100%", maxWidth: 860 }}>
      {/* Header */}
      <div style={{ marginBottom: 24 }}>
        <h2 className="tt-screen-title">
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
        </h2>
        <p className="tt-screen-sub">
          When enabled, spoken commands like "open Safari" or "set brightness to 50%" are
          executed directly instead of being typed into the active field.
        </p>
      </div>

      {/* How it works */}
      <section style={{ marginBottom: 24 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: "var(--text-md)",
            fontWeight: 650,
            letterSpacing: "var(--tracking-tight)",
            marginBottom: 12,
          }}
        >
          <Icon name="zap" size={16} style={{ display: "inline", verticalAlign: "middle" }} />
          How it works
        </div>
        <ol
          style={{
            margin: 0,
            padding: "0 0 0 20px",
            display: "flex",
            flexDirection: "column",
            gap: 8,
            fontSize: "var(--text-sm)",
            color: "var(--text-secondary)",
            lineHeight: 1.5,
          }}
        >
          <li>
            Turn on <strong>Enable Command Mode</strong> below.
          </li>
          <li>
            Pick a decider model that is installed and available (the picker marks
            unusable models with the reason why).
          </li>
          <li>
            {commandHotkey.trim()
              ? `Press the command hotkey (${commandHotkey.trim()}) and say what to do, for example "open Safari". You can also start a normal dictation and begin it with the wake phrase "teletype" or "hey teletype"; the rest of the sentence runs as a command instead.`
              : 'Start a normal dictation and begin it with the wake phrase "teletype" or "hey teletype"; the rest of the sentence runs as a command instead. Or set a command hotkey on the Keybinds screen to start a command take directly.'}
          </li>
          <li>
            Teletype transcribes, classifies and runs the command. Each run appears in
            the Live Pipeline below.
          </li>
        </ol>
      </section>

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
            <div
              style={{
                fontSize: "var(--text-sm)",
                color: "var(--text-tertiary)",
                marginTop: 4,
              }}
            >
              {commandHotkey.trim()
                ? `Hotkey: ${commandHotkey.trim()}`
                : "Hotkey: not set (wake phrase still works)"}
            </div>
            {deciderRows.length > 0 && (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  marginTop: 10,
                }}
              >
                <label
                  htmlFor="decider-model"
                  style={{
                    fontSize: "var(--text-sm)",
                    color: "var(--text-tertiary)",
                  }}
                >
                  Decider
                </label>
                <select
                  id="decider-model"
                  value={deciderModel}
                  onChange={(e) => pickDeciderModel(e.target.value)}
                  style={{
                    fontSize: "var(--text-sm)",
                    padding: "4px 8px",
                    borderRadius: "var(--radius-xs, 6px)",
                    background: "var(--surface-2)",
                    color: "var(--text-primary)",
                    border: "1px solid var(--border)",
                  }}
                >
                  {deciderRows.map((row) => (
                    <option key={row.id} value={row.id} disabled={!row.available}>
                      {row.name}
                      {row.available
                        ? ""
                        : row.reason
                          ? ` (${row.reason})`
                          : " (not installed)"}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </div>
          <Toggle label="Enable Command Mode" checked={enabled} onChange={toggle} />
        </div>
        {deciderUnavailable && (
          <div
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: 8,
              margin: "0 16px 12px",
              padding: "10px 12px",
              borderRadius: "var(--radius-xs, 6px)",
              background: "var(--warning-soft)",
              border: "1px solid var(--warning)",
              fontSize: "var(--text-sm)",
              color: "var(--text-secondary)",
              lineHeight: 1.5,
            }}
          >
            <Icon name="alert" size={16} style={{ flexShrink: 0, marginTop: 1 }} />
            <span>
              <strong>Nothing can decide your commands yet.</strong>{" "}
              {noDeciderAvailable
                ? "No decider model is installed. Download one on the Models screen, then pick it here."
                : `The selected decider is unavailable${selectedDecider?.reason ? ` (${selectedDecider.reason})` : ""}. Pick another decider, or download the missing model on the Models screen.`}
            </span>
          </div>
        )}
      </div>

      {/* Tools */}
      <section style={{ marginBottom: 24 }}>
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
          <Icon name="terminal" size={16} style={{ display: "inline", verticalAlign: "middle" }} />
          What it can do
        </div>
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-tertiary)", marginBottom: 12 }}>
          The commands the app can run on your behalf.
        </p>
        <div className="tt-card" style={{ overflow: "hidden" }}>
          <table
            style={{
              width: "100%",
              borderCollapse: "collapse",
              fontSize: "var(--text-sm)",
            }}
          >
            <thead>
              <tr>
                {["Command", "What it does", "Example", "When it runs"].map((h) => (
                  <th
                    key={h}
                    style={{
                      textAlign: "left",
                      padding: "10px 16px",
                      fontWeight: 600,
                      color: "var(--text-secondary)",
                      fontSize: "var(--text-xs)",
                      textTransform: "uppercase",
                      letterSpacing: "0.04em",
                      borderBottom: "1px solid var(--border)",
                    }}
                  >
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {TOOLS.map((row) => (
                <tr key={row.tool}>
                  <td
                    style={{
                      padding: "10px 16px",
                      fontFamily: MONO,
                      fontSize: "var(--text-xs)",
                      fontWeight: 600,
                      whiteSpace: "nowrap",
                      borderBottom: "1px solid var(--border)",
                    }}
                  >
                    {row.tool}
                  </td>
                  <td
                    style={{
                      padding: "10px 16px",
                      color: "var(--text-secondary)",
                      borderBottom: "1px solid var(--border)",
                    }}
                  >
                    {row.summary}
                    {row.note && (
                      <span style={{ color: "var(--text-tertiary)" }}> {row.note}</span>
                    )}
                  </td>
                  <td
                    style={{
                      padding: "10px 16px",
                      color: "var(--text-tertiary)",
                      borderBottom: "1px solid var(--border)",
                    }}
                  >
                    {row.example}
                  </td>
                  <td
                    style={{
                      padding: "10px 16px",
                      color: "var(--text-secondary)",
                      whiteSpace: "nowrap",
                      borderBottom: "1px solid var(--border)",
                    }}
                  >
                    {row.outcome}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

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
          Real-time view of each command: what you said, how it was classified, and what happened.
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
            <div style={{ marginBottom: 6 }}>No commands yet.</div>
            <div style={{ fontSize: "var(--text-sm)", lineHeight: 1.6 }}>
              {enabled
                ? `Turn on the toggle above and press the command hotkey${commandHotkey.trim() ? ` (${commandHotkey.trim()})` : ""}, or start a dictation with the wake phrase "teletype", and say something like "open Safari". Each run appears here.`
                : `Turn on the toggle above, then start a command with the wake phrase "teletype" or the command hotkey. Each run appears here.`}
            </div>
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
