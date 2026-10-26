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
  specialty: { tag: "Instruction-following transforms", measured: false },
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
  specialty: { tag: "Polish + Prompt Engineer", measured: true },
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
  specialty: { tag: "Small & fast", measured: false },
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
    delete_model: null,
    delete_speech_model: null,
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

  it("marks a measured specialty as measured, and a claimed one as a description", async () => {
    // S1-mini and Qwen3.5 2B have bakeoff-proven roles, so their chips carry a
    // "measured" suffix. Qwen2.5 3B's role is inferred from the family's
    // training, so its chip is the tag alone, no measurement claim. The user
    // must not read three identical chips as three identical scores.
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, QWEN, QWEN35, NEUDECIDE],
    });
    const polish = await sectionByTitle("Polish models", "Qwen3.5 2B");
    // The measured rows announce it in the chip: the row carries a "measured"
    // marker next to the tag, so the distinction does not depend on the user
    // registering the dot or the tone as a status cue.
    expect(within(rowInSection(polish, "S1-mini")).getByText("Dictation cleanup")).toBeTruthy();
    expect(within(rowInSection(polish, "S1-mini")).getByText("· measured")).toBeTruthy();
    expect(
      within(rowInSection(polish, "Qwen3.5 2B")).getByText("Polish + Prompt Engineer"),
    ).toBeTruthy();
    expect(within(rowInSection(polish, "Qwen3.5 2B")).getByText("· measured")).toBeTruthy();
    // The unmeasured row shows its tag without a measurement claim.
    const qwenRow = rowInSection(polish, "Qwen2.5 3B");
    expect(within(qwenRow).getByText("Instruction-following transforms")).toBeTruthy();
    expect(within(qwenRow).queryByText("· measured")).toBeNull();
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

  it("offers Delete only on a downloaded model, hidden otherwise", async () => {
    // S1-mini is downloaded; Qwen2.5 3B is not. The row that has no bytes on
    // disk must not offer to delete them.
    renderModels();
    const polish = await sectionByTitle("Polish models", "S1-mini");
    expect(within(rowInSection(polish, "S1-mini")).getByRole("button", { name: "Delete model" })).toBeTruthy();
    expect(within(rowInSection(polish, "Qwen2.5 3B")).queryByRole("button", { name: "Delete model" })).toBeNull();
  });

  it("deletes on the second tap, not the first", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    renderModels({ list_models: [{ ...S1_MINI, selected: false }, NEUDECIDE] });
    const polish = await sectionByTitle("Polish models", "S1-mini");
    const row = rowInSection(polish, "S1-mini");
    fireEvent.click(within(row).getByRole("button", { name: "Delete model" }));
    // First tap only arms: nothing is deleted yet, and the control now asks to confirm.
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("delete_model", { id: "s1-mini" });
    fireEvent.click(within(row).getByRole("button", { name: "Confirm delete" }));
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("delete_model", { id: "s1-mini" }),
    );
  });

  it("deletes a dictation model on the second tap", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    renderModels({ list_speech_models: [{ ...ULTRA, selected: false }, WHISPER] });
    const dictation = await sectionByTitle("Dictation models", "Parakeet Ultra");
    const row = rowInSection(dictation, "Parakeet Ultra");
    fireEvent.click(within(row).getByRole("button", { name: "Delete model" }));
    fireEvent.click(within(row).getByRole("button", { name: "Confirm delete" }));
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("delete_speech_model", { id: "parakeet-ultra" }),
    );
  });

  it("shows the delete failure under the row on a rejected delete", async () => {
    // The delete rejects; a function-valued mock lets the row's post-failure
    // re-issue succeed instead of rejecting forever (which would leave an
    // unhandled rejection).
    let deleteCalls = 0;
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, NEUDECIDE],
      delete_model: () => {
        deleteCalls += 1;
        return deleteCalls === 1
          ? Promise.reject(new Error("file in use"))
          : Promise.resolve(null);
      },
    });
    const polish = await sectionByTitle("Polish models", "S1-mini");
    const row = rowInSection(polish, "S1-mini");
    fireEvent.click(within(row).getByRole("button", { name: "Delete model" }));
    fireEvent.click(within(row).getByRole("button", { name: "Confirm delete" }));
    await waitFor(() => expect(row.textContent).toContain("file in use"));
  });

  it("shows a retry banner, not an empty list, when the catalog fails to load", async () => {
    // Both catalogs reject: the screen must say it could not load and offer a
    // retry, not paint a blank list that reads as "no models".
    renderModels({
      list_models: Promise.reject(new Error("catalog down")),
      list_speech_models: Promise.reject(new Error("catalog down")),
    });
    await waitFor(() => expect(screen.getByText(/Could not load the model list/)).toBeTruthy());
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });

  // The host's own polish model: always present, use-only, never downloadable
  // and never deletable. `ModelStatus` carries no system flag, so the id is the
  // test. It is a polish/LLM row, so it renders in the polish list.
  const APPLE_INTELLIGENCE = {
    ...S1_MINI,
    id: "apple-intelligence",
    name: "Apple Intelligence",
    sizeMb: 0,
    description: "The model built into this computer.\nIt is always available and cannot be removed.",
    specialty: { tag: "Polish", measured: false },
    downloaded: true,
    selected: false,
    recommended: true,
    polishCapable: true,
    runsOnHost: true,
    verdict: "Recommended",
    verdictNote: "",
    licenseName: null,
    licenseUrl: null,
    requiresLicenseAccept: false,
    attribution: null,
  };

  it("renders the system model as a Recommended polish row", async () => {
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, APPLE_INTELLIGENCE, NEUDECIDE],
    });
    const polish = await sectionByTitle("Polish models", "Apple Intelligence");
    const row = rowInSection(polish, "Apple Intelligence");
    // It is a polish row and it carries the Recommended badge.
    expect(within(row).getByText("Recommended")).toBeTruthy();
    // Its description is shown in place of the size/speed/accuracy hint.
    expect(within(row).getByText(/model built into this computer/)).toBeTruthy();
    // No "0 MB" size hint.
    expect(within(row).queryByText(/0 MB/)).toBeNull();
  });

  it("offers no Download and no Delete control for the system model", async () => {
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, APPLE_INTELLIGENCE, NEUDECIDE],
    });
    const polish = await sectionByTitle("Polish models", "Apple Intelligence");
    const row = rowInSection(polish, "Apple Intelligence");
    // It is always present, so no Download control.
    expect(within(row).queryByRole("button", { name: "Download" })).toBeNull();
    // It has no bytes on disk to free, so no Delete control either.
    expect(within(row).queryByRole("button", { name: "Delete model" })).toBeNull();
  });

  it("invokes select_model when the system model's Use is pressed", async () => {
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, APPLE_INTELLIGENCE, NEUDECIDE],
    });
    const polish = await sectionByTitle("Polish models", "Apple Intelligence");
    const row = rowInSection(polish, "Apple Intelligence");
    fireEvent.click(within(row).getByRole("button", { name: "Use" }));
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("select_model", {
        id: "apple-intelligence",
      }),
    );
  });

  it("disarms a pending delete when another row is selected", async () => {
    // Arm delete on S1-mini, then Use on the system row: the arm must clear so
    // the confirm state does not linger across an unrelated interaction.
    renderModels({
      list_models: [{ ...S1_MINI, selected: false }, APPLE_INTELLIGENCE, NEUDECIDE],
    });
    const polish = await sectionByTitle("Polish models", "Apple Intelligence");
    const s1Row = rowInSection(polish, "S1-mini");
    fireEvent.click(within(s1Row).getByRole("button", { name: "Delete model" }));
    expect(within(s1Row).getByRole("button", { name: "Confirm delete" })).toBeTruthy();
    const appleRow = rowInSection(polish, "Apple Intelligence");
    fireEvent.click(within(appleRow).getByRole("button", { name: "Use" }));
    await waitFor(() =>
      expect(within(s1Row).queryByRole("button", { name: "Confirm delete" })).toBeNull(),
    );
  });
});
