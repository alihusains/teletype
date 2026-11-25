//! Offline speaker diarization via sherpa-onnx (plan: meeting feature,
//! Windows parity for "who said what").
//!
//! Uses sherpa-onnx's combined `OfflineSpeakerDiarization` — pyannote
//! segmentation + 3D-Speaker campplus embeddings + the built-in fast
//! clustering — all on the same prebuilt CPU ONNX Runtime the Parakeet
//! recognizer already links, so it runs cross-platform with no C toolchain.
//!
//! ## Models
//!
//! Two pinned artifacts under `<models>/sherpa/diarization/`:
//!
//! - `sherpa-onnx-pyannote-segmentation-3-0/` (extracted from the
//!   `speaker-segmentation-models` release archive; `model.onnx` inside)
//! - `3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx` (from the
//!   `speaker-recongition-models` release; English VoxCeleb, 16 kHz)
//!
//! [`diarization_available`] reports whether both are installed; the
//! download path lives in the desktop crate (it owns the network layer).

use std::path::{Path, PathBuf};

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

/// Extracted segmentation model directory (relative to the models dir).
pub const SEGMENTATION_DIR: &str = "sherpa-onnx-pyannote-segmentation-3-0";
/// The English campplus embedding model file.
pub const EMBEDDING_FILE: &str = "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx";

/// The diarization models live under the sherpa models folder.
pub fn diarization_dir(models_dir: &Path) -> PathBuf {
    models_dir.join("sherpa").join("diarization")
}

/// `<models>/sherpa/diarization/sherpa-onnx-pyannote-segmentation-3-0/model.onnx`
pub fn segmentation_model(models_dir: &Path) -> PathBuf {
    diarization_dir(models_dir)
        .join(SEGMENTATION_DIR)
        .join("model.onnx")
}

/// `<models>/sherpa/diarization/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`
pub fn embedding_model(models_dir: &Path) -> PathBuf {
    diarization_dir(models_dir).join(EMBEDDING_FILE)
}

/// Both artifacts present on disk (the download path verifies checksums).
pub fn diarization_available(models_dir: &Path) -> bool {
    segmentation_model(models_dir).is_file() && embedding_model(models_dir).is_file()
}

/// One diarization segment: `[start_ms, end_ms)` spoken by `speaker`
/// (a raw cluster id; stable labelling happens in the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiarizationSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u32,
}

/// A loaded diarizer. Creation is cheap enough to do per meeting; the
/// models stay resident for the object's lifetime.
pub struct SpeakerDiarizer {
    inner: OfflineSpeakerDiarization,
}

impl std::fmt::Debug for SpeakerDiarizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeakerDiarizer").finish_non_exhaustive()
    }
}

impl SpeakerDiarizer {
    /// Loads the diarization models from `models_dir`. `None` when either
    /// artifact is missing or the model fails to initialize — callers fall
    /// back to the single-speaker labels.
    pub fn try_new(models_dir: &Path) -> Option<Self> {
        let seg = segmentation_model(models_dir);
        let emb = embedding_model(models_dir);
        if !seg.is_file() || !emb.is_file() {
            return None;
        }
        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(seg.to_string_lossy().to_string()),
                    window_shift_ratio: 0.1,
                },
                num_threads: 1,
                ..Default::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(emb.to_string_lossy().to_string()),
                num_threads: 1,
                ..Default::default()
            },
            // Auto cluster count; the default cosine distance threshold.
            clustering: FastClusteringConfig::default(),
            ..Default::default()
        };
        let inner = OfflineSpeakerDiarization::create(&config)?;
        Some(Self { inner })
    }

    /// Diarizes 16 kHz mono `samples`. The model is 16 kHz only; a rate
    /// mismatch is a `None`, not a panic.
    pub fn process(&self, samples: &[f32]) -> Option<Vec<DiarizationSegment>> {
        if self.inner.sample_rate() != 16_000 {
            return None;
        }
        let result = self.inner.process(samples)?;
        let segments = result
            .sort_by_start_time()
            .into_iter()
            .map(|s| DiarizationSegment {
                start_ms: (s.start * 1000.0).round() as u64,
                end_ms: (s.end * 1000.0).round() as u64,
                speaker: s.speaker as u32,
            })
            .collect();
        Some(segments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_paths_live_under_the_diarization_dir() {
        let dir = Path::new("/tmp/models");
        assert_eq!(
            segmentation_model(dir),
            dir.join("sherpa")
                .join("diarization")
                .join(SEGMENTATION_DIR)
                .join("model.onnx")
        );
        assert_eq!(
            embedding_model(dir),
            dir.join("sherpa").join("diarization").join(EMBEDDING_FILE)
        );
        assert!(!diarization_available(dir));
    }

    #[test]
    fn missing_models_yield_no_diarizer() {
        let tmp = std::env::temp_dir().join("teletype-diar-test");
        let _ = std::fs::create_dir_all(&tmp);
        assert!(SpeakerDiarizer::try_new(&tmp).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
