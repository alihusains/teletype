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
  heatmap: Array.from({ length: 200 }, (_, i) => ({
    date: demoNow - (200 - i) * demoDay,
    words: (i * 37) % 4000,
    level: Math.min(4, Math.floor(((i * 37) % 4000) / 900)),
  })),
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
  personalizationCorrections: 5678,
  dictionaryLearned: 34567,
  // The LearningCard renders nothing without this, so the dev preview would
  // never exercise it. Aliases are deliberately long: the row truncates.
  learning: {
    editRate7d: 18.34,
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
        "Best English accuracy. Excels when others are talking nearby. English only.",
      recommended: false,
      englishOnly: true,
      downloaded: true,
      selected: false,
      speed: 9.7,
      accuracy: 9.5,
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
      purpose: "Dictation cleanup",
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
      purpose: "Dictation cleanup",
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
      purpose: "Polish + Prompt Engineer",
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
      purpose: "",
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
      purpose: "",
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
      purpose: "",
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
  // Mirrors StyleResolution in commands/personalization.rs.
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
  // Reviews the Styles axes as the all-rounder model sees them; without this
  // the preview's default model takes neither the S1 nor the 2B branch.
  const qwen35 = params.has("qwen35");
  const onboarding = params.has("onboarding");
  // Reviews the CPU-only llama-server build (Windows, Intel Mac) on Apple
  // Silicon, where the polish rows a host cannot carry would otherwise all
  // report true and the "slow on this computer" label would never render.
  const cpuHost = params.has("cpu");
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
