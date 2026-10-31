import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../components/HotkeyRecorder";
import {
  DownloadProgressBar,
  type DownloadProgress,
} from "../components/DownloadProgress";
import { useTauriEvent } from "../lib/useTauriEvent";
import { errorMessage } from "../lib/errors";
import { ShieldIcon } from "../lib/PrivacyBadge";
import { Icon } from "../components/Icon";
import OnboardingPermissionsStep from "./OnboardingPermissionsStep";
import TryHotkeyCheck from "../components/TryHotkeyCheck";

interface SpeechModelStatus {
  id: string;
  name: string;
  engine: string;
  sizeMb: number;
  description: string;
  recommended: boolean;
  englishOnly: boolean;
  downloaded: boolean;
  selected: boolean;
}

interface LlmModelStatus {
  id: string;
  name: string;
  sizeMb: number;
  description: string;
  downloaded: boolean;
  selected: boolean;
  recommended: boolean;
  polishCapable: boolean;
}

// Result of `get_speech_model_ready_state` (no args). The backend reports
// whether the selected model is loaded and usable, plus a human-ready line.
interface SpeechModelReady {
  id: string;
  downloaded: boolean;
  ready: boolean;
  message: string;
}

interface Settings {
  hotkey: string;
  selectedSpeechModel: string;
  hasCompletedOnboarding: boolean;
  [key: string]: unknown;
}

const STEPS = ["Privacy", "Permissions", "Dictation", "Polish", "Hotkey", "Done"] as const;

