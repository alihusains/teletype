//! Speech (dictation) model catalog.
//!
//! Five local engines:
//! - Parakeet TDT v3 (NVIDIA, via whisper.cpp's parakeet engine) — the
//!   state-of-the-art ggml build, the cross-platform default.
//! - Parakeet Core ML (FluidAudio) — the Neural Engine build on macOS 14+
//!   Apple Silicon, including Parakeet Ultra.
//! - sherpa-onnx NeMo Parakeet — CPU int8 builds, the Windows default.
//! - Parakeet.cpp (mudler/parakeet.cpp, C++/ggml) — GGUF-native loader that
//!   handles mixed-quant models (e.g. Fluid Speech Mini).
//! - Whisper GGML files published by whisper.cpp on Hugging Face — the
//!   fallback for languages Parakeet does not cover.
//!
//! The same lineup SpeakType ships.

use serde::Serialize;

const WHISPER_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const PARAKEET_BASE_URL: &str = "https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main";
const FLUID_REPO_BASE_URL: &str = "https://huggingface.co";
const SHERPA_MODELS_BASE_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

/// Which local engine loads a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    Whisper,
    Parakeet,
    /// Parakeet via FluidAudio Core ML on the Neural Engine (macOS 14+,
    /// Apple Silicon only).
    ParakeetCoreml,
    /// NeMo Parakeet via sherpa-onnx (CPU; the Windows default).
    SherpaParakeet,
    /// Parakeet.cpp (mudler/parakeet.cpp, C++/ggml) — GGUF-native loader
    /// that handles mixed-quant models (e.g. Fluid Speech Mini).
    ParakeetCpp,
}

impl Engine {
    /// Whether this engine ships on `platform`.
    ///
    /// This must mirror the target-gated dependencies in `Cargo.toml`:
    /// `whisper-rs`, `parakeet-sys`, and `parakeet-cpp-sys` are macOS-only;
    /// `sherpa-onnx` is unconditional; the Core ML bridge needs the Neural
    /// Engine (Apple Silicon, macOS 14+).
    pub fn available_on(self, platform: Platform) -> bool {
        match self {
            Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp => platform.is_macos(),
            Engine::ParakeetCoreml => {
                matches!(platform, Platform::MacosAppleSilicon { major } if major >= 14)
            }
            Engine::SherpaParakeet => true,
        }
    }

    /// Whether this build actually compiled the engine, given the host's
    /// engine-capability facts.
    ///
    /// The platform check in [`Self::available_on`] only says the engine's
    /// dependency *ships* on the platform; this says the build in front of us
    /// can actually load it. Only parakeet.cpp needs a fact beyond the
    /// platform: the dylib is compiled by a build script, so a build without
    /// it must not offer its models. Core ML is deliberately NOT duplicated
    /// here — the FluidAudio bridge keeps its existing `fluid_available`
    /// argument at the [`SpeechModel`] level.
    pub fn built_on(self, platform: Platform, parakeet_cpp_available: bool) -> bool {
        match self {
            Engine::ParakeetCpp => platform.is_macos() && parakeet_cpp_available,
            _ => true,
        }
    }

    /// Short display name for the engine, shown by the model list UI.
    pub fn label(self) -> &'static str {
        match self {
            Engine::Whisper => "whisper.cpp",
            Engine::Parakeet => "parakeet (ggml)",
            Engine::ParakeetCoreml => "Neural Engine (Core ML)",
            Engine::SherpaParakeet => "sherpa-onnx (CPU)",
            Engine::ParakeetCpp => "parakeet.cpp",
        }
    }

    /// Human-readable platform requirement, shown next to the engine label.
    pub fn platform_label(self) -> &'static str {
        match self {
            Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp => "macOS only",
            Engine::ParakeetCoreml => "Apple Silicon, macOS 14+",
            Engine::SherpaParakeet => "Windows, macOS, Linux",
        }
    }
}

/// Languages Parakeet TDT v3 transcribes.
const PARAKEET_V3_LANGUAGES: &[&str] = &[
    "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv", "mt",
    "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
];

/// One downloadable speech model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModel {
    pub id: &'static str,
    pub name: &'static str,
    /// The model file name inside the models directory.
    #[serde(skip)]
    pub file: &'static str,
    /// The engine that loads this file.
    pub engine: Engine,
    /// Approximate download size in MB. Used as a sanity bound on the
    /// download: a transfer cut short is discarded rather than installed.
    pub size_mb: u32,
    /// Hex SHA-256 of the exact published artifact.
    ///
    /// Single-file entries pin the publisher's own checksum: the Hugging Face
    /// LFS `lfs.oid` (which is the SHA-256 of the resolved bytes) or the
    /// GitHub release asset digest. `download_speech_model` verifies it
    /// before installing, so a truncated transfer or a repointed
    /// `resolve/main` ref is rejected rather than loaded.
    ///
    /// `None` only for Core ML entries: those name a multi-file checkpoint
    /// repo that the FluidAudio bridge fetches itself, so there is no single
    /// artifact to hash. `every_single_file_model_pins_its_bytes` enforces
    /// that no other entry is left unpinned.
    #[serde(skip)]
    pub sha256: Option<&'static str>,
    /// English-only model.
    pub english_only: bool,
    /// Static compile-time flag kept for the catalog's own tests and docs.
    ///
    /// This is NOT what the app recommends at runtime: the honest
    /// recommendation is [`recommended_speech_model_id`], which is computed
    /// per host (platform, macOS version, language, and whether this build
    /// links the FluidAudio Core ML bridge). `speech_model_list` sets the
    /// runtime `recommended` row from that function, so a user on Windows or
    /// on a bridge-less Mac is never pointed at a model they cannot load.
    /// Do not read this field to decide what to default a user to.
    pub recommended: bool,
    /// Companion model, not a dictation engine: the Core ML CTC spotter is
    /// downloaded by FluidAudio on demand for vocabulary boosting, so the
    /// model picker must not offer it as something that transcribes.
    pub auxiliary: bool,
    pub description: &'static str,
    /// Languages the model can transcribe, or `None` for every Whisper language (99).
    pub languages: Option<&'static [&'static str]>,
    /// Relative speed score out of 10 (shown as a bar in the UI).
    pub speed: f64,
    /// Relative accuracy score out of 10 (shown as a bar in the UI).
    pub accuracy: f64,
    /// Minimum RAM in GB for comfortable use.
    pub min_ram_gb: u32,
    /// Optional download base URL override. `None` (the default) means the
    /// engine's default base URL is used in `url()`.
    pub base_url: Option<&'static str>,
}

impl SpeechModel {
    /// Whether the binary on `platform` can load this model. Listing a model
    /// a host cannot load produces a dead download, so the catalog list, the
    /// default pick, and the persisted-selection migration all consult this.
    pub fn available_on(&self, platform: Platform) -> bool {
        self.engine.available_on(platform)
    }

    /// Whether the app may *offer* this model on `platform`: the host can load
    /// it, it is a dictation model rather than an auxiliary companion, and the
    /// Core ML rows only exist when this build actually links the FluidAudio
    /// bridge (`fluid_available`).
    ///
    /// This is the single predicate behind the model list filter and
    /// [`recommended_speech_model_id`], so the recommended row is always a row
    /// that is present in the list. Before this existed the list filtered Core
    /// ML out while the recommendation still named it, which left onboarding
    /// with no card to render.
    /// Offer gate with runtime facts resolved by the caller (tests, previews).
    ///
    /// Same predicate as [`Self::offered_on`] plus the engine-build check:
    /// a model is only offered when this build compiled the engine that loads
    /// it (`parakeet_cpp_available` for the parakeet.cpp rows; the other
    /// engines inherit it from their platform check).
    pub fn offered_on_with(
        &self,
        platform: Platform,
        fluid_available: bool,
        parakeet_cpp_available: bool,
    ) -> bool {
        self.available_on(platform)
            && !self.auxiliary
            && (self.engine != Engine::ParakeetCoreml || fluid_available)
            && self.engine.built_on(platform, parakeet_cpp_available)
    }

