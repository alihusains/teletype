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

// ---- Presentational helpers (render-only, no state of their own) ----------

type RailItem =
  | { kind: "speech"; id: string; name: string; tagline: string; recommended: boolean; selected: boolean }
  | { kind: "llm"; id: string; name: string; tagline: string; recommended: boolean; selected: boolean }
  | { kind: "openai"; id: string; name: string; tagline: string; recommended: boolean; selected: boolean };

function RailGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div style={{ marginBottom: 14 }}>
      <div
        style={{
          fontSize: 11,
          fontWeight: 700,
          letterSpacing: 0.6,
          textTransform: "uppercase",
          color: "var(--text-secondary)",
          padding: "0 10px",
          marginBottom: 6,
        }}
      >
        {title}
      </div>
      {children}
    </div>
  );
}

function RailRow({
  item,
  active,
  onClick,
}: {
  item: RailItem;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        display: "block",
        width: "100%",
        textAlign: "left",
        padding: "8px 10px",
        marginBottom: 2,
        borderRadius: 10,
        border: active ? "1.5px solid var(--accent)" : "1.5px solid transparent",
        background: active ? "var(--accent-soft)" : "transparent",
        cursor: "pointer",
        fontFamily: "inherit",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
        <span
          style={{
            fontSize: 13,
            fontWeight: 600,
            color: active ? "var(--accent)" : "var(--text, inherit)",
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {item.name}
        </span>
        {item.recommended && (
          <Icon name="sparkles" size={12} color="var(--success, #22c55e)" strokeWidth={2} />
        )}
      </div>
      <div
        style={{
          fontSize: 11,
          color: "var(--text-secondary)",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
          marginTop: 1,
        }}
      >
        {item.tagline}
      </div>
    </button>
  );
}

function StatusChip({ tone, label }: { tone: "success" | "accent" | "neutral"; label: string }) {
  const color =
    tone === "success"
      ? "var(--success, #22c55e)"
      : tone === "accent"
        ? "var(--accent)"
        : "var(--text-secondary)";
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        fontSize: 12,
        fontWeight: 600,
        color,
        background:
          tone === "success" ? "var(--success-soft, #dcfce7)" : tone === "accent" ? "var(--accent-soft)" : "var(--surface)",
        border: "1px solid var(--border)",
        padding: "2px 10px",
        borderRadius: 12,
      }}
    >
      <span style={{ width: 7, height: 7, borderRadius: "50%", background: color, display: "inline-block" }} />
      {label}
    </span>
  );
}

function DetailCard({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ marginBottom: 12 }}>
      <div
        style={{
          fontSize: 11,
          fontWeight: 700,
          letterSpacing: 0.6,
          textTransform: "uppercase",
          color: "var(--accent)",
          marginBottom: 6,
        }}
      >
        {label}
      </div>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: "14px 16px",
        }}
      >
        {children}
      </div>
    </div>
  );
}

