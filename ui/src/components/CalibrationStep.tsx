import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/errors";
import {
  parseProfileKey,
  profileLabel,
  profilePromise,
  profileBudgetLabel,
} from "../lib/profileLabels";

interface ProfileOption {
  key: string;
  label: string;
  budgetPercent: number;
}

interface CalibrationOptions {
  modelId: string;
  capability: string;
  supported: ProfileOption[];
  unsupported: ProfileOption[];
  modelLoaded: boolean;
}

interface SampleResult {
  sampleId: string;
  profile: string;
  before: string;
  after: string;
  transformed: boolean;
  error: { code: string; message: string } | null;
}

interface Prompt {
  id: string;
  label: string;
  hint: string;
  sample: string;
}

/** The three prompts of §8.2. Mirrored from the desktop command so onboarding
 *  can prefill editable examples; the backend remains the source of truth. */
const PROMPTS: readonly Prompt[] = [
  {
    id: "email",
    label: "An email",
    hint: "A quick update to a teammate or client",
    sample:
      "hey i wanted to check in on the q3 numbers. i'll send the full breakdown tomorrow but the short version is we're on track.",
  },
  {
    id: "slack",
    label: "A Slack reply",
    hint: "A short message to your team",
    sample:
      "yeah that works for me, can we push it to thursday though? i've got a conflict wednesday afternoon.",
  },
  {
    id: "note",
    label: "A note",
    hint: "A thought you want to remember",
    sample:
      "remember to ask priya about the vendor contract and also follow up with dan about the design review next week.",
  },
];

function initialTexts(): Record<string, string> {
  return Object.fromEntries(PROMPTS.map((p) => [p.id, p.sample]));
}

const ALL_PROFILES: ProfileOption[] = [
  { key: "clean", label: "Clean", budgetPercent: 15 },
  { key: "natural", label: "Natural", budgetPercent: 30 },
  { key: "structured", label: "Structured", budgetPercent: 45 },
];

/** Fallback used only if the options command fails; the step still renders and
 *  stays skippable rather than trapping the user on a dead screen. */
const FALLBACK_OPTIONS: CalibrationOptions = {
  modelId: "",
  capability: "unknown",
  supported: ALL_PROFILES,
  unsupported: [],
  modelLoaded: false,
};

