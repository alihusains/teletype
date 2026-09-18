//! Build-time only crate: compiles and links the `parakeet` and `ggml`
//! static libraries from whisper.cpp v1.9.4.
//!
//! The CMake build happens in `build.rs`, which emits `cargo:root` (its
//! OUT_DIR) so downstream build scripts can locate the static libraries and
//! re-emit the native link directives. See `teletype-speech/build.rs`.
