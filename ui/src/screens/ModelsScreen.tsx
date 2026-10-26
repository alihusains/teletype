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
import { useNow } from "../lib/useNow";
import { DownloadProgressBar } from "../components/DownloadProgress";
import {
  clearDownload,
  finishDownload,
  loadAcceptedLicenses,
  markDownloading,
  saveAcceptedLicense,
  snapshotDownloads,
  subscribeDownloads,
} from "../lib/downloadStore";
import { Icon } from "../components/Icon";
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
      Mirrors `CatalogEntry::specialty`. `measured` separates a role this repo's
      bakeoff proved from one inferred from the family or the publisher. */
  specialty: { tag: string; measured: boolean };
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

/** The system polish model. `ModelStatus` carries no system flag, so the id is
    the test: it is the host's own model, always present, never downloadable and
    never deletable. */
const isSystemModel = (id: string) => id === "apple-intelligence";

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

/** The row's primary action. The system model is use-only: it has no download
    to offer, so it does not fall through to a Download control. */
function ActionButton({
  selected,
  downloaded,
  useOnly,
  busy,
  busyLabel,
  disabled,
  downloading,
  percent,
  verifying,
  licenseBlocked,
  onUse,
  onDownload,
}: {
  selected: boolean;
  downloaded: boolean;
  /** True for the system model: a Use control is the only action it has. */
  useOnly?: boolean;
  /** This row's own select is in flight. */
  busy: boolean;
  /** What "busy" says, e.g. "Starting (12s)". */
  busyLabel?: string;
  /** Another row's select is in flight, so this control is locked. */
  disabled?: boolean;
  downloading: boolean;
  percent: number | null;
  verifying: boolean;
  licenseBlocked: boolean;
  onUse: () => void;
  onDownload: () => void;
}) {
  if (selected) return null;
  if (downloaded || useOnly) {
    return (
      <PrimaryButton onClick={onUse} disabled={busy || disabled}>
        {busy ? busyLabel ?? "Starting…" : "Use"}
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

function StatusChip({ selected, downloaded }: { selected: boolean; downloaded: boolean }) {
  if (selected) return <Chip tone="success">In use</Chip>;
  if (downloaded) return <Chip tone="neutral">Downloaded</Chip>;
  return null;
}

/** The two-tap delete control, rendered beside a row's primary action.
    Hidden until the model is downloaded; arms on the first tap, deletes on
    the second. `deleting` shows the in-flight state and locks the control. */
function DeleteButton({
  visible,
  armed,
  deleting,
  disabled,
  onArm,
  onConfirm,
}: {
  visible: boolean;
  armed: boolean;
  deleting: boolean;
  disabled: boolean;
  onArm: () => void;
  onConfirm: () => void;
}) {
  if (!visible) return null;
  if (deleting) {
    return (
      <SecondaryButton disabled onClick={() => {}}>
        Deleting…
      </SecondaryButton>
    );
  }
  const label = armed ? "Confirm delete" : "Delete model";
  return (
    <SecondaryButton
      danger={armed}
      onClick={armed ? onConfirm : onArm}
      disabled={disabled}
    >
      <Icon name="trash" size={14} />
      <span
        aria-live="polite"
        style={{ position: "absolute", width: 1, height: 1, overflow: "hidden", clip: "rect(0 0 0 0)" }}
      >
        {label}
      </span>
    </SecondaryButton>
  );
}

export default function ModelsScreen() {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [speechModels, setSpeechModels] = useState<SpeechModelStatus[]>([]);
  const store = useDownloadStore();
  const [downloadError, setDownloadError] = useState<{ id: string; message: string } | null>(null);
  // A row whose model-list fetch failed. Without this, a rejected
  // `list_models`/`list_speech_models` painted a blank list that read as "no
  // models" and gave the user nothing to retry.
  const [catalogError, setCatalogError] = useState<string | null>(null);
  // Two-tap delete: the row the user has armed for deletion. A second tap
  // within the window deletes; any other interaction disarms it.
  const [confirmingDelete, setConfirmingDelete] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState<{ id: string; message: string } | null>(null);
  const [selectingId, setSelectingId] = useState<string | null>(null);
  // When the select started, in ms. `select_model` blocks for the whole
  // warm-up (it stops the old server, spawns the new one and waits for it to
  // answer), which can be a minute on a CPU host, so the row counts up instead
  // of sitting on a motionless "Starting…".
  const [selectingSince, setSelectingSince] = useState(0);
  const selectingNow = useNow(1000, selectingId !== null);
  // Downloads whose `done` event has been retired by a catalog read.
  const [finishedIds, setFinishedIds] = useState<Set<string>>(() => new Set());
  // Real runtime state for both models. Polled coarsely; this screen is the
  // one place the user goes specifically to answer "what is loaded?", so it
  // must not be a mount-time snapshot.
  const runtime = useRuntimeStatus().status;

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
  // Set when `detect_ollama` rejects. The section still renders its fallback
  // rows; this only replaces "not running" with the reason it is unknown.
  const [ollamaError, setOllamaError] = useState<string | null>(null);
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
      .catch((e) => {
        setOllamaRunning(false);
        setOllamaError(errorMessage(e));
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
        setCatalogError(errorMessage(e));
        return [] as ModelStatus[];
      }),
      invoke<SpeechModelStatus[]>("list_speech_models").catch((e) => {
        console.error(e);
        setCatalogError(errorMessage(e));
        return [] as SpeechModelStatus[];
      }),
    ]).then(([llm, speech]) => {
      if (llm.length || speech.length) setCatalogError(null);
      setModels(llm);
      setSpeechModels(speech);
      return llm;
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
    setConfirmingDelete(null);
    try {
      await invoke("select_speech_model", { id });
    } catch (e) {
      setDownloadError({ id, message: errorMessage(e) });
    }
    refresh();
  };

  const download = async (id: string, speech = false) => {
    setConfirmingDelete(null);
    markDownloading(id);
    setDownloadError(null);
    try {
      await invoke(speech ? "download_speech_model" : "download_model", {
        id,
        ...(speech ? {} : { licenseAccepted: acceptedLicenses[id] === true }),
      });
    } catch (e) {
      clearDownload(id);
      const msg = errorMessage(e);
      if (!msg.toLowerCase().includes("cancel")) {
        setDownloadError({ id, message: msg });
      }
      refresh();
      return;
    }
    // Success does not clear the store row here. The backend's `done` event
    // arrives either side of this reply, and clearing on the reply dropped the
    // bar a beat before the catalog knew the file existed. The `done` row holds
    // "Finishing…" until one catalog read returns (see below).
    refresh();
  };

  const toggleLicense = (id: string, checked: boolean) => {
    setAcceptedLicenses((prev) => ({ ...prev, [id]: checked }));
    saveAcceptedLicense(id, checked);
  };

  // Two-tap delete. The first tap only arms the row (turns the control red and
  // says "Delete? confirm"); it deletes nothing, so a misclick frees no bytes.
  // A second tap within the window deletes; any other interaction disarms it.
  const armDelete = (id: string) => {
    if (deletingId) return;
    setConfirmingDelete(id);
    setDeleteError(null);
    window.setTimeout(() => setConfirmingDelete((cur) => (cur === id ? null : cur)), 4000);
  };

  const cancelDelete = () => setConfirmingDelete(null);

  const doDelete = async (id: string, speech: boolean) => {
    setConfirmingDelete(null);
    setDeletingId(id);
    setDeleteError(null);
    try {
      await invoke(speech ? "delete_speech_model" : "delete_model", { id });
    } catch (e) {
      setDeletingId(null);
      setDeleteError({ id, message: errorMessage(e) });
      return;
    }
    setDeletingId(null);
    refresh();
  };

  const selectLocal = async (id: string) => {
    if (selectingId) return;
    setConfirmingDelete(null);
    const startedAt = Date.now();
    setSelectingId(id);
    setSelectingSince(startedAt);
    setDownloadError(null);
    try {
      await invoke("select_model", { id });
      setSelectedProvider("local-server");
      refresh();
    } catch (e) {
      setDownloadError({ id, message: errorMessage(e) });
    } finally {
      // The label floor exists for a near-instant reply (an already-warm
      // server): without it "Starting…" appears and vanishes inside one frame.
      // The warm-up itself is normally far longer, so this only ever adds a
      // beat to a fast path.
      const left = 1200 - (Date.now() - startedAt);
      if (left > 0) await new Promise((r) => setTimeout(r, left));
      setSelectingId(null);
      setSelectingSince(0);
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

  const rowAction = (m: { id: string; downloaded: boolean; selected: boolean }) => {
    const p = store.progress[m.id];
    return {
      downloading: store.active.includes(m.id),
      percent: p ? Math.round(p.percent) : null,
      verifying: p?.status === "verifying",
    };
  };

  // A download retires on the backend's `done` progress event, not on the
  // `download_model` reply. The row holds "Finishing…" (the bar's own `done`
  // label) until one catalog read returns, then becomes the Downloaded chip.
  // Clearing on the reply instead left the row neither downloading nor
  // downloaded for a frame or two, and the chip appeared before the file did.
  const finishedKeys = Object.entries(store.progress)
    .filter(([, p]) => p.status === "done")
    .map(([id]) => id)
    .join(",");
  useEffect(() => {
    if (!finishedKeys) return;
    let alive = true;
    const ids = finishedKeys.split(",");
    // Hold the finishing label for a beat. The catalog read below usually takes
    // longer than this; the floor only stops a fast read from retiring the row
    // before "Finishing…" has been painted once.
    const timer = window.setTimeout(() => {
      void refresh().then(() => {
        if (!alive) return;
        setFinishedIds((prev) => {
          const next = new Set(prev);
          for (const id of ids) next.add(id);
          return next;
        });
        for (const id of ids) finishDownload(id);
      });
    }, 400);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
    // `refresh` is a fresh closure each render; the key is what changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [finishedKeys]);

  /** A download the backend reported done, whether or not the catalog has
      caught up yet. */
  const isDownloaded = (m: { id: string; downloaded: boolean }) =>
    m.downloaded || finishedIds.has(m.id);

  /** How long this row has been starting, in whole seconds, or null when it
      is not the row being started. */
  const elapsedSecs = (id: string): number | null => {
    if (selectingId !== id || selectingSince === 0) return null;
    return Math.max(0, Math.floor((selectingNow - selectingSince) / 1000));
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
    const delErr = deleteError?.id === id ? deleteError.message : null;
    const needLicense = opts.gated && !acceptedLicenses[id];
    if (!p && !err && !delErr && !needLicense) return null;
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
        {delErr && (
          <div style={{ fontSize: "var(--text-xs)", color: "var(--danger)" }}>
            {delErr}
            <button
              type="button"
              onClick={cancelDelete}
              style={{
                marginLeft: 8,
                color: "var(--text-secondary)",
                background: "none",
                border: "none",
                cursor: "pointer",
                fontSize: "var(--text-xs)",
              }}
            >
              Dismiss
            </button>
          </div>
        )}
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

      {catalogError && (
        <div
          style={{
            marginBottom: 16,
            padding: "10px 14px",
            display: "flex",
            alignItems: "center",
            gap: 12,
            fontSize: "var(--text-sm)",
            color: "var(--danger)",
            border: "1px solid var(--danger)",
            borderRadius: "var(--radius-sm)",
          }}
        >
          <span style={{ flex: 1 }}>Could not load the model list: {catalogError}</span>
          <SecondaryButton danger onClick={() => void refresh()}>
            Retry
          </SecondaryButton>
        </div>
      )}

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
                    <StatusChip selected={m.selected} downloaded={isDownloaded(m)} />
                    <ActionButton
                      selected={m.selected}
                      downloaded={isDownloaded(m)}
                      busy={false}
                      disabled={selectingId !== null}
                      licenseBlocked={false}
                      {...rowAction(m)}
                      onUse={() => void selectSpeech(m.id)}
                      onDownload={() => void download(m.id, true)}
                    />
                    <DeleteButton
                      visible={isDownloaded(m)}
                      armed={confirmingDelete === m.id}
                      deleting={deletingId === m.id}
                      disabled={
                        selectingId !== null ||
                        deletingId !== null ||
                        store.active.includes(m.id)
                      }
                      onArm={() => armDelete(m.id)}
                      onConfirm={() => void doDelete(m.id, true)}
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
              const downloaded = isDownloaded(m);
              const system = isSystemModel(m.id);
              // The system model ships with the host: it is never downloaded, so
              // there are no bytes to delete and nothing to fetch. It is a
              // polish/LLM row, so it renders here, not in a section of its own.
              const gated = !system && m.requiresLicenseAccept && !acceptedLicenses[m.id] && !downloaded;
              const busy = selectingId === m.id;
              const secs = elapsedSecs(m.id);
              const slowHint =
                busy && !m.runsOnHost
                  ? " · this model does not run natively on this computer, so it can take a while to start"
                  : "";
              // The system model has no size and no measured speed, accuracy or
              // language, so the usual "size · speed · accuracy · language" hint
              // would print a meaningless "0 MB". Its own description says what
              // it is instead.
              const hint = system
                ? m.description
                : `${sizeLabel(m.sizeMb)} · ${m.verdictNote || m.description}${slowHint}`;
              return (
                <div key={m.id}>
                  <Row first={i === 0} label={m.name} hint={hint}>
                    <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      {!system && m.specialty && (
                        <Chip tone="neutral">
                          {m.specialty.tag}
                          {m.specialty.measured && (
                            <span
                              style={{
                                marginLeft: 5,
                                color: "var(--text-tertiary)",
                                fontWeight: 400,
                              }}
                            >
                              · measured
                            </span>
                          )}
                        </Chip>
                      )}
                      {!system && !m.runsOnHost && <Chip tone="warning">Slow here</Chip>}
                      {(m.id === polishLead || system) && !inUse && (
                        <Chip tone="success">Recommended</Chip>
                      )}
                      <StatusChip selected={inUse} downloaded={downloaded} />
                      <ActionButton
                        selected={inUse}
                        downloaded={downloaded}
                        useOnly={system}
                        busy={busy}
                        busyLabel={secs !== null ? `Starting (${secs}s)` : undefined}
                        disabled={selectingId !== null && !busy}
                        licenseBlocked={gated}
                        {...rowAction(m)}
                        onUse={() => void selectLocal(m.id)}
                        onDownload={() => void download(m.id)}
                      />
                      <DeleteButton
                        visible={!system && downloaded}
                        armed={confirmingDelete === m.id}
                        deleting={deletingId === m.id}
                        disabled={
                          selectingId !== null ||
                          deletingId !== null ||
                          store.active.includes(m.id)
                        }
                        onArm={() => armDelete(m.id)}
                        onConfirm={() => void doDelete(m.id, false)}
                      />
                    </span>
                  </Row>
                  {rowBody(m.id, {
                    gated: m.requiresLicenseAccept && !downloaded,
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
              {isDownloaded(toolModels[0]) ? "· installed" : "· not downloaded"}
            </span>
            {!isDownloaded(toolModels[0]) && (
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
                ollamaError
                  ? `Could not check for a local Ollama server: ${ollamaError}. An API endpoint below still works.`
                  : ollamaRunning
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
