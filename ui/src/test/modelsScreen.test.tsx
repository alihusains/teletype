// claim: ui-behavior
/**
 * The AI Models screen is one column of task-first rows: every model says what
 * state it is in and carries its own action, and the split on `polishCapable`
 * (the UI mirror of `CatalogEntry::polish_capable`) is what keeps the Command
 * Mode decider out of the list a user picks a polish engine from. `select_model`
 * refuses a non-polish entry, so a row that offered "Use" for it could only ever
 * produce an error.
 *
 * Before this layout the screen was a rail plus a detail pane, so a row's
 * Download button existed only after clicking the row, and the state of the
 * other eight models was invisible.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import ModelsScreen from "../screens/ModelsScreen";
import { invoke } from "@tauri-apps/api/core";
import { mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const S1_MINI = {
  id: "s1-mini",
  name: "S1-mini",
  sizeMb: 380,
  description: "Small polish model, tuned on our Polish corpus.",
  purpose: "Dictation cleanup",
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

const NEUDECIDE = {
  ...S1_MINI,
  id: "neudecide-q4",
  name: "NeuDecide Q4 (decider)",
  sizeMb: 41,
  description: "Speech-to-tool-call decider for Command Mode (ONNX, runs in-process).",
  selected: false,
  recommended: false,
  polishCapable: false,
  verdict: "Not tested",
  verdictNote: "",
  licenseName: "Apache-2.0",
  licenseUrl: "https://example.invalid/license",
  attribution: null,
};

const QWEN = {
  ...S1_MINI,
  id: "qwen2.5-3b",
  name: "Qwen2.5 3B",
  sizeMb: 1900,
  description: "Best local model in our tests.",
  purpose: "",
  downloaded: false,
  selected: false,
  runsOnHost: false,
  verdictNote: "Best in our tests, may follow dictated instructions",
  attribution: null,
};

// Deliberately the same attribution string as S1-mini: the strip below the
// polish rows has to say it once, not once per row that ships it.
const QWEN35 = {
  ...S1_MINI,
  id: "qwen3.5-2b",
  name: "Qwen3.5 2B",
  sizeMb: 1173,
  description: "The all-rounder.",
  purpose: "Polish + Prompt Engineer",
  downloaded: false,
  selected: false,
  recommended: false,
  runsOnHost: true,
  verdictNote: "Best all-rounder: Polish, style controls and Prompt Engineer",
  attribution: "Qwen3.5 by Alibaba (Apache-2.0)",
};

const GEMMA = {
  ...S1_MINI,
  id: "gemma2-2b",
  name: "Gemma 2 2B",
  sizeMb: 1500,
  description: "Google's small model.",
  purpose: "",
  downloaded: false,
  selected: false,
  runsOnHost: true,
  recommended: false,
  verdict: "Mixed results",
  verdictNote: "Mixed results, often mishandles other languages",
  licenseName: "Google Gemma Terms of Use",
  licenseUrl: "https://ai.google.dev/gemma/terms",
  requiresLicenseAccept: true,
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

afterEach(() => {
  cleanup();
  resetTauriMocks();
  localStorage.clear();
});

function renderModels(overrides: Record<string, unknown> = {}) {
  mockListenCapture();
  mockInvokeResponses({
    list_models: [S1_MINI, QWEN, GEMMA, NEUDECIDE],
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

/** The `Section` surface that holds a title's rows. Waiting on a row name is
    the sync point: `list_models` lands after the first paint, so asserting
    against the pre-load render would pass for the wrong reason. */
async function sectionByTitle(title: string, rowName: string): Promise<HTMLElement> {
  const el = (await screen.findByText(title, { exact: true })).closest("section");
  if (!el) throw new Error(`no section for "${title}"`);
  await waitFor(() => within(el as HTMLElement).getByText(rowName));
  return el as HTMLElement;
}

/** `Row` stamps its own label onto the element, so a section can hold several
    Download buttons and still be asserted on one at a time. The wrapper is the
    whole row: the license box and the progress bar render under the `Row`, not
    inside it. */
function rowInSection(section: HTMLElement, label: string): HTMLElement {
  const row = section.querySelector<HTMLElement>(`[data-settings-label="${label}"]`);
  if (!row) throw new Error(`no "${label}" row in this section`);
  return row.parentElement ?? row;
}

