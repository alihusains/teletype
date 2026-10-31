// claim: ui-behavior
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import SettingsScreen from "../screens/SettingsScreen";
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
  tapToPasteClipboard: false,
  autoApplyTransform: false,
  showTrayIcon: true,
  hasCompletedOnboarding: true,
  selectedLlmModel: "other-model",
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
  vadAutoStop: true,
  vadSilenceMs: 700,
  noiseReductionEnabled: false,
  enableDeveloperTab: false,
  polishGateEnabled: false,
  polishGateThresholdWords: 10,
  polishMaxChangePercent: 40,
  restoreEmoji: false,
  spokenEmoji: false,
  spokenPunctuation: false,
  modelUnloadDelaySecs: 60,
  activeStyleProfile: "default",
  appLanguageOverrides: {},
  enabledPacks: [],
  selectedLlmProvider: "builtin",
  theme: "system",
  reduceMotion: false,
  polishRules: {},
  listStyle: "bullets",
  quickAddHotkey: "Cmd+Shift+K",
  commandHotkey: "Cmd+Shift+.",
  commandWakePhrase: false,
  pendingNotice: null,
  polishWholeTextAtEnd: false,
  polishSkipWhenClean: true,
  instantStart: "always",
  preferBuiltinMicWithBt: false,
  audioConditioning: false,
  learnFromEdits: true,
  learnNamesFromScreen: true,
  keepAudioForSelfTests: false,
};

function seed(overrides: Record<string, unknown> = {}) {
  const settings = { ...SETTINGS, ...overrides };
  mockInvokeResponses({
    get_settings: settings,
    save_settings: settings,
    list_input_devices: [],
    list_style_profiles: [],
    get_app_language_overrides: {},
    get_app_style_overrides: {},
    get_transcripts_dir: "",
    get_runtime_status: {
      speech: { state: "ready", label: "Speech", modelId: "test", detail: "ready", blocking: false },
      llm: { state: "none", label: "LLM", modelId: "test", detail: "not loaded", blocking: false },
    },
  });
}

async function renderSettings() {
  mockListenCapture();
  render(<SettingsScreen active={false} />);
  await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());
}

describe("Polish change ceiling", () => {
  beforeEach(() => {
    resetTauriMocks();
    seed();
  });

  it("defaults to 40% and names the setting in plain language", async () => {
    await renderSettings();
    const slider = screen.getByLabelText("Polish change limit") as HTMLInputElement;
    expect(slider.value).toBe("40");
    const row = slider.closest("[data-settings-label]");
    expect(row?.textContent).toContain(
      "Let Polish change up to 40% of the words",
    );
  });

  it("saves a raised ceiling", async () => {
    await renderSettings();

    fireEvent.change(screen.getByLabelText("Polish change limit"), {
      target: { value: "70" },
    });

    await waitFor(() => {
      const saved = invokeCalls().find((c) => c.cmd === "save_settings");
      expect(saved).toBeTruthy();
      const payload = saved?.args as {
        settings: { polishMaxChangePercent: number };
      };
      expect(payload.settings.polishMaxChangePercent).toBe(70);
    });
    const slider = screen.getByLabelText("Polish change limit") as HTMLInputElement;
    expect(slider.value).toBe("70");
    expect(slider.closest("[data-settings-label]")?.textContent).toContain(
      "Let Polish change up to 70% of the words",
    );
  });

  it("turns the ceiling off at 100%", async () => {
    await renderSettings();

    fireEvent.change(screen.getByLabelText("Polish change limit"), {
      target: { value: "100" },
    });

    const row = screen
      .getByLabelText("Polish change limit")
      .closest("[data-settings-label]");
    expect(row?.textContent).toContain("Let Polish rewrite freely");
    expect(row?.textContent).toContain("Off");
    await waitFor(() => {
      const saved = invokeCalls().find((c) => c.cmd === "save_settings");
      const payload = saved?.args as {
        settings: { polishMaxChangePercent: number };
      };
      expect(payload.settings.polishMaxChangePercent).toBe(100);
    });
  });
});
