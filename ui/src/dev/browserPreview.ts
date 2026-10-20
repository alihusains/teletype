// Dev-only Tauri IPC shim for reviewing the Vite URL in a normal browser.
//
// The app normally runs inside the Tauri webview, which injects
// `__TAURI_INTERNALS__`. Opening the same Vite URL in Safari/Chrome leaves it
// undefined, and the first invoke() throws before the first screen paints.
// This installs an in-memory stand-in with the same fixtures the screenshot
// harness uses, so every screen renders and settings controls visibly
// round-trip (saves are kept in memory for the session).
//
// Activated from main.tsx only when this is a dev build AND
// `__TAURI_INTERNALS__` is absent. The desktop webview and production builds
// never reach it.

const BASE_SETTINGS: Record<string, unknown> = {
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
  pasteLastHotkey: "",
};

const RUNTIME_STATUS = {
  speech: { state: "ready", label: "Speech", modelId: "test", detail: "ready", blocking: false },
  llm: { state: "none", label: "LLM", modelId: "test", detail: "not loaded", blocking: false },
};

const INSIGHTS = {
  topPhrases: [],
  topApps: [],
  avgWordsPerDictation: 0,
  busiestHour: null,
  vocabularySize: 0,
  dictionaryWords: 0,
  learnedWords: 0,
  totalWords: 0,
  totalDictations: 0,
  streakDays: 0,
  longestStreakDays: 0,
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
  personalizationCorrections: 0,
  dictionaryLearned: 0,
  polishStatus: null,
  learning: null,
};

const RESPONSES: Record<string, unknown> = {
  get_profile: {
    language: "en-US",
    preferences: [],
    learnFromEdits: true,
    learnAppSpecific: true,
    learnTerminology: true,
  },
  get_insights: INSIGHTS,
  get_runtime_status: RUNTIME_STATUS,
  get_app_language_overrides: {},
  get_app_style_overrides: {},
  get_transcripts_dir: "",
  get_permissions: [
    { kind: "microphone", state: "granted" },
    { kind: "accessibility", state: "granted" },
  ],
  get_usage_stats: { autotextCounts: {} },
  get_scratchpad_text: "",
  get_dictation_traces: [],
  get_dictation_trace: null,
  get_logs: [],
  detect_ollama: false,
  has_llm_secret: false,
  list_dictation_history: [],
  list_dictionary: [],
  list_packs: [],
  list_user_packs: [],
  list_transforms: [],
  list_input_devices: [],
  list_style_profiles: [],
  list_autotext: [],
  list_speech_models: [],
  list_openai_providers: [],
};

type Callback = (payload: unknown) => void;

interface PreviewWindow {
  __TAURI_INTERNALS__?: unknown;
  __TAURI_EVENT_PLUGIN_INTERNALS__?: {
    unregisterListener: (event: string, eventId: number) => void;
  };
  __TELE_E2E_EMIT__?: (event: string, payload: unknown) => void;
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

export function installBrowserPreview(): void {
  const w = window as unknown as PreviewWindow;
  if (w.__TAURI_INTERNALS__) return;

  let settings = { ...BASE_SETTINGS };
  let nextId = 1;
  const callbacks: Record<number, Callback | null> = {};
  const listeners: Record<number, string> = {};
  const globals = window as unknown as Record<string, unknown>;

  w.__TELE_E2E_EMIT__ = (event, payload) => {
    for (const [id, name] of Object.entries(listeners)) {
      if (name === event) callbacks[Number(id)]?.({ event, id: Number(id), payload });
    }
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (event, eventId) => {
      if (listeners[eventId] === event) delete listeners[eventId];
    },
  };

  w.__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, cmdArgs?: Record<string, unknown>) => {
      if (cmd === "plugin:event|listen") {
        const id = nextId++;
        const handler = Number(cmdArgs?.handler);
        callbacks[id] = (globals[`_${handler}`] as Callback | undefined) ?? null;
        listeners[id] = String(cmdArgs?.event ?? "");
        return id;
      }
      if (cmd === "plugin:event|unlisten") return null;
      if (cmd === "get_settings") return clone(settings);
      if (cmd === "save_settings") {
        const next = cmdArgs?.settings;
        if (next && typeof next === "object") {
          settings = { ...settings, ...(next as Record<string, unknown>) };
        }
        return null;
      }
      if (Object.prototype.hasOwnProperty.call(RESPONSES, cmd)) {
        return clone(RESPONSES[cmd]);
      }
      if (cmd.startsWith("plugin:")) return null;
      if (cmd.startsWith("list_")) return [];
      if (cmd.startsWith("get_")) return {};
      return null;
    },
    transformCallback: (cb: Callback) => {
      const id = nextId++;
      callbacks[id] = cb;
      globals[`_${id}`] = cb;
      return id;
    },
    unregisterCallback: (id: number) => {
      delete callbacks[id];
    },
    convertFileSrc: (path: string) => path,
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
      windows: [],
      webviews: [],
    },
  };

  // eslint-disable-next-line no-console
  console.info("[teletype] browser preview shim active: sample data, settings persist in memory.");
}