export default function CalibrationStep({
  onComplete,
  onBack,
}: {
  /** Receives the fresh settings returned by `record_calibration`. */
  onComplete: (settings: unknown) => void;
  onBack?: () => void;
}) {
  const [options, setOptions] = useState<CalibrationOptions>(FALLBACK_OPTIONS);
  const [texts, setTexts] = useState<Record<string, string>>(initialTexts);
  const [results, setResults] = useState<SampleResult[]>([]);
  const [choice, setChoice] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const generate = useCallback(async (current: Record<string, string>) => {
    setGenerating(true);
    setError(null);
    try {
      const samples = PROMPTS.map((p) => ({ id: p.id, text: current[p.id] ?? "" })).filter(
        (s) => s.text.trim().length > 0,
      );
      if (samples.length === 0) {
        setResults([]);
        return;
      }
      const rows = await invoke<SampleResult[]>("generate_calibration_samples", { samples });
      setResults(rows);
    } catch (e) {
      // A generation failure must show deterministic text + a clear message,
      // never an endless spinner. The error is rendered beside each sample.
      setResults([]);
      setError(errorMessage(e));
    } finally {
      setGenerating(false);
    }
  }, []);

  useEffect(() => {
    let alive = true;
    invoke<CalibrationOptions>("calibration_profile_options")
      .then((o) => {
        if (alive) setOptions(o);
      })
      .catch(() => {
        /* keep the fallback; the step stays skippable */
      });
    generate(initialTexts());
    return () => {
      alive = false;
    };
  }, [generate]);

  const finish = async (skipped: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const updated = await invoke<unknown>("record_calibration", {
        outcome: { skipped, profile: skipped ? null : (choice ?? "natural") },
      });
      onComplete(updated);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  const resultsFor = (sampleId: string, profileKey: string) =>
    results.filter((r) => r.sampleId === sampleId && r.profile === profileKey);
  const anyModelError = results.some((r) => r.error !== null) || !options.modelLoaded;

  return (
    <div data-testid="calibration-step">
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>Let&apos;s tune Polish to you</h3>
        <span
          style={{
            fontSize: 10,
            fontWeight: 600,
            letterSpacing: "0.06em",
            textTransform: "uppercase",
            color: "var(--accent)",
            border: "1px solid var(--accent)",
            borderRadius: 999,
            padding: "2px 8px",
          }}
        >
          60 seconds
        </span>
      </div>
      <p style={{ color: "var(--text-secondary)", fontSize: 13, lineHeight: 1.5, marginBottom: 16 }}>
        Type three short takes, or dictate them with your hotkey — no new permission needed. See
        how each profile handles your own words, then pick the one that sounds like you. Teletype
        never invents words and you can always revert.
      </p>

      <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 16 }}>
        {PROMPTS.map((p) => (
          <label key={p.id} style={{ display: "block", fontSize: 12 }}>
            <span style={{ display: "block", fontWeight: 600, marginBottom: 2 }}>
              {p.label} <span style={{ color: "var(--text-tertiary)", fontWeight: 400 }}>· {p.hint}</span>
            </span>
            <textarea
              aria-label={`${p.label} sample`}
              value={texts[p.id] ?? ""}
              onChange={(e) => setTexts((prev) => ({ ...prev, [p.id]: e.target.value }))}
              rows={2}
              style={{
                width: "100%",
                boxSizing: "border-box",
                padding: 8,
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--bg)",
                color: "var(--text)",
                font: "inherit",
                fontSize: 12,
                resize: "vertical",
              }}
            />
          </label>
        ))}
        <button type="button" onClick={() => generate(texts)} disabled={generating}>
          {generating ? "Generating…" : "Refresh examples"}
        </button>
      </div>

      {!options.modelLoaded && (
        <div
          role="status"
          style={{
            marginBottom: 14,
            padding: "10px 12px",
            borderRadius: 8,
            border: "1px solid var(--border)",
            background: "var(--surface-2)",
            fontSize: 12,
            color: "var(--text-secondary)",
            lineHeight: 1.5,
          }}
        >
          No polish model is loaded, so there are no examples to compare yet. Your text is shown
          exactly as typed. You can skip — dictation still works, and you can calibrate later in
          Settings.
        </div>
      )}

      {options.unsupported.length > 0 && (
        <div role="status" style={{ marginBottom: 14, fontSize: 12, color: "var(--text-secondary)" }}>
          This model can&apos;t do {options.unsupported.map((o) => o.label).join(" or ")} — those
          were left out of the comparison. A full-Polish model unlocks them in Settings → Models.
        </div>
      )}

      {error && (
        <div role="alert" style={{ marginBottom: 14, fontSize: 12, color: "var(--danger)" }}>
          {error}
        </div>
      )}

      {options.supported.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 18 }}>
          {options.supported.map((o) => {
            const key = parseProfileKey(o.key);
            const rows = PROMPTS.map((p) => ({ prompt: p, cells: resultsFor(p.id, o.key) }));
            return (
              <div
                key={o.key}
                style={{
                  padding: 12,
                  borderRadius: 10,
                  border: `1px solid ${choice === o.key ? "var(--accent)" : "var(--border)"}`,
                  background: "var(--bg)",
                }}
              >
                <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 2 }}>
                  {profileLabel(key)} <span style={{ fontWeight: 400, color: "var(--text-secondary)" }}>— “{profilePromise(key)}”</span>
                </div>
                <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginBottom: 8 }}>
                  {profileBudgetLabel(key)}
                </div>
                {rows.map(({ prompt, cells }) => (
                  <div key={prompt.id} style={{ marginBottom: 6, fontSize: 12 }}>
                    <strong>{prompt.label}:</strong>{" "}
                    {cells.length === 0 ? (
                      <span>—</span>
                    ) : (
                      cells.map((cell) => (
                        <div key={cell.profile} style={{ marginTop: 2, lineHeight: 1.5 }}>
                          <span style={{ color: "var(--text-tertiary)", textDecoration: "line-through" }}>
                            {cell.before}
                          </span>
                          {" → "}
                          <span>{cell.after}</span>
                          {cell.error && (
                            <span style={{ color: "var(--danger)", marginLeft: 6 }}>{cell.error.message}</span>
                          )}
                        </div>
                      ))
                    )}
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      )}

      <div role="radiogroup" aria-label="Which sounds like you" style={{ marginBottom: 18 }}>
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Which sounds like you?</div>
        {options.supported.map((o) => (
          <label key={o.key} style={{ display: "block", fontSize: 13, marginBottom: 6, cursor: "pointer" }}>
            <input
              type="radio"
              role="radio"
              name="calibration-profile"
              aria-label={o.label}
              checked={choice === o.key}
              onChange={() => setChoice(o.key)}
              style={{ marginRight: 8 }}
            />
            {o.label}
          </label>
        ))}
        <label style={{ display: "block", fontSize: 13, cursor: "pointer" }}>
          <input
            type="radio"
            role="radio"
            name="calibration-profile"
            aria-label="Not sure — use Natural"
            checked={choice === "natural"}
            onChange={() => setChoice("natural")}
            style={{ marginRight: 8 }}
          />
          Not sure — use Natural.
        </label>
      </div>

      {anyModelError && (
        <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginBottom: 10 }}>
          Examples are shown as deterministic text because the model could not run. This never
          blocks dictation.
        </div>
      )}

      <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
        {onBack ? <button onClick={onBack}>Back</button> : <span />}
        <div style={{ display: "flex", gap: 8 }}>
          <button disabled={busy} onClick={() => finish(true)}>
            Skip for now
          </button>
          <button
            className="primary"
            disabled={busy || choice === null}
            onClick={() => finish(false)}
          >
            {busy ? "Saving…" : "Use this profile"}
          </button>
        </div>
      </div>
    </div>
  );
}
