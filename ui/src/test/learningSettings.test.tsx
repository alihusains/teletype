/**
 * P7.8 — Settings "Learning" section: the three learning toggles, the
 * disabled Contacts-import row, and the destructive "Delete all learning
 * data" action wired to the delete_learning_data command.
 *
 * Uses the same Tauri-mock harness as smoke.test.tsx / a11y.test.tsx.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import SettingsScreen from "../screens/SettingsScreen";
import { mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
// SettingsScreen imports its rainbow visuals from the pill module. Mock them
// down to the pure exports the screen uses so these settings checks stay
// focused on behavior, not the pill's visual internals.
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

// The full, valid settings object the screen reads on mount. Mirrors the
// Rust Settings struct (camelCase) including the three new learning fields.
const SETTINGS_MOCK = {
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

const BASE_INVOQUES: Record<string, unknown> = {
  get_settings: SETTINGS_MOCK,
  list_input_devices: [],
  list_style_profiles: [],
  get_app_language_overrides: {},
  get_app_style_overrides: {},
  get_transcripts_dir: "",
  get_runtime_status: {
    speech: { state: "ready", label: "Speech", modelId: "test", detail: "ready", blocking: false },
    llm: { state: "none", label: "LLM", modelId: "test", detail: "not loaded", blocking: false },
  },
};

async function renderSettings() {
  mockInvokeResponses(BASE_INVOQUES);
  mockListenCapture();
  const utils = render(<SettingsScreen active={false} />);
  await waitFor(() => {
    expect(screen.queryByText("Loading…")).toBeNull();
  });
  return utils;
}

function savedSettings(call: number): Record<string, unknown> | undefined {
  const calls = vi.mocked(invoke).mock.calls.filter((c) => c[0] === "save_settings");
  const c = calls[call];
  return c ? (c[1] as { settings: Record<string, unknown> }).settings : undefined;
}

describe("Settings Learning section (P7.8)", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("renders the learning toggle, the working Contacts import, and the delete button", async () => {
    await renderSettings();
    expect(screen.getByText("Learn from my edits")).toBeTruthy();
    expect(screen.getByText("Import from Contacts…")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Delete all learning data" })).toBeTruthy();
  });

  it("shows no control for a setting the engine never reads", async () => {
    // Both rows were removed rather than left saving a value nothing consumes;
    // the fields stay in the Settings shape because the backend persists them.
    await renderSettings();
    expect(screen.queryByText("Learn names from screen")).toBeNull();
    expect(screen.queryByText(/Keep audio for self-tests/)).toBeNull();
    expect(screen.queryByText("Coming in a later release")).toBeNull();
  });

  it("toggling a learning checkbox saves the flipped flag", async () => {
    await renderSettings();
    const toggle = screen.getByRole("switch", { name: "Learn from my edits" });
    toggle.click();
    await waitFor(() => {
      const saved = savedSettings(0);
      expect(saved).toBeTruthy();
      expect(saved!.learnFromEdits).toBe(false);
      // The other two flags survive the round trip untouched.
      expect(saved!.learnNamesFromScreen).toBe(true);
      expect(saved!.keepAudioForSelfTests).toBe(false);
    });
  });

  it("deleting learning data confirms, invokes the command, and shows a note", async () => {
    // The delete command is not in the base mock: program it per-test.
    mockInvokeResponses({ ...BASE_INVOQUES, delete_learning_data: null });
    // The screen subscribes to runtime-status; without a capture `listen`
    // returns undefined and `useTauriEvent` throws on mount.
    mockListenCapture();
    render(<SettingsScreen active={false} />);
    await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    screen.getByRole("button", { name: "Delete all learning data" }).click();
    await waitFor(() => {
      const calls = vi.mocked(invoke).mock.calls.filter(
        (c) => c[0] === "delete_learning_data"
      );
      expect(calls).toHaveLength(1);
    });
    expect(await screen.findByText(/Learning data deleted/i)).toBeTruthy();
    expect(confirmSpy).toHaveBeenCalledTimes(1);
    confirmSpy.mockRestore();
  });
});
