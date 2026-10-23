// claim: ui-behavior
/**
 * The onboarding Polish step offers the models this computer can actually run,
 * by name, with the download button on screen. It asks `list_models` for the
 * first-run shortlist (`{ shortlist: true }`), which the backend filters through
 * `polish_shortlist_ids(gpu_accelerated)`: an Apple Silicon Mac gets the
 * Metal-accelerated set, a Windows or Intel host gets the CPU-only rows.
 *
 * Before that the step rendered one anonymous card labelled "Local model
 * (offline)" with a size, so a first-run user could not tell what they were
 * downloading, and the card's action stayed hidden until they clicked the card
 * itself.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import OnboardingScreen from "../screens/OnboardingScreen";
import { mockInvokeResponses, invokeCalls, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const SPEAKOFLOW = {
  id: "speakoflow-mini",
  name: "SpeakoFlow Mini",
  sizeMb: 834,
  description: "Reliable cleanup that preserves names and numbers.",
  downloaded: false,
  selected: false,
  recommended: true,
  polishCapable: true,
  runsOnHost: true,
};

const S1_MINI = {
  ...SPEAKOFLOW,
  id: "s1-mini",
  name: "S1-mini",
  sizeMb: 462,
  description: "Small open cleanup model, happiest in English.",
  downloaded: true,
  recommended: true,
};

const QWEN = {
  ...SPEAKOFLOW,
  id: "qwen2.5-3b",
  name: "Qwen2.5 3B",
  sizeMb: 1900,
  description: "Best in our tests.",
};

const NEUDECIDE = {
  ...S1_MINI,
  id: "neudecide-q4",
  name: "NeuDecide Q4 (decider)",
  sizeMb: 41,
  polishCapable: false,
};

const ULTRA = {
  id: "parakeet-ultra-coreml",
  name: "Parakeet Ultra (Neural Engine)",
  engine: "fluidaudio",
  sizeMb: 600,
  description: "Fast on-device transcription.",
  recommended: true,
  englishOnly: false,
  downloaded: true,
  selected: true,
};

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

/** Walks Privacy, Permissions and Dictation so the assertions land on Polish. */
async function toPolishStep(polishRows: Record<string, unknown>[]) {
  mockListenCapture();
  mockInvokeResponses({
    get_settings: { hotkey: "Fn", selectedSpeechModel: ULTRA.id, hasCompletedOnboarding: false },
    get_permissions: [
      { kind: "microphone", state: "granted" },
      { kind: "accessibility", state: "granted" },
    ],
    list_speech_models: [ULTRA],
    list_models: polishRows,
    get_speech_model_ready_state: {
      id: ULTRA.id,
      downloaded: true,
      ready: true,
      message: "Parakeet Ultra is loaded.",
    },
    download_model: null,
    select_model: null,
  });
  render(<OnboardingScreen onCompleted={vi.fn()} />);
  // One tick between clicks: React removes the old step's button in the commit,
  // and a `fireEvent.click` on a detached node never reaches the root listener,
  // so a tight loop silently loses steps.
  for (let i = 0; i < 3; i++) {
    const next = await screen.findByText(/^(Continue|Continue without polishing)$/);
    fireEvent.click(next);
    await new Promise((r) => setTimeout(r, 60));
  }
  await screen.findByText("Polish your writing (optional)");
}

describe("Onboarding polish step", () => {
  it("asks the backend for the shortlist this host can run", async () => {
    await toPolishStep([S1_MINI, SPEAKOFLOW]);
    expect(invokeCalls()).toContainEqual({ cmd: "list_models", args: { shortlist: true } });
  });

  it("names the model instead of describing a category", async () => {
    await toPolishStep([S1_MINI, SPEAKOFLOW]);
    expect(screen.getByText(S1_MINI.name)).toBeTruthy();
    expect(screen.getByText(S1_MINI.description)).toBeTruthy();
    expect(screen.queryByText(/Local model/i)).toBeNull();
  });

  it("keeps a Command Mode tool out of the choice", async () => {
    await toPolishStep([S1_MINI, SPEAKOFLOW, NEUDECIDE]);
    expect(screen.queryByText(NEUDECIDE.name)).toBeNull();
  });

  it("puts the download button on the card without a click first", async () => {
    await toPolishStep([{ ...S1_MINI, downloaded: false }, SPEAKOFLOW]);
    expect(
      screen.getByRole("button", { name: `Download (${S1_MINI.sizeMb} MB)` }),
    ).toBeTruthy();
  });

  it("says Ready when the shortlisted model is already on disk", async () => {
    await toPolishStep([S1_MINI, SPEAKOFLOW]);
    expect(screen.getByText("Ready")).toBeTruthy();
  });

  it("recommends once, even though three rows carry the flag", async () => {
    await toPolishStep([S1_MINI, SPEAKOFLOW, QWEN]);
    expect(screen.getAllByText("Recommended")).toHaveLength(1);
  });

  it("does not list the card's own model under the disclosure", async () => {
    await toPolishStep([S1_MINI, SPEAKOFLOW, QWEN]);
    // The disclosure names how many choices are behind it; it used to read as
    // plain grey text with no sign it could be opened.
    const toggle = screen.getByRole("button", { name: /Show 2 other models/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText(SPEAKOFLOW.name)).toBeTruthy();
    // Once, on the card. The disclosure underneath used to repeat it with a
    // second Select control next to the Download already being offered.
    expect(screen.getAllByText(S1_MINI.name)).toHaveLength(1);
  });

  it("lets a first run continue without downloading anything", async () => {
    await toPolishStep([{ ...S1_MINI, downloaded: false }, SPEAKOFLOW]);
    const next = screen.getByText("Continue without polishing");
    expect(next.hasAttribute("disabled")).toBe(false);
    fireEvent.click(next);
    await screen.findByText("Your dictation hotkey");
    expect(invokeCalls()).not.toContainEqual(
      expect.objectContaining({ cmd: "download_model" }),
    );
  });
});