    pub fn offered_on(&self, platform: Platform, fluid_available: bool) -> bool {
        self.offered_on_with(
            platform,
            fluid_available,
            crate::parakeet_cpp::engine_available(),
        )
    }

    /// The path handed to `SpeechProvider::load` under `models_dir`.
    ///
    /// One ggml/GGUF file for Whisper/Parakeet/Parakeet.cpp; the extracted
    /// archive directory for sherpa; the models *base* directory for the
    /// FluidAudio bridge, which appends its own repo folder.
    pub fn provider_path(&self, models_dir: &std::path::Path) -> std::path::PathBuf {
        match self.engine {
            Engine::ParakeetCoreml => models_dir.join("fluid"),
            Engine::SherpaParakeet => models_dir.join("sherpa").join(sherpa_dir_name(self.file)),
            Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp => models_dir.join(self.file),
        }
    }

    /// Where this model's bytes live under `models_dir`, used for installed /
    /// downloaded status. Differs from [`Self::provider_path`] only for the
    /// Core ML bridge, which receives the base directory.
    pub fn install_path(&self, models_dir: &std::path::Path) -> std::path::PathBuf {
        if self.engine == Engine::ParakeetCoreml {
            models_dir.join("fluid").join(fluid_dir_name(self.file))
        } else {
            self.provider_path(models_dir)
        }
    }

    pub fn url(&self) -> String {
        if let Some(base) = self.base_url {
            return format!("{base}/{}", self.file);
        }
        match self.engine {
            Engine::Whisper => format!("{WHISPER_BASE_URL}/{}", self.file),
            Engine::Parakeet => format!("{PARAKEET_BASE_URL}/{}", self.file),
            // Core ML entries name a Hugging Face *repo* (their `file` is the
            // repo directory name); the files inside are fetched by the
            // FluidAudio bridge at load time.
            Engine::ParakeetCoreml => {
                format!("{FLUID_REPO_BASE_URL}/FluidInference/{}", self.file)
            }
            // sherpa-onnx ships one tar.bz2 release asset per model.
            Engine::SherpaParakeet => format!("{SHERPA_MODELS_BASE_URL}/{}", self.file),
            Engine::ParakeetCpp => {
                format!("{PARAKEET_BASE_URL}/{}", self.file)
            }
        }
    }

    /// Whether this model can transcribe `language`.
    ///
    /// The model's own claim: `None`, empty, or `"auto"` is `true` for every
    /// row ("auto" is not a request for a language, it is a request for the
    /// engine's own default); a non-English row (Whisper, Parakeet TDT v3)
    /// honours any language; an English-only row honours only `"en"` (after
    /// normalization, so `"en-US"` counts).
    ///
    /// This is the single language-capability *predicate*: the take path and
    /// the Settings engine picker both route through it, so a future engine
    /// gets the check by construction. It deliberately cannot see per-row
    /// limitations narrower than `english_only` (a Parakeet build that lists
    /// only 25 of Whisper's 99 languages): [`entry_supports`] layers the row's
    /// `languages` table on top for that case, and [`pick_engine`]'s detour
    /// consults the candidate row directly.
    pub fn honours_language(&self, language: Option<&str>) -> bool {
        match normalize_language(language) {
            None => true,
            Some("en") => true,
            Some(_) => !self.english_only,
        }
    }

    /// Human-readable language label.
    pub fn language_label(&self) -> String {
        if self.english_only {
            "English only".to_string()
        } else if let Some(langs) = self.languages {
            format!("{} languages", langs.len())
        } else {
            "99 languages".to_string()
        }
    }
}

/// Human-readable name for a normalized two-letter language code, used in
/// user-facing notices ("cannot transcribe Polish"). Unknown codes fall
/// back to the code itself.
///
/// Covers the languages the settings picker offers; a small table is the
/// right size, and the fallback keeps a new entry from ever producing a
/// blank message.
pub fn language_name(code: &str) -> &str {
    match code {
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "ca" => "Catalan",
        "cs" => "Czech",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "es" => "Spanish",
        "et" => "Estonian",
        "fa" => "Persian",
        "fi" => "Finnish",
        "fr" => "French",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "it" => "Italian",
        "ja" => "Japanese",
        "ko" => "Korean",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "mt" => "Maltese",
        "nl" => "Dutch",
        "no" => "Norwegian",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sv" => "Swedish",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "ur" => "Urdu",
        "zh" => "Chinese",
        other => other,
    }
}

/// The directory a sherpa release archive extracts to: its file name without
/// the `.tar.bz2` suffix.
fn sherpa_dir_name(file: &str) -> &str {
    file.strip_suffix(".tar.bz2").unwrap_or(file)
}

/// The cache folder FluidAudio uses for a Core ML repo slug.
///
/// Mirrors `Repo.folderName` in FluidAudio's `ModelNames.swift` at the pinned
/// 0.17.7 revision: the default case strips `-coreml`, while the CTC repos
/// carry explicit folder names.
fn fluid_dir_name(file: &str) -> &str {
    match file {
        "parakeet-ctc-110m-coreml" | "parakeet-ctc-0.6b-coreml" => file,
        _ => file.strip_suffix("-coreml").unwrap_or(file),
    }
}

/// A default template for Whisper models, to reduce repetition.
const WHISPER: SpeechModel = SpeechModel {
    id: "",
    name: "",
    file: "",
    engine: Engine::Whisper,
    size_mb: 0,
    sha256: None,
    english_only: false,
    recommended: false,
    auxiliary: false,
    description: "",
    languages: None,
    speed: 0.0,
    accuracy: 0.0,
    min_ram_gb: 0,
    base_url: None,
};

