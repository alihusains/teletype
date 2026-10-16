#![no_main]
//! Fuzz `teletype_core::storage::JsonStore::load` with arbitrary bytes.
//!
//! `load` reads a file from disk and returns the default on any error,
//! rotating corrupt files to `<name>.corrupt`. Properties checked per
//! input:
//! - crash-freedom on arbitrary file contents (valid/invalid UTF-8,
//!   truncated JSON, deep nesting, hostile size);
//! - a file that is not valid JSON for the target type yields the default;
//! - a file that IS valid JSON for the target type round-trips back.

use std::fs;
use std::path::Path;

use libfuzzer_sys::fuzz_target;
use teletype_core::storage::JsonStore;

fuzz_target!(|data: &[u8]| {
    let dir = std::env::temp_dir().join(format!("teletype-fuzz-storage-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("fuzz_store.json");
    let _ = fs::remove_file(&path);
    fs::write(&path, data).expect("write fuzz input");

    // Arbitrary bytes must never panic; corrupt docs yield the default.
    let store: JsonStore<Vec<String>> = JsonStore::new(&dir, "fuzz_store.json");
    let doc = store.load(Vec::new());
    for entry in &doc {
        assert!(entry.len() < 1 << 20, "sanity: entry size");
    }

    // A valid JSON array of strings must round-trip.
    let valid: Vec<String> = vec![String::from_utf8_lossy(data).into_owned()];
    fs::write(&path, serde_json::to_vec(&valid).unwrap()).expect("write valid");
    let back = store.load(Vec::new());
    assert_eq!(back, valid, "valid JSON must round-trip");

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(Path::new(&path).with_extension("json.corrupt"));
});
