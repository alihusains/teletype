// claim: ui-behavior
/**
 * The Models screen's open states: a `select_model` that is running says how
 * long it has been running (and warns when this host cannot run the model
 * natively), a download that finished holds a "finishing" state until the
 * refreshed catalog confirms `downloaded: true`, and a failed `detect_ollama`
 * settles the list instead of holding it behind "Loading models…".
 *
 * Reuses `modelsScreen.test.tsx`'s mocking style (`tauriMock` helpers, the
 * same catalog fixtures) rather than inventing a new one.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import ModelsScreen from "../screens/ModelsScreen";
import { invoke } from "@tauri-apps/api/core";
import {
  emitTauriEvent,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";
import * as downloadStore from "../lib/downloadStore";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const S1_MINI = {
  id: "s1-mini",
  name: "S1-mini",
  sizeMb: 380,
  description: "Small polish model, tuned on our Polish corpus.",
  specialty: { tag: "Dictation cleanup", measured: true },
  downloaded: true,
  selected: true,
  recommended: true,
  polishCapable: true,
  runsOnHost: true,
  verdict: "Recommended",
  verdictNote: "Best measured polish model.",
  licenseName: null,
  licenseUrl: null,
  requiresLicenseAccept: false,
  attribution: "S1-mini by Superwhisper",
};

const QWEN = {
  ...S1_MINI,
  id: "qwen2.5-3b",
  name: "Qwen2.5 3B",
  sizeMb: 1900,
  description: "Best local model in our tests.",
  specialty: { tag: "Instruction-following transforms", measured: false },
  downloaded: false,
  selected: false,
  runsOnHost: false,
  verdictNote: "Best in our tests, may follow dictated instructions",
  attribution: null,
};

const NEUDECIDE = {
  ...S1_MINI,
  id: "neudecide-q4",
  name: "NeuDecide Q4 (decider)",
  sizeMb: 41,
  downloaded: true,
  selected: false,
  recommended: false,
  polishCapable: false,
  verdict: "Not tested",
  verdictNote: "",
  licenseName: "Apache-2.0",
  licenseUrl: "https://example.invalid/license",
  attribution: null,
};

const ULTRA = {
  id: "parakeet-ultra",
  name: "Parakeet Ultra",
  engine: "parakeet-cpp",
  engineLabel: "parakeet.cpp",
  platformLabel: "Apple Silicon",
  sizeMb: 640,
  description: "Fast on-device transcription.",
  recommended: true,
  englishOnly: true,
  downloaded: true,
  selected: true,
  speed: 9.6,
  accuracy: 9.1,
  minRamGb: 8,
  languageLabel: "English",
};

const WHISPER = {
  ...ULTRA,
  id: "large-v3-turbo-q5",
  name: "Whisper large-v3 turbo",
  engine: "whisper",
  engineLabel: "whisper.cpp",
  platformLabel: "macOS, Windows",
  sizeMb: 547,
  recommended: false,
  englishOnly: false,
  downloaded: false,
  selected: false,
  speed: 7.5,
  accuracy: 9.4,
  languageLabel: "99 languages",
};

function progressPayload(overrides: Record<string, unknown>): Record<string, unknown> {
  return {
    id: "qwen2.5-3b",
    kind: "llm",
    status: "downloading",
    fileName: "",
    fileIndex: 0,
    fileCount: 1,
    fileDownloadedBytes: 0,
    fileTotalBytes: 0,
    downloadedBytes: 0,
    totalBytes: 2_000_000_000,
    percent: 42.5,
    speedBps: 0,
    etaSeconds: null,
    error: null,
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
  localStorage.clear();
  vi.useRealTimers();
});

function renderModels(overrides: Record<string, unknown> = {}) {
  mockListenCapture();
  // The bar renders from the shared download store, which App starts once in
  // production. Start it here against the mocked `listen` so progress events
  // reach the same store the screen subscribes to.
  downloadStore.resetDownloadStoreForTests();
  downloadStore.startDownloadStore();
  mockInvokeResponses({
    list_models: [S1_MINI, QWEN, NEUDECIDE],
    list_speech_models: [ULTRA, WHISPER],
    detect_ollama: false,
    has_llm_secret: false,
    select_model: null,
    select_speech_model: null,
    download_model: null,
    download_speech_model: null,
    get_settings: {
      selectedLlmProvider: "local-server",
      openaiBaseUrl: "https://api.openai.com/v1",
      openaiModel: "gpt-4o-mini",
    },
    get_runtime_status: {
      speech: { state: "ready", label: "Speech", modelId: "parakeet-ultra", detail: "ready", blocking: false },
      llm: { state: "ready", label: "LLM", modelId: "s1-mini", detail: "ready", blocking: false },
    },
    ...overrides,
  });
  return render(<ModelsScreen />);
}

/** Waits for the section's row (the sync point: the catalog lands after the
    first paint) and returns the row's wrapper: `rowInSection`'s contract from
    `modelsScreen.test.tsx`, so the license box and progress bar under the
    `Row` are included. */