pub const CATALOG: &[SpeechModel] = &[
    SpeechModel {
        id: "parakeet-tdt-v3",
        name: "Parakeet v3",
        file: "ggml-parakeet-tdt-0.6b-v3-q8_0.bin",
        engine: Engine::Parakeet,
        size_mb: 640,
        sha256: Some("4d64e9e96c2792186d072fde0034df0ad670cf680a2f53069052ead827fd600e"),
        recommended: true,
        description: "Fastest model, with punctuation. English and 24 European languages.",
        languages: Some(PARAKEET_V3_LANGUAGES),
        speed: 9.7,
        accuracy: 9.2,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "fluid-speech-mini",
        name: "Fluid Speech Mini",
        file: "fluid-speech-mini-q2k-q4k.gguf",
        engine: Engine::ParakeetCpp,
        size_mb: 347,
        sha256: Some("f0796bf40274cc49d3db58563c23cd024b52ca7e2ae4749ccf27af85841726b5"),
        english_only: true,
        languages: Some(&["en"]),
        description: "Best English accuracy. Excels when others are talking nearby. English only.",
        speed: 9.7,
        accuracy: 9.5,
        min_ram_gb: 2,
        base_url: Some("https://huggingface.co/FluidVoice-Labs/fluid-speech-mini-gguf/resolve/main"),
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-tdt-v3-q4",
        name: "Parakeet v3 (compact)",
        file: "ggml-parakeet-tdt-0.6b-v3-q4_0.bin",
        engine: Engine::Parakeet,
        size_mb: 356,
        sha256: Some("aa7fe2f5fb47d863ca23e8b1d490632d63a2599f515268b6d6bd656158dad45e"),
        description: "Same speed as Parakeet v3, smaller download. Slightly less accurate.",
        languages: Some(PARAKEET_V3_LANGUAGES),
        speed: 9.7,
        accuracy: 8.8,
        min_ram_gb: 2,
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-tdt-v3-f16",
        name: "Parakeet v3 (high precision)",
        file: "ggml-parakeet-tdt-0.6b-v3-f16.bin",
        engine: Engine::Parakeet,
        size_mb: 1260,
        sha256: Some("833bffc9513b2cae867ee9e51633cfd11e4d51aaa5597c8ac02159385a2b426f"),
        description: "Highest accuracy Parakeet build. Needs more RAM.",
        languages: Some(PARAKEET_V3_LANGUAGES),
        speed: 9.5,
        accuracy: 9.4,
        min_ram_gb: 6,
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-ultra-coreml",
        name: "Parakeet Ultra (Neural Engine)",
        file: "parakeet-ultra-coreml",
        engine: Engine::ParakeetCoreml,
        size_mb: 600,
        sha256: None,
        description: "Highest accuracy on Mac, runs on the Neural Engine. Default on macOS 14+ Apple Silicon.",
        languages: Some(PARAKEET_V3_LANGUAGES),
        speed: 9.9,
        accuracy: 9.6,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-v2-coreml",
        name: "Parakeet v2 (Neural Engine, English)",
        file: "parakeet-tdt-0.6b-v2-coreml",
        engine: Engine::ParakeetCoreml,
        size_mb: 600,
        sha256: None,
        english_only: true,
        languages: Some(&["en"]),
        description: "English-only Neural Engine build, tuned for highest recall.",
        speed: 9.9,
        accuracy: 9.5,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-v3-coreml",
        name: "Parakeet v3 (Neural Engine)",
        file: "parakeet-tdt-0.6b-v3-coreml",
        engine: Engine::ParakeetCoreml,
        size_mb: 600,
        sha256: None,
        description: "Neural Engine fallback build for macOS 14+ Apple Silicon.",
        languages: Some(PARAKEET_V3_LANGUAGES),
        speed: 9.9,
        accuracy: 9.3,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-ctc-110m-coreml",
        name: "Parakeet CTC 110M (vocabulary spotter)",
        file: "parakeet-ctc-110m-coreml",
        engine: Engine::ParakeetCoreml,
        size_mb: 100,
        sha256: None,
        english_only: true,
        auxiliary: true,
        languages: Some(&["en"]),
        description: "Auxiliary vocabulary spotter for the Neural Engine build. Installed automatically once the dictionary has terms; not a dictation model.",
        speed: 0.0,
        accuracy: 0.0,
        min_ram_gb: 2,
        ..WHISPER
    },
    SpeechModel {
        id: "sherpa-parakeet-v3-int8",
        name: "Parakeet v3 (sherpa-onnx)",
        file: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
        engine: Engine::SherpaParakeet,
        size_mb: 465,
        sha256: Some("5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf"),
        description: "Default on Windows. Runs on CPU, English and 24 European languages.",
        languages: Some(PARAKEET_V3_LANGUAGES),
        speed: 8.5,
        accuracy: 9.0,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "sherpa-parakeet-v2-int8",
        name: "Parakeet v2 (sherpa-onnx, English)",
        file: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2",
        engine: Engine::SherpaParakeet,
        size_mb: 461,
        sha256: Some("157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad"),
        english_only: true,
        languages: Some(&["en"]),
        description: "English-only sherpa-onnx build for Windows.",
        speed: 8.5,
        accuracy: 9.0,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "large-v3-turbo-q5",
        name: "Whisper Large v3 Turbo (compressed)",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size_mb: 547,
        sha256: Some("394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"),
        description:
            "Near-flagship accuracy at a third of the size. Auto-selected for languages Parakeet does not cover (Arabic, Hindi, Japanese, …).",
        speed: 7.5,
        accuracy: 9.4,
        min_ram_gb: 6,
        ..WHISPER
    },
    SpeechModel {
        id: "large-v3-turbo",
        name: "Whisper Large v3 Turbo",
        file: "ggml-large-v3-turbo.bin",
        size_mb: 1624,
        sha256: Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
        description: "The most accurate Whisper model. Best on machines with a fast GPU.",
        speed: 7.0,
        accuracy: 9.5,
        min_ram_gb: 8,
        ..WHISPER
    },
    SpeechModel {
        id: "small-en",
        name: "Whisper Small (English)",
        file: "ggml-small.en.bin",
        size_mb: 466,
        sha256: Some("c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d"),
        english_only: true,
        languages: Some(&["en"]),
        description: "Good balance of speed and accuracy for English.",
        speed: 8.0,
        accuracy: 8.5,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "base-en",
        name: "Whisper Base (English)",
        file: "ggml-base.en.bin",
        size_mb: 142,
        sha256: Some("a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002"),
        english_only: true,
        languages: Some(&["en"]),
        description: "Fast on any machine. Fine for short English dictation.",
        speed: 9.0,
        accuracy: 7.5,
        min_ram_gb: 2,
        ..WHISPER
    },
    SpeechModel {
        id: "base",
        name: "Whisper Base",
        file: "ggml-base.bin",
        size_mb: 142,
        sha256: Some("60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
        description: "Small and quick to download. A good way to get started in any language.",
        speed: 9.0,
        accuracy: 7.3,
        min_ram_gb: 2,
        ..WHISPER
    },
    SpeechModel {
        id: "tiny",
        name: "Whisper Tiny",
        file: "ggml-tiny.bin",
        size_mb: 75,
        sha256: Some("be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
        description: "Fastest Whisper model and least accurate. Useful for testing.",
        speed: 9.5,
        accuracy: 6.0,
        min_ram_gb: 2,
        ..WHISPER
    },
];

/// Finds a catalog entry by id.
pub fn find(id: &str) -> Option<&'static SpeechModel> {
    CATALOG.iter().find(|m| m.id == id)
}

/// Normalizes a Settings language for routing: `"en-US"`/`"en_US"` become
/// `"en"`, and `"auto"`/empty/whitespace become `None` ("the engine decides"),
/// which selects the platform default instead of routing to Whisper.
///
/// `Settings::language` ships as `"auto"`, so every caller that threads the
/// stored value through here treats a fresh install as "no language set".
fn normalize_language(language: Option<&str>) -> Option<&str> {
    let base = language
        .map(|l| l.trim())
        .map(|l| l.split(['-', '_']).next().unwrap_or(l))?;
    if base.is_empty() || base.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(base)
    }
}

/// Whether the catalog entry `id` can transcribe `language` (the raw
/// Settings value — BCP-47 tags and whitespace included — are normalized
/// here; `None` matches every model).
///
/// The single list predicate for the Settings engine picker: the row's
/// own `languages` table (when present — `None` = the engine's full
/// table, i.e. Whisper's 99) must agree with
/// [`SpeechModel::honours_language`], so the two cannot drift apart.
///
/// The table is consulted only when the row carries one, which is also
/// the only shape where a row can list fewer languages than the engine
/// claims (a Parakeet build with the 25-language list): the table half
/// answers that case, the predicate half the English-only case.
fn entry_supports(entry: &SpeechModel, language: Option<&str>) -> bool {
    match normalize_language(language) {
        None => true,
        Some(lang) => {
            entry.languages.is_none_or(|langs| langs.contains(&lang))
                && entry.honours_language(Some(lang))
        }
    }
}

