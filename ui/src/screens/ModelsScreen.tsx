// The AI Models screen.
//
// Operated as settings, not as a file browser: one column, sections in the
// order a user needs them (dictation, then polish, then the optional engines),
// and every row carries its own action. The rail-plus-detail-pane this replaced
// needed a click on a row before a Download button existed anywhere, so the
// common task, "get me a model that works", took two passes, and it hid the
// state of the eight rows the user was not currently looking at.
//
// Availability is not decided here. Both lists come from the backend already
// filtered for what this host can load (`list_speech_models` for the platform,
// `list_models` for `runsOnHost`); this screen only says plainly what each
// row's state is and what the button next to it does.

import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ReadinessPanel, useRuntimeStatus } from "../lib/runtimeStatus";
import { errorMessage } from "../lib/errors";
import { openExternal } from "../lib/dialogs";
import { DownloadProgressBar } from "../components/DownloadProgress";
import {
  clearDownload,
  loadAcceptedLicenses,
  markDownloading,
  saveAcceptedLicense,
  snapshotDownloads,
  subscribeDownloads,
} from "../lib/downloadStore";
import {
  Chip,
  EmptyState,
  PrimaryButton,
  Row,
  Section,
  SecondaryButton,
  TextInput,
} from "../settings/primitives";

interface ModelStatus {
  id: string;
  name: string;
  sizeMb: number;
  description: string;
  /** What this row is worth choosing for, e.g. "Polish + Prompt Engineer".
      Empty when the description already says it. Mirrors `CatalogEntry::purpose`. */
  purpose: string;
  downloaded: boolean;
  selected: boolean;
  recommended: boolean;
  /** False for Command Mode's decider and the extraction tools: downloadable,
      never a polish engine, so they render in their own section. Mirrors
      `CatalogEntry::polish_capable`; the backend refuses to select them. */
  polishCapable: boolean;
  /** False when this host's llama-server has no GPU backend and the row is too
      large to answer in seconds. Mirrors `CatalogEntry::runs_on_host`. The row
      stays offered (someone with 32 GB of RAM may still want it) but it is
      labelled, and no first run is pointed at it. */
  runsOnHost: boolean;
  verdict: string;
  verdictNote: string;
  licenseName: string | null;
  licenseUrl: string | null;
  requiresLicenseAccept: boolean;
  attribution: string | null;
}

interface SpeechModelStatus {
  id: string;
  name: string;
  engine: string;
  engineLabel: string;
  platformLabel: string;
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

interface OllamaModel {
  name: string;
  size: number;
  modified: string;
  verdict: string;
  verdictNote: string;
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
    return new URL(url).hostname;
  } catch {
    return "openai";
  }
}

/** Re-renders when the app-wide download store changes. */
function useDownloadStore() {
  return useSyncExternalStore(subscribeDownloads, snapshotDownloads);
}

/** In use first, then whatever is already on disk, then the rest in catalog
    order. This replaces the "On this computer" / "Available to download"
    grouping, which cut the same decision in two and left the recommended row
    wherever its group happened to fall. */
function byStatus<T extends { selected: boolean; downloaded: boolean }>(items: T[]): T[] {
  return [...items].sort(
    (a, b) =>
      Number(b.selected) - Number(a.selected) || Number(b.downloaded) - Number(a.downloaded),
  );
}

function StatusChip({ selected, downloaded }: { selected: boolean; downloaded: boolean }) {
  if (selected) return <Chip tone="success">In use</Chip>;
  if (downloaded) return <Chip tone="neutral">Downloaded</Chip>;
  return null;
}