/** Same catalog-load sync point as the row helpers: fake timers are on, so
    polling must be driven by `vi.advanceTimersByTimeAsync`, not real wait. */
async function waitForRow(section: HTMLElement, rowName: string) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  for (let i = 0; i < 20; i++) {
    if (section.querySelector(`[data-settings-label="${rowName}"]`)) return;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20);
    });
  }
  if (!section.querySelector(`[data-settings-label="${rowName}"]`)) {
    throw new Error(`no "${rowName}" row in section`);
  }
}

async function rowWrapper(title: string, rowName: string): Promise<HTMLElement> {
  const section = (await screen.findByText(title, { exact: true })).closest("section");
  if (!section) throw new Error(`no section for "${title}"`);
  await waitFor(() => within(section as HTMLElement).getByText(rowName));
  const row = section.querySelector<HTMLElement>(`[data-settings-label="${rowName}"]`);
  if (!row) throw new Error(`no "${rowName}" row in this section`);
  return row.parentElement ?? row;
}

/** The row's wrapper (hint, controls and body) after the catalog has landed. */
function rowFor(section: HTMLElement, rowName: string): HTMLElement {
  const row = section.querySelector<HTMLElement>(`[data-settings-label="${rowName}"]`);
  if (!row) throw new Error(`no "${rowName}" row in section`);
  return row.parentElement ?? row;
}

/** The wrapper around a named row in the polish section, after the catalog has
    landed. `findByText`/`waitFor` cannot be used under fake timers here, so the
    section is found synchronously (its title renders before the catalog) and
    the rows are awaited through `advanceTimersByTimeAsync`. */
/** The polish section. Its title renders before the catalog, so this works
    under fake timers, where `findByText`/`waitFor` would wait on timers that
    only advance when the test says so. */
function polishSection(): HTMLElement {
  const section = screen.getByText("Polish models", { exact: true }).closest("section");
  if (!section) throw new Error("no polish section");
  return section as HTMLElement;
}

async function polishRow(rowName: string): Promise<HTMLElement> {
  const section = polishSection();
  await waitForRow(section, rowName);
  return rowFor(section, rowName);
}

