//! Speech (dictation) model catalog.
//!
//! Four local engines:
//! - Parakeet TDT v3 (NVIDIA, via whisper.cpp's parakeet engine) — the
//!   state-of-the-art ggml build, the cross-platform default.
//! - Parakeet Core ML (FluidAudio) — the Neural Engine build on macOS 14+
//!   Apple Silicon, including Parakeet Ultra.
//! - sherpa-onnx NeMo Parakeet — CPU int8 builds, the Windows default.
//! - Whisper GGML files published by whisper.cpp on Hugging Face — the
//!   fallback for languages Parakeet does not cover.

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
    /// Hex SHA-256 of the published file, when we have pinned one.
    ///
    /// `None` for every entry today, which is a real gap: the LLM model catalog
    /// pins all 8 of its shards and the speech catalog pins nothing, so a
    /// compromised mirror or a mutable `resolve/main` ref can change the bytes
    /// the app downloads. The verify path in `download_speech_model` reads this
    /// field, so filling one in is a one-line change per model and needs no
    /// other code.
    #[serde(skip)]
    pub sha256: Option<&'static str>,
    /// English-only model.
    pub english_only: bool,
    /// Recommended as the default dictation model.
    pub recommended: bool,
    pub description: &'static str,
    /// Languages the model can transcribe, or `None` for every Whisper language (99).
    pub languages: Option<&'static [&'static str]>,
    /// Relative speed score out of 10 (shown as a bar in the UI).
    pub speed: f64,
    /// Relative accuracy score out of 10 (shown as a bar in the UI).
    pub accuracy: f64,
    /// Minimum RAM in GB for comfortable use.
    pub min_ram_gb: u32,
}

