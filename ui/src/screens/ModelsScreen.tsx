import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { DownloadProgressBar } from "../components/DownloadProgress";
import {
  clearDownload,
  loadAcceptedLicenses,
  markDownloading,
  saveAcceptedLicense,
  snapshotDownloads,
  subscribeDownloads,
} from "../lib/downloadStore";

interface ModelStatus {
  id: string;
  name: string;
  sizeMb: number;
  description: string;
  downloaded: boolean;
  selected: boolean;
  recommended: boolean;
  licenseName: string | null;
  licenseUrl: string | null;
  requiresLicenseAccept: boolean;
  attribution: string | null;
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
  speed: number;
  accuracy: number;
  minRamGb: number;
  languageLabel: string;
}

const sizeLabel = (mb: number) =>
  mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;

/// Renders a row of small bars (like SpeakType's speed/accuracy indicators).
function Bars({ value, max = 10 }: { value: number; max?: number }) {
  const filled = Math.round((value / max) * 8);
  return (
    <div style={{ display: "flex", gap: 2, alignItems: "flex-end" }}>
      {Array.from({ length: 8 }, (_, i) => (
        <div
          key={i}
          style={{
            width: 4,
            height: 4 + i * 1.5,
            borderRadius: 1,
            background: i < filled ? "var(--accent)" : "var(--border)",
          }}
        />
      ))}
    </div>
  );
}

function speedLabel(speed: number): string {
  if (speed >= 9.5) return "Blazing";
  if (speed >= 8.5) return "Very fast";
  if (speed >= 7.5) return "Fast";
  if (speed >= 6.0) return "Moderate";
  return "Slow";
}

function accuracyLabel(accuracy: number): string {
  if (accuracy >= 9.5) return "Flawless";
  if (accuracy >= 9.0) return "Sharp";
  if (accuracy >= 8.0) return "Solid";
  if (accuracy >= 6.5) return "Fair";
  return "Basic";
}

/** Derives the keychain account id from a base URL (P0-9). */
function hostFromUrl(url: string): string {
  try {
    const u = new URL(url);
    return u.hostname;
  } catch {
    return "openai";
  }
}

/** Re-renders when the app-wide download store changes. */
function useDownloadStore() {
  return useSyncExternalStore(subscribeDownloads, snapshotDownloads);
}

