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

/// Which local engine loads a model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    Whisper,
    Parakeet,
}

/// One downloadable speech model.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModel {
    pub id: &'static str,
    pub name: &'static str,
    /// The model file name inside the models directory.
    pub file: &'static str,
    /// The engine that loads this file.
    pub engine: Engine,
    /// Approximate download size in MB.
    pub size_mb: u32,
    /// English-only model.
    pub english_only: bool,
    /// Recommended as the default dictation model.
    pub recommended: bool,
    pub description: &'static str,
}

impl SpeechModel {
    pub fn url(&self) -> String {
        match self.engine {
            Engine::Whisper => format!("{WHISPER_BASE_URL}/{}", self.file),
            Engine::Parakeet => format!("{PARAKEET_BASE_URL}/{}", self.file),
        }
    }
}

pub const CATALOG: &[SpeechModel] = &[
    SpeechModel {
        id: "parakeet-tdt-v3",
        name: "Parakeet TDT v3",
        file: "ggml-parakeet-tdt-0.6b-v3-q8_0.bin",
        engine: Engine::Parakeet,
        size_mb: 637,
        english_only: false,
        recommended: true,
        description: "State-of-the-art dictation: very fast with punctuation. English and 24 European languages.",
    },
    SpeechModel {
        id: "large-v3-turbo-q5",
        name: "Whisper Large v3 Turbo (compressed)",
        file: "ggml-large-v3-turbo-q5_0.bin",
        engine: Engine::Whisper,
        size_mb: 547,
        english_only: false,
        recommended: false,
        description: "Near-flagship accuracy at a third of the size. Great for dictation in any language.",
    },
    SpeechModel {
        id: "small-en",
        name: "Whisper Small (English)",
        file: "ggml-small.en.bin",
        engine: Engine::Whisper,
        size_mb: 466,
        english_only: true,
        recommended: false,
        description: "Good balance of speed and accuracy for English.",
    },
    SpeechModel {
        id: "base-en",
        name: "Whisper Base (English)",
        file: "ggml-base.en.bin",
        engine: Engine::Whisper,
        size_mb: 142,
        english_only: true,
        recommended: false,
        description: "Fast on any machine. Fine for short English dictation.",
    },
    SpeechModel {
        id: "base",
        name: "Whisper Base",
        file: "ggml-base.bin",
        engine: Engine::Whisper,
        size_mb: 142,
        english_only: false,
        recommended: false,
        description: "Small and quick to download. A good way to get started in any language.",
    },
    SpeechModel {
        id: "tiny",
        name: "Whisper Tiny",
        file: "ggml-tiny.bin",
        engine: Engine::Whisper,
        size_mb: 75,
        english_only: false,
        recommended: false,
        description: "Fastest Whisper model and least accurate. Useful for testing.",
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
            }
            assert!(m.url().ends_with(m.file));
        }
    }

    #[test]
    fn parakeet_is_recommended() {
        assert!(find("parakeet-tdt-v3").unwrap().recommended);
        assert_eq!(find("parakeet-tdt-v3").unwrap().engine, Engine::Parakeet);
    }
}
