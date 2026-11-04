// claim: asr-quality
//! Real-engine contract test for the transcribe.cpp rows.
//!
//! The dictation delivery path reads `transcribe_detailed` and, when the
//! engine claims `word_timings`, rebuilds the sentence from the word rows it
//! got back. `TranscribeCppProvider` documents that a model returning no word
//! rows is a valid result, so the pair (non-empty text, empty word list) is a
//! shape the app has to survive. It did not: that shape produced a take that
//! pasted nothing while the log reported a successful decode, and no test saw
//! it because the eval path uses the plain `transcribe` call.
//!
//! This prints the shape each shipped transcribe.cpp row actually returns, so
//! the contract is measured against the real artifact rather than inferred.
//!
//!     cargo test -p teletype-speech --test real_transcribe_cpp_words -- --ignored

use std::path::{Path, PathBuf};

use teletype_speech::{SpeechProvider, TranscribeCppProvider};

/// The rows this release ships on the transcribe.cpp runtime.
const ROWS: [&str; 3] = [
    "granite-speech-470m-q4",
    "moonshine-streaming-medium-q8",
    "nemotron-3.5-asr-streaming-q4",
];

fn models_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TELETYPE_MODELS_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Library/Application Support/com.teletype.app/models")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Minimal 16-bit PCM WAV reader, matching the local readers the other
/// speech-crate tests carry (the crate keeps no decode dep).
fn read_wav_f32(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(&bytes[0..4], b"RIFF", "not a RIFF file: {}", path.display());
    assert_eq!(
        &bytes[8..12],
        b"WAVE",
        "not a WAVE file: {}",
        path.display()
    );
    let mut offset = 12usize;
    let mut data: Option<&[u8]> = None;
    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if chunk_id == b"data" {
            let start = offset + 8;
            data = Some(&bytes[start..(start + chunk_size).min(bytes.len())]);
        }
        offset += 8 + chunk_size + chunk_size % 2;
    }
    let data = data.expect("no data chunk");
    data.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect()
}

#[test]
#[ignore = "requires the transcribe.cpp dylib and downloaded model files"]
fn transcribe_cpp_rows_report_their_word_row_shape() {
    if !teletype_speech::transcribe::engine_available() {
        eprintln!("SKIP: transcribe.cpp engine not built into this target");
        return;
    }
    let audio = read_wav_f32(&fixture("hello.wav"));

    for id in ROWS {
        let entry = teletype_speech::find(id).unwrap_or_else(|| panic!("{id} is a catalog entry"));
        let model = entry.provider_path(&models_dir());
        if !model.is_file() {
            eprintln!("SKIP: {id} model {} not downloaded", model.display());
            continue;
        }

        let mut provider = TranscribeCppProvider::new();
        provider
            .load(&model)
            .unwrap_or_else(|e| panic!("{id}: load {} failed: {e}", model.display()));

        // "auto" is what the live dictation path passes; the eval path too.
        let plain = provider
            .transcribe(&audio, "auto")
            .unwrap_or_else(|e| panic!("{id}: plain transcribe failed: {e}"));
        assert!(
            !plain.trim().is_empty(),
            "{id}: hello.wav yielded empty text"
        );

        let detailed = provider
            .transcribe_detailed(&audio, "auto")
            .unwrap_or_else(|e| panic!("{id}: detailed transcribe failed: {e}"));
        assert!(
            !detailed.text.trim().is_empty(),
            "{id}: detailed path yielded empty text"
        );

        // The measured contract: text lands, and the word rows may be absent.
        eprintln!(
            "{id}: plain={plain:?} detailed={:?} words={} word_timings={}",
            detailed.text,
            detailed.words.len(),
            provider.capabilities().word_timings,
        );

        provider.unload();
    }
}