function RecommendedPill() {
  return (
    <span
      style={{
        fontSize: 11,
        fontWeight: 600,
        color: "var(--success, #22c55e)",
        background: "var(--success-soft, #dcfce7)",
        padding: "1px 8px",
        borderRadius: 10,
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
      }}
    >
      <Icon name="sparkles" size={11} strokeWidth={2} /> Recommended
    </span>
  );
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

  // Which detail pane is showing: "speech:<id>" | "llm:<id>" | "openai"
  const [selection, setSelection] = useState<string>("");

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

  // Keep the selection pointing at something that exists; default to the first
  // speech model (or the first rail entry) once data loads.
  useEffect(() => {
    if (speechModels.length === 0) return;
    const first = speechModels[0];
    setSelection((cur) => {
      if (cur.startsWith("speech:") && speechModels.some((m) => m.id === cur.slice(7))) return cur;
      if (cur.startsWith("llm:")) return cur;
      if (cur === "openai") return cur;
      return `speech:${first.id}`;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [speechModels]);

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

  const speechById = (id: string) => speechModels.find((m) => m.id === id);
  const llmById = (id: string) => models.find((m) => m.id === id);

  // ---- Rail entries -------------------------------------------------------
  const speechRail = (m: SpeechModelStatus): RailItem => ({
    kind: "speech",
    id: m.id,
    name: m.name,
    tagline: m.description,
    recommended: m.recommended,
    selected: m.selected,
  });
  const llmRail = (m: ModelStatus): RailItem => ({
    kind: "llm",
    id: m.id,
    name: m.name,
    tagline: `${m.description} · ${sizeLabel(m.sizeMb)}`,
    recommended: m.recommended,
    selected: m.selected && selectedProvider !== "openai-compat",
  });

  const openaiActive = selectedProvider === "openai-compat";
  const openaiRail: RailItem = {
    kind: "openai",
    id: "openai",
    name: "OpenAI-compatible API",
    tagline: hasKey ? "Your API key" : "Bring your own endpoint",
    recommended: false,
    selected: openaiActive,
  };

  const selectRail = (item: RailItem) =>
    setSelection(item.kind === "openai" ? "openai" : `${item.kind}:${item.id}`);

  // ---- Detail panes -------------------------------------------------------
  const renderSpeechDetail = (m: SpeechModelStatus) => {
    const chip = m.selected
      ? { tone: "success" as const, label: "In use" }
      : m.downloaded
        ? { tone: "accent" as const, label: "Downloaded" }
        : { tone: "neutral" as const, label: "Not installed" };
    const downloading = store.active.includes(m.id);
    return (
      <>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            flexWrap: "wrap",
            marginBottom: 16,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
            <h3 style={{ fontSize: 18, fontWeight: 700, margin: 0 }}>{m.name}</h3>
            {m.recommended && <RecommendedPill />}
          </div>
          <StatusChip tone={chip.tone} label={chip.label} />
        </div>

        <DetailCard label={m.downloaded ? "This model" : "Download"}>
          {m.downloaded ? (
            <div style={{ display: "flex", alignItems: "center", gap: 16, flexWrap: "wrap" }}>
              <div style={{ flex: 1, fontSize: 13, color: "var(--text-secondary)" }}>
                {sizeLabel(m.sizeMb)} · {m.languageLabel}
                {m.engine === "whisper" ? " · Neural Engine" : ""}
              </div>
              <button
                className={m.selected ? "primary" : ""}
                onClick={() => selectSpeech(m.id)}
                style={{ fontSize: 13 }}
              >
                {m.selected ? "Selected" : "Select"}
              </button>
            </div>
          ) : (
            <div>
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  gap: 12,
                }}
              >
                <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
                  Download size: {sizeLabel(m.sizeMb)}
                </div>
                <button
                  onClick={() => download(m.id, true)}
                  disabled={downloading}
                  style={{ fontSize: 13 }}
                >
                  {downloading ? "Downloading…" : "Download"}
                </button>
              </div>
              {store.progress[m.id] && <DownloadProgressBar progress={store.progress[m.id]} />}
              {downloadError?.id === m.id && (
                <div style={{ fontSize: 12, color: "#ef4444", marginTop: 8 }}>
                  {downloadError.message}
                </div>
              )}
            </div>
          )}
        </DetailCard>

        <DetailCard label="Why use this model">
          <p style={{ fontSize: 13, margin: 0, lineHeight: 1.6 }}>
            {m.description}
          </p>
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 8, display: "flex", gap: 16, flexWrap: "wrap" }}>
            <span>
              <strong style={{ color: "var(--text)" }}>Speed:</strong> {speedLabel(m.speed)}
            </span>
            <span>
              <strong style={{ color: "var(--text)" }}>Accuracy:</strong> {accuracyLabel(m.accuracy)}
            </span>
          </div>
        </DetailCard>
      </>
    );
  };

  const renderLlmDetail = (m: ModelStatus) => {
    const isLlmActive = m.selected && selectedProvider !== "openai-compat";
    const chip = isLlmActive
      ? { tone: "success" as const, label: "In use" }
      : m.downloaded
        ? { tone: "accent" as const, label: "Downloaded" }
        : { tone: "neutral" as const, label: "Not installed" };
    const downloading = store.active.includes(m.id);
    const licenseBlocked = m.requiresLicenseAccept && acceptedLicenses[m.id] !== true;
    return (
      <>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            flexWrap: "wrap",
            marginBottom: 16,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
            <h3 style={{ fontSize: 18, fontWeight: 700, margin: 0 }}>{m.name}</h3>
            {m.recommended && <RecommendedPill />}
            {m.attribution && (
              <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>{m.attribution}</span>
            )}
          </div>
          <StatusChip tone={chip.tone} label={chip.label} />
        </div>

        <DetailCard label={m.downloaded ? "This model" : "Download"}>
          {m.downloaded ? (
            <div style={{ display: "flex", alignItems: "center", gap: 16, flexWrap: "wrap" }}>
              <div style={{ flex: 1, fontSize: 13, color: "var(--text-secondary)" }}>
                {sizeLabel(m.sizeMb)}
                {m.licenseName ? ` · ${m.licenseName}` : ""}
              </div>
              <button
                className={isLlmActive ? "primary" : ""}
                disabled={selectingId !== null}
                onClick={() => selectLocal(m.id)}
                style={{ fontSize: 13 }}
              >
                {selectingId === m.id
                  ? "Starting…"
                  : isLlmActive
                    ? "Selected"
                    : "Select"}
              </button>
            </div>
          ) : (
            <div>
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  gap: 12,
                }}
              >
                <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
                  Download size: {sizeLabel(m.sizeMb)}
                </div>
                <button
                  onClick={() => download(m.id)}
                  disabled={downloading || licenseBlocked}
                  title={licenseBlocked ? "Accept the license first" : undefined}
                  style={{ fontSize: 13 }}
                >
                  {downloading
                    ? store.progress[m.id]?.status === "verifying"
                      ? "Verifying…"
                      : store.progress[m.id]
                        ? `${Math.round(store.progress[m.id].percent)}%`
                        : "Downloading…"
                    : "Download"}
                </button>
              </div>
              {store.progress[m.id] && <DownloadProgressBar progress={store.progress[m.id]} />}
              {downloadError?.id === m.id && (
                <div style={{ fontSize: 12, color: "#ef4444", marginTop: 8 }}>
                  {downloadError.message}
                </div>
              )}
              {m.requiresLicenseAccept && !m.downloaded && m.licenseUrl && (
                <div
                  style={{
                    fontSize: 12,
                    marginTop: 10,
                    display: "flex",
                    gap: 10,
                    alignItems: "center",
                    flexWrap: "wrap",
                  }}
                >
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
          )}
        </DetailCard>

        <DetailCard label="Why use this model">
          <p style={{ fontSize: 13, margin: 0, lineHeight: 1.6 }}>{m.description}</p>
          {m.licenseName && (
            <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 8 }}>
              License: {m.licenseName}
            </div>
          )}
        </DetailCard>
      </>
    );
  };

  const renderOpenaiDetail = () => {
    const chip = openaiActive
      ? { tone: "success" as const, label: "In use" }
      : { tone: "neutral" as const, label: "Configure" };
    return (
      <>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            flexWrap: "wrap",
            marginBottom: 16,
          }}
        >
          <h3 style={{ fontSize: 18, fontWeight: 700, margin: 0 }}>OpenAI-compatible API</h3>
          <StatusChip tone={chip.tone} label={chip.label} />
        </div>

        <DetailCard label="Your own setup">
          <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 12 }}>
            Works with OpenAI, OpenRouter, Groq, LM Studio, Ollama, or any custom endpoint that
            speaks <code>/v1/chat/completions</code>.
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
              {activating ? "Connecting…" : openaiActive ? "Reconnect" : "Use this API"}
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
        </DetailCard>

        <DetailCard label="Why use this">
          <p style={{ fontSize: 13, margin: 0, lineHeight: 1.6 }}>
            Routes your transcribed text to any OpenAI-compatible endpoint you choose. Audio never
            leaves this computer — only the text you dictate is sent, to the host you configure
            above.
          </p>
        </DetailCard>
      </>
    );
  };

  const selectedSpeech = selection.startsWith("speech:") ? speechById(selection.slice(7)) : undefined;
  const selectedLlm = selection.startsWith("llm:") ? llmById(selection.slice(4)) : undefined;

  const detail =
    selection === "openai"
      ? renderOpenaiDetail()
      : selectedSpeech
        ? renderSpeechDetail(selectedSpeech)
        : selectedLlm
          ? renderLlmDetail(selectedLlm)
          : null;

  return (
    <div style={{ maxWidth: 960 }}>
      <div style={{ marginBottom: 16 }}>
        <h2 style={{ fontSize: 20, fontWeight: 700 }}>AI Models</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13, marginTop: 4 }}>
          Pick the model that transcribes your dictation, and how your text gets polished.
        </p>
        <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 6 }}>
          <strong style={{ color: "var(--text)" }}>Active transform:</strong>{" "}
          {selectedProvider === "openai-compat"
            ? `API (${openaiModel})`
            : selectedProvider === "local-server"
              ? `Local (${modelStatus || "llama-server"})`
              : "None"}
        </div>
      </div>

      <div style={{ display: "flex", gap: 16, alignItems: "flex-start" }}>
        {/* Left rail */}
        <div style={{ width: 216, flexShrink: 0 }}>
          <RailGroup title="On this computer">
            {onThisComputer.length === 0 ? (
              <div style={{ fontSize: 12, color: "var(--text-secondary)", padding: "2px 10px" }}>
                Nothing downloaded yet.
              </div>
            ) : (
              onThisComputer.map((m) => (
                <RailRow
                  key={m.id}
                  item={speechRail(m)}
                  active={selection === `speech:${m.id}`}
                  onClick={() => selectRail(speechRail(m))}
                />
              ))
            )}
          </RailGroup>

          {available.length > 0 && (
            <RailGroup title="Available to download">
              {available.map((m) => (
                <RailRow
                  key={m.id}
                  item={speechRail(m)}
                  active={selection === `speech:${m.id}`}
                  onClick={() => selectRail(speechRail(m))}
                />
              ))}
            </RailGroup>
          )}

          {models.length > 0 && (
            <RailGroup title="Local LLM">
              {models.map((m) => (
                <RailRow
                  key={m.id}
                  item={llmRail(m)}
                  active={selection === `llm:${m.id}`}
                  onClick={() => selectRail(llmRail(m))}
                />
              ))}
            </RailGroup>
          )}

          <RailGroup title="Cloud / API">
            <RailRow item={openaiRail} active={selection === "openai"} onClick={() => selectRail(openaiRail)} />
          </RailGroup>
        </div>

        {/* Right detail pane */}
        <div style={{ flex: 1, minWidth: 0 }}>
          {detail ?? (
            <div
              style={{
                background: "var(--surface)",
                border: "1px solid var(--border)",
                borderRadius: "var(--radius)",
                padding: 24,
                fontSize: 13,
                color: "var(--text-secondary)",
                textAlign: "center",
              }}
            >
              {speechModels.length === 0
                ? "No speech models available yet."
                : "Select a model on the left to see its details."}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
