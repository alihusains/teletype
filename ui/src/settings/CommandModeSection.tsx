import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Section, Row, PrimaryButton } from "./primitives";

const EXAMPLES = [
  "write an email to",
  "shorten this",
  "make it professional",
  "make this casual",
  "translate this",
  "make bullets",
  "open Safari",
];

/// Command Mode (roadmap C1.1 text commands + C1.2 local actions): a place to
/// type a command and see exactly what it will do. The production entry point
/// is the dictation hotkey + a spoken command; this panel is the discoverable,
/// keyboard-operable surface for trying each intent without a microphone.
export function CommandModeSection() {
  const [command, setCommand] = useState("");
  const [result, setResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async (text: string) => {
    if (!text.trim() || busy) return;
    setBusy(true);
    setResult(null);
    try {
      const msg = await invoke<string>("run_command", { utterance: text });
      setResult({ ok: true, message: msg });
    } catch (err) {
      setResult({ ok: false, message: String(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title="Command Mode"
      hint="Speak or type a command. It drafts contextual text into the field you're in (never sends it), rewrites selected text, or opens an allowlisted app. Ambiguous commands are refused, not guessed."
    >
      <Row label="Try a command" hint="Type a command and press Run. The result appears below, exactly as the pill would show it." first>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          <input
            aria-label="Command text"
            value={command}
            placeholder="e.g. shorten this, or open Safari"
            onChange={(e) => setCommand(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") run(command);
            }}
            style={{
              width: 260,
              padding: "5px 8px",
              fontSize: "var(--text-xs)",
              color: "var(--text)",
              background: "var(--surface-2)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius-sm)",
            }}
          />
          <PrimaryButton onClick={() => run(command)} disabled={busy || !command.trim()}>
            {busy ? "Running…" : "Run"}
          </PrimaryButton>
        </div>
      </Row>
      <Row label="Examples" hint="Tap one to fill the box.">
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", justifyContent: "flex-end" }}>
          {EXAMPLES.map((ex) => (
            <button
              key={ex}
              type="button"
              onClick={() => setCommand(ex)}
              style={{
                background: "var(--surface-2)",
                border: "1px solid var(--border)",
                borderRadius: 8,
                padding: "4px 10px",
                fontSize: "var(--text-xs)",
                color: "var(--text)",
                cursor: "pointer",
              }}
            >
              {ex}
            </button>
          ))}
        </div>
      </Row>
      {result ? (
        <Row label={result.ok ? "Result" : "Refused / error"}>
          <div
            role="status"
            aria-live="polite"
            style={{
              fontSize: "var(--text-sm)",
              color: result.ok ? "var(--text)" : "var(--danger, #c0392b)",
              whiteSpace: "pre-wrap",
              maxWidth: 420,
            }}
          >
            {result.message}
          </div>
        </Row>
      ) : null}
    </Section>
  );
}
