// claim: ui-behavior
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import OnboardingScreen from "../screens/OnboardingScreen";
import { invokeCalls, mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

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

const QWEN = {
  id: "qwen3.5-2b",
  name: "Qwen 2B",
  sizeMb: 1200,
  description: "Full Polish editor.",
  downloaded: true,
  selected: true,
  recommended: true,
  polishCapable: true,
};

const SETTINGS = {
  hotkey: "Fn",
  selectedSpeechModel: ULTRA.id,
  hasCompletedOnboarding: false,
  selectedLlmProvider: "local-server",
  selectedLlmModel: QWEN.id,
  polishProfile: "natural",
  polishAppProfiles: {},
};

function options(capability: string, supported: string[], unsupported: string[], loaded = true) {
  const defs: Record<string, { label: string; budgetPercent: number }> = {
    clean: { label: "Clean", budgetPercent: 15 },
    natural: { label: "Natural", budgetPercent: 30 },
    structured: { label: "Structured", budgetPercent: 45 },
  };
  return {
    modelId: QWEN.id,
    capability,
    supported: supported.map((k) => ({ key: k, ...defs[k] })),
    unsupported: unsupported.map((k) => ({ key: k, ...defs[k] })),
    modelLoaded: loaded,
  };
}

function results() {
  const rows: Record<string, unknown>[] = [];
  for (const sample of ["email", "slack", "note"]) {
    for (const profile of ["clean", "natural", "structured"]) {
      rows.push({
        sampleId: sample,
        profile,
        before: `before ${sample}`,
        after: `after ${sample} (${profile})`,
        transformed: true,
        error: null,
      });
    }
  }
  return rows;
}

async function toCalibration(opts: Record<string, unknown>) {
  mockListenCapture();
  mockInvokeResponses({
    get_settings: SETTINGS,
    get_permissions: [
      { kind: "microphone", state: "granted" },
      { kind: "accessibility", state: "granted" },
    ],
    list_speech_models: [ULTRA],
    list_models: [QWEN],
    get_speech_model_ready_state: {
      id: ULTRA.id,
      downloaded: true,
      ready: true,
      message: "Ready.",
    },
    download_model: null,
    select_model: null,
    calibration_profile_options: opts,
    generate_calibration_samples: results(),
    record_calibration: { ...SETTINGS, hasCompletedOnboarding: false },
    save_settings: null,
  });
  render(<OnboardingScreen onCompleted={vi.fn()} />);
  for (let i = 0; i < 5; i++) {
    const next = await screen.findByText(/^(Continue|Continue without polishing)$/);
    fireEvent.click(next);
    await new Promise((r) => setTimeout(r, 60));
  }
  await screen.findByTestId("calibration-step");
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

describe("First-run calibration step", () => {
  it("shows the promise and edit budget for supported profiles", async () => {
    await toCalibration(options("editor", ["clean", "natural", "structured"], []));
    expect(await screen.findByText(/Fix only what is wrong\. Keep my words\./)).toBeTruthy();
    expect(screen.getByText(/Edit budget: up to 45% of words/)).toBeTruthy();
    expect(screen.getByLabelText("An email sample")).toBeTruthy();
  });

  it("shows unsupported-profile copy and leaves those profiles out of the choice", async () => {
    await toCalibration(options("cleanup-only", ["clean"], ["natural", "structured"]));
    expect(screen.getByText(/can't do Natural or Structured/)).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Clean" })).toBeTruthy();
    expect(screen.queryByRole("radio", { name: "Natural" })).toBeNull();
    expect(screen.getByRole("radio", { name: "Not sure — use Natural" })).toBeTruthy();
  });

  it("records a skip and keeps the existing defaults", async () => {
    await toCalibration(options("editor", ["clean", "natural", "structured"], []));
    fireEvent.click(screen.getByRole("button", { name: "Skip for now" }));
    await waitFor(() => {
      const call = invokeCalls().find((c) => c.cmd === "record_calibration");
      expect(call).toBeTruthy();
      expect(call?.args).toEqual({ outcome: { skipped: true, profile: null } });
    });
    expect(await screen.findByText(/You’re all set/)).toBeTruthy();
  });

  it("uses Natural on the not-sure path", async () => {
    await toCalibration(options("editor", ["clean", "natural", "structured"], []));
    fireEvent.click(screen.getByRole("radio", { name: "Not sure — use Natural" }));
    fireEvent.click(screen.getByRole("button", { name: "Use this profile" }));
    await waitFor(() => {
      const call = invokeCalls().find((c) => c.cmd === "record_calibration");
      expect(call?.args).toEqual({ outcome: { skipped: false, profile: "natural" } });
    });
  });

  it("saves the chosen profile", async () => {
    await toCalibration(options("editor", ["clean", "natural", "structured"], []));
    fireEvent.click(screen.getByRole("radio", { name: "Structured" }));
    fireEvent.click(screen.getByRole("button", { name: "Use this profile" }));
    await waitFor(() => {
      const call = invokeCalls().find((c) => c.cmd === "record_calibration");
      expect(call?.args).toEqual({ outcome: { skipped: false, profile: "structured" } });
    });
  });

  it("explains the no-model case and stays skippable", async () => {
    await toCalibration(options("cleanup-only", ["clean"], ["natural", "structured"], false));
    expect(await screen.findByText(/No polish model is loaded/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Skip for now" }).hasAttribute("disabled")).toBe(false);
  });
});