/// Flip to `true` only if the P3.8 bake-off shows Parakeet v2 beating Ultra
/// on the English human corpus; both English default branches honor it.
pub const V2_ENGLISH_PREFERRED: bool = false;

/// The host facts [`pick_engine`] needs (the app fills these in at startup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS on Apple Silicon; `major` is the macOS major version (14+ gets
    /// the Neural Engine engines).
    MacosAppleSilicon { major: u16 },
    /// macOS on Intel — no Neural Engine, so the ggml build is the default.
    MacosIntel,
    /// Windows — sherpa-onnx is the default engine.
    Windows,
    /// Anything else (Linux, …) — the ggml build is the default.
    Other,
}

impl Platform {
    /// macOS of either architecture.
    pub fn is_macos(self) -> bool {
        matches!(
            self,
            Platform::MacosAppleSilicon { .. } | Platform::MacosIntel
        )
    }
}

/// Detects the host platform for [`pick_engine`].
///
/// The macOS major version comes from `sw_vers` (no extra dependency). A
/// failed probe reports major 0, which selects the ggml build — the safe
/// fallback on every Mac.
pub fn host_platform() -> Platform {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            let major = std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| parse_macos_major(&s))
                .unwrap_or(0);
            Platform::MacosAppleSilicon { major }
        } else {
            Platform::MacosIntel
        }
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Other
    }
}

fn parse_macos_major(version: &str) -> Option<u16> {
    version.trim().split('.').next()?.parse().ok()
}

/// The id of a [`SpeechModel`] in [`CATALOG`].
pub type ModelId = &'static str;

/// Picks the default speech-model id for the current host and language.
///
/// Selection order (plan §3.2):
/// 1. an explicit user pin wins when this host can load it (unknown and
///    unloadable ids are ignored);
/// 2. a language outside Parakeet's 25 routes to Whisper (99 languages),
///    where Whisper ships;
/// 3. macOS 14+ Apple Silicon uses the Core ML Ultra build (or v2 for
///    English once [`V2_ENGLISH_PREFERRED`] is flipped by the P3.8 bake-off);
/// 4. Windows uses the sherpa-onnx int8 build;
/// 5. Intel Macs and older macOS keep the ggml Parakeet v3; every other
///    host uses sherpa-onnx, the only cross-platform engine.
///
/// The returned model is always loadable on `platform`; that invariant is
/// what keeps a Windows or Linux install from selecting a macOS-only engine.
///
/// `language` is the Settings language ("en", "de", …, "auto", or a BCP-47
/// tag such as "en-US"); `None`/`"auto"` mean "engine decides", which selects
/// the platform default rather than routing to Whisper.
pub fn pick_engine(pinned: Option<&str>, platform: Platform, language: Option<&str>) -> ModelId {
    // 1. Pin first, but never a model this host cannot load: a selection
    //    carried over from another platform must not brick dictation.
    if let Some(id) = pinned {
        if let Some(entry) = find(id) {
            if entry.available_on(platform) {
                // Return the catalog entry's own `'static` id, not the
                // borrowed input, so the signature can stay `-> ModelId`.
                return entry.id;
            }
        }
    }

    // Normalize "en-US" → "en"; treat "auto"/empty as "no language set".
    let lang = normalize_language(language);

    // 2. Whisper covers every language the best platform candidate cannot
    //    transcribe, where it ships. The candidate is the same row the
    //    platform match below would pick, so the detour and the platform
    //    answer cannot disagree about the language: its `languages` table
    //    (25 for Parakeet, `None` = the engine's full 99 for Whisper) is the
    //    row's own claim, layered on `honours_language` (the English-only
    //    half) by `entry_supports`. Where Whisper does not ship (Windows),
    //    the platform default below is the closest loadable engine.
    let platform_default: &str = match platform {
        Platform::MacosAppleSilicon { major } if major >= 14 => {
            if V2_ENGLISH_PREFERRED {
                "parakeet-v2-coreml"
            } else {
                "parakeet-ultra-coreml"
            }
        }
        Platform::Windows => {
            if V2_ENGLISH_PREFERRED {
                "sherpa-parakeet-v2-int8"
            } else {
                "sherpa-parakeet-v3-int8"
            }
        }
        Platform::MacosAppleSilicon { .. } | Platform::MacosIntel => "parakeet-tdt-v3",
        Platform::Other => "sherpa-parakeet-v3-int8",
    };
    if let Some(l) = lang {
        let detours = find("large-v3-turbo-q5").is_some_and(|m| m.available_on(platform))
            && !find(platform_default).is_some_and(|m| entry_supports(m, Some(l)));
        if detours {
            return "large-v3-turbo-q5";
        }
    }

    match platform {
        // 3. macOS 14+ on Apple Silicon runs Core ML on the Neural Engine.
        //    The candidate is the same row step 2 consulted, so the detour
        //    and this answer cannot disagree about the language.
        Platform::MacosAppleSilicon { major } if major >= 14 => {
            if V2_ENGLISH_PREFERRED && lang == Some("en") {
                "parakeet-v2-coreml"
            } else {
                platform_default
            }
        }
        // 4. Windows runs sherpa-onnx on CPU.
        Platform::Windows => {
            if V2_ENGLISH_PREFERRED && lang == Some("en") {
                "sherpa-parakeet-v2-int8"
            } else {
                platform_default
            }
        }
        // 5. Intel Macs and older macOS keep the ggml build; other hosts
        //    (Linux, …) can only load sherpa-onnx.
        Platform::MacosAppleSilicon { .. } | Platform::MacosIntel => platform_default,
        Platform::Other => platform_default,
    }
}

/// Dictation models tried by [`recommended_speech_model_id`] when
/// [`pick_engine`]'s answer cannot be offered on this build, best first.
///
/// Every entry is a real dictation model, so the fallback can never name a
/// row the list hides (the CTC spotter is deliberately absent).
const RECOMMENDED_FALLBACKS: &[ModelId] = &[
    "parakeet-tdt-v3",
    "parakeet-tdt-v3-q4",
    "large-v3-turbo-q5",
    "sherpa-parakeet-v3-int8",
];