describe("AI Models screen", () => {
  it("keeps a Command Mode tool out of the polish list", async () => {
    renderModels();
    const polish = await sectionByTitle("Polish models", "S1-mini");
    expect(within(polish).queryByText(NEUDECIDE.name)).toBeNull();
  });

  it("shows the Command Mode decider as a status line, not a selectable row", async () => {
    renderModels();
    // The decider is rendered as a subtle one-line note, not a Section with a
    // bordered card. It names the model and its install state.
    const note = await screen.findByText(/Command Mode uses/);
    expect(note).toBeTruthy();
    expect(note.closest("div")?.textContent).toContain(NEUDECIDE.name);
    expect(note.closest("div")?.textContent).toContain("installed");
  });

  it("offers no Use control for a model the backend refuses to select", async () => {
    renderModels();
    const note = await screen.findByText(/Command Mode uses/);
    const container = note.closest("div");
    expect(container).toBeTruthy();
    expect(within(container as HTMLElement).queryByRole("button", { name: "Use" })).toBeNull();
  });

  it("uses a real polish engine when its row is pressed", async () => {
    renderModels({ list_models: [{ ...S1_MINI, selected: false }, QWEN, NEUDECIDE] });
    const polish = await sectionByTitle("Polish models", "S1-mini");
    fireEvent.click(within(polish).getByRole("button", { name: "Use" }));
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("select_model", { id: "s1-mini" }),
    );
  });

  it("names the model in use and gives it no second action", async () => {
    renderModels();
    const polish = await sectionByTitle("Polish models", "S1-mini");
    expect(within(polish).getByText("In use")).toBeTruthy();
    // Qwen is downloadable and Gemma is license-gated, so the section has other
    // buttons; the row that is already running must not offer to start again.
    expect(within(polish).queryByRole("button", { name: "Use" })).toBeNull();
  });

  it("labels a model this computer cannot run quickly, and still offers it", async () => {
    renderModels();
    const polish = await sectionByTitle("Polish models", "Qwen2.5 3B");
    const qwen = rowInSection(polish, "Qwen2.5 3B");
    expect(within(qwen).getByText("Slow here")).toBeTruthy();
    expect(within(qwen).getByText(/1\.9 GB/)).toBeTruthy();
    // The chip warns; it does not remove the choice. A machine with enough RAM
    // can still use this row, and hiding it would say otherwise.
    expect(within(qwen).getByRole("button", { name: "Download" })).toBeTruthy();
    expect(within(rowInSection(polish, "S1-mini")).queryByText("Slow here")).toBeNull();
  });

  it("gates a licensed download on the acceptance box", async () => {
    renderModels();
    const polish = await sectionByTitle("Polish models", "Gemma 2 2B");
    const gemma = rowInSection(polish, "Gemma 2 2B");
    expect(within(gemma).getByText("I accept the terms")).toBeTruthy();
    expect(within(gemma).getByRole("button", { name: "Download" }).hasAttribute("disabled")).toBe(
      true,
    );
    // An unlicensed row in the same section keeps its button live, so the gate
    // is per-model and not a section-wide lockdown.
    expect(within(rowInSection(polish, "Qwen2.5 3B")).getByRole("button", { name: "Download" }).hasAttribute("disabled")).toBe(
      false,
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("download_model", expect.anything());
  });

  it("labels each shortlisted row with the job it is for", async () => {
    // The two rows a first run offers sit side by side, so each one has to say
    // what it is good at: the cleanup model cannot follow an instruction, and
    // the 2B is the one that runs Prompt Engineer.
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, QWEN35, NEUDECIDE],
    });
    const polish = await sectionByTitle("Polish models", "Qwen3.5 2B");
    expect(within(rowInSection(polish, "S1-mini")).getByText("Dictation cleanup")).toBeTruthy();
    expect(
      within(rowInSection(polish, "Qwen3.5 2B")).getByText("Polish + Prompt Engineer"),
    ).toBeTruthy();
  });

  it("recommends one model, not every model it likes", async () => {
    // Three catalog rows carry `recommended: true`. With a per-row badge the
    // polish list showed two green "Recommended" chips at once, which reads as
    // no recommendation at all.
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, QWEN, GEMMA, NEUDECIDE],
    });
    const polish = await sectionByTitle("Polish models", "S1-mini");
    expect(within(polish).getAllByText("Recommended")).toHaveLength(1);
    expect(within(rowInSection(polish, "S1-mini")).getByText("Recommended")).toBeTruthy();
  });

  it("gives the dictation list the same single badge", async () => {
    renderModels({
      list_speech_models: [{ ...ULTRA, selected: false }, { ...WHISPER, recommended: true }],
    });
    const dictation = await sectionByTitle("Dictation models", "Parakeet Ultra");
    expect(within(dictation).getAllByText("Recommended")).toHaveLength(1);
  });

  it("states the attribution once, however many rows repeat it", async () => {
    renderModels();
    const polish = await sectionByTitle("Polish models", "S1-mini");
    expect(within(polish).getAllByText(/S1-mini by Superwhisper/)).toHaveLength(1);
  });

  it("puts an action on every dictation row", async () => {
    renderModels();
    const dictation = await sectionByTitle("Dictation models", "Whisper large-v3 turbo");
    expect(within(dictation).getByText("In use")).toBeTruthy();
    fireEvent.click(within(dictation).getByRole("button", { name: "Download" }));
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("download_speech_model", {
        id: "large-v3-turbo-q5",
      }),
    );
  });

  it("selects a dictation model that is already on disk", async () => {
    renderModels({
      list_speech_models: [{ ...ULTRA, selected: false }, WHISPER],
    });
    const dictation = await sectionByTitle("Dictation models", "Parakeet Ultra");
    fireEvent.click(within(dictation).getByRole("button", { name: "Use" }));
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("select_speech_model", { id: "parakeet-ultra" }),
    );
  });
});
