/**
 * §9.2 — vitest-axe accessibility tests for the main screens.
 *
 * Renders each screen headlessly inside the established Tauri-mock harness
 * (see smoke.test.tsx) and asserts axe finds no violations.
 *
 * jsdom has no layout engine, so `color-contrast` is disabled: axe cannot
 * compute effective foreground/background contrast without computed CSS
 * layout, and every contrast finding here would be a false positive. No
 * other rule is disabled.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { axe } from "vitest-axe";

import HomeScreen from "../screens/HomeScreen";
import InsightsScreen, { type Insights } from "../screens/InsightsScreen";
import DictionaryScreen from "../screens/DictionaryScreen";
import PersonalizationScreen from "../screens/PersonalizationScreen";
import SettingsScreen from "../screens/SettingsScreen";
import KeybindsScreen from "../screens/KeybindsScreen";
import { mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";


// SettingsScreen imports its rainbow visuals from the pill module. Mock them
// down to the pure exports the screen uses so these accessibility checks stay
// focused on the settings UI, not the pill's visual internals.
vi.mock("../pill", async () => {
  const noop = () => null;
  return {
    default: noop,
    RainbowLips: noop,
    RainbowMeter: noop,
    rainbowColor: (t: number) => `hsl(${Math.round(t * 360)}, 80%, 50%)`,
  };
});

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
// PermissionsUpdatesLicenseSections (rendered inside Settings) subscribes to
// the webview focus event; jsdom has no webview, so stub the window handle.
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onFocusChanged: () => Promise.resolve(() => {}),
  }),
}));

// jsdom limitation: no real layout, so contrast cannot be computed.
const AXE_RULES = { rules: { "color-contrast": { enabled: false } } };

const RUNTIME_STATUS = {
  speech: {
    state: "ready",
    label: "Speech",
    modelId: "test",
    detail: "ready",
    blocking: false,
  },
  llm: {
    state: "none",
    label: "LLM",
    modelId: "test",
    detail: "not loaded",
    blocking: false,
  },
};

function insightsMock(): Insights {
  return {
    topPhrases: [],
    topApps: [],
    appCategories: [],
    avgWordsPerDictation: 0,
    busiestHour: null,
    topDays: [],
    bestDay: null,
    hourBands: [],
    topWords: [],
    habitWords: { total: 0, per1kWords: 0, items: [], trend: 0 },
    vocabularySize: 0,
    dictionaryWords: 0,
    learnedWords: 0,
    totalWords: 0,
    totalDictations: 0,
    streakDays: 0,
    longestStreakDays: 0,
    activeDaysLast7: 0,
    priorWindowWords: null,
    wordsChangePct: null,
    impact: {
      wordsPerMinute: null,
      timesFaster: null,
      timeSavedMinutes: 0,
      timeSavedLabel: "0 min",
      minutesSpoken: null,
      minutesTyped: 0,
      essays: 0,
      ratedTakes: 0,
      skippedTakes: 0,
    },
    records: [],
    milestones: [],
    heatmap: [],
    daily: [],
    wordsToday: 0,
    wordsLast7Days: 0,
    avgWordsPerDay: 0,
    firstActivityAt: null,
    fillerRemoved: 0,
    autotextExpansions: 0,
    topAutotext: [],
    topFillers: [],
    personalizationCorrections: 0,
    dictionaryLearned: 0,
    polishStatus: null,
    learning: null,
  };
}

const SETTINGS_MOCK = {
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
  duckOtherAudio: true,
  duckLevel: 0.2,
  pauseMediaWhileDictating: false,
  pasteLastHotkey: '',
};

/**
 * The invoke commands every covered screen reads on mount. Any command a
 * screen hits that is not listed here rejects loudly (tauriMock default),
 * so the suite fails if a screen gains a new load-time invoke.
 */
const BASE_INVOQUES: Record<string, unknown> = {
  get_settings: SETTINGS_MOCK,
  get_profile: {
    language: "en-US",
    preferences: [],
    learnFromEdits: true,
    learnAppSpecific: true,
    learnTerminology: true,
  },
  get_insights: insightsMock(),
  list_dictation_history: [],
  list_dictionary: [],
  list_packs: [],
  list_user_packs: [],
  list_transforms: [],
  list_input_devices: [],
  list_style_profiles: [],
  get_app_language_overrides: {},
  get_app_style_overrides: {},
  get_transcripts_dir: "",
  get_runtime_status: RUNTIME_STATUS,
};

async function renderScreen(element: React.ReactElement) {
  mockInvokeResponses(BASE_INVOQUES);
  mockListenCapture();
  const utils = render(element);
  // Let every mount-time invoke + state update settle before axe runs.
  await new Promise((r) => setTimeout(r, 0));
  return utils;
}

describe("a11y: main screens have no axe violations", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("HomeScreen (History)", async () => {
    const { container } = await renderScreen(<HomeScreen onNavigate={() => {}} />);
    const results = await axe(container, AXE_RULES);
    expect(results.violations).toEqual([]);
  });

  it("InsightsScreen", async () => {
    const { container } = await renderScreen(<InsightsScreen />);
    const results = await axe(container, AXE_RULES);
    expect(results.violations).toEqual([]);
  });

  it("DictionaryScreen", async () => {
    const { container } = await renderScreen(<DictionaryScreen />);
    const results = await axe(container, AXE_RULES);
    expect(results.violations).toEqual([]);
  });

  it("PersonalizationScreen", async () => {
    const { container } = await renderScreen(<PersonalizationScreen />);
    const results = await axe(container, AXE_RULES);
    expect(results.violations).toEqual([]);
  });

  it("SettingsScreen", async () => {
    const { container } = await renderScreen(<SettingsScreen active={true} />);
    const results = await axe(container, AXE_RULES);
    expect(results.violations).toEqual([]);
  });

  it("KeybindsScreen", async () => {
    const { container } = await renderScreen(<KeybindsScreen />);
    const results = await axe(container, AXE_RULES);
    expect(results.violations).toEqual([]);
  });
});
