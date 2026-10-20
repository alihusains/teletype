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
  selectedSpeechModel: "parakeet-tdt-0.6b-v3",
  language: "en-US",
  inputDevice: "default",
  restoreClipboard: true,
  keepTextOnClipboard: false,
  autoApplyTransform: false,
  showTrayIcon: true,
  hasCompletedOnboarding: true,
  selectedLlmModel: "qwen3-0.6b",
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
  speech: {
    state: "ready",
    label: "Parakeet TDT 0.6B v3",
    modelId: "parakeet-tdt-0.6b-v3",
    detail: "On disk. Loads on your first dictation.",
    blocking: false,
  },
  llm: {
    state: "none",
    label: "Local transform",
    modelId: "",
    detail: "No polish model selected. Dictation still works; Polish will be skipped.",
    blocking: false,
  },
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
    s1Control: { styling: "semi-formal", structure: "lists", context: "general" },
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

// `?demo=1` fills the list screens with representative rows. The default
// preview stays empty so a reviewer sees the real first-run state; the demo is
// only for checking typography, wrapping, and dense rows without the desktop
// backend. It never changes what production builds ship.
const DEMO_RESPONSES: Record<string, unknown> = {
  list_autotext: [
    {
      id: "a1",
      trigger: "/email",
      snippet: "my email",
      replacement: "ali@example.com",
      description: "Personal email",
      enabled: true,
      scope: "everywhere",
      createdAt: 0,
      updatedAt: 0,
    },
    {
      id: "a2",
      trigger: "/sig",
      snippet: "my signature",
      replacement: "Best,\nAli",
      description: "Email sign-off",
      enabled: true,
      scope: "everywhere",
      createdAt: 0,
      updatedAt: 0,
    },
    {
      id: "a3",
      trigger: "/addr",
      snippet: "",
      replacement: "221B Baker Street\nLondon NW1 6XE",
      description: "",
      enabled: false,
      scope: "everywhere",
      createdAt: 0,
      updatedAt: 0,
    },
  ],
  get_usage_stats: { autotextCounts: { "/email": 42, "/sig": 18 } },
  list_system_autotext: [
    { id: "s1", trigger: "/comma", snippet: "comma", replacement: ",", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
    { id: "s2", trigger: "/period", snippet: "period", replacement: ".", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
    { id: "s3", trigger: "/question", snippet: "question mark", replacement: "?", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
    { id: "s4", trigger: "/newline", snippet: "new line", replacement: "\n", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
    { id: "s5", trigger: "/openparen", snippet: "open parenthesis", replacement: "(", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
    { id: "s6", trigger: "/doubleequals", snippet: "double equals", replacement: "==", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
    { id: "s7", trigger: "/dollar", snippet: "dollar sign", replacement: "$", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
    { id: "s8", trigger: "/ellipsis", snippet: "ellipsis", replacement: "…", description: "", enabled: true, scope: "everywhere", createdAt: 0, updatedAt: 0 },
  ],
  list_dictionary: [
    {
      id: "d1",
      word: "Teletype",
      pronunciation: "TEL-uh-type",
      createdAt: 0,
      fuzzy: true,
      aliases: ["tele type", "teletype app"],
      usageCount: 12,
    },
    {
      id: "d2",
      word: "Kubernetes",
      pronunciation: "koo-ber-NET-eez",
      createdAt: 0,
      fuzzy: true,
      aliases: ["kube"],
      usageCount: 4,
    },
    {
      id: "d3",
      word: "Ali Sorathiya",
      pronunciation: "",
      createdAt: 0,
      fuzzy: true,
      aliases: [],
      usageCount: 27,
    },
  ],
  list_packs: [
    {
      id: "pack-tech",
      name: "Tech & product",
      description: "Engineering terms and product names.",
      termCount: 48,
      enabled: true,
    },
    {
      id: "pack-finance",
      name: "Finance",
      description: "Markets, instruments, and reporting terms.",
      termCount: 31,
      enabled: false,
    },
  ],
  list_user_packs: [
    {
      id: "pack-company",
      name: "My company",
      description: "Names and acronyms we use every day.",
      termCount: 6,
      enabled: true,
    },
  ],
  list_pack_terms: [
    { canonical: "Kubernetes", mishearings: ["kubernets", "kube"] },
    { canonical: "PostgreSQL", mishearings: ["postgres", "post gress"] },
    { canonical: "telemetry", mishearings: ["telemetree"] },
  ],
  list_style_profiles: [
    {
      id: "style-concise",
      name: "Concise",
      description: "Short sentences. No filler.",
      stylePhrases: ["be brief", "cut filler", "one idea per sentence"],
      createdAt: 0,
      updatedAt: 0,
    },
    {
      id: "style-professional",
      name: "Professional",
      description: "Polished and neutral for work.",
      stylePhrases: ["use complete sentences", "avoid slang"],
      createdAt: 0,
      updatedAt: 0,
    },
    {
      id: "style-casual",
      name: "Casual",
      description: "Relaxed and friendly.",
      stylePhrases: ["use contractions", "be warm"],
      createdAt: 0,
      updatedAt: 0,
    },
    {
      id: "style-support",
      name: "Support emails",
      description: "Warm, specific, and calm.",
      stylePhrases: ["acknowledge the issue", "end with the next step"],
      createdAt: 0,
      updatedAt: 0,
    },
  ],
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

  const params = new URLSearchParams(window.location.search);
  const demo = params.has("demo");
  const ready = params.has("ready");
  const s1 = params.has("s1");
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
      if (cmd === "get_settings") {
        return clone({
          ...settings,
          ...(s1 ? { selectedLlmModel: "s1-mini" } : {}),
          ...(demo ? { activeStyleProfile: "style-professional" } : {}),
        });
      }
      if (cmd === "get_runtime_status" && ready) {
        return clone({
          ...RUNTIME_STATUS,
          llm: {
            ...RUNTIME_STATUS.llm,
            state: "ready",
            modelId: "qwen3-0.6b",
            label: "Local transform",
            detail: "Loaded and ready for Polish.",
          },
        });
      }
      if (cmd === "save_settings") {
        const next = cmdArgs?.settings;
        if (next && typeof next === "object") {
          settings = { ...settings, ...(next as Record<string, unknown>) };
        }
        return null;
      }
      if (demo && Object.prototype.hasOwnProperty.call(DEMO_RESPONSES, cmd)) {
        return clone(DEMO_RESPONSES[cmd]);
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
