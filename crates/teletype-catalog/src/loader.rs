//! Loader: read a catalog from a directory (working copy) or the bundled
//! shipped defaults, and expose the accessors and path/url helpers the old
//! `SpeechModel` methods provided.
//!
//! The three JSON files live in the crate root and are bundled via
//! `include_str!` as the shipped defaults.

use std::path::{Path, PathBuf};

use crate::engine::Engine;
use crate::schema::Catalog;
use crate::CatalogError;

pub const WHISPER_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
pub const PARAKEET_BASE_URL: &str = "https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main";
pub const FLUID_REPO_BASE_URL: &str = "https://huggingface.co";
pub const SHERPA_MODELS_BASE_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

impl Catalog {
    /// The shipped-default catalog, parsed from the three bundled JSON files
    /// and validated. Empty domains are valid (R2), so this does not panic
    /// before the data files are populated.
    pub fn bundled() -> Self {
        let speech: Vec<crate::schema::SpeechEntry> =
            serde_json::from_str(include_str!("../speech.json"))
                .expect("bundled speech.json is valid JSON");
        let llm: Vec<crate::schema::LlmEntry> = serde_json::from_str(include_str!("../llm.json"))
            .expect("bundled llm.json is valid JSON");
        let command: Vec<crate::schema::CommandEntry> =
            serde_json::from_str(include_str!("../command.json"))
                .expect("bundled command.json is valid JSON");
        let catalog = Catalog {
            speech,
            llm,
            command,
        };
        crate::validate::validate_catalog(&catalog).expect("bundled catalog passes validation");
        catalog
    }

    /// Load a catalog from a working-copy directory. Refuses to return a
    /// catalog if any file is missing or fails validation; the caller falls
    /// back to `bundled()` on error.
    pub fn load_from_dir(dir: &Path) -> Result<Catalog, CatalogError> {
        let read = |name: &str| -> Result<String, CatalogError> {
            std::fs::read_to_string(dir.join(name)).map_err(CatalogError::Io)
        };
        let speech: Vec<crate::schema::SpeechEntry> =
            serde_json::from_str(&read("speech.json")?)
                .map_err(|e| CatalogError::InvalidJson(e.to_string()))?;
        let llm: Vec<crate::schema::LlmEntry> = serde_json::from_str(&read("llm.json")?)
            .map_err(|e| CatalogError::InvalidJson(e.to_string()))?;
        let command: Vec<crate::schema::CommandEntry> =
            serde_json::from_str(&read("command.json")?)
                .map_err(|e| CatalogError::InvalidJson(e.to_string()))?;
        let catalog = Catalog {
            speech,
            llm,
            command,
        };
        crate::validate::validate_catalog(&catalog)?;
        Ok(catalog)
    }

    pub fn speech(&self) -> &[crate::schema::SpeechEntry] {
        &self.speech
    }

    pub fn llm(&self) -> &[crate::schema::LlmEntry] {
        &self.llm
    }

    pub fn command(&self) -> &[crate::schema::CommandEntry] {
        &self.command
    }

    pub fn speech_by_id(&self, id: &str) -> Option<&crate::schema::SpeechEntry> {
        self.speech.iter().find(|e| e.id == id)
    }

    pub fn llm_by_id(&self, id: &str) -> Option<&crate::schema::LlmEntry> {
        self.llm.iter().find(|e| e.id == id)
    }

    pub fn command_by_id(&self, id: &str) -> Option<&crate::schema::CommandEntry> {
        self.command.iter().find(|e| e.id == id)
    }