/// The id of the model this build should recommend for `platform` and
/// `language`, guaranteed to be a model [`SpeechModel::offered_on`] accepts.
///
/// [`pick_engine`] alone is not enough: it routes on platform, OS version and
/// language but cannot see the binary it is running inside, so on an Apple
/// Silicon macOS 14+ host whose build did not link the FluidAudio bridge (CI
/// builds, a build configured without the Swift toolchain) it still answers
/// `parakeet-ultra-coreml`. The model list filters those rows out, which used
/// to leave onboarding with a recommendation that matched no row: no card
/// rendered and no way to continue. Intel Macs and a failed macOS version
/// probe are already handled inside [`pick_engine`] (they answer the ggml
/// build), so this function leaves them alone.
///
/// Semantics, in order:
/// 1. `pick_engine`'s answer wins when this build can offer it: Apple Silicon
///    macOS 14+ **with** the bridge gets `parakeet-ultra-coreml`, Windows gets
///    `sherpa-parakeet-v3-int8`, macOS 13 and Intel get `parakeet-tdt-v3`.
/// 2. Otherwise the first entry in [`RECOMMENDED_FALLBACKS`] this build can
///    offer and that transcribes `language` is used, so an Apple Silicon
///    macOS 14+ machine without the bridge falls back to `parakeet-tdt-v3`
///    (same 25 languages as the build it replaces).
/// 3. A language outside Parakeet's list keeps the Whisper route, but only on
///    platforms where Whisper ships; elsewhere the platform default stands.
///
/// `language` is the Settings value ("en", "en-US", "auto", `""`, `None`);
/// "auto" and empty behave like `None`, i.e. "the engine decides".
///
/// The result is always a catalog entry, never an empty id, so a host with no
/// offerable model at all (impossible today: sherpa-onnx is unconditional)
/// still reports [`pick_engine`]'s platform default instead of nothing.
pub fn recommended_speech_model_id(
    platform: Platform,
    language: Option<&str>,
    fluid_available: bool,
) -> ModelId {
    let offerable = |id: ModelId| find(id).is_some_and(|m| m.offered_on(platform, fluid_available));

    let pick = pick_engine(None, platform, language);
    if offerable(pick) {
        return pick;
    }

    let lang = normalize_language(language);
    for &id in RECOMMENDED_FALLBACKS {
        if find(id)
            .is_some_and(|m| m.offered_on(platform, fluid_available) && entry_supports(m, lang))
        {
            return id;
        }
    }

    // Last resort: the first offered dictation model that covers the language,
    // then any offered model. Keeps the contract "the recommendation is always
    // in the list" even if the catalog grows an unusual engine.
    if let Some(m) = CATALOG
        .iter()
        .find(|m| m.offered_on(platform, fluid_available) && entry_supports(m, lang))
    {
        return m.id;
    }
    if let Some(m) = CATALOG
        .iter()
        .find(|m| m.offered_on(platform, fluid_available))
    {
        return m.id;
    }
    pick
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_single_file_model_pins_its_bytes() {
        for m in CATALOG {
            match m.engine {
                // The Core ML bridge fetches a multi-file checkpoint repo; no
                // single artifact exists whose hash could be pinned.
                Engine::ParakeetCoreml => assert!(
                    m.sha256.is_none(),
                    "{} is a multi-file repo and must not carry a single-file hash",
                    m.id
                ),
                _ => {
                    let sha = m
                        .sha256
                        .unwrap_or_else(|| panic!("{} has no SHA-256 pin", m.id));
                    assert_eq!(sha.len(), 64, "{}: {sha}", m.id);
                    assert!(
                        sha.chars()
                            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                        "{} is not lowercase hex: {sha}",
                        m.id
                    );
                }
            }
        }
    }

    #[test]
    fn catalog_ids_are_unique() {
        let mut ids: Vec<_> = CATALOG.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
    }

    #[test]
    fn exactly_one_recommended() {
        // The static flag is the catalog's compile-time reference default only.
        // The runtime recommendation is `recommended_speech_model_id`, which is
        // per host; `recommended_speech_model_id_is_always_an_offered_row`
        // guards that one instead.
        assert_eq!(CATALOG.iter().filter(|m| m.recommended).count(), 1);
    }

    #[test]
    fn find_returns_entries() {
        assert!(find("large-v3-turbo-q5").is_some());
        assert!(find("no-such-model").is_none());
        // The publisher never shipped a v2 ggml build (ggml-org/parakeet-GGUF
        // holds v3 only), so the old entry was a guaranteed 404 download.
        assert!(find("parakeet-tdt-v2").is_none());
    }

    #[test]
    fn urls_point_at_the_right_repos() {
        for m in CATALOG {
            match m.engine {
                Engine::Whisper => assert!(m.url().starts_with(WHISPER_BASE_URL)),
                Engine::Parakeet => assert!(m.url().starts_with(PARAKEET_BASE_URL)),
                Engine::ParakeetCoreml => {
                    assert!(m
                        .url()
                        .starts_with("https://huggingface.co/FluidInference/"))
                }
                Engine::SherpaParakeet => assert!(m.url().starts_with(SHERPA_MODELS_BASE_URL)),
                Engine::ParakeetCpp => {
                    if m.base_url.is_none() {
                        assert!(m.url().starts_with(PARAKEET_BASE_URL));
                    }
                }
            }
            assert!(m.url().ends_with(m.file));
        }
    }

    #[test]
    fn fluid_speech_mini_entry() {
        let m = find("fluid-speech-mini").expect("fluid-speech-mini entry");
        assert_eq!(m.engine, Engine::ParakeetCpp);
        assert_eq!(
            m.url(),
            "https://huggingface.co/FluidVoice-Labs/fluid-speech-mini-gguf/resolve/main/fluid-speech-mini-q2k-q4k.gguf"
        );
        assert_eq!(
            m.sha256,
            Some("f0796bf40274cc49d3db58563c23cd024b52ca7e2ae4749ccf27af85841726b5")
        );
        assert!(m.english_only);
        assert!(!m.recommended);
    }

    #[test]
    fn parakeet_is_recommended() {
        // Static catalog flag only: this is the cross-platform reference build.
        // Runtime recommendation lives in `recommended_speech_model_id`.
        assert!(find("parakeet-tdt-v3").unwrap().recommended);
        assert_eq!(find("parakeet-tdt-v3").unwrap().engine, Engine::Parakeet);
    }

    #[test]
    fn language_labels() {
        assert_eq!(
            find("parakeet-tdt-v3").unwrap().language_label(),
            "25 languages"
        );
        assert_eq!(find("small-en").unwrap().language_label(), "English only");
        assert_eq!(find("base").unwrap().language_label(), "99 languages");
    }

    #[test]
    fn coreml_and_sherpa_entries_are_well_formed() {
        // The `recommended` assertions here check the static catalog flag
        // (exactly one entry carries it), not the runtime pick: Ultra *is* the
        // runtime recommendation on an Apple Silicon macOS 14+ build that links
        // the bridge, as `recommended_speech_model_id` proves.
        let ultra = find("parakeet-ultra-coreml").unwrap();
        assert_eq!(ultra.engine, Engine::ParakeetCoreml);
        assert_eq!(ultra.languages, Some(PARAKEET_V3_LANGUAGES));
        assert!(!ultra.recommended);
        assert_eq!(ultra.language_label(), "25 languages");

        let v2 = find("parakeet-v2-coreml").unwrap();
        assert_eq!(v2.engine, Engine::ParakeetCoreml);
        assert!(v2.english_only);
        assert_eq!(
            find("parakeet-v3-coreml").unwrap().engine,
            Engine::ParakeetCoreml
        );
        let ctc = find("parakeet-ctc-110m-coreml").unwrap();
        assert_eq!(ctc.engine, Engine::ParakeetCoreml);
        assert!(!ctc.recommended);

        let sherpa = find("sherpa-parakeet-v3-int8").unwrap();
        assert_eq!(sherpa.engine, Engine::SherpaParakeet);
        assert_eq!(sherpa.languages, Some(PARAKEET_V3_LANGUAGES));
        assert!(find("sherpa-parakeet-v2-int8").unwrap().english_only);
    }

    #[test]
    fn pick_engine_defaults_by_platform() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(pick_engine(None, mac14, None), "parakeet-ultra-coreml");
        // German on an Apple Silicon macOS 15 Mac stays on the Neural Engine
        // build: the Core ML Ultra row lists the 25 Parakeet v3 languages
        // (German included), so the Whisper detour does not fire. This is the
        // deliberate behaviour of routing the detour through the candidate
        // row's table + honours_language instead of the old free-standing
        // 25-language table; a Core ML row that lists fewer languages than
        // the engine claims (e.g. no `ar`) still routes to Whisper, per
        // `uncovered_languages_route_to_whisper_only_where_whisper_ships`.
        assert_eq!(
            pick_engine(None, Platform::MacosAppleSilicon { major: 15 }, Some("de")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            pick_engine(None, Platform::MacosAppleSilicon { major: 14 }, Some("de")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            pick_engine(None, Platform::MacosAppleSilicon { major: 13 }, None),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            pick_engine(None, Platform::MacosIntel, Some("en")),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            pick_engine(None, Platform::Windows, None),
            "sherpa-parakeet-v3-int8"
        );
        // Linux and other hosts can only load sherpa-onnx, not ggml.
        assert_eq!(
            pick_engine(None, Platform::Other, Some("fr")),
            "sherpa-parakeet-v3-int8"
        );
    }

    /// Every host fact the recommendation and the model list must agree on.
    const PLATFORMS: &[Platform] = &[
        Platform::MacosAppleSilicon { major: 26 },
        Platform::MacosAppleSilicon { major: 15 },
        Platform::MacosAppleSilicon { major: 14 },
        Platform::MacosAppleSilicon { major: 13 },
        Platform::MacosIntel,
        Platform::Windows,
        Platform::Other,
    ];
    const LANGUAGES: &[Option<&str>] = &[
        None,
        Some(""),
        Some("auto"),
        Some("en"),
        Some("en-US"),
        Some("de"),
        Some("ar"),
        Some("ja-JP"),
        Some("und"),
    ];

    /// The core bug this function exists to prevent: the recommendation named
    /// a Core ML row that the list filter had already stripped, so onboarding
    /// rendered no model card and Continue stayed disabled. The recommendation
    /// must always be a row the list emits.
    #[test]
    fn recommended_speech_model_id_is_always_an_offered_row() {
        for platform in PLATFORMS {
            for language in LANGUAGES {
                for fluid in [true, false] {
                    let id = recommended_speech_model_id(*platform, *language, fluid);
                    let entry = find(id).unwrap_or_else(|| {
                        panic!("{id} from {platform:?}/{language:?} is not in CATALOG")
                    });
                    assert!(
                        entry.offered_on(*platform, fluid),
                        "recommended {id} on {platform:?} for {language:?} with bridge={fluid}, \
                         but the model list filters that row out"
                    );
                    // The same predicate the list uses, spelled out here so a
                    // future change to `offered_on` cannot silently drift.
                    assert!(entry.available_on(*platform), "{id} cannot load");
                    assert!(!entry.auxiliary, "{id} is not a dictation model");
                    if entry.engine == Engine::ParakeetCoreml {
                        assert!(fluid, "{id} needs the bridge that is absent");
                    }
                }
            }
        }
    }

    /// Exactly one row carries the recommendation on every host: a list with
    /// none is the onboarding deadlock, a list with two is a coin flip.
    #[test]
    fn recommended_speech_model_id_is_unique_per_host() {
        for platform in PLATFORMS {
            for language in LANGUAGES {
                for fluid in [true, false] {
                    let id = recommended_speech_model_id(*platform, *language, fluid);
                    let matches = CATALOG
                        .iter()
                        .filter(|m| m.offered_on(*platform, fluid) && m.id == id)
                        .count();
                    assert_eq!(
                        matches, 1,
                        "{platform:?}/{language:?}/bridge={fluid} -> {id}"
                    );
                }
            }
        }
    }

    #[test]
    fn recommended_speech_model_id_requires_the_fluid_bridge_for_core_ml() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        // The bridge is what makes the Neural Engine rows real.
        assert_eq!(
            recommended_speech_model_id(mac14, None, true),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            recommended_speech_model_id(mac14, Some("en"), true),
            "parakeet-ultra-coreml"
        );
        // Without it (CI build, a bridge-less configuration, or a failed
        // version probe) the ggml Parakeet v3 takes over: the same 25
        // languages, on the engine an Intel or macOS 13 Mac also gets.
        assert_eq!(
            recommended_speech_model_id(mac14, None, false),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            recommended_speech_model_id(mac14, Some("de"), false),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            recommended_speech_model_id(
                Platform::MacosAppleSilicon { major: 0 },
                Some("en"),
                false
            ),
            "parakeet-tdt-v3"
        );
    }

    #[test]
    fn recommended_speech_model_id_matches_the_platform_defaults() {
        // Hosts that never touch Core ML are unaffected by the bridge flag.
        for fluid in [true, false] {
            assert_eq!(
                recommended_speech_model_id(Platform::MacosAppleSilicon { major: 13 }, None, fluid),
                "parakeet-tdt-v3"
            );
            assert_eq!(
                recommended_speech_model_id(Platform::MacosIntel, Some("en"), fluid),
                "parakeet-tdt-v3"
            );
            assert_eq!(
                recommended_speech_model_id(Platform::Windows, None, fluid),
                "sherpa-parakeet-v3-int8"
            );
            assert_eq!(
                recommended_speech_model_id(Platform::Other, Some("fr"), fluid),
                "sherpa-parakeet-v3-int8"
            );
        }
    }

    /// An Arabic user must never land on Ultra: it cannot transcribe Arabic,
    /// and the fallback chain keeps the language route instead of silently
    /// handing back a Parakeet build.
    #[test]
    fn recommended_speech_model_id_respects_the_language() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        for lang in ["ar", "hi", "ur", "ja", "zh", "ko", "tr", "fa"] {
            for fluid in [true, false] {
                assert_eq!(
                    recommended_speech_model_id(mac14, Some(lang), fluid),
                    "large-v3-turbo-q5",
                    "macOS 14/{lang} with bridge={fluid}"
                );
            }
            // Whisper does not ship for Windows, so the recommendation stays on
            // the engine the binary can actually load.
            for fluid in [true, false] {
                assert_eq!(
                    recommended_speech_model_id(Platform::Windows, Some(lang), fluid),
                    "sherpa-parakeet-v3-int8",
                    "Windows/{lang} with bridge={fluid}"
                );
            }
        }
        // A covered language keeps the platform default.
        assert_eq!(
            recommended_speech_model_id(Platform::Windows, Some("uk"), false),
            "sherpa-parakeet-v3-int8"
        );
        // "auto", empty, and None all mean "the engine decides".
        for unset in [None, Some(""), Some("auto"), Some("AUTO"), Some("  auto  ")] {
            assert_eq!(
                recommended_speech_model_id(mac14, unset, true),
                "parakeet-ultra-coreml",
                "{unset:?} must not route to Whisper"
            );
        }
        // BCP-47 tags normalize to the base language.
        assert_eq!(
            recommended_speech_model_id(Platform::Windows, Some("pt-BR"), false),
            "sherpa-parakeet-v3-int8"
        );
        assert_eq!(
            recommended_speech_model_id(mac14, Some("ja-JP"), true),
            "large-v3-turbo-q5"
        );
    }

    #[test]
    fn offered_on_is_the_list_predicate() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        let ultra = find("parakeet-ultra-coreml").unwrap();
        assert!(ultra.offered_on(mac14, true));
        assert!(!ultra.offered_on(mac14, false), "no bridge, no Core ML row");
        assert!(!ultra.offered_on(Platform::MacosAppleSilicon { major: 13 }, true));
        assert!(!ultra.offered_on(Platform::Windows, true));

        // The CTC spotter is a boosting companion: never offered, even though
        // the host can load it.
        let ctc = find("parakeet-ctc-110m-coreml").unwrap();
        assert!(ctc.available_on(mac14));
        assert!(!ctc.offered_on(mac14, true));
        assert!(!ctc.offered_on(mac14, false));

        let sherpa = find("sherpa-parakeet-v3-int8").unwrap();
        for p in PLATFORMS {
            assert!(sherpa.offered_on(*p, true) && sherpa.offered_on(*p, false));
        }
    }

    #[test]
    fn uncovered_languages_route_to_whisper_only_where_whisper_ships() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        for lang in ["ar", "hi", "ur", "ja", "zh", "ko", "tr", "fa"] {
            assert_eq!(
                pick_engine(None, mac14, Some(lang)),
                "large-v3-turbo-q5",
                "macOS/{lang}"
            );
            // Whisper is macOS-only, so Windows keeps its loadable engine
            // rather than selecting a model the binary cannot load.
            assert_eq!(
                pick_engine(None, Platform::Windows, Some(lang)),
                "sherpa-parakeet-v3-int8",
                "Windows/{lang}"
            );
        }
        // "auto" means "no language set": platform default, not Whisper.
        assert_eq!(
            pick_engine(None, mac14, Some("auto")),
            "parakeet-ultra-coreml"
        );
        // BCP-47 tags normalize to the base language: en-US is covered.
        assert_eq!(
            pick_engine(None, mac14, Some("en-US")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(pick_engine(None, mac14, Some("ja-JP")), "large-v3-turbo-q5");
    }

    #[test]
    fn pin_wins_only_when_the_host_can_load_it() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(
            pick_engine(Some("large-v3-turbo"), mac14, Some("ar")),
            "large-v3-turbo"
        );
        assert_eq!(
            pick_engine(Some("sherpa-parakeet-v2-int8"), mac14, None),
            "sherpa-parakeet-v2-int8"
        );
        // A pin from another platform is not loadable here; automatic
        // selection continues instead of selecting a dead engine.
        assert_eq!(
            pick_engine(Some("parakeet-tdt-v3"), Platform::Windows, Some("ja")),
            "sherpa-parakeet-v3-int8"
        );
        assert_eq!(
            pick_engine(Some("large-v3-turbo"), Platform::Windows, Some("en")),
            "sherpa-parakeet-v3-int8"
        );
        // Unknown ids are ignored (they cannot be loaded), so automatic
        // selection continues.
        assert_eq!(
            pick_engine(Some("no-such-model"), mac14, None),
            "parakeet-ultra-coreml"
        );
    }

    #[test]
    fn parakeet_cpp_unavailable_hides_fluid() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        let mini = find("fluid-speech-mini").unwrap();
        assert!(
            !mini.offered_on_with(mac14, true, false),
            "no compiled parakeet.cpp dylib, no Fluid Speech Mini row"
        );
        assert!(mini.offered_on_with(mac14, true, true));

        // The platform gate still wins: parakeet.cpp is macOS-only, so a
        // Windows host with a "built" engine gets no row either.
        assert!(!mini.offered_on_with(Platform::Windows, true, true));
    }

    #[test]
    fn engine_labels_are_stable() {
        assert_eq!(Engine::Whisper.label(), "whisper.cpp");
        assert_eq!(Engine::Whisper.platform_label(), "macOS only");
        assert_eq!(Engine::Parakeet.label(), "parakeet (ggml)");
        assert_eq!(Engine::Parakeet.platform_label(), "macOS only");
        assert_eq!(Engine::ParakeetCoreml.label(), "Neural Engine (Core ML)");
        assert_eq!(
            Engine::ParakeetCoreml.platform_label(),
            "Apple Silicon, macOS 14+"
        );
        assert_eq!(Engine::SherpaParakeet.label(), "sherpa-onnx (CPU)");
        assert_eq!(
            Engine::SherpaParakeet.platform_label(),
            "Windows, macOS, Linux"
        );
        assert_eq!(Engine::ParakeetCpp.label(), "parakeet.cpp");
        assert_eq!(Engine::ParakeetCpp.platform_label(), "macOS only");
    }

    #[test]
    fn engine_availability_follows_what_ships() {
        // whisper-rs, parakeet-sys, and parakeet-cpp-sys are macOS-only
        // dependencies; sherpa-onnx is unconditional.
        for p in [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosIntel,
        ] {
            for id in [
                "parakeet-tdt-v3",
                "large-v3-turbo-q5",
                "fluid-speech-mini",
                "sherpa-parakeet-v3-int8",
            ] {
                assert!(find(id).unwrap().available_on(p), "{id} on {p:?}");
            }
        }
        assert!(find("parakeet-ultra-coreml")
            .unwrap()
            .available_on(Platform::MacosAppleSilicon { major: 14 }));

        // Core ML needs the Neural Engine: Apple Silicon on macOS 14+.
        let ultra = find("parakeet-ultra-coreml").unwrap();
        assert!(!ultra.available_on(Platform::MacosAppleSilicon { major: 13 }));
        assert!(!ultra.available_on(Platform::MacosIntel));

        for p in [Platform::Windows, Platform::Other] {
            for id in [
                "parakeet-tdt-v3",
                "large-v3-turbo-q5",
                "fluid-speech-mini",
                "parakeet-ultra-coreml",
            ] {
                assert!(!find(id).unwrap().available_on(p), "{id} on {p:?}");
            }
            assert!(find("sherpa-parakeet-v3-int8").unwrap().available_on(p));
            assert!(find("sherpa-parakeet-v2-int8").unwrap().available_on(p));
        }
    }

    #[test]
    fn pick_engine_never_returns_an_unavailable_model() {
        let platforms = [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosAppleSilicon { major: 13 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ];
        for p in platforms {
            for lang in [None, Some("en"), Some("de"), Some("ar"), Some("auto")] {
                let id = pick_engine(None, p, lang);
                let model = find(id).unwrap_or_else(|| panic!("{id} missing from CATALOG"));
                assert!(
                    model.available_on(p),
                    "pick_engine chose {id} on {p:?} for {lang:?}, but the host cannot load it"
                );
            }
        }
    }

    #[test]
    fn honours_language_arms() {
        let en_only = find("fluid-speech-mini").unwrap();
        assert!(en_only.english_only);
        // None / empty / "auto" are not a request for a language: every row
        // honours them (the engine uses its own default).
        assert!(en_only.honours_language(None));
        assert!(en_only.honours_language(Some("")));
        assert!(en_only.honours_language(Some("auto")));
        assert!(en_only.honours_language(Some("AUTO")));
        assert!(en_only.honours_language(Some("   ")));
        // English in any BCP-47 spelling is honoured.
        assert!(en_only.honours_language(Some("en")));
        assert!(en_only.honours_language(Some("en-US")));
        // A non-English lock on an English-only row is the disagreement this
        // predicate exists to answer.
        assert!(!en_only.honours_language(Some("pl")));
        assert!(!en_only.honours_language(Some("de")));

        let multilingual = find("parakeet-tdt-v3").unwrap();
        assert!(!multilingual.english_only);
        assert!(multilingual.honours_language(None));
        assert!(multilingual.honours_language(Some("en")));
        assert!(multilingual.honours_language(Some("pl")));
        assert!(multilingual.honours_language(Some("de")));

        // Whisper carries no language table: it transcribes every language.
        let whisper = find("large-v3-turbo").unwrap();
        assert!(!whisper.english_only);
        assert!(whisper.honours_language(Some("pl")));
        assert!(whisper.honours_language(Some("ar")));

        // An English-only Whisper build honours English but not more.
        let small_en = find("small-en").unwrap();
        assert!(small_en.english_only);
        assert!(small_en.honours_language(Some("en")));
        assert!(!small_en.honours_language(Some("de")));
    }

    #[test]
    fn catalog_rows_agree_on_language_support() {
        // The catalog's `languages` table and `honours_language` must not
        // contradict each other for any row. A table that lists a language
        // is the row's claim of support, so the predicate must agree; and a
        // predicate that claims a concrete non-English language is an
        // English-only row's negation, so the table (when present) must not
        // withhold it. The `languages: None` shape (Whisper's full 99)
        // carries no table to check.
        for m in CATALOG {
            for l in ["en", "de", "pl", "ar", "ja", "fr", "ru", "uk"] {
                match m.languages {
                    Some(langs) => {
                        if langs.contains(&l) {
                            assert!(
                                m.honours_language(Some(l)),
                                "{} lists `{l}` but honours_language says no",
                                m.id
                            );
                        }
                    }
                    None => {
                        if m.english_only && l != "en" {
                            assert!(
                                !m.honours_language(Some(l)),
                                "{} has no table but honours `{l}` despite english_only",
                                m.id
                            );
                        }
                    }
                }
            }
            // The take path's question, directly: an English-only row never
            // honours a non-English language.
            if m.english_only {
                assert!(
                    !m.honours_language(Some("pl")),
                    "{} is english_only yet honours Polish",
                    m.id
                );
            }
        }
    }

    #[test]
    fn pick_engine_language_detour_agrees_with_row_capability() {
        // The detour must agree with the platform candidate's own claim: where
        // Whisper ships, detour iff the candidate's table + predicate
        // (`entry_supports`) say it cannot transcribe the language; where
        // Whisper does not, the candidate always stands. A future flag flip
        // (V2_ENGLISH_PREFERRED), new engine, or edited table cannot make the
        // two disagree without this failing. `entry_supports` is the same
        // predicate the Settings engine picker uses to list a model's
        // languages, so this also keeps the picker and `pick_engine` honest
        // against each other.
        let platforms = [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosAppleSilicon { major: 13 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ];
        for p in platforms {
            for lang in ["en", "en-US", "de", "pl", "ar", "ja-JP", "uk", "auto"] {
                let candidate: &str = match p {
                    Platform::MacosAppleSilicon { major } if major >= 14 => "parakeet-ultra-coreml",
                    Platform::Windows => "sherpa-parakeet-v3-int8",
                    Platform::MacosAppleSilicon { .. } | Platform::MacosIntel => "parakeet-tdt-v3",
                    Platform::Other => "sherpa-parakeet-v3-int8",
                };
                // `entry_supports` is the single list predicate the Settings
                // engine picker uses to decide whether a model's language
                // list covers a language, so the detour must agree with it
                // directly (a row's `honours_language` alone cannot see a
                // 25-language table that withholds `en-US`, and the
                // free-standing table is gone by design). The test passes
                // the raw language — BCP-47 tags included — exactly as the
                // picker does; the row normalizes it, so "en-US" is
                // covered by a table that lists "en".
                let can_support = find(candidate).is_some_and(|m| entry_supports(m, Some(lang)));
                let whisper_ships = find("large-v3-turbo-q5").is_some_and(|m| m.available_on(p));
                let picked = pick_engine(None, p, Some(lang));
                assert!(
                    picked == candidate || (whisper_ships && picked == "large-v3-turbo-q5"),
                    "{p:?}/{lang}: picked {picked}, expected {candidate} or Whisper"
                );
                assert_eq!(
                    picked == "large-v3-turbo-q5",
                    whisper_ships && !can_support,
                    "{p:?}/{lang}: Whisper detour disagrees with the candidate row"
                );
            }
        }
    }

    #[test]
    fn only_the_ctc_spotter_is_auxiliary() {
        let ctc = find("parakeet-ctc-110m-coreml").expect("ctc entry");
        assert!(
            ctc.auxiliary,
            "the CTC spotter is a boosting companion, not a dictation model"
        );
        assert_eq!(CATALOG.iter().filter(|m| m.auxiliary).count(), 1);
    }

    #[test]
    fn install_and_provider_paths_follow_engine_layout() {
        let dir = std::path::Path::new("/models");

        let ggml = find("parakeet-tdt-v3").unwrap();
        assert_eq!(ggml.provider_path(dir), dir.join(ggml.file));
        assert_eq!(ggml.install_path(dir), dir.join(ggml.file));

        // The sherpa release archive extracts to a directory named after the
        // archive without its `.tar.bz2` suffix.
        let sherpa = find("sherpa-parakeet-v3-int8").unwrap();
        let sherpa_dir = dir
            .join("sherpa")
            .join("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8");
        assert_eq!(sherpa.provider_path(dir), sherpa_dir);
        assert_eq!(sherpa.install_path(dir), sherpa_dir);

        // The FluidAudio bridge takes the base directory and appends the
        // repo folder itself; the installed bytes live in that repo folder.
        // At the pinned 0.17.7 revision `Repo.folderName` strips `-coreml`
        // from the repo slug, so Ultra lives in `parakeet-ultra`, not
        // `parakeet-ultra-coreml`.
        let fluid = find("parakeet-ultra-coreml").unwrap();
        assert_eq!(fluid.provider_path(dir), dir.join("fluid"));
        assert_eq!(
            fluid.install_path(dir),
            dir.join("fluid").join("parakeet-ultra")
        );
        let v3 = find("parakeet-v3-coreml").unwrap();
        assert_eq!(
            v3.install_path(dir),
            dir.join("fluid").join("parakeet-tdt-0.6b-v3")
        );
    }

    #[test]
    fn parses_macos_major_versions() {
        assert_eq!(parse_macos_major("14.5"), Some(14));
        assert_eq!(parse_macos_major("15.0.1"), Some(15));
        assert_eq!(parse_macos_major("13"), Some(13));
        assert_eq!(parse_macos_major(" 26.1\n"), Some(26));
        assert_eq!(parse_macos_major(""), None);
        assert_eq!(parse_macos_major("not-a-version"), None);
    }

    #[test]
    fn host_platform_matches_the_build_target() {
        let p = host_platform();
        if cfg!(target_os = "macos") {
            match p {
                Platform::MacosAppleSilicon { major } => {
                    assert!(cfg!(target_arch = "aarch64"));
                    // 0 means the version probe failed; otherwise the value
                    // must be a real macOS major.
                    assert!(major == 0 || major >= 10, "detected macOS {major}");
                }
                Platform::MacosIntel => assert!(!cfg!(target_arch = "aarch64")),
                other => panic!("macOS host reported {other:?}"),
            }
        } else if cfg!(target_os = "windows") {
            assert_eq!(p, Platform::Windows);
        } else {
            assert_eq!(p, Platform::Other);
        }
    }

    #[test]
    fn english_stays_ultra_while_the_v2_flag_is_off() {
        // `V2_ENGLISH_PREFERRED` is const-false until the P3.8 bake-off, so
        // the English branches must not produce v2. (If the flag flips, this
        // test flips with it and the two assertions below become v2 picks.)
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(
            pick_engine(None, mac14, Some("en")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            pick_engine(None, Platform::Windows, Some("en")),
            "sherpa-parakeet-v3-int8"
        );
    }

    #[test]
    fn every_pick_returns_a_catalog_entry() {
        let platforms = [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ];
        for p in platforms {
            for lang in [None, Some("en"), Some("ar"), Some("auto")] {
                let id = pick_engine(None, p, lang);
                assert!(find(id).is_some(), "{id} missing from CATALOG");
            }
        }
    }
}
