import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../components/HotkeyRecorder";
import WebviewHotkeyRecorder, { usesWebviewCapture } from "../components/WebviewHotkeyRecorder";
import {
  DownloadProgressBar,
  type DownloadProgress,
} from "../components/DownloadProgress";
import { useTauriEvent } from "../lib/useTauriEvent";
import { ShieldIcon } from "../lib/PrivacyBadge";
import { Icon } from "../components/Icon";
import { errorMessage } from "../lib/errors";

interface Permission {
  kind: string;
  state: "granted" | "denied" | "notDetermined" | "unsupported";
}

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

const STEPS = ["Privacy", "Permissions", "Dictation model", "Hotkey", "Done"] as const;

function sizeLabel(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

// Step header: title + one supporting line.
function StepHeader({ title, sub }: { title: string; sub?: string }) {
  return (
    <div style={{ marginBottom: 20 }}>
      <h3 style={{ fontSize: "var(--text-xl)", fontWeight: 650, letterSpacing: "var(--tracking-display)", lineHeight: 1.15, marginBottom: 6 }}>{title}</h3>
      {sub && (
        <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", lineHeight: 1.55, maxWidth: "65ch", margin: 0 }}>{sub}</p>
      )}
    </div>
  );
}

// One primary action per step: filled accent, 42px, weight 600.
function PrimaryButton({
  children,
  onClick,
  disabled,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      style={{
        height: 42,
        padding: "0 22px",
        borderRadius: 11,
        background: disabled ? "var(--surface-2)" : "var(--accent-strong)",
        color: disabled ? "var(--text-secondary)" : "var(--on-accent)",
        border: disabled ? "1px solid var(--border-subtle)" : "none",
        fontWeight: 600,
        fontSize: "var(--text-md)",
        fontFamily: "inherit",
        cursor: disabled ? "not-allowed" : "pointer",
        transition: "background var(--dur-fast) var(--ease)",
      }}
      onMouseEnter={(e) => {
        if (!disabled) e.currentTarget.style.background = "var(--accent-strong-hover)";
      }}
      onMouseLeave={(e) => {
        if (!disabled) e.currentTarget.style.background = "var(--accent-strong)";
      }}
    >
      {children}
    </button>
  );
}

// Ghost secondary for back/skip.
function GhostButton({ children, onClick }: { children: React.ReactNode; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        height: 42,
        padding: "0 16px",
        borderRadius: 11,
        background: "transparent",
        color: "var(--text-secondary)",
        border: "none",
        fontWeight: 500,
        fontSize: "var(--text-md)",
        fontFamily: "inherit",
        cursor: "pointer",
        transition: "background var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease)",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--surface-hover)";
        e.currentTarget.style.color = "var(--text)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
        e.currentTarget.style.color = "var(--text-secondary)";
      }}
    >
      {children}
    </button>
  );
}

// Slim step indicator: a line per step, accent for the current/completed
// run, hairline for the rest. The sequence itself is the meaning, so no
// numbered kickers.
function StepProgress({ step }: { step: number }) {
  return (
    <div style={{ display: "flex", gap: 6, marginBottom: 28 }} aria-hidden="true">
      {STEPS.map((label, i) => (
        <span
          key={label}
          style={{
            flex: 1,
            height: 3,
            borderRadius: "var(--radius-pill)",
            background: i <= step ? "var(--accent-strong)" : "var(--border)",
            transition: "background var(--dur) var(--ease)",
          }}
        />
      ))}
    </div>
  );
}

