#![no_main]
//! Fuzz the hotwords-file format consumed by the sherpa-onnx recognizer.
//!
//! There is no standalone hotwords *loader* function in the crate:
//! `SherpaParakeetProvider::config_for` (crates/teletype-speech/src/sherpa.rs)
//! takes a hotwords path and hands the raw file to
//! `OfflineRecognizerConfig::hotwords_file`, where sherpa-onnx parses it.
//! That parsing happens inside the native lib and needs the full model
//! directory, so it cannot be fuzzed in-process.
//!
//! Instead this target fuzzes the *shape* of the file the app would write:
//! one hotword per line, arbitrary bytes. It asserts the invariants the
//! Rust side relies on when it passes the file through:
//! - the lossy string view never panics and is idempotent;
//! - the file round-trips through a temp file exactly as written (the app
//!   writes the file, sherpa reads it back);
//! - line splitting is total over arbitrary bytes.
//!
//! When a real loader lands (e.g. a `load_hotwords` that validates lines),
//! point this target at it and keep the same invariants.

use std::fs;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    // Idempotence of the lossy view.
    let again = String::from_utf8_lossy(text.as_bytes());
    assert_eq!(&*text, &*again, "from_utf8_lossy must be idempotent");

    // The app writes one hotword per line; verify the line discipline the
    // downstream parser expects: no NUL bytes survive a lossy decode.
    for line in text.lines() {
        assert!(!line.contains('\0'), "lossy decode must not yield NULs");
    }

    // Round-trip through a temp file: what the app writes is what sherpa
    // reads. Written bytes must come back byte-identical.
    let path = std::env::temp_dir().join(format!("teletype-fuzz-hotwords-{}", std::process::id()));
    fs::write(&path, data).expect("write hotwords file");
    let back = fs::read(&path).expect("read hotwords file");
    assert_eq!(
        &back,
        data,
        "hotwords file must round-trip byte-identical"
    );
    let _ = fs::remove_file(&path);
});
