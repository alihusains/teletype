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

// Type-only, so this adds no runtime edge from the preview shim into a screen.
import type { Settings } from "../screens/SettingsScreen";

// Typed as `Settings` rather than `Record<string, unknown>` on purpose: the
// fixture used to be missing keys the real `get_settings` returns, and a screen
// doing `setX(s.field)` then set a defined value to `undefined`, which React
// reports as a controlled input going uncontrolled. `tsc --noEmit` is now the
// guard, so a new required setting has to be added here or the typecheck fails.
const BASE_SETTINGS: Settings = {
  hotkey: "Cmd+Shift+Space",
  recordingMode: "auto",
  selectedSpeechModel: "parakeet-tdt-0.6b-v3",
  language: "en-US",
  inputDevice: "default",
  restoreClipboard: true,
  keepTextOnClipboard: false,
  tapToPasteClipboard: false,
  autoApplyTransform: false,
  showTrayIcon: true,
  hasCompletedOnboarding: true,
  selectedLlmModel: "qwen3-0.6b",
  typingAutotextEnabled: false,
  autotextTiming: "immediate",
  removeFillerWords: true,
  fillerWords: [],
  fillerStrength: "standard",
  pillPosition: "bottomCenter",
  alwaysShowPill: false,
  livePreviewEnabled: false,
  cancelHotkey: "Escape",
  smartInsertion: true,
  playRecordingSounds: false,
  recordingSound: "ding",
  warmEnginePolicy: "keep",
  matchStrictness: "standard",
  dictionaryEnabled: true,
  pillStyle: "classic",
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
  // Every key the real `get_settings` returns has to be here. These three were
  // missing, and the screens that read them do so with no fallback.
  scratchpadEnabled: false,
  openaiBaseUrl: "https://api.openai.com/v1",
  openaiModel: "gpt-4o-mini",
  speechEngine: "parakeet",
  commandModeEnabled: false,
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

// A clock fixed at module load, so a demo session's rows stay in a sensible
// order instead of drifting as each call re-reads Date.now().
const demoNow = Date.now();
const demoDay = 1000 * 60 * 60 * 24;

// Stress data for the Insights cards: huge counters, long labels, and more
// days than the chart has bars. If any card breaks layout or the page grows a
// horizontal scrollbar, it shows up here.
const DEMO_INSIGHTS = {
  ...INSIGHTS,
  topPhrases: [
    { label: "Let me know if you have any questions or need anything further from my side", count: 41, share: 0.34 },
    { label: "Thanks for the quick turnaround", count: 27, share: 0.22 },
    { label: "Can you send me the latest numbers for the quarterly business review", count: 19, share: 0.16 },
    { label: "Sounds good", count: 12, share: 0.1 },
  ],
  topApps: [
    { label: "Slack — #platform-eng-and-deployments", count: 312, share: 0.38 },
    { label: "Gmail", count: 241, share: 0.29 },
    { label: "File: TTSOL-en-A-2026-Q3-board-readout-final-v2-REVIEWED.wav", count: 88, share: 0.11 },
    { label: "Google Docs", count: 61, share: 0.07 },
  ],
  avgWordsPerDictation: 1284,
  busiestHour: 11,
  appCategories: [
    { label: "Chat", count: 1240, share: 1.0 },
    { label: "Email", count: 880, share: 0.71 },
    { label: "Code", count: 420, share: 0.34 },
    { label: "Docs", count: 210, share: 0.17 },
    { label: "Other", count: 90, share: 0.07 },
  ],
  activeDaysLast7: 7,
  priorWindowWords: 98765,
  wordsChangePct: 24.8,
  topDays: [
    { label: "Monday", count: 3200, share: 1.0 },
    { label: "Wednesday", count: 2800, share: 0.875 },
    { label: "Friday", count: 2400, share: 0.75 },
  ],
  bestDay: "Monday",
  hourBands: [
    { label: "Morning", count: 5200, share: 1.0 },
    { label: "Afternoon", count: 4600, share: 0.885 },
    { label: "Evening", count: 2100, share: 0.404 },
    { label: "Late night", count: 940, share: 0.181 },
  ],
  topWords: [
    { label: "project", count: 312, share: 1.0 },
    { label: "client", count: 287, share: 0.92 },
    { label: "deadline", count: 244, share: 0.78 },
    { label: "review", count: 198, share: 0.63 },
    { label: "launch", count: 156, share: 0.5 },
    { label: "quarter", count: 134, share: 0.43 },
    { label: "stakeholder", count: 112, share: 0.36 },
    { label: "pipeline", count: 98, share: 0.31 },
  ],
  habitWords: {
    total: 23,
    per1kWords: 1.8,
    items: [
      { label: "like", count: 12, share: 1.0 },
      { label: "basically", count: 7, share: 0.58 },
      { label: "honestly", count: 4, share: 0.33 },
    ],
    trend: -1,
  },
  vocabularySize: 12345678,
  dictionaryWords: 1234,
  learnedWords: 567890,
  totalWords: 1234567890,
  totalDictations: 98765,
  streakDays: 1234,
  longestStreakDays: 5678,
  impact: {
    wordsPerMinute: 1284,
    timesFaster: 999,
    timeSavedMinutes: 1234567,
    timeSavedLabel: "20,576 h",
    minutesSpoken: 987654,
    minutesTyped: 12345678,
    essays: 12345,
    ratedTakes: 6789,
    skippedTakes: 1234,
  },
  records: [
    { label: "Longest dictation", value: "4,128,903 words" },
    { label: "Fastest turnaround", value: "1,234,567 ms" },
    { label: "Biggest single day", value: "987,654,321" },
  ],
  milestones: [
    {
      category: "Words written",
      current: 1234567890,
      items: [
        { label: "1 thousand", threshold: 1000, reached: true },
        { label: "1 million", threshold: 1000000, reached: true },
        { label: "1 billion", threshold: 1000000000, reached: true },
      ],
    },
    {
      category: "Day streak",
      current: 1234,
      items: [
        { label: "7 days", threshold: 7, reached: true },
        { label: "100 days", threshold: 100, reached: true },
        { label: "1,000 days", threshold: 1000, reached: true },
      ],
    },
  ],
  heatmap: Array.from({ length: 200 }, (_, i) => {
    // Weekends stay quiet so the preview reads like a real working week.
    const weekend = i % 7 === 5 || i % 7 === 6;
    const words = weekend ? (i % 3) * 40 : 380 + ((i * 137) % 2600);
    return {
      date: demoNow - (200 - i) * demoDay,
      words,
      level: Math.min(4, Math.floor(words / 620)),
    };
  }),
  daily: Array.from({ length: 90 }, (_, i) => ({
    label: new Date(demoNow - (90 - i) * demoDay).toLocaleDateString(undefined, { month: "short", day: "numeric" }),
    date: demoNow - (90 - i) * demoDay,
    words: 12345 + ((i * 977) % 40000),
    dictations: 1234 + (i % 90),
  })),
  wordsToday: 1234567890,
  wordsLast7Days: 98765432,
  avgWordsPerDay: 1234567,
  firstActivityAt: demoNow - 400 * demoDay,
  fillerRemoved: 123456,
  autotextExpansions: 987654,
  topAutotext: [
    { label: "/email", count: 3120, share: 1.0 },
    { label: "my signature", count: 1540, share: 0.49 },
    { label: "sounds good", count: 640, share: 0.21 },
    { label: "meeting link", count: 210, share: 0.07 },
  ],
  topFillers: [
    { label: "um", count: 5420, share: 1.0 },
    { label: "uh", count: 3110, share: 0.57 },
    { label: "like", count: 2100, share: 0.39 },
    { label: "you know", count: 880, share: 0.16 },
  ],
  personalizationCorrections: 5678,
  dictionaryLearned: 34567,
  // The LearningCard renders nothing without this, so the dev preview would
  // never exercise it. Aliases are deliberately long: the row truncates.
  learning: {
    editRate7d: 18.34,
    keptCorrections: 61,
    undoneCorrections: 9,
    daily: Array.from({ length: 30 }, (_, i) => ({
      day: demoNow - (30 - i) * demoDay,
      words: 8000 + ((i * 613) % 12000),
      editedWords: 2200 - i * 62 + ((i * 37) % 300),
    })),
    accuracy: Array.from({ length: 6 }, (_, i) => ({
      ts: demoNow - (6 - i) * 7 * demoDay,
      werBefore: 0.182 - i * 0.011,
      werAfter: 0.141 - i * 0.009,
    })),
    termsLearnedWeek: [
      { term: "PyTorch", aliases: ["pie torch", "pi touch"], learnedAt: demoNow - 2 * demoDay },
      { term: "Kubernetes", aliases: ["coon a pettis"], learnedAt: demoNow - 4 * demoDay },
      {
        term: "TTSOL-en-A-2026-Q3-board-readout-final-v2",
        aliases: ["t t s o l en a twenty twenty six q three board readout final version two"],
        learnedAt: demoNow - 6 * demoDay,
      },
    ],
    topFixes: [
      { from: "pie torch", to: "PyTorch", count: 34567 },
      { from: "coon a pettis", to: "Kubernetes", count: 8912 },
      { from: "gRPC endpoint", to: "gRPC", count: 234 },
      { from: "the quarterly business review deck", to: "QBR deck", count: 12 },
    ],
  },
};

const RESPONSES: Record<string, unknown> = {
  get_profile: {
    language: "en-US",
    // Four shapes the Personalization tab renders differently: a rule you wrote,
    // a word correction it learned (with the heard/written pair), an app-scoped
    // rule, and one observation that is not in force yet.
    preferences: [
      {
        id: "demo-pref-explicit",
        description: "Always write Q3, not quarter three",
        phrase: "write Q3, not quarter three",
        explicit: true,
        scope: "global",
        count: 0,
        createdAt: demoNow - 21 * demoDay,
        updatedAt: demoNow - 21 * demoDay,
      },
      {
        id: "demo-pref-term",
        description: "You write Kubernetes, the model hears kubernetes",
        phrase: "use Kubernetes",
        explicit: false,
        scope: "global",
        count: 6,
        createdAt: demoNow - 12 * demoDay,
        updatedAt: demoNow - 2 * demoDay,
        learnedFrom: "kubernetes",
        learnedTo: "Kubernetes",
      },
      {
        id: "demo-pref-email",
        description: "Open an email with Hi, not Dear",
        phrase: "open with Hi",
        explicit: false,
        scope: { appType: "email" },
        count: 3,
        createdAt: demoNow - 9 * demoDay,
        updatedAt: demoNow - 3 * demoDay,
      },
      {
        id: "demo-pref-waiting",
        description: "Drop just from a request",
        phrase: "drop just",
        explicit: false,
        scope: { appType: "chat" },
        count: 1,
        createdAt: demoNow - demoDay,
        updatedAt: demoNow - demoDay,
      },
    ],
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
  list_notes: [],
  get_open_note: null,
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
      scope: "Gmail",
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
  // Long names and long bodies: the history rows must wrap, never clip.
  list_dictation_history: [
    {
      id: "h1",
      createdAt: demoNow - 1000 * 60 * 8,
      text: "Just a reminder that the Q3 board readout is drafted and I would like your comments before Thursday, because legal reviews it Friday and the deck has to go to the printer over the weekend.",
      appName: "File: TTSOL-en-A-2026-Q3-board-readout-final-v2-REVIEWED.wav",
      appType: "Document",
    },
    {
      id: "h2",
      createdAt: demoNow - 1000 * 60 * 63,
      text: "Thanks for the quick turnaround on the invoice, that clears it for this month.",
      appName: "Gmail",
      appType: "Email",
    },
    {
      id: "h3",
      createdAt: demoNow - 1000 * 60 * 60 * 5,
      text: "The migration is behind because the schema check keeps timing out on staging; I am going to pin the older client until the platform team ships the fix they promised last sprint.",
      appName: "Slack — #platform-eng-and-deployments",
      appType: "Chat",
    },
  ],
  list_transforms: [
    {
      id: "builtin-polish",
      name: "Polish",
      description: "Clean up wording, keep the meaning.",
      instruction: "Rewrite for clarity. Keep every fact.",
      shortcut: "",
      enabled: true,
      builtIn: true,
      autoApply: true,
    },
    {
      id: "builtin-filler",
      name: "Remove filler",
      description: "Drop ums, likes and you-knows.",
      instruction: "Remove filler words.",
      shortcut: "",
      enabled: true,
      builtIn: true,
      autoApply: false,
    },
    {
      id: "builtin-summary",
      name: "Summarise",
      description: "Compress into the key points.",
      instruction: "Summarise in three bullets.",
      shortcut: "Cmd+Shift+J",
      enabled: false,
      builtIn: true,
      autoApply: false,
    },
  ],
  // Rows copied from the real catalogs (`teletype-speech/src/catalog.rs`,
  // `teletype-inference/src/catalog.rs`) so the Models screen can be reviewed
  // without the desktop backend, including the split between polish engines
  // and the two Command Mode rows that carry `polishCapable: false`.
  list_speech_models: [
    {
      id: "parakeet-ultra-coreml",
      name: "Parakeet Ultra (Neural Engine)",
      engine: "ParakeetCoreml",
      engineLabel: "Neural Engine (Core ML)",
      platformLabel: "Apple Silicon, macOS 14+",
      sizeMb: 600,
      description:
        "Highest accuracy on Mac, runs on the Neural Engine. Default on macOS 14+ Apple Silicon.",
      recommended: true,
      englishOnly: false,
      downloaded: true,
      selected: true,
      speed: 9.9,
      accuracy: 9.6,
      minRamGb: 4,
      languageLabel: "25 languages",
    },
    {
      id: "fluid-speech-mini",
      name: "Fluid Speech Mini",
      engine: "ParakeetCpp",
      engineLabel: "parakeet.cpp",
      platformLabel: "macOS only",
      sizeMb: 347,
      description:
        "Compact English-only build. Loads in under a second, but less accurate than Parakeet Ultra on noisy or accented speech.",
      recommended: false,
      englishOnly: true,
      downloaded: true,
      selected: false,
      speed: 9.7,
      accuracy: 8.8,
      minRamGb: 2,
      languageLabel: "English only",
    },
    {
      id: "parakeet-tdt-v3",
      name: "Parakeet v3 (TDT)",
      engine: "Parakeet",
      engineLabel: "ggml Parakeet",
      platformLabel: "macOS only",
      sizeMb: 640,
      description:
        "Runs on the GPU via ggml. Default on Intel Macs and macOS 13. English and 24 European languages.",
      recommended: false,
      englishOnly: false,
      downloaded: false,
      selected: false,
      speed: 9.7,
      accuracy: 9.2,
      minRamGb: 4,
      languageLabel: "25 languages",
    },
    {
      id: "large-v3-turbo-q5",
      name: "Whisper large-v3 turbo",
      engine: "Whisper",
      engineLabel: "whisper.cpp",
      platformLabel: "macOS only",
      sizeMb: 547,
      description: "Use when your language is not covered by Parakeet. 99 languages.",
      recommended: false,
      englishOnly: false,
      downloaded: false,
      selected: false,
      speed: 7.5,
      accuracy: 9.4,
      minRamGb: 4,
      languageLabel: "99 languages",
    },
  ],
  list_models: [
    {
      id: "s1-mini",
      name: "S1-mini",
      sizeMb: 462,
      description:
        "Small open cleanup model, happiest in English. Pairs with Tone / Structure styles.",
      specialty: { tag: "Dictation cleanup", measured: true },
      downloaded: true,
      selected: true,
      recommended: true,
      polishCapable: true,
      verdict: "Recommended",
      verdictNote: "Best for English: small, fast, and tuned for dictation cleanup",
      licenseName: "Apache-2.0",
      licenseUrl:
        "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE",
      requiresLicenseAccept: false,
      attribution: "S1-mini by Superwhisper",
    },
    {
      id: "speakoflow-mini",
      name: "SpeakoFlow Mini",
      sizeMb: 834,
      description:
        "Purpose-built dictation cleanup: fixes grammar, spelling, fillers and spoken symbols; leaves correct text and names/numbers untouched. A cleanup model, not a rewriter.",
      specialty: { tag: "Dictation cleanup", measured: true },
      downloaded: true,
      selected: false,
      recommended: true,
      polishCapable: true,
      verdict: "Recommended",
      verdictNote:
        "Reliable cleanup: restrained, preserves names/numbers, fixes self-corrections",
      licenseName: "Apache-2.0",
      licenseUrl: "https://huggingface.co/SpeakoFlow/speakoflow-mini/blob/main/LICENSE",
      requiresLicenseAccept: false,
      attribution: "SpeakoFlow Mini by SpeakoFlow (Apache-2.0)",
    },
    {
      id: "qwen3.5-2b",
      name: "Qwen3.5 2B",
      sizeMb: 1173,
      description:
        "The all-rounder: cleans up, tightens, rewords and structures dictation, honors the Polish settings, and runs Prompt Engineer.",
      specialty: { tag: "Polish + Prompt Engineer", measured: true },
      downloaded: false,
      selected: false,
      recommended: false,
      polishCapable: true,
      verdict: "Recommended",
      verdictNote: "Best all-rounder: tightens, rewords and structures Polish, plus Prompt Engineer",
      licenseName: "Apache-2.0",
      licenseUrl: "https://huggingface.co/Qwen/Qwen3.5-2B/blob/main/LICENSE",
      requiresLicenseAccept: false,
      attribution: "Qwen3.5 by Alibaba (Apache-2.0)",
    },
    {
      id: "qwen2.5-3b",
      name: "Qwen2.5 3B",
      sizeMb: 1900,
      description: "Best local model in our tests. Good for style profiles and transforms.",
      specialty: { tag: "Instruction-following transforms", measured: false },
      downloaded: false,
      selected: false,
      recommended: true,
      polishCapable: true,
      verdict: "Recommended",
      verdictNote: "Best in our tests, may follow dictated instructions",
      licenseName: "Apache-2.0",
      licenseUrl: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/blob/main/LICENSE",
      requiresLicenseAccept: false,
      attribution: "Qwen2.5 by Alibaba (Apache-2.0)",
    },
    {
      id: "gemma2-2b",
      name: "Gemma 2 2B",
      sizeMb: 1500,
      description: "Google's small model. Mixed results with non-English dictation.",
      specialty: { tag: "Small & fast", measured: false },
      downloaded: false,
      selected: false,
      recommended: false,
      polishCapable: true,
      verdict: "Mixed results",
      verdictNote: "Mixed results, often mishandles other languages",
      licenseName: "Google Gemma Terms of Use",
      licenseUrl: "https://ai.google.dev/gemma/terms",
      requiresLicenseAccept: false,
      attribution: "Gemma 2 by Google (Google License)",
    },
    {
      id: "neudecide-q4",
      name: "NeuDecide Q4 (decider)",
      sizeMb: 41,
      description: "Speech-to-tool-call decider for Command Mode (ONNX, runs in-process).",
      specialty: { tag: "Command Mode decider", measured: false },
      downloaded: true,
      selected: false,
      recommended: false,
      polishCapable: false,
      verdict: "Not tested",
      verdictNote: "Default Command Mode decider (Addendum A); not measured on our corpus yet.",
      licenseName: "Apache-2.0",
      licenseUrl: "https://huggingface.co/neuphonic/neudecide/blob/main/README.md",
      requiresLicenseAccept: false,
      attribution: "NeuDecide by neuphonic (Apache-2.0)",
    },
  ],
  // Mirrors StyleResolution in commands/personalization.rs. The `capability`
  // field is added per request by `styleCapability`, because it depends on
  // which model the URL asked for.
  style_resolution: {
    frontmostApp: "Gmail",
    frontmostAppType: "email",
    layers: [
      {
        rank: 1,
        key: "explicit",
        label: "This dictation's pick",
        styleId: "",
        styleName: "",
        winner: false,
        note: "Only applies when you choose a style for one dictation. Nothing is picked now.",
      },
      {
        rank: 2,
        key: "app_override",
        label: "This app's override",
        styleId: "style-support",
        styleName: "Support emails",
        winner: true,
        note: "You pinned a style for Gmail.",
      },
      {
        rank: 3,
        key: "category_default",
        label: "Automatic for this kind of app",
        styleId: "style-professional",
        styleName: "Professional",
        winner: false,
        note: "Email and documents use Professional automatically.",
      },
      {
        rank: 4,
        key: "global",
        label: "Global pick",
        styleId: "style-professional",
        styleName: "Professional",
        winner: false,
        note: "The same as the automatic choice, so nothing changes.",
      },
    ],
    effectivePhrases: [
      "acknowledge the issue",
      "end with the next step",
      "use complete sentences",
    ],
  },
  preview_style_profile:
    "I have drafted the Q3 board readout. Please send your comments before Thursday; legal reviews it on Friday and the deck goes to print over the weekend.",
  get_app_style_overrides: { "com.google.Chrome": "style-casual", Slack: "style-concise" },
  get_insights: DEMO_INSIGHTS,
};

/**
 * Mirrors `teletype_core::style::model_style`, so the Style tab can be reviewed
 * for every model class without loading a model: `?demo` picks a model that
 * reads style phrases, `?demo&s1` and `?demo&qwen35` pick the control-line
 * models, and SpeakoFlow Mini (the shipped default) reads neither.
 */
const STYLE_CHANNELS: Record<string, "fixedBehaviour" | "controlLine" | "stylePhrases"> = {
  "s1-mini": "controlLine",
  "qwen3.5-2b": "controlLine",
  "speakoflow-mini": "fixedBehaviour",
};

function styleCapability(modelId: string) {
  const rows = (DEMO_RESPONSES.list_models ?? []) as { id: string; name: string }[];
  const channel = STYLE_CHANNELS[modelId] ?? "stylePhrases";
  return {
    modelId,
    modelName: rows.find((r) => r.id === modelId)?.name ?? (modelId || "No model selected"),
    channel,
    // A cleanup-only fine-tune runs nothing but Polish, so phrases never reach
    // its prompt; the 2B takes them outside Polish.
    stylesApply: channel === "stylePhrases" || modelId === "qwen3.5-2b",
    readsControlLine: channel === "controlLine",
    compareMeaningful: channel === "stylePhrases",
  };
}

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

// ?latency reviews the U2 per-stage latency panel with numbers that are not
// invented: this is the exact `latency_summary` output over the 200 most
// recent real traces on the author's machine, read back through the Rust
// aggregator rather than recomputed by hand. The all-lengths total row stays
// over budget, and the short-take figure is scoped to the engine the newest
// take ran on.
const LATENCY_SUMMARY = {
  window: 200,
  releaseToPasteP95MsMax: 700,
  shortTakeP95Ms: 892,
  shortTakeCount: 41,
  shortTakeAudioMsMax: 5000,
  shortTakeEngine: "parakeet-ultra-coreml",
  stages: [
    { stage: "capture", label: "Capture", order: 0, userFacing: true, p50Ms: 1902, p95Ms: 27687, sampleCount: 185 },
    { stage: "asr", label: "Transcribe", order: 1, userFacing: true, p50Ms: 831, p95Ms: 5645, sampleCount: 131 },
    { stage: "polish_first_token", label: "Polish (first token)", order: 2, userFacing: false, p50Ms: 92, p95Ms: 464, sampleCount: 83 },
    { stage: "polish", label: "Polish (full)", order: 3, userFacing: true, p50Ms: 200, p95Ms: 1250, sampleCount: 82 },
    { stage: "inject", label: "Inject", order: 4, userFacing: true, p50Ms: 58, p95Ms: 198, sampleCount: 123 },
    { stage: "total", label: "Release to paste", order: 5, userFacing: true, p50Ms: 882, p95Ms: 3648, sampleCount: 117 },
  ],
};

// ?latency=within reviews the same panel when the promise is met, the state a
// shipped build should reach. Only the short-take figure changes; the table
// rows are the same measured numbers.
const LATENCY_WITHIN = {
  ...LATENCY_SUMMARY,
  shortTakeP95Ms: 640,
};

// ?latency=sparse reviews the same panel on a machine with no polish model:
// both polish rows are unmeasured, so the panel must show a hyphen, never a
// zero, and the total must still render. The short-take figure also loses its
// engine scope here, which is the second copy of the sentence.
const LATENCY_SPARSE = {
  ...LATENCY_SUMMARY,
  shortTakeEngine: null,
  shortTakeP95Ms: 1399,
  shortTakeCount: 82,
  stages: LATENCY_SUMMARY.stages.map((s) =>
    s.stage.startsWith("polish") ? { ...s, p50Ms: null, p95Ms: null, sampleCount: 0 } : s
  ),
};

export function installBrowserPreview(): void {
  const w = window as unknown as PreviewWindow;
  if (w.__TAURI_INTERNALS__) return;

  const params = new URLSearchParams(window.location.search);
  const demo = params.has("demo");
  const ready = params.has("ready");
  const s1 = params.has("s1");
  // Reviews the Styles axes as the all-rounder model sees them; without this
  // the preview's default model takes neither the S1 nor the 2B branch.
  const qwen35 = params.has("qwen35");
  // Reviews the Style tab as the shipped default model sees it: SpeakoFlow Mini
  // reads neither the phrases nor the control line, and no other flag shows that.
  const speako = params.has("speako");
  const onboarding = params.has("onboarding");
  // Reviews the CPU-only llama-server build (Windows, Intel Mac) on Apple
  // Silicon, where the polish rows a host cannot carry would otherwise all
  // report true and the "slow on this computer" label would never render.
  const cpuHost = params.has("cpu");
  // Reviews the U2 latency panel with real measured numbers (`?latency`), with
  // polish unmeasured (`?latency=sparse`), or with the budget met
  // (`?latency=within`). Without the flag the backend stub returns no rows and
  // the panel shows its empty state.
  const latency = params.get("latency");
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
          ...(onboarding ? { hasCompletedOnboarding: false } : {}),
          ...(s1 ? { selectedLlmModel: "s1-mini" } : {}),
          ...(qwen35 ? { selectedLlmModel: "qwen3.5-2b" } : {}),
          ...(speako ? { selectedLlmModel: "speakoflow-mini" } : {}),
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
      if (cmd === "get_latency_summary" && latency !== null) {
        return clone(
          latency === "sparse" ? LATENCY_SPARSE : latency === "within" ? LATENCY_WITHIN : LATENCY_SUMMARY
        );
      }
      if (cmd === "save_settings") {
        const next = cmdArgs?.settings;
        if (next && typeof next === "object") {
          settings = { ...settings, ...(next as Record<string, unknown>) };
        }
        return null;
      }
      if (cmd === "set_app_icon") {
        // Mirrors the Rust command: persist the id and push settings-changed so
        // the sidebar icon and this screen's checkmark both move.
        const id = String(cmdArgs?.id ?? "");
        if (id) settings = { ...settings, appIcon: id };
        w.__TELE_E2E_EMIT__?.("settings-changed", undefined);
        return null;
      }
      if (cmd === "list_speech_models") {
        const fromDemo = demo && Object.prototype.hasOwnProperty.call(DEMO_RESPONSES, cmd);
        const table = fromDemo ? DEMO_RESPONSES : RESPONSES;
        const rows = (table[cmd] ?? []) as { id: string }[];
        // The desktop backend shortlists the first-run rows in Rust
        // (`catalog::shortlist_ids`). The rows below are the canned Mac catalog,
        // so the preview applies the same three ids and onboarding can be
        // reviewed without a desktop build.
        if (!cmdArgs?.shortlist) return clone(rows);
        const shortlist = ["parakeet-ultra-coreml", "parakeet-tdt-v3", "large-v3-turbo-q5"];
        return clone(shortlist.map((id) => rows.find((r) => r.id === id)).filter(Boolean));
      }
      if (cmd === "list_models") {
        const fromDemo = demo && Object.prototype.hasOwnProperty.call(DEMO_RESPONSES, cmd);
        const table = fromDemo ? DEMO_RESPONSES : RESPONSES;
        const rows = (table[cmd] ?? []) as {
          id: string;
          sizeMb: number;
          polishCapable: boolean;
        }[];
        // `catalog::runs_on_host`, mirrored so `?cpu` reviews the Windows and
        // Intel-Mac build (CPU-only llama-server) on this Mac. The cutoff is the
        // parameter scale, so download size stands in for it: every row the Rust
        // catalog keeps for a CPU host is under 900 MB, and every row it witholds
        // is over 1400.
        const withHost = rows.map((r) => ({ ...r, runsOnHost: !cpuHost || r.sizeMb <= 900 }));
        if (!cmdArgs?.shortlist) return clone(withHost);
        const ids = cpuHost
          ? ["s1-mini", "speakoflow-mini"]
          : ["s1-mini", "qwen3.5-2b"];
        const picked = String(settings.selectedLlmModel ?? "");
        const byId = (id: string) => withHost.find((r) => r.id === id);
        const out = ids.map(byId).filter(Boolean);
        // `model_list` keeps the user's own pick visible even off-shortlist.
        const extra = byId(picked);
        if (extra?.polishCapable && !ids.includes(picked)) out.push(extra);
        return clone(out);
      }
      if (demo && Object.prototype.hasOwnProperty.call(DEMO_RESPONSES, cmd)) {
        // The Style tab renders from the model index, so the fixture has to
        // carry the capability of whichever model the URL picked.
        if (cmd === "style_resolution") {
          const id = s1
            ? "s1-mini"
            : qwen35
              ? "qwen3.5-2b"
              : speako
                ? "speakoflow-mini"
                : String(settings.selectedLlmModel ?? "");
          const base = (DEMO_RESPONSES.style_resolution ?? {}) as Record<string, unknown>;
          return clone({ ...base, capability: styleCapability(id) });
        }
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