    /// Whether the app may offer this speech model on `platform`: the host
    /// can load it, it is a dictation model rather than an auxiliary
    /// companion, the Core ML rows only exist when this build links the
    /// FluidAudio bridge, and this build compiled the engine that loads it.
    pub fn speech_offered_on(
        &self,
        entry: &crate::schema::SpeechEntry,
        platform: crate::engine::Platform,
        fluid_available: bool,
        parakeet_cpp_available: bool,
    ) -> bool {
        let engine = match Engine::from_name(&entry.engine) {
            Ok(e) => e,
            Err(_) => return false,
        };
        let transcribe_cpp_available =
            matches!(engine, Engine::TranscribeCpp) && platform.is_macos();
        engine.available_on(platform)
            && !entry.auxiliary
            && (engine != Engine::ParakeetCoreml || fluid_available)
            && engine.built_on(platform, parakeet_cpp_available, transcribe_cpp_available)
            && !(platform.is_macos() && engine == Engine::SherpaParakeet)
    }

    /// The path handed to the speech provider under `models_dir`.
    pub fn speech_provider_path(
        &self,
        entry: &crate::schema::SpeechEntry,
        models_dir: &Path,
    ) -> PathBuf {
        let engine = Engine::from_name(&entry.engine).unwrap_or(Engine::Whisper);
        let file = entry
            .artifacts
            .first()
            .map(|a| a.file.as_str())
            .unwrap_or(&entry.id);
        match engine {
            Engine::ParakeetCoreml => models_dir.join("fluid"),
            Engine::SherpaParakeet => models_dir.join("sherpa").join(sherpa_dir_name(file)),
            Engine::Qwen3Asr => models_dir.join(&entry.id),
            Engine::AppleSpeech => models_dir.join("apple-speech"),
            Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp | Engine::TranscribeCpp => {
                models_dir.join(file)
            }
            Engine::Onnx => models_dir.join(&entry.id),
        }
    }

    /// Where this model's bytes live under `models_dir`, used for installed
    /// / downloaded status. Differs from `speech_provider_path` only for the
    /// Core ML bridge, which receives the base directory.
    pub fn speech_install_path(
        &self,
        entry: &crate::schema::SpeechEntry,
        models_dir: &Path,
    ) -> PathBuf {
        let engine = Engine::from_name(&entry.engine).unwrap_or(Engine::Whisper);
        if engine == Engine::ParakeetCoreml {
            let file = entry
                .artifacts
                .first()
                .map(|a| a.file.as_str())
                .unwrap_or(&entry.id);
            models_dir.join("fluid").join(fluid_dir_name(file))
        } else {
            self.speech_provider_path(entry, models_dir)
        }
    }

    /// The download URL for this speech model's primary artifact.
    pub fn speech_url(&self, entry: &crate::schema::SpeechEntry) -> String {
        let file = match entry.artifacts.first() {
            Some(a) => a.file.as_str(),
            None => return String::new(),
        };
        if let Some(base) = &entry.base_url {
            return format!("{base}/{file}");
        }
        let engine = Engine::from_name(&entry.engine).unwrap_or(Engine::Whisper);
        match engine {
            Engine::Whisper => format!("{WHISPER_BASE_URL}/{file}"),
            Engine::Parakeet | Engine::ParakeetCpp => format!("{PARAKEET_BASE_URL}/{file}"),
            Engine::ParakeetCoreml => format!("{FLUID_REPO_BASE_URL}/FluidInference/{file}"),
            Engine::SherpaParakeet => format!("{SHERPA_MODELS_BASE_URL}/{file}"),
            Engine::Qwen3Asr | Engine::TranscribeCpp => {
                tracing::warn!("{:?} row has no base_url; returning an empty URL", engine);
                String::new()
            }
            Engine::AppleSpeech => {
                tracing::warn!("apple speech rows are not downloadable; returning an empty URL");
                String::new()
            }
            Engine::Onnx => String::new(),
        }
    }
}

/// The directory a sherpa release archive extracts to: its file name without
/// the `.tar.bz2` suffix.
fn sherpa_dir_name(file: &str) -> &str {
    file.strip_suffix(".tar.bz2").unwrap_or(file)
}

/// The cache folder FluidAudio uses for a Core ML repo slug.
fn fluid_dir_name(file: &str) -> &str {
    match file {
        "parakeet-ctc-110m-coreml" | "parakeet-ctc-0.6b-coreml" => file,
        _ => file.strip_suffix("-coreml").unwrap_or(file),
    }
}