/** The one control a row is about: use what is here, or fetch what is not. */
function ActionButton({
  selected,
  downloaded,
  busy,
  downloading,
  percent,
  verifying,
  licenseBlocked,
  onUse,
  onDownload,
}: {
  selected: boolean;
  downloaded: boolean;
  busy: boolean;
  downloading: boolean;
  percent: number | null;
  verifying: boolean;
  licenseBlocked: boolean;
  onUse: () => void;
  onDownload: () => void;
}) {
  if (selected) return null;
  if (downloaded) {
    return (
      <PrimaryButton onClick={onUse} disabled={busy}>
        {busy ? "Starting…" : "Use"}
      </PrimaryButton>
    );
  }
  if (downloading) {
    return (
      <PrimaryButton disabled onClick={() => {}}>
        {verifying ? "Verifying…" : percent !== null ? `${percent}%` : "Downloading…"}
      </PrimaryButton>
    );
  }
  if (licenseBlocked) {
    return (
      <SecondaryButton disabled onClick={() => {}}>
        Download
      </SecondaryButton>
    );
  }
  return <SecondaryButton onClick={onDownload}>Download</SecondaryButton>;
}

export default function ModelsScreen() {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [speechModels, setSpeechModels] = useState<SpeechModelStatus[]>([]);
  const store = useDownloadStore();
  const [downloadError, setDownloadError] = useState<{ id: string; message: string } | null>(null);
  const [selectingId, setSelectingId] = useState<string | null>(null);
  // Real runtime state for both models. Polled coarsely; this screen is the
  // one place the user goes specifically to answer "what is loaded?", so it
  // must not be a mount-time snapshot.
  const runtime = useRuntimeStatus();

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

  // Ollama state
  const [ollamaRunning, setOllamaRunning] = useState(false);
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([]);
  const [ollamaChecking, setOllamaChecking] = useState(true);
  // The OpenAI form is collapsed by default: it is a power-user path, and
  // rendering it on every visit to the Models screen made the page feel busy
  // before the two rows every user actually needs (dictation + polish) could
  // be read.
  const [showCloudForm, setShowCloudForm] = useState(false);

  useEffect(() => {
    refresh();
    invoke<boolean>("detect_ollama")
      .then((running) => {
        setOllamaRunning(running);
        if (running) {
          invoke<OllamaModel[]>("list_ollama_models")
            .then(setOllamaModels)
            .catch(console.error);
        }
        setOllamaChecking(false);
      })
      .catch(() => {
        setOllamaRunning(false);
        setOllamaChecking(false);
      });
    invoke<{ selectedLlmProvider: string; openaiBaseUrl: string; openaiModel: string }>(
      "get_settings",
    )
      .then((s) => {
        setSelectedProvider(s.selectedLlmProvider || "");
        setOpenaiBaseUrl(s.openaiBaseUrl);
        setOpenaiModel(s.openaiModel);
      })
      .catch(console.error);
    invoke<boolean>("has_llm_secret", { providerId: hostFromUrl(openaiBaseUrl) })
      .then(setHasKey)
      .catch(console.error);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Both catalogs are stored in one batch. Resolved separately, whichever
  // `invoke` returned first painted a half-populated screen, and the two lists
  // landed on different ticks.
  const refresh = () =>
    Promise.all([
      invoke<ModelStatus[]>("list_models").catch((e) => {
        console.error(e);
        return [] as ModelStatus[];
      }),
      invoke<SpeechModelStatus[]>("list_speech_models").catch((e) => {
        console.error(e);
        return [] as SpeechModelStatus[];
      }),
    ]).then(([llm, speech]) => {
      setModels(llm);
      setSpeechModels(speech);
    });

  const validateBaseUrl = (): string | null => {
    if (!openaiBaseUrl.trim()) return "Enter a base URL.";
    try {
      new URL(openaiBaseUrl.trim());
      return null;
    } catch {
      return "That is not a URL. Use the full endpoint, e.g. https://api.openai.com/v1";
    }
  };

  const saveOpenaiSettings = async () => {
    const err = validateBaseUrl();
    if (err) {
      setTestResult({ ok: false, message: err });
      return;
    }
    const settings = await invoke<Record<string, unknown>>("get_settings");
    await invoke("save_settings", {
      settings: {
        ...settings,
        openaiBaseUrl: openaiBaseUrl.trim(),
        openaiModel,
      },
    });
  };

  const saveApiKey = async () => {
    if (!apiKey.trim()) return;
    const urlErr = validateBaseUrl();
    if (urlErr) {
      setTestResult({ ok: false, message: urlErr });
      return;
    }
    setSavingKey(true);
    setTestResult(null);
    try {
      await invoke("set_llm_secret", {
        providerId: hostFromUrl(openaiBaseUrl.trim()),
        secret: apiKey.trim(),
      });
      setHasKey(true);
      setApiKey("");
      setTestResult({ ok: true, message: "Key saved in the macOS keychain." });
    } catch (e) {
      setTestResult({ ok: false, message: errorMessage(e) });
    }
    setSavingKey(false);
  };

  const clearApiKey = async () => {
    try {
      await invoke("clear_llm_secret", { providerId: hostFromUrl(openaiBaseUrl) });
      setHasKey(false);
      setTestResult({ ok: true, message: "Key removed." });
    } catch (e) {
      setTestResult({ ok: false, message: errorMessage(e) });
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
      setTestResult({ ok: false, message: errorMessage(e) });
    }
    setTesting(false);
  };

  const connectOpenai = async () => {
    setActivating(true);
    setTestResult(null);
    try {
      if (apiKey.trim()) {
        await invoke("set_llm_secret", {
          providerId: hostFromUrl(openaiBaseUrl),
          secret: apiKey.trim(),
        });
        setHasKey(true);
        setApiKey("");
      }
      await saveOpenaiSettings();
      await invoke("select_openai_provider");
      setSelectedProvider("openai-compat");
      setTestResult({ ok: true, message: "Connected." });
      refresh();
    } catch (e) {
      setTestResult({ ok: false, message: errorMessage(e) });
    }
    setActivating(false);
  };

  const selectSpeech = async (id: string) => {
    try {
      await invoke("select_speech_model", { id });
    } catch (e) {
      setDownloadError({ id, message: errorMessage(e) });
    }
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
      const msg = errorMessage(e);
      if (!msg.toLowerCase().includes("cancel")) {
        setDownloadError({ id, message: msg });
      }
      refresh();
      return;
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
      setDownloadError({ id, message: errorMessage(e) });
    } finally {
      setSelectingId(null);
    }
  };

  const selectOllamaModel = async (modelName: string) => {
    // Pre-fill the OpenAI-compat settings with Ollama's endpoint and select it.
    const settings = await invoke<Record<string, unknown>>("get_settings");
    await invoke("save_settings", {
      settings: { ...settings, openaiBaseUrl: "http://127.0.0.1:11434/v1", openaiModel: modelName },
    });
    await invoke("select_openai_provider").catch(console.error);
    setSelectedProvider("openai-compat");
    refresh();
  };

  const polishModels = models.filter((m) => m.polishCapable);
  // Three polish rows and two dictation rows carry `recommended: true`, so the
  // badge used to sit on several rows at once and stopped meaning "start here".
  // One row wears it: the first in catalog order that this host can run.
  const polishLead = polishModels.find((m) => m.recommended && m.runsOnHost)?.id;
  const speechLead = speechModels.find((m) => m.recommended)?.id;
  // Quantisations of one publisher's model share a credit line and sit next to
  // each other in the list, so a plain per-row join printed the same name twice.
  const attributions = [
    ...new Set(
      models
        .filter((m) => m.polishCapable)
        .map((m) => m.attribution)
        .filter((a): a is string => Boolean(a)),
    ),
  ];
  const toolModels = models.filter((m) => !m.polishCapable);
  // Ollama is reached through the same OpenAI-compatible provider, so the
  // loopback host is what tells "an Ollama model is in use" apart from "a cloud
  // endpoint is in use". Without it both rows claim In use at once.
  const usingOllama =
    selectedProvider === "openai-compat" && hostFromUrl(openaiBaseUrl) === "127.0.0.1";
  const cloudInUse = selectedProvider === "openai-compat" && !usingOllama;

  const rowAction = (m: { id: string; downloaded: boolean; selected: boolean }, busy = false) => {
    const p = store.progress[m.id];
    return {
      busy,
      downloading: store.active.includes(m.id),
      percent: p ? Math.round(p.percent) : null,
      verifying: p?.status === "verifying",
    };
  };

  /** The block under a row: the license gate, live progress, or the failure that
      stopped it. Rendered only when there is something to say, so a quiet list
      stays a quiet list. */
  const rowBody = (
    id: string,
    opts: { gated: boolean; licenseName: string | null; licenseUrl: string | null },
  ) => {
    const p = store.progress[id];
    const err = downloadError?.id === id ? downloadError.message : null;
    const needLicense = opts.gated && !acceptedLicenses[id];
    if (!p && !err && !needLicense) return null;
    return (
      <div style={{ padding: "0 14px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
        {needLicense && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              flexWrap: "wrap",
              fontSize: "var(--text-xs)",
              color: "var(--text-secondary)",
            }}
          >
            <label style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={acceptedLicenses[id] === true}
                onChange={(e) => toggleLicense(id, e.target.checked)}
              />
              I accept the terms
            </label>
            {opts.licenseUrl && (
              <a
                href={opts.licenseUrl}
                onClick={(e) => {
                  e.preventDefault();
                  openExternal(opts.licenseUrl as string);
                }}
                style={{ color: "var(--accent)" }}
              >
                Read the {opts.licenseName ?? "license"}
              </a>
            )}
          </div>
        )}
        {p && <DownloadProgressBar progress={p} />}
        {err && <div style={{ fontSize: "var(--text-xs)", color: "var(--danger)" }}>{err}</div>}
      </div>
    );
  };

  return (
    <div style={{ width: "100%", maxWidth: 860 }}>
      <div style={{ marginBottom: 16 }}>
        <h2 className="tt-screen-title">AI Models</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13, marginTop: 4 }}>
          A dictation model turns your voice into text; a polish model cleans it up. Both run on
          this computer.
        </p>
      </div>

      {/* Replaces a static "Active transform: Local (<status>)" line.
          `get_model_status` returned one String fetched once on mount, so this
          used to render "not loaded (loads 'x' on first use)" — derived from
          settings, not from whether anything was actually running — under a
          bold "Active transform" label, with no way to tell the two apart.
          Both models are now named with the sentence the backend supplies. */}
      <div style={{ marginBottom: 20, maxWidth: 520 }}>
        <ReadinessPanel
          status={runtime}
          onFix={() =>
            document
              .getElementById("ai-models-list")
              ?.scrollIntoView({ behavior: "smooth", block: "start" })
          }
        />
      </div>

      {/* `Section` already carries the 28px rhythm in its own margin, so this
          column must not add a gap on top of it. */}
      <div id="ai-models-list">
        <Section
          title="Dictation models"
          hint="One model transcribes at a time. The row marked In use is the one running now; choosing another takes effect on your next recording."
        >
          {speechModels.length === 0 ? (
            <EmptyState>No dictation model can run on this computer.</EmptyState>
          ) : (
            byStatus(speechModels).map((m, i) => (
              <div key={m.id}>
                <Row
                  first={i === 0}
                  label={m.name}
                  hint={`${sizeLabel(m.sizeMb)} · ${speedLabel(m.speed)} · ${accuracyLabel(
                    m.accuracy,
                  )} · ${m.languageLabel}`}
                >
                  <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    {m.id === speechLead && !m.selected && (
                      <Chip tone="success">Recommended</Chip>
                    )}
                    <StatusChip selected={m.selected} downloaded={m.downloaded} />
                    <ActionButton
                      selected={m.selected}
                      downloaded={m.downloaded}
                      licenseBlocked={false}
                      {...rowAction(m)}
                      onUse={() => void selectSpeech(m.id)}
                      onDownload={() => void download(m.id, true)}
                    />
                  </span>
                </Row>
                {rowBody(m.id, { gated: false, licenseName: null, licenseUrl: null })}
              </div>
            ))
          )}
        </Section>

        <Section
          title="Polish models"
          hint="Cleans up grammar, fillers and spoken punctuation. A row marked Polish + Prompt Engineer also tightens, rewords and structures the text and runs your transform presets; the cleanup-only rows cannot. Nothing is uploaded."
        >
          {polishModels.length === 0 ? (
            <EmptyState>No polish model is available for this computer.</EmptyState>
          ) : (
            byStatus(polishModels).map((m, i) => {
              const inUse = m.selected && !cloudInUse;
              const gated = m.requiresLicenseAccept && !acceptedLicenses[m.id] && !m.downloaded;
              return (
                <div key={m.id}>
                  <Row
                    first={i === 0}
                    label={m.name}
                    hint={`${sizeLabel(m.sizeMb)} · ${m.verdictNote || m.description}`}
                  >
                    <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      {m.purpose && <Chip tone="neutral">{m.purpose}</Chip>}
                      {!m.runsOnHost && <Chip tone="warning">Slow here</Chip>}
                      {m.id === polishLead && !inUse && (
                        <Chip tone="success">Recommended</Chip>
                      )}
                      <StatusChip selected={inUse} downloaded={m.downloaded} />
                      <ActionButton
                        selected={inUse}
                        downloaded={m.downloaded}
                        licenseBlocked={gated}
                        {...rowAction(m, selectingId !== null)}
                        onUse={() => void selectLocal(m.id)}
                        onDownload={() => void download(m.id)}
                      />
                    </span>
                  </Row>
                  {rowBody(m.id, {
                    gated: m.requiresLicenseAccept && !m.downloaded,
                    licenseName: m.licenseName,
                    licenseUrl: m.licenseUrl,
                  })}
                </div>
              );
            })
          )}
          {attributions.length > 0 && (
            <div
              style={{
                padding: "10px 14px",
                fontSize: "var(--text-xs)",
                color: "var(--text-tertiary)",
                borderTop: "1px solid var(--border-subtle)",
                lineHeight: 1.5,
              }}
            >
              Weights come from their publishers, under the license each one ships with:{" "}
              {attributions.join(" · ")}
            </div>
          )}
        </Section>

        {toolModels.length > 0 && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: "0 14px",
              marginBottom: 28,
              fontSize: "var(--text-xs)",
              color: "var(--text-tertiary)",
            }}
          >
            <span>
              Command Mode uses:{" "}
              <strong style={{ color: "var(--text-secondary)" }}>
                {toolModels[0].name}
              </strong>{" "}
              {toolModels[0].downloaded ? "· installed" : "· not downloaded"}
            </span>
            {!toolModels[0].downloaded && (
              <SecondaryButton
                onClick={() => void download(toolModels[0].id)}
                disabled={store.active.includes(toolModels[0].id)}
              >
                {store.active.includes(toolModels[0].id)
                  ? `${Math.round(store.progress[toolModels[0].id]?.percent ?? 0)}%`
                  : "Download"}
              </SecondaryButton>
            )}
          </div>
        )}

        <Section
          title="Other engines"
          hint="A model already running on this machine, or an API you bring a key to. Picking one stops polish from using a local model."
        >
          {ollamaChecking ? (
            <Row first label="Ollama" hint="Checking for a local Ollama server…" />
          ) : ollamaRunning && ollamaModels.length > 0 ? (
            ollamaModels.map((o, i) => {
              const inUse = usingOllama && openaiModel === o.name;
              return (
                <Row
                  key={o.name}
                  first={i === 0}
                  label={o.name}
                  hint={`Ollama · ${sizeLabel(Math.round(o.size / 1e6))} · ${o.verdictNote || o.verdict}`}
                >
                  <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    {inUse && <Chip tone="success">In use</Chip>}
                    {!inUse && (
                      <SecondaryButton onClick={() => void selectOllamaModel(o.name)}>
                        Use
                      </SecondaryButton>
                    )}
                  </span>
                </Row>
              );
            })
          ) : (
            <Row
              first
              label="Ollama"
              hint={
                ollamaRunning
                  ? "Running, but it has no models. Pull one, then reload this screen."
                  : "Not running. Start Ollama and its models show up here."
              }
            />
          )}

          <Row
            label="OpenAI-compatible API"
            hint="Any endpoint that speaks /v1/chat/completions. Your text is sent to it."
          >
            <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
              {cloudInUse && <Chip tone="success">In use</Chip>}
              {!cloudInUse && hasKey && <Chip tone="neutral">Key saved</Chip>}
              <SecondaryButton onClick={() => setShowCloudForm((v) => !v)}>
                {showCloudForm ? "Hide" : "Configure"}
              </SecondaryButton>
            </span>
          </Row>

          {showCloudForm && (
          <div
            style={{
              padding: "12px 14px 14px",
              display: "flex",
              flexDirection: "column",
              gap: 8,
              borderTop: "1px solid var(--border-subtle)",
            }}
          >
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
              <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>
                Presets
              </span>
              {[
                { label: "OpenAI", url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
                {
                  label: "Google Gemini",
                  url: "https://generativelanguage.googleapis.com/v1beta/openai",
                  model: "gemini-2.0-flash",
                },
              ].map((p) => (
                <SecondaryButton
                  key={p.label}
                  onClick={() => {
                    setOpenaiBaseUrl(p.url);
                    setOpenaiModel(p.model);
                    setTestResult(null);
                  }}
                >
                  {p.label}
                </SecondaryButton>
              ))}
            </div>
            <TextInput
              label="Base URL"
              value={openaiBaseUrl}
              onChange={(v) => {
                setOpenaiBaseUrl(v);
                setTestResult(null);
              }}
              placeholder="https://api.openai.com/v1"
              width={320}
            />
            <TextInput
              label="Model id"
              value={openaiModel}
              onChange={(v) => {
                setOpenaiModel(v);
                setTestResult(null);
              }}
              placeholder="gpt-4o-mini"
              width={320}
            />
            <div style={{ display: "flex", gap: 6, alignItems: "center", flexWrap: "wrap" }}>
              <TextInput
                label="API key"
                type="password"
                value={apiKey}
                onChange={(v) => {
                  setApiKey(v);
                  setTestResult(null);
                }}
                placeholder={hasKey ? "Saved. Type to replace it." : "API key (optional for local)"}
                width={220}
              />
              {!!apiKey.trim() && (
                <SecondaryButton onClick={() => void saveApiKey()} disabled={savingKey}>
                  {savingKey ? "Saving…" : "Save key"}
                </SecondaryButton>
              )}
              {hasKey && (
                <SecondaryButton onClick={() => void clearApiKey()} danger>
                  Remove key
                </SecondaryButton>
              )}
            </div>
            <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
              <SecondaryButton onClick={() => void testConnection()} disabled={testing}>
                {testing ? "Testing…" : "Test connection"}
              </SecondaryButton>
              <PrimaryButton onClick={() => void connectOpenai()} disabled={activating}>
                {activating ? "Connecting…" : cloudInUse ? "Reconnect" : "Use this API"}
              </PrimaryButton>
            </div>
            {testResult && (
              <div
                style={{
                  fontSize: "var(--text-xs)",
                  color: testResult.ok ? "var(--success)" : "var(--danger)",
                }}
              >
                {testResult.message}
              </div>
            )}
          </div>
          )}
        </Section>
      </div>
    </div>
  );
}
