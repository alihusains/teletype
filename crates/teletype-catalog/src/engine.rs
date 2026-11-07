//! Engine registry: the existing `Engine` enum re-keyed by a stable string
//! name, plus `Onnx` for the NeuDecide command decider.
//!
//! `Platform` is DEFINED here so the catalog crate has no speech dependency;
//! `teletype-speech` re-exports it.

use crate::CatalogError;

/// The host platform facts the engine predicates need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS on Apple Silicon; `major` is the macOS major version (14+ gets
    /// the Neural Engine engines).
    MacosAppleSilicon { major: u16 },
    /// macOS on Intel - no Neural Engine, so the ggml build is the default.
    MacosIntel,
    /// Windows - sherpa-onnx is the default engine.
    Windows,
    /// Anything else (Linux, ...) - the ggml build is the default.
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

/// Which local engine loads a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Whisper,
    Parakeet,
    ParakeetCoreml,
    SherpaParakeet,
    ParakeetCpp,
    Qwen3Asr,
    AppleSpeech,
    TranscribeCpp,
    /// The NeuDecide command decider; no speech provider, never offered as a
    /// speech row.
    Onnx,
}

impl Engine {
    /// Resolve a registry name to its engine. Unknown names are an error,
    /// never a variant (closed-engine-set guarantee).
    pub fn from_name(name: &str) -> Result<Engine, CatalogError> {
        match name {
            "whisper" => Ok(Engine::Whisper),
            "parakeet" => Ok(Engine::Parakeet),
            "parakeet-coreml" => Ok(Engine::ParakeetCoreml),
            "sherpa" => Ok(Engine::SherpaParakeet),
            "parakeet-cpp" => Ok(Engine::ParakeetCpp),
            "llama-server" => Ok(Engine::Qwen3Asr),
            "apple-speech" => Ok(Engine::AppleSpeech),
            "transcribe-cpp" => Ok(Engine::TranscribeCpp),
            "onnx" => Ok(Engine::Onnx),
            other => Err(CatalogError::UnknownEngine(other.to_string())),
        }
    }

    /// The stable registry name for this engine.
    pub fn name(self) -> &'static str {
        match self {
            Engine::Whisper => "whisper",
            Engine::Parakeet => "parakeet",
            Engine::ParakeetCoreml => "parakeet-coreml",
            Engine::SherpaParakeet => "sherpa",
            Engine::ParakeetCpp => "parakeet-cpp",
            Engine::Qwen3Asr => "llama-server",
            Engine::AppleSpeech => "apple-speech",
            Engine::TranscribeCpp => "transcribe-cpp",
            Engine::Onnx => "onnx",
        }
    }

    /// Whether this engine ships on `platform`.
    pub fn available_on(self, platform: Platform) -> bool {
        match self {
            Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp => platform.is_macos(),
            Engine::ParakeetCoreml => {
                matches!(platform, Platform::MacosAppleSilicon { major } if major >= 14)
            }
            Engine::SherpaParakeet => true,
            Engine::AppleSpeech => {
                matches!(platform, Platform::MacosAppleSilicon { major } if major >= 26)
            }
            Engine::Qwen3Asr => platform.is_macos(),
            Engine::TranscribeCpp => platform.is_macos(),
            Engine::Onnx => true,
        }
    }

    /// Whether this build actually compiled the engine, given the host's
    /// engine-capability facts.
    ///
    /// The catalog crate has no speech dependency, so the two build facts
    /// (parakeet.cpp and transcribe.cpp availability) are passed in by the
    /// caller that owns those engine modules.
    pub fn built_on(
        self,
        platform: Platform,
        parakeet_cpp_available: bool,
        transcribe_cpp_available: bool,
    ) -> bool {
        match self {
            Engine::ParakeetCpp => platform.is_macos() && parakeet_cpp_available,
            Engine::TranscribeCpp => platform.is_macos() && transcribe_cpp_available,
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
            Engine::Qwen3Asr => "Qwen3-ASR (llama.cpp)",
            Engine::AppleSpeech => "Apple speech (on-device)",
            Engine::TranscribeCpp => "transcribe.cpp",
            Engine::Onnx => "ONNX decider",
        }
    }

    /// Human-readable platform requirement, shown next to the engine label.
    pub fn platform_label(self) -> &'static str {
        match self {
            Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp => "macOS only",
            Engine::ParakeetCoreml => "Apple Silicon, macOS 14+",
            Engine::SherpaParakeet => "Windows, macOS, Linux",
            Engine::Qwen3Asr => "macOS only (llama-server)",
            Engine::AppleSpeech => "Apple Silicon, macOS 26+",
            Engine::TranscribeCpp => "macOS only (transcribe.cpp)",
            Engine::Onnx => "any",
        }
    }
}