export default function OnboardingScreen({
  onCompleted,
}: {
  onCompleted: () => void;
}) {
  const [step, setStep] = useState(0);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [permissions, setPermissions] = useState<Permission[]>([]);
  const [speechModels, setSpeechModels] = useState<SpeechModelStatus[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [readyMsg, setReadyMsg] = useState<string | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);

  // Ask the backend whether the selected speech model is loaded and usable.
  // When it is, surface its ready line in the model card. Failures are silent:
  // the command may not exist yet (backend lands in parallel), and readiness is
  // only a confirmation, never a gate.
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

  const refreshPermissions = useCallback(() => {
    invoke<Permission[]>("get_permissions")
      .then(setPermissions)
      .catch((e) => console.error("get_permissions failed:", e));
  }, []);

  useEffect(() => {
    invoke<Settings>("get_settings")
      .then(setSettings)
      .catch((e) => console.error("get_settings failed:", e));
    refreshPermissions();
    invoke<SpeechModelStatus[]>("list_speech_models")
      .then(setSpeechModels)
      .catch((e) => console.error("list_speech_models failed:", e));
  }, [refreshPermissions]);

  // Permission checks can be slow on first launch (TCC lookups). If the
  // initial fetch is still pending, retry once so the step never looks blank.
  useEffect(() => {
    if (permissions.length > 0) return;
    const id = setTimeout(refreshPermissions, 1500);
    return () => clearTimeout(id);
  }, [permissions, refreshPermissions]);

  const allPermissionsGranted =
    permissions.length > 0 && permissions.every((p) => p.state === "granted");

  const finish = async () => {
    setError(null);
    if (!settings) {
      // get_settings may not have resolved yet — try fetching it directly.
      try {
        const s = await invoke<Settings>("updateSettingsPatch", {
          patch: { hasCompletedOnboarding: true },
        });
        setSettings(s);
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
      await invoke("select_speech_model", { id });
      await invoke("download_speech_model", { id });
      await invoke<SpeechModelStatus[]>("list_speech_models").then(setSpeechModels);
      checkReady();
    } catch (e) {
      console.error("download failed", e);
      setError(errorMessage(e));
    } finally {
      setDownloading(null);
      setProgress(null);
    }
  };

  const selectedModel = speechModels.find((m) => m.selected);
  const modelReady = selectedModel?.downloaded ?? false;
  const recommendedModel = speechModels.find((m) => m.recommended);

  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 24,
      }}
    >
      {/* .tt-page equivalent: centered content column on the sunlit canvas. */}
      <div
        style={{
          width: "100%",
          maxWidth: 720,
          background: "var(--bg)",
          borderRadius: "var(--radius)",
          padding: "40px 48px",
          boxSizing: "border-box",
        }}
      >
        <div style={{ fontSize: "var(--text-xl)", fontWeight: 650, letterSpacing: "var(--tracking-display)", marginBottom: 4 }}>
          Welcome to Teletype
        </div>
        <div style={{ color: "var(--text-secondary)", fontSize: "var(--text-sm)", marginBottom: 28 }}>
          A few quick steps to get dictation working. Everything runs locally.
        </div>

        {/* Progress */}
        <StepProgress step={step} />

        {/* Step 0: Privacy */}
        {step === 0 && (
          <div>
            <div
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                fontSize: "var(--text-xs)",
                fontWeight: 600,
                letterSpacing: 0.4,
                color: "var(--success)",
                background: "var(--success-soft)",
                border: "1px solid color-mix(in srgb, var(--success) 25%, transparent)",
                borderRadius: "var(--radius-pill)",
                padding: "3px 10px",
                marginBottom: 14,
              }}
            >
              <ShieldIcon size={13} color="var(--success)" />
              On-device
            </div>
            <h3 style={{ fontSize: "var(--text-xl)", fontWeight: 650, letterSpacing: "var(--tracking-display)", marginBottom: 8 }}>
              Your voice never leaves this device.
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: "var(--text-sm)", lineHeight: 1.55, marginBottom: 28, maxWidth: "65ch" }}>
              Teletype hears you, transcribes you, and polishes your words
              entirely on this Mac. Your audio, transcripts, and history are
              never uploaded. If you later connect your own cloud model for
              polishing, only the text you dictated is sent to that provider,
              under your key.
            </p>
            <div style={{ display: "flex", justifyContent: "flex-end" }}>
              <PrimaryButton onClick={() => setStep(1)}>
                Continue
              </PrimaryButton>
            </div>
          </div>
        )}

        {/* Step 1: Permissions */}
        {step === 1 && (
          <div>
            <StepHeader
              title="Grant permissions"
              sub="Teletype needs the microphone to hear you and accessibility to type into whatever app you're using."
            />
            <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 28 }}>
              {permissions.map((p) => (
                <div
                  key={p.kind}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "10px 14px",
                    background: "var(--surface)",
                    borderRadius: "var(--radius-sm)",
                    border: `1px solid ${p.state === "granted" ? "var(--success)" : "var(--border)"}`,
                  }}
                >
                  <span style={{ flex: 1, textTransform: "capitalize" }}>{p.kind}</span>
                  {p.state === "granted" ? (
                    <span style={{ color: "var(--success)", fontSize: "var(--text-sm)", display: "inline-flex", alignItems: "center", gap: 5 }}>
                      <Icon name="check" size={13} color="var(--success)" />
                      Granted
                    </span>
                  ) : (
                    <>
                      <button
                        onClick={async () => {
                          await invoke("request_permission", { kind: p.kind }).catch(console.error);
                          // The TCC prompt is async — poll a few times so the row
                          // flips to Granted as soon as the OS reflects the grant.
                          [800, 2000, 4000].forEach((ms) => setTimeout(refreshPermissions, ms));
                        }}
                      >
                        Grant
                      </button>
                      <button
                        onClick={() =>
                          invoke("open_permission_settings", { kind: p.kind }).catch(console.error)
                        }
                      >
                        Open Settings
                      </button>
                    </>
                  )}
                </div>
              ))}
              {permissions.length === 0 && (
                <div style={{ color: "var(--text-secondary)", fontSize: "var(--text-sm)" }}>
                  Loading permissions…
                </div>
              )}
            </div>
            <div style={{ display: "flex", justifyContent: "flex-end" }}>
              <PrimaryButton disabled={!allPermissionsGranted} onClick={() => setStep(2)}>
                Continue
              </PrimaryButton>
            </div>
          </div>
        )}

        {/* Step 2: Dictation model */}
        {step === 2 && (
          <div>
            <StepHeader
              title="Set up your dictation model"
              sub="One download and you can start speaking. Everything runs on this device."
            />

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

            {speechModels.length > 1 && (
              <ModelStepAdvanced
                models={speechModels}
                downloading={downloading}
                progress={progress}
                error={error}
                showAdvanced={showAdvanced}
                onToggle={() => setShowAdvanced((v) => !v)}
                onDownload={downloadModel}
                onModelsChanged={setSpeechModels}
              />
            )}

            <div style={{ display: "flex", justifyContent: "space-between", marginTop: 28 }}>
              <GhostButton onClick={() => setStep(1)}>Back</GhostButton>
              <PrimaryButton disabled={!modelReady} onClick={() => setStep(3)}>
                Continue
              </PrimaryButton>
            </div>
          </div>
        )}

        {/* Step 3: Hotkey */}
        {step === 3 && (
          <div>
            <StepHeader
              title="Your dictation hotkey"
              sub='Hold this key anywhere to dictate. Release to type. Click "Change" and press a new combination to rebind.'
            />
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "14px 18px",
                background: "var(--surface)",
                borderRadius: "var(--radius-sm)",
                border: "1px solid var(--border)",
                marginBottom: 28,
              }}
            >
              {usesWebviewCapture() ? (
                <WebviewHotkeyRecorder
                  value={settings?.hotkey || ""}
                  onSave={(hotkey) => {
                    if (!settings) return;
                    void invoke("save_settings", {
                      settings: { ...settings, hotkey },
                    }).catch(console.error);
                    setSettings({ ...settings, hotkey });
                  }}
                />
              ) : (
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
              )}
              <span style={{ color: "var(--text-secondary)", fontSize: "var(--text-sm)" }}>
                Hold to talk, release to type
              </span>
            </div>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <GhostButton onClick={() => setStep(2)}>Back</GhostButton>
              <PrimaryButton onClick={() => setStep(4)}>
                Continue
              </PrimaryButton>
            </div>
          </div>
        )}

        {/* Step 4: Done */}
        {step === 4 && (
          <div style={{ textAlign: "center", padding: "8px 0" }}>
            <div style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", width: 44, height: 44, borderRadius: "50%", background: "var(--success-soft)", color: "var(--success)", marginBottom: 14 }}>
              <Icon name="check" size={22} color="var(--success)" />
            </div>
            <h3 style={{ fontSize: "var(--text-lg)", fontWeight: 650, letterSpacing: "var(--tracking-tight)", marginBottom: 8 }}>
              You're all set
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: "var(--text-sm)", lineHeight: 1.55, maxWidth: "50ch", margin: "0 auto 28px" }}>
              Hold your hotkey and start talking. Transcribed text is typed into
              whatever app is focused.
            </p>
            <PrimaryButton disabled={busy} onClick={finish}>
              {busy ? "Saving…" : "Start using Teletype"}
            </PrimaryButton>
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
// label is driven by download/selection state. Collapses the advanced list
// choice so a stranger reaches "speak" without scrolling.
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
        background: "var(--surface)",
        borderRadius: "var(--radius)",
        boxShadow: "var(--shadow-1)",
        border: "none",
        marginBottom: 4,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
        <span style={{ fontSize: "var(--text-md)", fontWeight: 600 }}>{model.name}</span>
        <span
          style={{
            fontSize: "var(--text-xs)",
            color: "var(--accent-ink)",
            border: "1px solid var(--accent)",
            borderRadius: "var(--radius-xs)",
            padding: "1px 6px",
            fontWeight: 600,
          }}
        >
          Recommended
        </span>
      </div>
      <div style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", marginBottom: 14 }}>
        {sizeLabel(model.sizeMb)} · Runs 100% on this device.
      </div>

      {isReady ? (
        <div
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            color: "var(--success)",
            fontSize: "var(--text-sm)",
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
        <div style={{ fontSize: "var(--text-xs)", color: "var(--danger)", marginTop: 6 }}>{error}</div>
      )}

      {readyMsg && (
        <div
          style={{
            marginTop: 12,
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: "var(--text-sm)",
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

// The "Choose a different model" disclosure: the full, pre-existing per-model
// list, kept verbatim so the advanced path is unchanged. Collapsed by default
// so it is not the default onboarding path.
function ModelStepAdvanced({
  models,
  downloading,
  progress,
  error,
  showAdvanced,
  onToggle,
  onDownload,
  onModelsChanged,
}: {
  models: SpeechModelStatus[];
  downloading: string | null;
  progress: DownloadProgress | null;
  error: string | null;
  showAdvanced: boolean;
  onToggle: () => void;
  onDownload: (id: string) => void;
  onModelsChanged: (models: SpeechModelStatus[]) => void;
}) {
  return (
    <div style={{ marginTop: 12 }}>
      <button
        onClick={onToggle}
        style={{
          background: "none",
          border: "none",
          padding: 0,
          color: "var(--text-secondary)",
          fontSize: "var(--text-sm)",
          cursor: "pointer",
        }}
      >
        {showAdvanced ? "Hide other models" : "Choose a different model"}
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
                background: "var(--surface)",
                borderRadius: "var(--radius-sm)",
                border: `1px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
              }}
            >
              <div style={{ flex: 1 }}>
                <div style={{ fontWeight: 500, fontSize: "var(--text-sm)" }}>
                  {m.name}
                  {m.recommended && (
                    <span
                      style={{
                        marginLeft: 8,
                        fontSize: "var(--text-xs)",
                        color: "var(--accent-ink)",
                        border: "1px solid var(--accent)",
                        borderRadius: "var(--radius-xs)",
                        padding: "0 5px",
                      }}
                    >
                      Recommended
                    </span>
                  )}
                </div>
                <div style={{ fontSize: "var(--text-xs)", color: "var(--text-secondary)" }}>
                  {sizeLabel(m.sizeMb)}
                  {m.downloaded && " · installed"}
                </div>
                {progress && progress.id === m.id && (
                  <DownloadProgressBar progress={progress} />
                )}
                {error && downloading === m.id && (
                  <div style={{ fontSize: "var(--text-xs)", color: "var(--danger)", marginTop: 4 }}>
                    {error}
                  </div>
                )}
              </div>
              {m.downloaded ? (
                <button
                  className={m.selected ? "primary" : ""}
                  onClick={async () => {
                    await invoke("select_speech_model", { id: m.id }).catch(console.error);
                    await invoke<SpeechModelStatus[]>("list_speech_models").then(onModelsChanged);
                  }}
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
