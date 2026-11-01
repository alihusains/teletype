//! Apple on-device speech provider (SpeechAnalyzer: SpeechTranscriber and
//! DictationTranscriber), macOS 26+ only, zero app download.
//!
//! The models live in system storage and are installed by the OS through
//! `AssetInventory`, so there is no file on disk for Teletype to download or
//! pin: a catalog row for this engine carries `size_mb: 0` and `sha256: None`.
//!
//! ## Platform split
//!
//! - **macOS on Apple Silicon with the Swift bridge** (this host class):
//!   [`AppleSpeech`] wraps the Swift C ABI (`src/bridge.rs`,
//!   `swift/Sources/TeletypeApple/Bridge.swift`). `load` checks
//!   availability and the locale's asset status; the first `load` for an
//!   uninstalled locale installs the assets (blocking, network, bounded by a
//!   10 minute timeout).
//! - **Everything else** (Windows, Linux, Intel macOS, macOS builds without
//!   the Swift toolchain): the crate compiles a stub provider that reports
//!   the platform requirement from [`SpeechProvider::load`]. The crate never
//!   silently pretends to work.
//!
//! All bridge calls block the calling thread, and the first `load` may
//! download assets: run them from a worker thread, never from the UI thread.

#[cfg(teletype_apple_swift)]
mod bridge;
#[cfg(teletype_apple_swift)]
pub use bridge::{
    install_assets, locale_status, supported_locales, AppleSpeech, AssetStatus,
    INSTALL_TIMEOUT_SECS,
};

#[cfg(not(teletype_apple_swift))]
mod unsupported;
#[cfg(not(teletype_apple_swift))]
pub use unsupported::{
    install_assets, locale_status, supported_locales, AppleSpeech, AssetStatus,
    INSTALL_TIMEOUT_SECS,
};

/// Which transcriber module a catalog row asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleModule {
    /// `SpeechTranscriber`, the general-purpose module (45 locales).
    Transcriber,
    /// `DictationTranscriber`, the "older devices" module (54 locales, the
    /// 9 extra ones mostly European) the catalog offers as a second row.
    Dictation,
}

impl AppleModule {
    /// The bridge's wire name (`tta_transcribe` takes the locale; this selects
    /// which module the Swift side picks when both support it).
    pub fn as_str(self) -> &'static str {
        match self {
            AppleModule::Transcriber => "transcriber",
            AppleModule::Dictation => "dictation",
        }
    }
}

impl std::fmt::Display for AppleModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AppleModule::Transcriber => "SpeechTranscriber",
            AppleModule::Dictation => "DictationTranscriber",
        })
    }
}

/// True when this build links the Apple speech Swift bridge (macOS Apple
/// Silicon builds without `TELETYPE_APPLE_DISABLE_SWIFT=1`). Callers can use
/// it to pick the engine at runtime without repeating the cfg.
pub const fn bridge_built() -> bool {
    cfg!(teletype_apple_swift)
}
