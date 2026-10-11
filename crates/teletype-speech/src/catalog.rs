//! Speech (dictation) model catalog.
//!
//! Two local engines:
//! - Parakeet TDT v3 (NVIDIA, via whisper.cpp's parakeet engine) — the
//!   state-of-the-art dictation model, recommended default.
//! - Whisper GGML files published by whisper.cpp on Hugging Face.
//!
//! The same lineup SpeakType ships.

use serde::Serialize;

const WHISPER_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const PARAKEET_BASE_URL: &str = "https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main";
/// Cactus-Compute publishes the Whistle bundle under this org.
const CACTUS_BASE_URL: &str = "https://huggingface.co/Cactus-Compute/whistle/resolve/main";

/// Which local engine loads a model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    Whisper,
    Parakeet,
    /// Cactus Whistle (Cactus-Compute), loaded through the `libcactus_engine`
    /// FFI. English-first, multilingual-capable, ~30 s per inference window.
    Whistle,
}

/// What a speech model can do, independent of which engine loads it.
///
/// Phase 0 of the command/agent roadmap: the app must be able to select
/// between STT engines and know their capabilities without branching on the
/// engine name throughout the codebase.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    /// The engine accepts audio incrementally (streaming / partial results).
    pub streaming: bool,
    /// The engine reports per-word timestamps.
    pub word_timestamps: bool,
    /// The engine can auto-detect the spoken language.
    pub language_detection: bool,
    /// The engine accepts hotword / keyword biasing.
    pub keyword_biasing: bool,
    /// The engine can produce speech embeddings.
    pub embeddings: bool,
    /// Maximum seconds of audio per single inference call, when the engine
    /// documents a hard limit (Whistle: ~30 s). `None` = no known limit.
    pub max_audio_seconds: Option<u32>,
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
    /// Exact published file size in bytes, when pinned. Shown in the UI
    /// instead of the rounded `size_mb` and used as the download's expected
    /// total. `None` keeps the legacy rounded-MB behavior.
    #[serde(skip)]
    pub size_bytes: Option<u64>,
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
    /// Whether the model can be downloaded and used right now. `false` for
    /// engines whose native runtime is not yet bundled (Whistle until the
    /// Cactus static library ships). The UI shows it as "coming soon".
    #[serde(default)]
    pub available: bool,
}

impl SpeechModel {
    pub fn url(&self) -> String {
        match self.engine {
            Engine::Whisper => format!("{WHISPER_BASE_URL}/{}", self.file),
            Engine::Parakeet => format!("{PARAKEET_BASE_URL}/{}", self.file),
            Engine::Whistle => format!("{CACTUS_BASE_URL}/{}", self.file),
        }
    }

    /// Capability metadata for this model, derived from its engine.
    pub fn capabilities(&self) -> ModelCapabilities {
        match self.engine {
            Engine::Whisper => ModelCapabilities {
                streaming: false,
                word_timestamps: false,
                language_detection: true,
                keyword_biasing: false,
                embeddings: false,
                max_audio_seconds: None,
            },
            Engine::Parakeet => ModelCapabilities {
                streaming: false,
                word_timestamps: false,
                language_detection: true,
                keyword_biasing: false,
                embeddings: false,
                max_audio_seconds: None,
            },
            // Whistle documents a ~30 s per-inference window; the Cactus
            // engine exposes streaming transcribe and (later) biasing.
            Engine::Whistle => ModelCapabilities {
                streaming: true,
                word_timestamps: false,
                language_detection: true,
                keyword_biasing: true,
                embeddings: true,
                max_audio_seconds: Some(30),
            },
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
    size_bytes: None,
    sha256: None,
    english_only: false,
    recommended: false,
    description: "",
    languages: None,
    speed: 0.0,
    accuracy: 0.0,
    min_ram_gb: 0,
    available: true,
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
        id: "large-v3-turbo-q5",
        name: "Whisper Large v3 Turbo (compressed)",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size_mb: 547,
        sha256: None,
        description:
            "Near-flagship accuracy at a third of the size. Great for dictation in any language.",
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
    // Whistle (Cactus-Compute): English-first, multilingual-capable, built for
    // local deployment. Gated `available: false` until the Cactus static
    // runtime is bundled and a pinned `.cactus` bundle is published — the
    // entry exists so the engine is discoverable and the capability metadata
    // is testable today.
    SpeechModel {
        id: "whistle",
        name: "Whistle (Cactus)",
        file: "whistle.cact",
        engine: Engine::Whistle,
        size_mb: 17,
        size_bytes: Some(16_919_407),
        sha256: Some("b6e02f048568ac5d01a2042556c658061e699acbc0aa2a1439f52f3d461dffeb"),
        recommended: false,
        description: "16 MB local model from Cactus-Compute. Fast, on-device, 7 languages.",
        languages: Some(&["en", "de", "fr", "es", "it", "nl", "pl"]),
        speed: 9.0,
        accuracy: 8.5,
        min_ram_gb: 2,
        available: true,
        ..WHISPER
    },
];

/// Finds a catalog entry by id.
pub fn find(id: &str) -> Option<&'static SpeechModel> {
    CATALOG.iter().find(|m| m.id == id)
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
                Engine::Whistle => assert!(m.url().starts_with(CACTUS_BASE_URL)),
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
}