function sizeLabel(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

// Step index of the standalone "Try it" check used by repair mode.
const TRY_STEP = 6;

export default function OnboardingScreen({
  onCompleted,
  repair = false,
}: {
  onCompleted: () => void;
  /** Onboarding was finished earlier but a permission is missing: show only
   *  the permission step and the hotkey check, then return to the app. */
  repair?: boolean;
}) {
  const [step, setStep] = useState(repair ? 1 : 0);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [speechModels, setSpeechModels] = useState<SpeechModelStatus[]>([]);
  const [llmModels, setLlmModels] = useState<LlmModelStatus[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [readyMsg, setReadyMsg] = useState<string | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [showPolishAdvanced, setShowPolishAdvanced] = useState(false);
  const [polishChoice, setPolishChoice] = useState<"local" | "cloud" | "skip" | null>(null);

  // Ask the backend whether the selected speech model is loaded and usable.
  const checkReady = useCallback(() => {
    invoke<SpeechModelReady>("get_speech_model_ready_state")
      .then((r) => {
        if (r.ready) setReadyMsg(r.message);
      })
      .catch(() => {});
  }, []);

  useTauriEvent<DownloadProgress>("model-download-progress", (e) => {
    const p = e.payload;
    if (p.status === "done") {
      setProgress(null);
      checkReady();
      return;
    }
    setProgress(p);
  });

  useEffect(() => {
    invoke<Settings>("get_settings")
      .then(setSettings)
      .catch((e) => console.error("get_settings failed:", e));
    // The first-run view: the backend shortlists to the few rows worth choosing
    // between on this machine and language, instead of the whole catalog.
    // Settings asks for the full list.
    invoke<SpeechModelStatus[]>("list_speech_models", { shortlist: true })
      .then(setSpeechModels)
      .catch((e) => console.error("list_speech_models failed:", e));
    // Fetch LLM models for the polish step. The backend shortlists to the few
    // rows worth choosing on this machine (Mac GPU gets the accelerated set,
    // Windows/CPU gets the CPU set), same as the speech model step above.
    // Failures are silent: the step degrades to "skip" when no models are available.
    invoke<LlmModelStatus[]>("list_models", { shortlist: true })
      .then((ms) => setLlmModels(ms.filter((m) => m.polishCapable)))
      .catch(() => {});
  }, []);

  const finish = async () => {
    setError(null);
    if (!settings) {
      try {
        const s = await invoke<Settings>("get_settings");
        setSettings(s);
        await invoke("save_settings", { settings: { ...s, hasCompletedOnboarding: true } });
        onCompleted();
      } catch (e) {
        console.error(e);
        setError(`Couldn't finish setup: ${e}`);
      }
      return;
    }
    setBusy(true);
    try {
      await invoke("save_settings", {
        settings: { ...settings, hasCompletedOnboarding: true },
      });
      onCompleted();
    } catch (e) {
      console.error(e);
      setBusy(false);
      setError(`Couldn't finish setup: ${e}`);
    }
  };

  const downloadModel = async (id: string) => {
    setDownloading(id);
    setError(null);
    try {
      await invoke("download_speech_model", { id });
      await invoke("select_speech_model", { id }).catch(() => {});
      await invoke<SpeechModelStatus[]>("list_speech_models", { shortlist: true }).then(setSpeechModels);
      checkReady();
    } catch (e) {
      console.error("download failed", e);
      setError(errorMessage(e));
    } finally {
      setDownloading(null);
      setProgress(null);
    }
  };

  const refreshLlmModels = async () => {
    const ms = await invoke<LlmModelStatus[]>("list_models", { shortlist: true });
    setLlmModels(ms.filter((m) => m.polishCapable));
  };

  const downloadLlmModel = async (id: string) => {
    setDownloading(id);
    setError(null);
    try {
      await invoke("download_model", { id });
      await invoke("select_model", { id }).catch(() => {});
      await refreshLlmModels();
    } catch (e) {
      console.error("llm download failed", e);
      setError(errorMessage(e));
    } finally {
      setDownloading(null);
      setProgress(null);
    }
  };

  const selectSpeechModel = async (id: string) => {
    await invoke("select_speech_model", { id }).catch(console.error);
    await invoke<SpeechModelStatus[]>("list_speech_models", { shortlist: true }).then(setSpeechModels);
  };

  const selectLlmModel = async (id: string) => {
    await invoke("select_model", { id }).catch(console.error);
    await refreshLlmModels().catch(console.error);
  };

  const selectedModel = speechModels.find((m) => m.selected);
  const modelReady = selectedModel?.downloaded ?? false;
  const recommendedModel = speechModels.find((m) => m.recommended);
  const recommendedLlm = llmModels.find((m) => m.selected) ?? llmModels[0];
  const llmReady = recommendedLlm?.downloaded ?? false;
  // The local card is the primary path, so it is open as soon as the backend has
  // a model to offer. While `polishChoice` was still null the card looked
  // selected but carried no Download button at all: the button that starts the
  // only action on this step was hidden behind a click on the card itself.
  const polishPicked = polishChoice ?? (llmModels.length > 0 ? "local" : null);
  // Several catalog rows carry `recommended: true`. One badge per step, on the
  // row the list actually leads with, wherever that row happens to sit.
  const polishLead = llmModels.find((m) => m.recommended)?.id;
  const speechLead = speechModels.find((m) => m.recommended)?.id;
  const others = llmModels.filter((m) => m.id !== recommendedLlm?.id);
  const otherSpeech = speechModels.filter((m) => m.id !== recommendedModel?.id);

  return (
    <div
      style={{
        minHeight: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 32,
        background: "var(--sky-gradient)",
        overflowY: "auto",
      }}
    >
      <div
        style={{
          width: "100%",
          maxWidth: 640,
          background: "var(--surface)",
          border: "1px solid color-mix(in srgb, var(--border) 60%, transparent)",
          borderRadius: 32,
          padding: "34px 40px 32px",
          boxShadow: "var(--shadow-3)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 26 }}>
          <img
            src="/teletype-app-icon-white.png"
            alt=""
            width={30}
            height={30}
            style={{ borderRadius: 8, boxShadow: "var(--shadow-1)" }}
          />
          <span
            style={{
              fontFamily: "var(--font-display)",
              fontSize: 18,
              fontWeight: 600,
              letterSpacing: "-0.04em",
              color: "var(--text)",
            }}
          >
            Teletype
          </span>
          <span
            style={{
              marginLeft: "auto",
              display: "inline-flex",
              alignItems: "center",
              gap: 5,
              padding: "4px 10px",
              borderRadius: "var(--radius-pill)",
              background: "var(--surface-2)",
              border: "1px solid var(--border-subtle)",
              fontSize: 10.5,
              fontWeight: 600,
              letterSpacing: "0.08em",
              textTransform: "uppercase",
              color: "var(--text-tertiary)",
            }}
          >
            On-device
          </span>
        </div>
        <h1
          style={{
            fontFamily: "var(--font-display)",
            fontSize: 34,
            fontWeight: 600,
            letterSpacing: "-0.04em",
            lineHeight: 1.06,
            marginBottom: 8,
          }}
        >
          {repair ? "Teletype needs access" : "Welcome to Teletype"}
        </h1>
        <p
          style={{
            color: "var(--text-secondary)",
            fontSize: 15,
            lineHeight: 1.49,
            letterSpacing: "-0.01em",
            marginBottom: 28,
            maxWidth: 480,
          }}
        >
          {repair
            ? "A permission is off, so dictation cannot work yet. Turn it on below and this page updates by itself."
            : "A few quick steps to get dictation working. Everything runs locally."}
        </p>

        {/* Progress: a quiet purple ribbon, the one place the accent carries
            across the whole first-run flow. */}
        <div style={{ display: repair ? "none" : "flex", gap: 6, marginBottom: 28 }}>
          {STEPS.map((label, i) => (
            <div key={label} style={{ flex: 1, minWidth: 0 }}>
              <div
                style={{
                  height: 3,
                  borderRadius: 100,
                  background: i <= step ? "var(--accent)" : "var(--border)",
                  transition: "background var(--dur) var(--ease)",
                }}
              />
              <div
                style={{
                  marginTop: 7,
                  fontSize: 10,
                  fontWeight: i === step ? 600 : 500,
                  letterSpacing: "0.02em",
                  color: i === step ? "var(--text)" : "var(--text-tertiary)",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {label}
              </div>
            </div>
          ))}
        </div>

        {/* Step 0: Privacy */}
        {step === 0 && (
          <div>
            <div
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                fontSize: 11,
                fontWeight: 600,
                letterSpacing: 0.4,
                color: "var(--success)",
                background: "var(--success-soft)",
                border: "1px solid color-mix(in srgb, var(--success) 25%, transparent)",
                borderRadius: 999,
                padding: "3px 10px",
                marginBottom: 14,
              }}
            >
              <ShieldIcon size={13} color="var(--success)" />
              On-device
            </div>
            <h3 style={{ fontFamily: "var(--font-display)", fontSize: 24, fontWeight: 600, letterSpacing: "-0.04em", marginBottom: 8 }}>
              Your voice never leaves this device.
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, lineHeight: 1.55, marginBottom: 20 }}>
              Teletype hears you, transcribes you, and polishes your words
              entirely on this device. Your audio, transcripts, and history are
              never uploaded. If you later connect your own cloud model for
              polishing, only the text you dictated is sent to that provider,
              under your key.
            </p>
            <div style={{ display: "flex", justifyContent: "flex-end" }}>
              <button className="primary" onClick={() => setStep(1)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 1: Permissions (live status, see OnboardingPermissionsStep) */}
        {step === 1 && (
          <OnboardingPermissionsStep
            onBack={repair ? undefined : () => setStep(0)}
            onContinue={() => setStep(repair ? TRY_STEP : 2)}
          />
        )}

        {/* Repair-mode step: confirm the hotkey works now that access is back. */}
        {step === TRY_STEP && (
          <div>
            <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>You're ready</h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
              Access is granted. No restart is needed.
            </p>
            <TryHotkeyCheck hotkey={settings?.hotkey || ""} />
            <div style={{ display: "flex", justifyContent: "flex-end" }}>
              <button className="primary" onClick={onCompleted}>
                Continue to Teletype
              </button>
            </div>
          </div>
        )}

        {/* Step 2: Dictation model */}
        {step === 2 && (
          <div>
            <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>
              Set up your dictation model
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
              One download and you can start speaking. Everything runs on this
              device.
            </p>

            {recommendedModel && (
              <ModelStepCard
                model={recommendedModel}
                downloading={downloading}
                progress={progress}
                error={error}
                readyMsg={readyMsg}
                onSetup={() => downloadModel(recommendedModel.id)}
              />
            )}

            {otherSpeech.length > 0 && (
              <ModelStepAdvanced
                models={otherSpeech}
                leadId={speechLead}
                downloading={downloading}
                progress={progress}
                error={error}
                showAdvanced={showAdvanced}
                onToggle={() => setShowAdvanced((v) => !v)}
                onDownload={downloadModel}
                onSelect={selectSpeechModel}
              />
            )}

            <div style={{ display: "flex", justifyContent: "space-between", marginTop: 20 }}>
              <button onClick={() => setStep(1)}>Back</button>
              <button className="primary" disabled={!modelReady} onClick={() => setStep(3)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 3: Polish model (optional) */}
        {step === 3 && (
          <div>
            <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>
              Polish your writing (optional)
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
              Teletype can clean up filler words, fix punctuation, and tighten
              your sentences. You can set this up now or add it later in
              Settings.
            </p>

            {/* Option 1: Local model. Named, because "Local model" alone does
                not say what you are downloading, and the pick is the backend's
                for this platform and language, not a fixed constant. */}
            <div
              style={{
                padding: 14,
                background: "var(--bg)",
                borderRadius: "var(--radius-sm)",
                border: `1px solid ${polishPicked === "local" ? "var(--accent)" : "var(--border)"}`,
                marginBottom: 10,
                cursor: "pointer",
              }}
              onClick={() => setPolishChoice("local")}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
                <span style={{ fontSize: 13, fontWeight: 600 }}>
                  {recommendedLlm?.name ?? "Local model (offline)"}
                </span>
                {recommendedLlm?.id === polishLead && (
                  <span
                    style={{
                      fontSize: 10,
                      color: "var(--accent)",
                      border: "1px solid var(--accent)",
                      borderRadius: 4,
                      padding: "1px 6px",
                      fontWeight: 600,
                    }}
                  >
                    Recommended
                  </span>
                )}
                {recommendedLlm && (
                  <span
                    style={{
                      fontSize: 10,
                      color: "var(--text-secondary)",
                      border: "1px solid var(--border)",
                      borderRadius: 4,
                      padding: "1px 6px",
                      fontWeight: 600,
                    }}
                  >
                    {sizeLabel(recommendedLlm.sizeMb)}
                  </span>
                )}
              </div>
              {recommendedLlm?.description && (
                <div style={{ fontSize: 12, marginBottom: 4 }}>{recommendedLlm.description}</div>
              )}
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
                Runs 100% on this device. No internet needed after download.
              </div>
              {polishPicked === "local" && recommendedLlm && (
                <>
                  {recommendedLlm.downloaded ? (
                    <div style={{ display: "inline-flex", alignItems: "center", gap: 6, color: "var(--success)", fontSize: 13, fontWeight: 600 }}>
                      <Icon name="check" size={16} color="var(--success)" />
                      Ready
                    </div>
                  ) : (
                    <button
                      className="primary"
                      disabled={downloading !== null}
                      onClick={(e) => {
                        e.stopPropagation();
                        downloadLlmModel(recommendedLlm.id);
                      }}
                    >
                      {downloading === recommendedLlm.id
                        ? progress
                          ? `Downloading… ${Math.round(progress.percent)}%`
                          : "Downloading…"
                        : `Download (${sizeLabel(recommendedLlm.sizeMb)})`}
                    </button>
                  )}
                  {downloading === recommendedLlm.id && progress && (
                    <DownloadProgressBar progress={progress} />
                  )}
                  {error && downloading === recommendedLlm.id && (
                    <div style={{ fontSize: 11, color: "var(--danger)", marginTop: 6 }}>{error}</div>
                  )}
                </>
              )}
            </div>

            {/* The card above already is the recommended model, so listing it
                again here put the same name on screen twice, with a second
                Select control next to the Download the user was about to press. */}
            {others.length > 0 && (
              <ModelStepAdvanced
                models={others}
                leadId={polishLead}
                downloading={downloading}
                progress={progress}
                error={error}
                showAdvanced={showPolishAdvanced}
                onToggle={() => setShowPolishAdvanced((v) => !v)}
                onDownload={downloadLlmModel}
                onSelect={selectLlmModel}
              />
            )}

            {/* Option 2: Cloud model */}
            <div
              style={{
                padding: 14,
                background: "var(--bg)",
                borderRadius: "var(--radius-sm)",
                border: `1px solid ${polishPicked === "cloud" ? "var(--accent)" : "var(--border)"}`,
                marginBottom: 10,
                cursor: "pointer",
              }}
              onClick={() => setPolishChoice("cloud")}
            >
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>
                Cloud model (bring your own key)
              </div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                Connect your own OpenAI, Anthropic, or other provider. Only
                dictated text is sent, under your key. You can configure this
                in Settings later.
              </div>
            </div>

            {/* Option 3: Skip */}
            <div
              style={{
                padding: 14,
                background: "var(--bg)",
                borderRadius: "var(--radius-sm)",
                border: `1px solid ${polishPicked === "skip" ? "var(--accent)" : "var(--border)"}`,
                marginBottom: 10,
                cursor: "pointer",
              }}
              onClick={() => setPolishChoice("skip")}
            >
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>
                Skip for now
              </div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                Dictation works without polishing. You can add a polish model
                later in Settings → Models.
              </div>
            </div>

            {polishPicked === "cloud" && (
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
                You'll be able to configure your cloud provider in Settings →
                Models after setup.
              </div>
            )}

            <div style={{ display: "flex", justifyContent: "space-between", marginTop: 20 }}>
              <button onClick={() => setStep(2)}>Back</button>
              <button
                className="primary"
                onClick={() => setStep(4)}
              >
                {/* The local card is open by default, so Continue has to say what
                    pressing it means when the download never happened. A disabled
                    button here would trap the step: polishing is optional, and the
                    only way past it would be to notice that "Skip for now" exists. */}
                {polishPicked === "local" && !llmReady && llmModels.length > 0
                  ? "Continue without polishing"
                  : "Continue"}
              </button>
            </div>
          </div>
        )}

        {/* Step 4: Hotkey */}
        {step === 4 && (
          <div>
            <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>
              Your dictation hotkey
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
              Hold this key anywhere to dictate. Release to type. Click
              "Change" and press a new combination to rebind.
            </p>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "14px 18px",
                background: "var(--bg)",
                borderRadius: "var(--radius-sm)",
                border: "1px solid var(--border)",
                marginBottom: 20,
              }}
            >
              <HotkeyRecorder
                value={settings?.hotkey || ""}
                onSave={async (hotkey) => {
                  if (!settings) return;
                  await invoke("save_settings", {
                    settings: { ...settings, hotkey },
                  }).catch(console.error);
                  setSettings({ ...settings, hotkey });
                }}
              />
              <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>
                Hold to talk, release to type
              </span>
            </div>
            <TryHotkeyCheck hotkey={settings?.hotkey || ""} />
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <button onClick={() => setStep(3)}>Back</button>
              <button className="primary" onClick={() => setStep(5)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 5: Done */}
        {step === 5 && (
          <div style={{ textAlign: "center", padding: "8px 0 4px" }}>
            <div
              style={{
                width: 56,
                height: 56,
                borderRadius: "50%",
                background: "var(--accent-soft)",
                display: "grid",
                placeItems: "center",
                margin: "0 auto 16px",
              }}
            >
              <Icon name="check" size={26} color="var(--accent)" />
            </div>
            <h3 style={{ fontFamily: "var(--font-display)", fontSize: 30, fontWeight: 600, letterSpacing: "-0.04em", lineHeight: 1.05, marginBottom: 10 }}>
              You’re all set
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 15, lineHeight: 1.49, margin: "0 auto 24px", maxWidth: 380 }}>
              Hold your hotkey and start talking. Transcribed text is typed into
              whatever app is focused.
            </p>
            <button className="primary" disabled={busy} onClick={finish} style={{ padding: "10px 26px", fontSize: 15 }}>
              {busy ? "Saving…" : "Start using Teletype"}
            </button>
            {error && (
              <p style={{ color: "var(--danger)", fontSize: "var(--text-sm)", marginTop: 12 }}>{error}</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// The one-click path for the recommended model: a single primary button whose
// label is driven by download/selection state.
function ModelStepCard({
  model,
  downloading,
  progress,
  error,
  readyMsg,
  onSetup,
}: {
  model: SpeechModelStatus;
  downloading: string | null;
  progress: DownloadProgress | null;
  error: string | null;
  readyMsg: string | null;
  onSetup: () => void;
}) {
  const isDownloading = downloading === model.id;
  const isReady = model.downloaded && model.selected;

  return (
    <div
      style={{
        padding: 18,
        background: "var(--bg)",
        borderRadius: "var(--radius-sm)",
        border: `1px solid ${model.selected ? "var(--accent)" : "var(--border)"}`,
        marginBottom: 4,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
        <span style={{ fontSize: 15, fontWeight: 600 }}>{model.name}</span>
        <span
          style={{
            fontSize: 10,
            color: "var(--accent)",
            border: "1px solid var(--accent)",
            borderRadius: 4,
            padding: "1px 6px",
            fontWeight: 600,
          }}
        >
          Recommended
        </span>
      </div>
      <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 14 }}>
        {sizeLabel(model.sizeMb)} · Runs 100% on this device.
      </div>

      {isReady ? (
        <div
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            color: "var(--success)",
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          <Icon name="check" size={16} color="var(--success)" />
          Ready
        </div>
      ) : (
        <button
          className="primary"
          disabled={downloading !== null}
          onClick={onSetup}
        >
          {isDownloading
            ? progress
              ? `Downloading… ${Math.round(progress.percent)}%`
              : "Downloading…"
            : model.downloaded
              ? "Set up"
              : `Download & set up (${sizeLabel(model.sizeMb)})`}
        </button>
      )}

      {isDownloading && progress && (
        <DownloadProgressBar progress={progress} />
      )}

      {error && downloading === model.id && (
        <div style={{ fontSize: 11, color: "var(--danger)", marginTop: 6 }}>{error}</div>
      )}

      {readyMsg && (
        <div
          style={{
            marginTop: 12,
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: 13,
            color: "var(--success)",
          }}
        >
          <Icon name="check" size={15} color="var(--success)" />
          {readyMsg}
        </div>
      )}
    </div>
  );
}

// The "Choose a different model" disclosure: the full per-model list.
// A model row the user can switch to. Both `SpeechModelStatus` and
// `LlmModelStatus` satisfy this, so the dictation and polish steps share one
// "choose a different model" list.
interface ModelOption {
  id: string;
  name: string;
  sizeMb: number;
  downloaded: boolean;
  selected: boolean;
  recommended: boolean;
}

function ModelStepAdvanced({
  models,
  leadId,
  downloading,
  progress,
  error,
  showAdvanced,
  onToggle,
  onDownload,
  onSelect,
}: {
  models: ModelOption[];
  /** The one row allowed to say "Recommended". Several catalog rows carry the
      flag, and the card this list sits under already wears it, so per-row
      badges put two recommendations on one step. */
  leadId?: string;
  downloading: string | null;
  progress: DownloadProgress | null;
  error: string | null;
  showAdvanced: boolean;
  onToggle: () => void;
  onDownload: (id: string) => void;
  onSelect: (id: string) => void;
}) {
  return (
    <div style={{ marginTop: 12, marginBottom: 10 }}>
      <button
        type="button"
        aria-expanded={showAdvanced}
        onClick={onToggle}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 5,
          background: "none",
          border: "none",
          padding: 0,
          color: "var(--accent)",
          fontSize: 12,
          fontWeight: 600,
          cursor: "pointer",
          textDecoration: "underline",
          textUnderlineOffset: "2px",
        }}
      >
        {showAdvanced
          ? "Hide other models"
          : `Show ${models.length} other model${models.length === 1 ? "" : "s"}`}
        <span aria-hidden="true" style={{ fontSize: 9, textDecoration: "none" }}>
          {showAdvanced ? "▲" : "▼"}
        </span>
      </button>
      {showAdvanced && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 10 }}>
          {models.map((m) => (
            <div
              key={m.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "10px 14px",
                background: "var(--bg)",
                borderRadius: "var(--radius-sm)",
                border: `1px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
              }}
            >
              <div style={{ flex: 1 }}>
                <div style={{ fontWeight: 500, fontSize: 13 }}>
                  {m.name}
                  {m.id === leadId && (
                    <span
                      style={{
                        marginLeft: 8,
                        fontSize: 10,
                        color: "var(--accent)",
                        border: "1px solid var(--accent)",
                        borderRadius: 4,
                        padding: "0 5px",
                      }}
                    >
                      Recommended
                    </span>
                  )}
                </div>
                <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>
                  {sizeLabel(m.sizeMb)}
                  {m.downloaded && " · installed"}
                </div>
                {progress && progress.id === m.id && (
                  <DownloadProgressBar progress={progress} />
                )}
                {error && downloading === m.id && (
                  <div style={{ fontSize: 11, color: "var(--danger)", marginTop: 4 }}>
                    {error}
                  </div>
                )}
              </div>
              {m.downloaded ? (
                <button
                  className={m.selected ? "primary" : ""}
                  onClick={() => onSelect(m.id)}
                >
                  {m.selected ? "Selected" : "Select"}
                </button>
              ) : (
                <button
                  disabled={downloading !== null}
                  onClick={() => onDownload(m.id)}
                >
                  {downloading === m.id
                    ? progress
                      ? `${Math.round(progress.percent)}%`
                      : "Downloading…"
                      : `Download (${sizeLabel(m.sizeMb)})`}
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
