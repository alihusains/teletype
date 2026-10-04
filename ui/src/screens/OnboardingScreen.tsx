import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../components/HotkeyRecorder";
import {
  DownloadProgressBar,
  type DownloadProgress,
} from "../components/DownloadProgress";
import { useTauriEvent } from "../lib/useTauriEvent";
import { ShieldIcon } from "../lib/PrivacyBadge";
import { Icon } from "../components/Icon";

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
      await invoke("select_speech_model", { id });
      await invoke("download_speech_model", { id });
      await invoke<SpeechModelStatus[]>("list_speech_models").then(setSpeechModels);
      checkReady();
    } catch (e) {
      console.error("download failed", e);
      setError(String(e));
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
      <div
        style={{
          width: "100%",
          maxWidth: 560,
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: 28,
        }}
      >
        <div style={{ fontSize: 20, fontWeight: 600, marginBottom: 4 }}>
          Welcome to Teletype
        </div>
        <div style={{ color: "var(--text-secondary)", marginBottom: 20 }}>
          A few quick steps to get dictation working. Everything runs locally.
        </div>

        {/* Progress */}
        <div style={{ display: "flex", gap: 8, marginBottom: 24 }}>
          {STEPS.map((label, i) => (
            <div
              key={label}
              style={{
                flex: 1,
                textAlign: "center",
                fontSize: 11,
                color: i <= step ? "var(--accent)" : "var(--text-secondary)",
                fontWeight: i === step ? 600 : 400,
                borderTop: `2px solid ${i <= step ? "var(--accent)" : "var(--border)"}`,
                paddingTop: 8,
              }}
            >
              {label}
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
                background: "var(--accent-soft)",
                border: "1px solid rgba(22,163,74,0.25)",
                borderRadius: 999,
                padding: "3px 10px",
                marginBottom: 14,
              }}
            >
              <ShieldIcon size={13} color="var(--success)" />
              On-device
            </div>
            <h3 style={{ fontSize: 19, fontWeight: 700, marginBottom: 8 }}>
              Your voice never leaves this device.
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, lineHeight: 1.55, marginBottom: 20 }}>
              Teletype hears you, transcribes you, and polishes your words
              entirely on this Mac. Your audio, transcripts, and history are
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

        {/* Step 1: Permissions */}
        {step === 1 && (
          <div>
            <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>
              Grant permissions
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
              Teletype needs the microphone to hear you and accessibility to
              type into whatever app you're using.
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 20 }}>
              {permissions.map((p) => (
                <div
                  key={p.kind}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "10px 14px",
                    background: "var(--bg)",
                    borderRadius: "var(--radius-sm)",
                    border: `1px solid ${p.state === "granted" ? "var(--success)" : "var(--border)"}`,
                  }}
                >
                  <span style={{ flex: 1, textTransform: "capitalize" }}>{p.kind}</span>
                  {p.state === "granted" ? (
                    <span style={{ color: "var(--success)", fontSize: 13 }}>✓ Granted</span>
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
                <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>
                  Loading permissions…
                </div>
              )}
            </div>
            <div style={{ display: "flex", justifyContent: "flex-end" }}>
              <button className="primary" disabled={!allPermissionsGranted} onClick={() => setStep(2)}>
                Continue
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

            <div style={{ display: "flex", justifyContent: "space-between", marginTop: 20 }}>
              <button onClick={() => setStep(1)}>Back</button>
              <button className="primary" disabled={!modelReady} onClick={() => setStep(3)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 3: Hotkey */}
        {step === 3 && (
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
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <button onClick={() => setStep(2)}>Back</button>
              <button className="primary" onClick={() => setStep(4)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 4: Done */}
        {step === 4 && (
          <div style={{ textAlign: "center", padding: "8px 0" }}>
            <div style={{ fontSize: 40, marginBottom: 12 }}>🎉</div>
            <h3 style={{ fontSize: 17, fontWeight: 600, marginBottom: 8 }}>
              You're all set
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 20 }}>
              Hold your hotkey and start talking. Transcribed text is typed into
              whatever app is focused.
            </p>
            <button className="primary" disabled={busy} onClick={finish} style={{ padding: "8px 24px" }}>
              {busy ? "Saving…" : "Start using Teletype"}
            </button>
            {error && (
              <p style={{ color: "#f87171", fontSize: 13, marginTop: 12 }}>{error}</p>
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
        <div style={{ fontSize: 11, color: "#ef4444", marginTop: 6 }}>{error}</div>
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
          fontSize: 12,
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
                background: "var(--bg)",
                borderRadius: "var(--radius-sm)",
                border: `1px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
              }}
            >
              <div style={{ flex: 1 }}>
                <div style={{ fontWeight: 500, fontSize: 13 }}>
                  {m.name}
                  {m.recommended && (
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
                  <div style={{ fontSize: 11, color: "#ef4444", marginTop: 4 }}>
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
