// claim: ui-behavior
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import SettingsScreen from "../screens/SettingsScreen";
import OnboardingScreen from "../screens/OnboardingScreen";
import {
  invokeCalls,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../pill", async () => {
  const noop = () => null;
  return {
    default: noop,
    RainbowLips: noop,
    RainbowMeter: noop,
    rainbowColor: (t: number) => `hsl(${Math.round(t * 360)}, 80%, 50%)`,
  };
});
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onFocusChanged: () => Promise.resolve(() => {}),
  }),
}));

const SETTINGS = {
  hotkey: "Cmd+Shift+Space",
  recordingMode: "auto",
  selectedSpeechModel: "test-model",
  language: "en-US",
  inputDevice: "default",
  restoreClipboard: true,
  keepTextOnClipboard: false,
  autoApplyTransform: false,
  showTrayIcon: true,
  hasCompletedOnboarding: true,
  selectedLlmModel: "qwen3.5-2b",
  typingAutotextEnabled: false,
  autotextTiming: "immediate",
  removeFillerWords: true,
  fillerWords: [],
  pillPosition: "topRight",
  alwaysShowPill: false,
  livePreviewEnabled: false,
  cancelHotkey: "Escape",
  smartInsertion: true,
  playRecordingSounds: false,
  recordingSound: "ding",
  warmEnginePolicy: "keep",
  matchStrictness: "standard",
  dictionaryEnabled: true,
  pillStyle: "default",
  appIcon: "white",
  transcriptsDir: "",
  saveRecordings: true,
  vadAutoStop: true,
  vadSilenceMs: 700,
  noiseReductionEnabled: false,
  enableDeveloperTab: false,
      sendCrashReports: true,
  polishGateEnabled: false,
  polishGateThresholdWords: 10,
  polishMaxChangePercent: 40,
  restoreEmoji: false,
  spokenEmoji: false,
  spokenPunctuation: false,
  activeStyleProfile: "default",
  appLanguageOverrides: {},
  enabledPacks: [],
  selectedLlmProvider: "local-server",
  theme: "system",
  reduceMotion: false,
  polishRules: {},
  listStyle: "bullets",
  quickAddHotkey: "Cmd+Shift+K",
  commandHotkey: "Cmd+Shift+.",
  commandWakePhrase: false,
  pendingNotice: null,
  polishWholeTextAtEnd: false,
  polishSkipWhenClean: false,
  instantStart: "always",
  preferBuiltinMicWithBt: false,
  audioConditioning: false,
  learnFromEdits: true,
  learnNamesFromScreen: true,
  keepAudioForSelfTests: false,
  polishProfile: "natural",
  polishAppProfiles: {},
};

function promptInfo(capability: string, profile: string, modelId: string) {
  return {
    modelId,
    editable: capability !== "cleanup-only",
    defaultPrompt: "prompt",
    note: "",
    profile: {
      profile,
      label: profile === "clean" ? "Clean" : "Structured",
      resolvedBy: "global",
      budgetPercent: profile === "clean" ? 15 : 45,
      effectiveMaxChangePercent: profile === "clean" ? 15 : 45,
      modelCapability: capability,
      modelId,
      reason: "",
    },
  };
}

function seed(overrides: Record<string, unknown> = {}, capability = "instruction-capable") {
  const settings = { ...SETTINGS, ...overrides };
  const modelId = String(settings.selectedLlmModel);
  mockInvokeResponses({
    get_settings: settings,
    save_settings: settings,
    get_polish_prompt_info: promptInfo(capability, "natural", modelId),
    list_input_devices: [],
    list_style_profiles: [],
    get_app_language_overrides: {},
    get_app_style_overrides: {},
    get_transcripts_dir: "",
    get_runtime_status: {
      speech: { state: "ready", label: "Speech", modelId: "test", detail: "ready", blocking: false },
      llm: { state: "loaded", label: "LLM", modelId, detail: "loaded", blocking: false },
    },
  });
}

