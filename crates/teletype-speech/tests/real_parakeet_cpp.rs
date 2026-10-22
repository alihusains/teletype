//! Real-engine test for the parakeet.cpp backend (Fluid Speech Mini).
//!
//! The engine is a dlopen'd dylib produced by a build script, so "it compiles"
//! says nothing about whether a given build can load a model, and the catalog
//! row is what maps a model id to that engine. This test wires the whole
//! chain — catalog entry -> engine availability -> `provider_path` -> load ->
//! transcribe — against the real artifact. It is `#[ignore]`d because it needs
//! a downloaded model, and it skips (rather than fails) when either artifact
//! is missing.
//!
//!     cargo test -p teletype-speech --test real_parakeet_cpp -- --ignored

use std::path::{Path, PathBuf};

use teletype_speech::{ParakeetCppProvider, SpeechError, SpeechProvider};

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
/// speech-crate tests and examples carry (the crate keeps no decode dep).
fn read_wav_f32(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(&bytes[0..4], b"RIFF", "not a RIFF file: {}", path.display());
    assert_eq!(&bytes[8..12], b"WAVE", "not a WAVE file: {}", path.display());
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
#[ignore = "requires the parakeet.cpp dylib and a downloaded Fluid Speech Mini model"]
fn fluid_speech_mini_loads_and_meets_the_provider_contract() {
    if !teletype_speech::parakeet_cpp::engine_available() {
        eprintln!(
            "SKIP: parakeet.cpp dylib not present in this build \
             (TELETYPE_SPEECH_PARAKEET_CPP_LIB unset or file missing)"
        );
        return;
    }

    let model = teletype_speech::find("fluid-speech-mini")
        .expect("fluid-speech-mini is a catalog entry")
        .provider_path(&models_dir());
    if !model.is_file() {
        eprintln!("SKIP: {} not downloaded", model.display());
        return;
    }

    let mut provider = ParakeetCppProvider::new();
    provider
        .load(&model)
        .unwrap_or_else(|e| panic!("load {} failed: {e}", model.display()));
    assert!(provider.is_loaded());

    // Real speech must produce text — the conformance suite's rule 2.
    let hello = read_wav_f32(&fixture("hello.wav"));
    let text = provider
        .transcribe(&hello, "en")
        .expect("hello.wav must transcribe on a loaded parakeet.cpp model");
    assert!(!text.trim().is_empty(), "hello.wav yielded empty text");
    eprintln!("hello.wav ({} samples) -> {text:?}", hello.len());

    // Silence must be NoSpeech, not a hallucinated word and not a crash —
    // the conformance suite's rule 3.
    let silence = read_wav_f32(&fixture("silence.wav"));
    match provider.transcribe(&silence, "en") {
        Err(SpeechError::NoSpeech) => {}
        other => panic!("silence.wav must yield NoSpeech, got {other:?}"),
    }

    provider.unload();
    assert!(!provider.is_loaded());

    // The same release the app performs on Quit. Parakeet.cpp keeps its GPU
    // backend in a process-global that static destruction never frees, so
    // skipping this makes the ggml Metal device abort in
    // `ggml_metal_rsets_free` while the process exits — the test body passes
    // but the run fails with SIGABRT.
    teletype_speech::parakeet_cpp::shutdown_backend();
}