export default function ModelsScreen() {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [speechModels, setSpeechModels] = useState<SpeechModelStatus[]>([]);
  const [modelStatus, setModelStatus] = useState("");
  const store = useDownloadStore();
  const [downloadError, setDownloadError] = useState<{ id: string; message: string } | null>(null);
  const [selectingId, setSelectingId] = useState<string | null>(null);

  // OpenAI-compatible connector state
  const [selectedProvider, setSelectedProvider] = useState("");
  const [openaiBaseUrl, setOpenaiBaseUrl] = useState("https://api.openai.com/v1");
  const [openaiModel, setOpenaiModel] = useState("gpt-4o-mini");
  const [apiKey, setApiKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [savingKey, setSavingKey] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [activating, setActivating] = useState(false);
  // License acceptance per gated model id (eg-1), persisted across sessions.
  const [acceptedLicenses, setAcceptedLicenses] = useState<Record<string, boolean>>(() =>
    loadAcceptedLicenses(),
  );

  useEffect(() => {
    refresh();
    invoke<{ selectedLlmProvider: string; openaiBaseUrl: string; openaiModel: string }>("get_settings")
      .then((s) => {
        setSelectedProvider(s.selectedLlmProvider || "");
        setOpenaiBaseUrl(s.openaiBaseUrl);
        setOpenaiModel(s.openaiModel);
      })
      .catch(console.error);
    const host = hostFromUrl(openaiBaseUrl);
    invoke<boolean>("has_llm_secret", { providerId: host })
      .then(setHasKey)
      .catch(console.error);
  }, []);

  const refresh = () => {
    invoke<ModelStatus[]>("list_models").then(setModels).catch(console.error);
    invoke<SpeechModelStatus[]>("list_speech_models").then(setSpeechModels).catch(console.error);
    invoke<string>("get_model_status").then(setModelStatus).catch(console.error);
  };

  const saveOpenaiSettings = async () => {
    const settings = await invoke<Record<string, unknown>>("get_settings");
    await invoke("save_settings", {
      settings: {
        ...settings,
        openaiBaseUrl,
        openaiModel,
      },
    });
  };

  const saveApiKey = async () => {
    if (!apiKey.trim()) return;
    setSavingKey(true);
    setTestResult(null);
    try {
      await invoke("set_llm_secret", { providerId: hostFromUrl(openaiBaseUrl), secret: apiKey.trim() });
      setHasKey(true);
      setApiKey("");
      setTestResult({ ok: true, message: "API key saved to keychain" });
    } catch (e) {
      setTestResult({ ok: false, message: String(e) });
    }
    setSavingKey(false);
  };

  const clearApiKey = async () => {
    try {
      await invoke("clear_llm_secret", { providerId: hostFromUrl(openaiBaseUrl) });
      setHasKey(false);
      setTestResult({ ok: true, message: "API key removed" });
    } catch (e) {
      setTestResult({ ok: false, message: String(e) });
    }
  };

  const testConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      await saveOpenaiSettings();
      const msg = await invoke<string>("test_llm_connection");
      setTestResult({ ok: true, message: msg });
    } catch (e) {
      setTestResult({ ok: false, message: String(e) });
    }
    setTesting(false);
  };

  const connectOpenai = async () => {
    setActivating(true);
    setTestResult(null);
    try {
      if (apiKey.trim()) {
        await invoke("set_llm_secret", { providerId: hostFromUrl(openaiBaseUrl), secret: apiKey.trim() });
        setHasKey(true);
        setApiKey("");
      }
      await saveOpenaiSettings();
      await invoke("select_openai_provider");
      setSelectedProvider("openai-compat");
      setTestResult({ ok: true, message: "Connected" });
      refresh();
    } catch (e) {
      setTestResult({ ok: false, message: String(e) });
    }
    setActivating(false);
  };

  const selectSpeech = async (id: string) => {
    await invoke("select_speech_model", { id });
    refresh();
  };

  const download = async (id: string, speech = false) => {
    markDownloading(id);
    setDownloadError(null);
    try {
      await invoke(speech ? "download_speech_model" : "download_model", {
        id,
        ...(speech ? {} : { licenseAccepted: acceptedLicenses[id] === true }),
      });
      clearDownload(id);
    } catch (e) {
      clearDownload(id);
      setDownloadError({ id, message: String(e) });
    }
    refresh();
  };

  const toggleLicense = (id: string, checked: boolean) => {
    setAcceptedLicenses((prev) => ({ ...prev, [id]: checked }));
    saveAcceptedLicense(id, checked);
  };

  const selectLocal = async (id: string) => {
    if (selectingId) return;
    setSelectingId(id);
    setDownloadError(null);
    try {
      await invoke("select_model", { id });
      setSelectedProvider("local-server");
      refresh();
    } catch (e) {
      setDownloadError({ id, message: String(e) });
    } finally {
      setSelectingId(null);
    }
  };

  const onThisComputer = speechModels.filter((m) => m.downloaded);
  const available = speechModels.filter((m) => !m.downloaded);

  const SpeechRow = ({ m, showDownload }: { m: SpeechModelStatus; showDownload: boolean }) => (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "14px 16px",
        borderBottom: "1px solid var(--border)",
        background: m.selected ? "var(--accent-soft)" : "transparent",
      }}
    >
      {/* Radio / check */}
      <div
        style={{
          width: 20,
          height: 20,
          borderRadius: "50%",
          border: `2px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        {m.selected && (
          <div style={{ width: 10, height: 10, borderRadius: "50%", background: "var(--accent)" }} />
        )}
      </div>

      {/* Name + description */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          <span style={{ fontWeight: 600, fontSize: 14 }}>{m.name}</span>
          {m.recommended && (
            <span
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: "var(--success, #22c55e)",
                background: "var(--success-soft, #dcfce7)",
                padding: "1px 8px",
                borderRadius: 10,
              }}
            >
              Recommended
            </span>
          )}
          {m.selected && (
            <span
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: "var(--accent)",
                display: "flex",
                alignItems: "center",
                gap: 3,
              }}
            >
              <Icon name="check" size={12} /> In use
            </span>
          )}
        </div>
        <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>
          {m.description}
        </div>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2, display: "flex", gap: 8 }}>
          <span>{m.languageLabel}</span>
          {m.engine === "whisper" && <span>· Neural Engine</span>}
        </div>
        {store.progress[m.id] && <DownloadProgressBar progress={store.progress[m.id]} />}
        {downloadError?.id === m.id && (
          <div style={{ fontSize: 11, color: "#ef4444", marginTop: 4 }}>
            Download failed: {downloadError.message}
          </div>
        )}
      </div>

      {/* Speed */}
      <div style={{ width: 90, flexShrink: 0 }}>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginBottom: 4 }}>
          {speedLabel(m.speed)}
        </div>
        <Bars value={m.speed} />
      </div>

      {/* Accuracy */}
      <div style={{ width: 90, flexShrink: 0 }}>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginBottom: 4 }}>
          {accuracyLabel(m.accuracy)}
        </div>
        <Bars value={m.accuracy} />
      </div>

      {/* Size */}
      <div style={{ width: 60, flexShrink: 0, fontSize: 13, color: "var(--text-secondary)" }}>
        {sizeLabel(m.sizeMb)}
      </div>

      {/* Action */}
      <div style={{ width: 80, flexShrink: 0, textAlign: "right" }}>
        {showDownload ? (
          <button
            onClick={() => download(m.id, true)}
            disabled={store.active.includes(m.id)}
            style={{ fontSize: 13 }}
          >
            {store.active.includes(m.id)
              ? store.progress[m.id]
                ? `${Math.round(store.progress[m.id].percent)}%`
                : "Downloading…"
              : "Download"}
          </button>
        ) : (
          <button
            className={m.selected ? "primary" : ""}
            onClick={() => selectSpeech(m.id)}
            style={{ fontSize: 13 }}
          >
            {m.selected ? "Selected" : "Select"}
          </button>
        )}
      </div>
    </div>
  );

  const tableHeader = (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "8px 16px",
        fontSize: 11,
        fontWeight: 600,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: 0.5,
        borderBottom: "1px solid var(--border)",
      }}
    >
      <div style={{ width: 20 }} />
      <div style={{ flex: 1 }}>Model</div>
      <div style={{ width: 90 }}>Speed</div>
      <div style={{ width: 90 }}>Accuracy</div>
      <div style={{ width: 60 }}>Size</div>
      <div style={{ width: 80 }} />
    </div>
  );

  return (
    <div style={{ maxWidth: 860 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
        <div>
          <h2 style={{ fontSize: 20, fontWeight: 700 }}>AI Models</h2>
          <p style={{ color: "var(--text-secondary)", fontSize: 13, marginTop: 4 }}>
            Every model runs on this computer. Pick the one Teletype uses to transcribe.
          </p>
        </div>
      </div>

      {/* On this computer */}
      <h3 style={{ fontSize: 15, fontWeight: 700, margin: "24px 0 2px" }}>On this computer</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
        Click a model to use it for dictation.
      </p>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {onThisComputer.length === 0 ? (
          <p style={{ padding: 24, fontSize: 13, color: "var(--text-secondary)", textAlign: "center" }}>
            No models downloaded yet. Grab one from below.
          </p>
        ) : (
          <>
            {tableHeader}
            {onThisComputer.map((m) => (
              <SpeechRow key={m.id} m={m} showDownload={false} />
            ))}
          </>
        )}
      </div>

      {/* Available to download */}
      <h3 style={{ fontSize: 15, fontWeight: 700, margin: "24px 0 2px" }}>Available to download</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
        Larger models are more accurate. Smaller ones are faster.
      </p>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {available.length === 0 ? (
          <p style={{ padding: 24, fontSize: 13, color: "var(--text-secondary)", textAlign: "center" }}>
            All models are downloaded.
          </p>
        ) : (
          <>
            {tableHeader}
            {available.map((m) => (
              <SpeechRow key={m.id} m={m} showDownload={true} />
            ))}
          </>
        )}
      </div>

      {/* Transforms (LLM) */}
      <h3 style={{ fontSize: 15, fontWeight: 700, margin: "28px 0 2px" }}>Transforms (LLM)</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
        Rewrites and polishes your dictated text.
      </p>
      <div style={{ padding: "10px 14px", background: "var(--surface)", borderRadius: "var(--radius)", marginBottom: 8, fontSize: 13 }}>
        <strong>Active provider:</strong>{" "}
        {selectedProvider === "openai-compat"
          ? `API (${openaiModel})`
          : selectedProvider === "local-server"
            ? `Local (${modelStatus || "llama-server"})`
            : "None"}
      </div>

      {/* OpenAI-compatible connector */}
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: "14px 16px",
          marginBottom: 12,
        }}
      >
        <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 2 }}>
          OpenAI-compatible API
        </div>
        <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
          Works with OpenAI, OpenRouter, Groq, LM Studio, Ollama, or any custom
          endpoint that speaks <code>/v1/chat/completions</code>.
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginBottom: 8 }}>
          <input
            value={openaiBaseUrl}
            onChange={(e) => setOpenaiBaseUrl(e.target.value)}
            placeholder="https://api.openai.com/v1"
            style={{ flex: "1 1 220px", minWidth: 180, fontSize: 13, padding: "6px 10px" }}
          />
          <input
            value={openaiModel}
            onChange={(e) => setOpenaiModel(e.target.value)}
            placeholder="Model id, e.g. gpt-4o-mini"
            style={{ flex: "1 1 160px", minWidth: 140, fontSize: 13, padding: "6px 10px" }}
          />
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={hasKey ? "API key saved (type to replace)" : "API key (optional for local)"}
            style={{ flex: "1 1 200px", minWidth: 160, fontSize: 13, padding: "6px 10px" }}
          />
          {apiKey.trim() && (
            <button onClick={saveApiKey} disabled={savingKey} style={{ fontSize: 13 }}>
              {savingKey ? "Saving…" : "Save key"}
            </button>
          )}
          {hasKey && (
            <button onClick={clearApiKey} style={{ fontSize: 13 }}>
              Clear key
            </button>
          )}
          <button onClick={testConnection} disabled={testing} style={{ fontSize: 13 }}>
            {testing ? "Testing…" : "Test connection"}
          </button>
          <button className="primary" onClick={connectOpenai} disabled={activating} style={{ fontSize: 13 }}>
            {activating ? "Connecting…" : selectedProvider === "openai-compat" ? "Reconnect" : "Use this API"}
          </button>
        </div>
        {testResult && (
          <div
            style={{
              marginTop: 8,
              fontSize: 12,
              color: testResult.ok ? "var(--success, #22c55e)" : "#ef4444",
            }}
          >
            {testResult.message}
          </div>
        )}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {models.map((m) => (
          <div
            key={m.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "10px 16px",
              background: "var(--surface)",
              borderRadius: "var(--radius-sm)",
              border: `1px solid ${m.selected && selectedProvider !== "openai-compat" ? "var(--accent)" : "var(--border)"}`,
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontWeight: 500, fontSize: 13, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                {m.name}
                {m.recommended && (
                  <span
                    style={{
                      fontSize: 11,
                      fontWeight: 600,
                      color: "var(--success, #22c55e)",
                      background: "var(--success-soft, #dcfce7)",
                      padding: "1px 8px",
                      borderRadius: 10,
                    }}
                  >
                    Recommended
                  </span>
                )}
                {m.licenseName && (
                  <span
                    style={{
                      fontSize: 10,
                      fontWeight: 600,
                      padding: "1px 7px",
                      borderRadius: 8,
                      background: m.requiresLicenseAccept ? "#fef3c7" : "var(--accent-soft)",
                      color: m.requiresLicenseAccept ? "#92400e" : "var(--accent)",
                    }}
                  >
                    {m.licenseName}
                  </span>
                )}
                {m.attribution && (
                  <span style={{ fontSize: 11, color: "var(--text-secondary)", fontWeight: 400 }}>
                    {m.attribution}
                  </span>
                )}
              </div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                {m.description} · {sizeLabel(m.sizeMb)}
              </div>
              {store.progress[m.id] && <DownloadProgressBar progress={store.progress[m.id]} />}
              {downloadError?.id === m.id && (
                <div style={{ fontSize: 11, color: "#ef4444", marginTop: 4 }}>
                  Download failed: {downloadError.message}
                </div>
              )}
              {m.requiresLicenseAccept && !m.downloaded && m.licenseUrl && (
                <div style={{ fontSize: 11, marginTop: 4, display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
                  <a href={m.licenseUrl} target="_blank" rel="noreferrer" style={{ color: "var(--accent)" }}>
                    Read license
                  </a>
                  <label style={{ display: "flex", alignItems: "center", gap: 5, cursor: "pointer" }}>
                    <input
                      type="checkbox"
                      checked={acceptedLicenses[m.id] === true}
                      onChange={(e) => toggleLicense(m.id, e.target.checked)}
                    />
                    I accept the license (personal use only)
                  </label>
                </div>
              )}
            </div>
            {m.downloaded ? (
              <button
                className={m.selected && selectedProvider !== "openai-compat" ? "primary" : ""}
                disabled={selectingId !== null}
                onClick={() => selectLocal(m.id)}
              >
                {selectingId === m.id
                  ? "Starting…"
                  : m.selected && selectedProvider !== "openai-compat"
                    ? "Selected"
                    : "Select"}
              </button>
            ) : (
              <button
                onClick={() => download(m.id)}
                disabled={
                  store.active.includes(m.id) ||
                  (m.requiresLicenseAccept && acceptedLicenses[m.id] !== true)
                }
                title={
                  m.requiresLicenseAccept && acceptedLicenses[m.id] !== true
                    ? "Accept the license first"
                    : undefined
                }
              >
                {store.active.includes(m.id)
                  ? store.progress[m.id]?.status === "verifying"
                    ? "Verifying…"
                    : store.progress[m.id]
                      ? `${Math.round(store.progress[m.id].percent)}%`
                      : "Downloading…"
                  : `Download (${sizeLabel(m.sizeMb)})`}
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