async function renderSettings() {
  mockListenCapture();
  render(<SettingsScreen active={false} />);
  await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());
}

describe("Polish profile settings", () => {
  beforeEach(() => {
    resetTauriMocks();
    seed();
  });

  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("saves the profile picked in the picker", async () => {
    await renderSettings();
    const picker = screen.getByTestId("profile-picker");
    fireEvent.click(within(picker).getByRole("radio", { name: "Structured" }));

    await waitFor(() => {
      const saved = invokeCalls().find((c) => c.cmd === "save_settings");
      expect(saved).toBeTruthy();
      const payload = saved?.args as { settings: { polishProfile: string } };
      expect(payload.settings.polishProfile).toBe("structured");
    });
  });

  it("shows the effective per-app profile, not just the stored override", async () => {
    resetTauriMocks();
    seed({ polishProfile: "structured", polishAppProfiles: { email: "clean" } });
    await renderSettings();

    const table = screen.getByRole("table");
    const emailRow = within(table).getByText("Email").closest("tr");
    expect(emailRow?.textContent).toContain("Effective: Clean");
    expect(emailRow?.textContent).toContain("Default: Structured");

    const chatRow = within(table).getByText("Chat").closest("tr");
    expect(chatRow?.textContent).toContain("Effective: Natural");
  });

  it("shows a cleanup-only banner and disables unsupported profiles", async () => {
    resetTauriMocks();
    seed(
      { selectedLlmModel: "speakoflow-mini", polishProfile: "structured" },
      "cleanup-only",
    );
    await renderSettings();

    expect(screen.getByText("Cleanup-only model")).toBeTruthy();
    expect(screen.getByText(/can honor Clean and Off/)).toBeTruthy();
    expect(screen.getByText(/cannot honor Natural, Structured, and Transform/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Models" })).toBeTruthy();

    const picker = screen.getByTestId("profile-picker");
    for (const name of ["Natural", "Structured", "Transform"]) {
      expect(within(picker).getByRole("radio", { name }).hasAttribute("disabled")).toBe(true);
    }
    for (const name of ["Clean", "Off"]) {
      expect(within(picker).getByRole("radio", { name }).hasAttribute("disabled")).toBe(false);
    }
  });
});

const SPEAKOFLOW = {
  id: "speakoflow-mini",
  name: "SpeakoFlow Mini",
  sizeMb: 834,
  description: "Reliable cleanup that preserves names and numbers.",
  downloaded: true,
  selected: false,
  recommended: true,
  polishCapable: true,
};

const S1_MINI = {
  ...SPEAKOFLOW,
  id: "s1-mini",
  name: "S1-mini",
  sizeMb: 462,
  description: "Small open cleanup model, happiest in English.",
};

const QWEN = {
  ...SPEAKOFLOW,
  id: "qwen2.5-3b",
  name: "Qwen2.5 3B",
  sizeMb: 1900,
  description: "Best in our tests.",
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
  for (let i = 0; i < 3; i++) {
    const next = await screen.findByText(/^(Continue|Continue without polishing)$/);
    fireEvent.click(next);
    await new Promise((r) => setTimeout(r, 60));
  }
  await screen.findByText("Polish your writing (optional)");
}

describe("Onboarding Polish contract copy", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("states the cleanup-only contract when only cleanup models are offered", async () => {
    await toPolishStep([S1_MINI, SPEAKOFLOW]);
    expect(screen.getByText(/A Cleanup-only model can honor Clean and Off/)).toBeTruthy();
    expect(screen.getByText(/cannot restructure a message/)).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: /Ready-to-send emails and docs/ }));
    expect(screen.getByText(/None of the models offered here can do it/)).toBeTruthy();
  });

  it("names a capable model when Structured is picked", async () => {
    await toPolishStep([S1_MINI, QWEN]);
    fireEvent.click(screen.getByRole("radio", { name: /Ready-to-send emails and docs/ }));
    expect(screen.getByText(/Qwen2\.5 3B in the list below can do it/)).toBeTruthy();
  });
});
