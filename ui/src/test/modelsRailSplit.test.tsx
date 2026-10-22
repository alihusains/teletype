/**
 * The Models rail splits on `polishCapable`, the UI mirror of
 * `CatalogEntry::polish_capable`. Only models the app can load as a polish
 * engine sit under "Clean up and rewrite your text"; the Command Mode decider
 * gets its own section and no Select control, because `select_model` refuses a
 * non-polish entry. Before the split, NeuDecide Q4 led the polish rail wearing
 * the Recommended badge and its detail pane was titled "Polish model".
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import ModelsScreen from "../screens/ModelsScreen";
import { mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const S1_MINI = {
  id: "s1-mini",
  name: "S1-mini",
  sizeMb: 380,
  description: "Small polish model, tuned on our Polish corpus.",
  downloaded: true,
  selected: true,
  recommended: true,
  polishCapable: true,
  verdict: "Recommended",
  verdictNote: "Best measured polish model.",
  licenseName: null,
  licenseUrl: null,
  requiresLicenseAccept: false,
  attribution: null,
};

const NEUDECIDE = {
  id: "neudecide-q4",
  name: "NeuDecide Q4 (decider)",
  sizeMb: 41,
  description: "Speech-to-tool-call decider for Command Mode (ONNX, runs in-process).",
  downloaded: true,
  selected: false,
  recommended: false,
  polishCapable: false,
  verdict: "Not tested",
  verdictNote: "",
  licenseName: "Apache-2.0",
  licenseUrl: "https://example.invalid/license",
  requiresLicenseAccept: false,
  attribution: null,
};

const SPEECH = {
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

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

/**
 * RailSection renders its title as the first child of its root div. `rowName`
 * is the sync point: list_models lands after the first paint, and asserting
 * against the pre-load render would pass for the wrong reason.
 */
async function railSection(title: string, rowName: string): Promise<HTMLElement> {
  const root = (await screen.findByText(title)).parentElement;
  if (!root) throw new Error(`no rail section root for "${title}"`);
  await waitFor(() => within(root).getByText(rowName));
  return root;
}

function renderModels(overrides: Record<string, unknown> = {}) {
  mockListenCapture();
  mockInvokeResponses({
    list_models: [S1_MINI, NEUDECIDE],
    list_speech_models: [SPEECH],
    detect_ollama: false,
    has_llm_secret: false,
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

describe("Models rail split", () => {
  it("keeps non-polish models out of the polish rail", async () => {
    renderModels();
    const polish = await railSection("Polish models", "S1-mini");
    expect(within(polish).queryByText(NEUDECIDE.name)).toBeNull();
  });

  it("lists them under a section that says what they are", async () => {
    renderModels();
    const tools = await railSection("Command Mode decider", NEUDECIDE.name);
    expect(within(tools).getByText(NEUDECIDE.name)).toBeTruthy();
  });

  it("offers no Select control for a model the backend refuses to select", async () => {
    renderModels();
    const tools = await railSection("Command Mode decider", NEUDECIDE.name);
    fireEvent.click(within(tools).getByText(NEUDECIDE.name));
    expect(
      await screen.findByText("Not a polish engine, so there is nothing to select here."),
    ).toBeTruthy();
    expect(screen.getByText("Command Mode tool")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Select" })).toBeNull();
    expect(screen.queryByText(/Recommended/)).toBeNull();
  });

  it("still selects a real polish engine", async () => {
    renderModels();
    const polish = await railSection("Polish models", "S1-mini");
    fireEvent.click(within(polish).getByText("S1-mini"));
    expect(screen.getByRole("button", { name: "Selected" })).toBeTruthy();
    expect(screen.getByText("Recommended · Best measured polish model.")).toBeTruthy();
  });

  it("opens the detail pane on a model, not on the cloud form", async () => {
    renderModels();
    await railSection("Polish models", "S1-mini");
    // findByText, not getByText: the rail row lands in the same commit as the
    // catalogs, while the default selection is the effect's follow-up render, so
    // a synchronous assert here races the pane it is meant to check.
    expect(await screen.findByText("Speech model")).toBeTruthy();
    // "Quick start" only exists inside the OpenAI-compatible pane. Before the fix
    // the default resolved to "openai" on the first tick (both lists still empty)
    // and the effect then preserved any existing "openai", so the pane stayed on
    // the cloud form after the real models arrived.
    expect(screen.queryByText("Quick start")).toBeNull();
  });

  it("falls through to a polish engine, never a Command Mode tool", async () => {
    // NeuDecide leads this list on purpose: without the polishCapable check the
    // default would resolve to `models[0]` and open a pane whose only action is
    // "nothing to select here".
    renderModels({
      list_models: [NEUDECIDE, S1_MINI],
      list_speech_models: [],
    });
    await railSection("Polish models", "S1-mini");
    expect(await screen.findByText("Polish model")).toBeTruthy();
    expect(screen.queryByText("Command Mode tool")).toBeNull();
    expect(screen.getByRole("button", { name: "Selected" })).toBeTruthy();
  });
});
