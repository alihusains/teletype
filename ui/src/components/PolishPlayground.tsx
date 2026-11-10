import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { measureWords } from "../screens/StylesScreen";
import { errorMessage } from "../lib/errors";

interface PolishPromptInfo {
  modelId: string;
  editable: boolean;
  defaultPrompt: string;
  note: string;
}

interface TestResult {
  text: string;
  transformed: boolean;
  skipMessage: string | null;
}

const SAMPLE_TEXT =
  "hey so about the deck i added some slides but im not sure if they go with your part. it seems kinda long maybe we should remove the market trends thing? i can look at it again tonight if u want. also the pricing slide might be wrong cuz the data changed. we should check before sending to the board";

/**
 * The AI Polish prompt section: shows the selected model's prompt, lets the
 * user edit it when the model's prompt path is editable, and offers a
 * playground to paste sample text and see the model's real output with the
 * current prompt. Fixed-contract models (S1-mini, SpeakoFlow Mini, and
 * Qwen3.5-2B's Polish control-line prompt) show a note and a read-only
 * prompt; the playground still runs with the model's default prompt.
 */
export default function PolishPlayground({ modelId }: { modelId: string }) {
  const [info, setInfo] = useState<PolishPromptInfo | null>(null);
  const [overrideText, setOverrideText] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const [sampleInput, setSampleInput] = useState(SAMPLE_TEXT);
  const [running, setRunning] = useState(false);
  const [output, setOutput] = useState("");
  const [runError, setRunError] = useState<string | null>(null);
  const [transformed, setTransformed] = useState(false);

  const load = useCallback(() => {
    invoke<PolishPromptInfo>("get_polish_prompt_info")
      .then((i) => {
        setInfo(i);
        invoke<Record<string, unknown>>("get_settings")
          .then((s) => {
            const stored = (s.polishPromptOverride as string | null | undefined) ?? "";
            setOverrideText(stored);
          })
          .catch(() => {});
      })
      .catch((e) => console.error("get_polish_prompt_info failed", e));
  }, []);

  useEffect(() => {
    setInfo(null);
    setOutput("");
    setRunError(null);
    load();
  }, [modelId, load]);

  const persistOverride = (next: string) => {
    setSaving(true);
    setSaveError(null);
    invoke<Record<string, unknown>>("get_settings")
      .then((s) =>
        invoke("save_settings", {
          settings: { ...s, polishPromptOverride: next.trim() ? next : null },
        }),
      )
      .then(() => setOverrideText(next))
      .catch((e) => setSaveError(errorMessage(e)))
      .finally(() => setSaving(false));
  };

  const resetOverride = () => {
    persistOverride("");
  };

  const run = async () => {
    if (!sampleInput.trim() || running) return;
    setRunning(true);
    setOutput("");
    setRunError(null);
    setTransformed(false);
    try {
      const result = await invoke<TestResult>("test_polish_prompt", {
        prompt: overrideText || null,
        input: sampleInput,
      });
      if (result.transformed) {
        setOutput(result.text);
        setTransformed(true);
      } else {
        setRunError(result.skipMessage ?? "The model returned your words unchanged.");
      }
    } catch (e) {
      setRunError(`Could not run this polish: ${errorMessage(e)}`);
    }
    setRunning(false);
  };

  if (!info) {
    return (
      <div style={{ fontSize: 13, color: "var(--text-tertiary, var(--text-secondary))" }}>
        Loading prompt info…
      </div>
    );
  }

  const delta = output && transformed ? measureWords(sampleInput, output) : null;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {/* The prompt: editable for generic-path models, read-only + note for
          fixed-contract models. */}
      <div>
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
          Polish prompt
          <span style={{ fontWeight: 500, color: "var(--text-secondary)" }}> for {info.modelId}</span>
        </div>
        {info.note && (
          <div
            role="status"
            style={{
              marginBottom: 8,
              padding: "10px 12px",
              borderRadius: 8,
              background: "var(--warning-soft)",
              border: "1px solid color-mix(in srgb, var(--warning) 30%, transparent)",
              fontSize: 12,
              lineHeight: 1.5,
              color: "var(--text-secondary)",
            }}
          >
            {info.note}
          </div>
        )}
        {info.editable ? (
          <>
            <textarea
              rows={8}
              aria-label="Polish prompt override"
              value={overrideText}
              onChange={(e) => setOverrideText(e.target.value)}
              placeholder={info.defaultPrompt}
              style={{
                width: "100%",
                fontSize: 12,
                lineHeight: 1.5,
                padding: "10px 12px",
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--bg)",
                resize: "vertical",
                fontFamily: "var(--font-mono, ui-monospace, monospace)",
              }}
            />
            <div style={{ fontSize: 11, color: "var(--text-tertiary, var(--text-secondary))", margin: "4px 0 6px" }}>
              Leave empty to use the model's default prompt. Your override is appended to the
              generic rules and used on every dictation.
            </div>
            <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
              <button onClick={resetOverride} disabled={saving} style={{ fontSize: 13 }}>
                Reset to default
              </button>
              <button className="primary" onClick={() => persistOverride(overrideText)} disabled={saving} style={{ fontSize: 13 }}>
                {saving ? "Saving…" : "Save"}
              </button>
            </div>
            {saveError && (
              <div style={{ fontSize: 12, color: "var(--warning)", marginTop: 6 }}>{saveError}</div>
            )}
          </>
        ) : (
          <pre
            style={{
              margin: 0,
              padding: "12px 14px",
              background: "var(--bg)",
              borderRadius: 8,
              border: "1px solid var(--border)",
              fontSize: 12,
              lineHeight: 1.5,
              whiteSpace: "pre-wrap",
              maxHeight: 200,
              overflowY: "auto",
              color: "var(--text-secondary)",
            }}
          >
            {info.defaultPrompt}
          </pre>
        )}
      </div>

      {/* The playground: sample in, model output out, with a word-count delta. */}
      <div>
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>Playground</div>
        <textarea
          rows={5}
          aria-label="Playground sample text"
          value={sampleInput}
          onChange={(e) => setSampleInput(e.target.value)}
          style={{
            width: "100%",
            fontSize: 13,
            lineHeight: 1.55,
            padding: "10px 12px",
            borderRadius: 8,
            border: "1px solid var(--border)",
            background: "var(--bg)",
            resize: "vertical",
          }}
        />
        <button
          className="primary"
          onClick={run}
          disabled={running || !sampleInput.trim()}
          style={{ width: "100%", marginTop: 8, padding: "8px 0", fontSize: 13, fontWeight: 600 }}
        >
          {running ? "Running…" : "Run with current prompt"}
        </button>
        <div
          aria-live="polite"
          style={{
            marginTop: 8,
            minHeight: 96,
            padding: "10px 12px",
            borderRadius: 8,
            border: output && transformed
              ? "1px solid var(--accent, #3b82f6)"
              : runError
                ? "1px solid color-mix(in srgb, var(--warning) 40%, transparent)"
                : "1px solid var(--border)",
            background: output && transformed ? "var(--accent-soft, rgba(59,130,246,0.08))" : "var(--bg)",
            fontSize: 13,
            lineHeight: 1.55,
            whiteSpace: "pre-wrap",
          }}
        >
          {output && transformed ? (
            output
          ) : runError ? (
            <div style={{ whiteSpace: "normal" }}>
              <div style={{ fontWeight: 700, color: "var(--warning)" }}>No polished text</div>
              <div style={{ marginTop: 4, color: "var(--text-secondary)" }}>{runError}</div>
            </div>
          ) : (
            <span style={{ color: "var(--text-tertiary)" }}>
              {running ? "Running…" : "Paste sample text above and press Run to see the model's output."}
            </span>
          )}
        </div>
        {delta && (
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 6 }}>
            {delta.words} words - {delta.label}
          </div>
        )}
      </div>
    </div>
  );
}
