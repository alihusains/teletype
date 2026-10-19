//! Fuzz target for `JsonStore::load` on arbitrary byte inputs.
//!
//! Goal: confirm that no arbitrary byte sequence causes a panic, memory
//! unsafety, or infinite loop in the storage deserialization path.
//!
//! Bugs caught by this target:
//! - Stack overflow on deeply-nested JSON (serde_json's default depth limit
//!   protects against this, but the target verifies it holds).
//! - Panic in the corrupt-file quarantine path when the parent directory
//!   check fails.
//!
//! Run with: `cargo fuzz run fuzz_storage_load`
//! (requires `cargo install cargo-fuzz` and a nightly toolchain)

#![no_main]

use libfuzzer_sys::fuzz_target;
use teletype_core::storage::JsonStore;
use teletype_core::dictionary::Dictionary;
use std::io::Write as _;

fuzz_target!(|data: &[u8]| {
    // Write the fuzzer-supplied bytes to a temp file so `JsonStore::load` reads
    // them the same way it does in production: via `std::fs::read`.
    let dir = std::env::temp_dir().join(format!(
        "teletype-fuzz-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("fuzz_store.json");

    // Write the fuzz input.
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = f.write_all(data);
    }

    // Load as a Dictionary (the most complex store type in production).
    // `JsonStore::load` must never panic — it must return the default on error.
    let store: JsonStore<Dictionary> = JsonStore::new(&dir, "fuzz_store.json");
    let _result = store.load(Dictionary::default());

    // Clean up so successive runs don't accumulate `.corrupt` files.
    let _ = std::fs::remove_file(&path);
    // Remove any quarantine files left by the corrupt-file handler.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().contains("corrupt") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
});