describe("Models screen open states", () => {
  it("counts up the elapsed time while a model is starting", async () => {
    vi.useFakeTimers();
    renderModels({ list_models: [{ ...S1_MINI, selected: false }, QWEN, NEUDECIDE] });
    const row = await polishRow("S1-mini");

    const button = within(row).getByRole("button", { name: "Use" });
    act(() => {
      fireEvent.click(button);
    });

    // First reading: the label has rendered with a starting-seconds value.
    const first = await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
      return within(row).getByText(/Starting \(\d+s\)/);
    });
    const firstText = first.textContent;
    expect(firstText).toMatch(/Starting \(\d+s\)/);

    // Second reading one tick later: the counter advanced. Exact copy is not
    // asserted, only that it increased.
    const second = await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      return within(row).getByText(/Starting \(\d+s\)/);
    });
    const firstSecs = Number(firstText?.match(/\((\d+)s\)/)?.[1]);
    const secondSecs = Number(second.textContent?.match(/\((\d+)s\)/)?.[1]);
    expect(secondSecs).toBeGreaterThan(firstSecs);
  });

  it("shows the slow-machine hint only for a model the host cannot run natively", async () => {
    vi.useFakeTimers();
    // Qwen is offered for use (so it must be downloaded) and does not run
    // natively here; S1-mini does. Only the Qwen row may carry the hint.
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, { ...QWEN, downloaded: true }, NEUDECIDE],
    });
    const qwenRow = await polishRow("Qwen2.5 3B");
    const section = polishSection();

    act(() => {
      fireEvent.click(within(qwenRow).getByRole("button", { name: "Use" }));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(
      within(qwenRow).queryByText(/does not run natively on this computer/),
    ).toBeTruthy();
    // The row that does run natively carries no such note.
    expect(
      within(rowFor(section, "S1-mini")).queryByText(
        /does not run natively on this computer/,
      ),
    ).toBeNull();

    cleanup();
    vi.useRealTimers();
    vi.useFakeTimers();
    renderModels({
      list_models: [
        { ...S1_MINI, id: "s1-native", selected: false, runsOnHost: true },
        NEUDECIDE,
      ],
    });
    const nativeRow = await polishRow("S1-mini");

    // A model that runs natively must not carry the hint.
    act(() => {
      fireEvent.click(within(nativeRow).getByRole("button", { name: "Use" }));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(
      within(nativeRow).queryByText(/does not run natively on this computer/),
    ).toBeNull();
  });

  it("keeps a finishing state between a done download and the confirmed catalog", async () => {
    renderModels({
      list_models: [
        S1_MINI,
        { ...QWEN, downloaded: false },
        NEUDECIDE,
      ],
    });
    const row = await rowWrapper("Polish models", "Qwen2.5 3B");
    fireEvent.click(within(row).getByRole("button", { name: "Download" }));

    // The download is in flight: the progress bar is under the row.
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("download_model", {
        id: "qwen2.5-3b",
        licenseAccepted: false,
      }),
    );
    // Program the store the way the backend does: a `downloading` payload,
    // then the `done` one the moment the transfer finishes.
    await act(async () => {
      await emitTauriEvent("model-download-progress", progressPayload({
        downloadedBytes: 500_000_000,
        percent: 25,
      }));
    });
    // While downloading: bar visible, no downloaded chip, no finishing copy.
    expect(within(row).queryByText("Downloaded")).toBeNull();
    expect(within(row).queryByText("Finishing…")).toBeNull();
    expect(within(row).getByText("25%")).toBeTruthy();

    // The download finishes: the bar must hold (as a finishing state) until
    // the refreshed catalog reports `downloaded: true`.
    await act(async () => {
      await emitTauriEvent("model-download-progress", progressPayload({
        status: "done",
        downloadedBytes: 2_000_000_000,
        percent: 100,
      }));
    });
    // Finishing state: the bar's label says so.
    expect(within(row).getByText("Finishing…")).toBeTruthy();
    expect(within(row).queryByText("Downloaded")).toBeNull();

    // Invoke resolves, the catalog refresh reports `downloaded: true`, and
    // `finishDownload` drops the store row: the chip takes over.
    await waitFor(
      () => expect(within(row).getByText("Downloaded")).toBeTruthy(),
      { timeout: 2000 },
    );
    expect(within(row).queryByText("Finishing…")).toBeNull();
    expect(
      vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === "list_models"),
    ).toBe(true);
  });

  it("ends loading and names the failure when detect_ollama fails", async () => {
    // `tauriMock` only programs resolve values, so a failing `detect_ollama`
    // gets its own implementation: everything resolves, the probe rejects.
    mockListenCapture();
    const settings = {
      selectedLlmProvider: "local-server",
      openaiBaseUrl: "https://api.openai.com/v1",
      openaiModel: "gpt-4o-mini",
    };
    const runtime = {
      speech: { state: "ready", label: "Speech", modelId: "parakeet-ultra", detail: "ready", blocking: false },
      llm: { state: "ready", label: "LLM", modelId: "s1-mini", detail: "ready", blocking: false },
    };
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case "list_models":
          return [S1_MINI, QWEN, NEUDECIDE];
        case "list_speech_models":
          return [ULTRA, WHISPER];
        case "detect_ollama":
          throw new Error("ollama probe failed");
        case "get_settings":
          return settings;
        case "has_llm_secret":
          return false;
        case "get_runtime_status":
          return runtime;
        case "select_model":
        case "select_speech_model":
        case "download_model":
        case "download_speech_model":
          return null;
        default:
          throw new Error(`[tauriMock] unexpected invoke: ${cmd}`);
      }
    });
    const { unmount } = render(<ModelsScreen />);

    // Loading ends: the "Loading models…" row is gone and the catalog rows
    // are on screen, even though the detection failed.
    await screen.findByText("S1-mini");
    expect(screen.queryByText("Loading models…")).toBeNull();

    // The not-detected note names the failure, and the section still renders
    // its fallback rows below it.
    expect(await screen.findByText(/Could not check for a local Ollama server/)).toBeTruthy();
    expect(screen.getByText("OpenAI-compatible API")).toBeTruthy();
    unmount();
  });
});