impl SpeechModel {
    pub fn url(&self) -> String {
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
    description: "",
    languages: None,
    speed: 0.0,
    accuracy: 0.0,
    min_ram_gb: 0,
};

pub const CATALOG: &[SpeechModel] = &[
    SpeechModel {
        id: "parakeet-tdt-v3",
        name: "Parakeet v3",
        file: "ggml-parakeet-tdt-0.6b-v3-q8_0.bin",
        engine: Engine::Parakeet,
        size_mb: 640,
        sha256: None,
        recommended: true,
        description: "Fastest model, with punctuation. English and 24 European languages.",
        languages: Some(PARAKEET_V3_LANGUAGES),
        speed: 9.7,
        accuracy: 9.2,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-tdt-v2",
        name: "Parakeet v2 (English)",
        file: "ggml-parakeet-tdt-0.6b-v2-q8_0.bin",
        engine: Engine::Parakeet,
        size_mb: 631,
        sha256: None,
        english_only: true,
        languages: Some(&["en"]),
        description: "Very fast and accurate for English.",
        speed: 9.8,
        accuracy: 9.1,
        min_ram_gb: 4,
        ..WHISPER
    },
    SpeechModel {
        id: "parakeet-tdt-v3-q4",
        name: "Parakeet v3 (compact)",
        file: "ggml-parakeet-tdt-0.6b-v3-q4_0.bin",
        engine: Engine::Parakeet,
        size_mb: 356,
        sha256: None,
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
        sha256: None,
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
        size_mb: 640,
        sha256: None,
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
        size_mb: 630,
        sha256: None,
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
        sha256: None,
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
        sha256: None,
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
        sha256: None,
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
        sha256: None,
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
        sha256: None,
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
        sha256: None,
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

/// Whether the Parakeet 25-language list covers `language` (Ultra and v3
/// share the tokenizer and language list; v2 is English-only).
fn parakeet_supports(language: &str) -> bool {
    PARAKEET_V3_LANGUAGES.contains(&language)
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

/// The id of a [`SpeechModel`] in [`CATALOG`].
pub type ModelId = &'static str;

/// Picks the default speech-model id for the current host and language.
///
/// Selection order (plan §3.2):
/// 1. an explicit user pin always wins (unknown ids are ignored, they cannot
///    be loaded);
/// 2. a language outside Parakeet's 25 routes to Whisper (99 languages);
/// 3. macOS 14+ Apple Silicon uses the Core ML Ultra build (or v2 for
///    English once [`V2_ENGLISH_PREFERRED`] is flipped by the P3.8 bake-off);
/// 4. Windows uses the sherpa-onnx int8 build;
/// 5. everything else keeps the proven ggml Parakeet v3.
///
/// `language` is the Settings language ("en", "de", …, "auto", or a BCP-47
/// tag such as "en-US"); `None`/`"auto"` mean "engine decides", which selects
/// the platform default rather than routing to Whisper.
pub fn pick_engine(pinned: Option<&str>, platform: Platform, language: Option<&str>) -> ModelId {
    // 1. Pin first: the user's explicit choice is never second-guessed.
    if let Some(id) = pinned {
        if let Some(entry) = find(id) {
            // Return the catalog entry's own `'static` id, not the borrowed
            // input, so the signature can stay `-> ModelId`.
            return entry.id;
        }
    }

    // Normalize "en-US" → "en"; treat "auto"/empty as "no language set".
    let lang = language
        .map(|l| l.split(['-', '_']).next().unwrap_or(l))
        .filter(|l| !l.is_empty() && *l != "auto");

    // 2. Whisper covers every language Parakeet does not.
    if matches!(lang, Some(l) if !parakeet_supports(l)) {
        return "large-v3-turbo-q5";
    }

    // 3. macOS 14+ on Apple Silicon runs Core ML on the Neural Engine.
    if let Platform::MacosAppleSilicon { major } = platform {
        if major >= 14 {
            return if V2_ENGLISH_PREFERRED && lang == Some("en") {
                "parakeet-v2-coreml"
            } else {
                "parakeet-ultra-coreml"
            };
        }
    }

    // 4. Windows runs sherpa-onnx on CPU.
    if platform == Platform::Windows {
        return if V2_ENGLISH_PREFERRED && lang == Some("en") {
            "sherpa-parakeet-v2-int8"
        } else {
            "sherpa-parakeet-v3-int8"
        };
    }

    // 5. Intel Macs, older macOS, Linux, …: the ggml build.
    "parakeet-tdt-v3"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique() {
        let mut ids: Vec<_> = CATALOG.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
    }

    #[test]
    fn exactly_one_recommended() {
        assert_eq!(CATALOG.iter().filter(|m| m.recommended).count(), 1);
    }

    #[test]
    fn find_returns_entries() {
        assert!(find("large-v3-turbo-q5").is_some());
        assert!(find("no-such-model").is_none());
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
            }
            assert!(m.url().ends_with(m.file));
        }
    }

    #[test]
    fn parakeet_is_recommended() {
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
        assert_eq!(
            pick_engine(None, Platform::MacosAppleSilicon { major: 15 }, Some("de")),
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
        assert_eq!(
            pick_engine(None, Platform::Other, Some("fr")),
            "parakeet-tdt-v3"
        );
    }

    #[test]
    fn uncovered_languages_route_to_whisper() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        for lang in ["ar", "hi", "ur", "ja", "zh", "ko", "tr", "fa"] {
            assert_eq!(
                pick_engine(None, mac14, Some(lang)),
                "large-v3-turbo-q5",
                "macOS/{lang}"
            );
            assert_eq!(
                pick_engine(None, Platform::Windows, Some(lang)),
                "large-v3-turbo-q5",
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
    fn pin_always_wins() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(
            pick_engine(Some("large-v3-turbo"), mac14, Some("ar")),
            "large-v3-turbo"
        );
        assert_eq!(
            pick_engine(Some("parakeet-tdt-v2"), Platform::Windows, Some("ja")),
            "parakeet-tdt-v2"
        );
        assert_eq!(
            pick_engine(Some("sherpa-parakeet-v2-int8"), mac14, None),
            "sherpa-parakeet-v2-int8"
        );
        // Unknown ids are ignored (they cannot be loaded), so automatic
        // selection continues.
        assert_eq!(
            pick_engine(Some("no-such-model"), mac14, None),
            "parakeet-ultra-coreml"
        );
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
