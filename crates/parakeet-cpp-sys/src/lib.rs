//! Build-time only crate: compiles and links the `parakeet` static library
//! from mudler/parakeet.cpp (pinned commit, see `build.rs`).
//!
//! The CMake build happens in `build.rs`, which emits `cargo:root` (its
//! OUT_DIR) so downstream build scripts can locate the static libraries and
//! re-emit the native link directives. See `teletype-speech/build.rs`.
