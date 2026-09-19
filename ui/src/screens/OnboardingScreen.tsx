import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Permission {
  kind: string;
  granted: boolean;
}

interface SpeechModelStatus {
  id: string;
  name: string;
  engine: string;
  sizeMb: number;
  description: string;
  recommended: boolean;
  english_only: boolean;
  downloaded: boolean;
  selected: boolean;
}

interface Settings {
  hotkey: string;
  selected_speech_model: string;
  has_completed_onboarding: boolean;
  [key: string]: unknown;
}

const STEPS = ["Permissions", "Dictation model", "Hotkey", "Done"] as const;

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
  const [busy, setBusy] = useState(false);

  const refreshPermissions = () =>
    invoke<Permission[]>("get_permissions").then(setPermissions).catch(console.error);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
    refreshPermissions();
    invoke<SpeechModelStatus[]>("list_speech_models")
      .then(setSpeechModels)
      .catch(console.error);
  }, []);

  const allPermissionsGranted =
    permissions.length > 0 && permissions.every((p) => p.granted);

  const finish = async () => {
    if (!settings) return;
    setBusy(true);
    try {
      await invoke("save_settings", {
        settings: { ...settings, has_completed_onboarding: true },
      });
      onCompleted();
    } catch (e) {
      console.error(e);
      setBusy(false);
    }
  };

  const downloadModel = async (id: string) => {
    setDownloading(id);
    try {
      await invoke("select_speech_model", { id });
      await invoke("download_speech_model", { id });
      await invoke<SpeechModelStatus[]>("list_speech_models").then(setSpeechModels);
    } catch (e) {
      console.error("download failed", e);
    } finally {
      setDownloading(null);
    }
  };

  const selectedModel = speechModels.find((m) => m.selected);
  const modelReady = selectedModel?.downloaded ?? false;

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

        {/* Step 0: Permissions */}
        {step === 0 && (
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
                    border: `1px solid ${p.granted ? "var(--success)" : "var(--border)"}`,
                  }}
                >
                  <span style={{ flex: 1, textTransform: "capitalize" }}>{p.kind}</span>
                  {p.granted ? (
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
              <button className="primary" disabled={!allPermissionsGranted} onClick={() => setStep(1)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 1: Dictation model */}
        {step === 1 && (
          <div>
            <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>
              Choose a dictation model
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
              This model transcribes your speech on-device. The recommended one
              is the fastest and most accurate.
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 8, marginBottom: 20 }}>
              {speechModels.map((m) => (
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
                  </div>
                  {m.downloaded ? (
                    <button
                      className={m.selected ? "primary" : ""}
                      onClick={async () => {
                        await invoke("select_speech_model", { id: m.id }).catch(console.error);
                        await invoke<SpeechModelStatus[]>("list_speech_models").then(setSpeechModels);
                      }}
                    >
                      {m.selected ? "Selected" : "Select"}
                    </button>
                  ) : (
                    <button
                      disabled={downloading !== null}
                      onClick={() => downloadModel(m.id)}
                    >
                      {downloading === m.id ? "Downloading…" : `Download (${sizeLabel(m.sizeMb)})`}
                    </button>
                  )}
                </div>
              ))}
            </div>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <button onClick={() => setStep(0)}>Back</button>
              <button className="primary" disabled={!modelReady} onClick={() => setStep(2)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 2: Hotkey */}
        {step === 2 && (
          <div>
            <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>
              Your dictation hotkey
            </h3>
            <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
              Hold this key anywhere to dictate. Release to type. You can change
              it later in Settings.
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
              <kbd
                style={{
                  fontFamily: "monospace",
                  fontSize: 15,
                  padding: "4px 10px",
                  background: "var(--surface)",
                  border: "1px solid var(--border)",
                  borderRadius: 6,
                }}
              >
                {settings?.hotkey || "⌥ Space"}
              </kbd>
              <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>
                Hold to talk, release to type
              </span>
            </div>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <button onClick={() => setStep(1)}>Back</button>
              <button className="primary" onClick={() => setStep(3)}>
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 3: Done */}
        {step === 3 && (
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
          </div>
        )}
      </div>
    </div>
  );
}
