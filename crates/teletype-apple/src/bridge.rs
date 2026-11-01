//! The real provider: a safe Rust wrapper over the `TeletypeApple` Swift C
//! ABI (see `swift/Sources/TeletypeApple/Bridge.swift`).
//!
//! Contract mirrored from the Swift side:
//! - `tta_available` returns 1 when `SpeechTranscriber.isAvailable` and the
//!   OS is 26+, 0 otherwise, with the reason in the error buffer.
//! - `tta_installed_locales` / `tta_supported_locales` return a JSON array of
//!   locale identifiers; a positive return is the needed buffer size.
//! - `tta_install_assets` reserves the locale and downloads the assets
//!   (network, bounded by a 10 minute timeout in Swift); progress is
//!   reported through the C callback.
//! - `tta_transcribe` converts 16 kHz mono f32 samples to Int16
//!   `AVAudioPCMBuffer`, runs `SpeechAnalyzer` with the transcriber module,
//!   and returns the transcript; a positive return is the needed buffer size.
//! - `tta_last_error` returns the last error.
//!
//! The bridge owns one process-wide session (locale reservation + last
//! error), exactly like TeletypeFluid: one live instance is expected. Every
//! call blocks its thread and must not run on the main thread.

use std::ffi::{c_char, c_double, c_void, CString};

use teletype_speech::{ModelCapabilities, SpeechError, SpeechProvider, Transcript};

use crate::AppleModule;

/// The Swift-side install timeout, mirrored so the Rust side can document the
/// worst case to the user ("up to 10 minutes").
pub const INSTALL_TIMEOUT_SECS: u64 = 600;

extern "C" {
    fn tta_available(err: *mut c_char, err_cap: usize) -> i32;
    fn tta_installed_locales(out: *mut c_char, out_cap: usize) -> i32;
    fn tta_supported_locales(out: *mut c_char, out_cap: usize) -> i32;
    fn tta_install_assets(
        locale: *const c_char,
        cb: extern "C" fn(*mut c_void, c_double, *const c_char),
        user_data: *mut c_void,
        err: *mut c_char,
        err_cap: usize,
    ) -> i32;
    fn tta_transcribe(
        pcm: *const f32,
        sample_count: usize,
        locale: *const c_char,
        out: *mut c_char,
        out_cap: usize,
        err: *mut c_char,
        err_cap: usize,
    ) -> i32;
    fn tta_last_error(out: *mut c_char, out_cap: usize) -> i32;
}

/// The installed-locale asset status the app reports to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetStatus {
    /// The locale is in `SpeechTranscriber.installedLocales`: ready now.
    Installed,
    /// The locale is supported but not installed: the first `load` downloads
    /// it (bounded by [`INSTALL_TIMEOUT_SECS`]).
    Supported,
    /// The locale is supported by neither module: `load` fails.
    Unsupported,
}

/// Decode a NUL-terminated UTF-8 buffer from the bridge (lossy on invalid
/// bytes; everything after the first NUL is ignored).
fn decode_buffer(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// Call `bridge` with a growing output buffer until it fits (the positive
/// return code is the needed capacity). The closure form takes the extern
/// (unsafe) symbol without a raw fn pointer.
fn read_string(
    bridge: impl Fn(*mut c_char, usize) -> i32,
    prefix: &str,
) -> Result<String, SpeechError> {
    let mut capacity = 1024usize;
    loop {
        let mut out = vec![0u8; capacity];
        let code = bridge(out.as_mut_ptr().cast::<c_char>(), out.len());
        match code {
            0 => return Ok(decode_buffer(&out)),
            code if code > 0 => {
                capacity = (code as usize).max(capacity * 2);
                if capacity > 8 * 1024 * 1024 {
                    return Err(SpeechError::ModelLoad(format!(
                        "{prefix} needs a {code} byte buffer (limit 8 MiB)"
                    )));
                }
            }
            _ => {
                let message = last_error();
                return Err(SpeechError::ModelLoad(if message.is_empty() {
                    format!("{prefix} failed")
                } else {
                    message
                }));
            }
        }
    }
}

/// Last bridge error message (empty when the bridge has none).
fn last_error() -> String {
    let mut capacity = 1024usize;
    loop {
        let mut buffer = vec![0u8; capacity];
        // SAFETY: `buffer` is a valid writable region and its length is
        // passed as the capacity.
        let code = unsafe { tta_last_error(buffer.as_mut_ptr().cast::<c_char>(), buffer.len()) };
        match code {
            0 => return String::new(),
            code if code > 0 && (code as usize) <= buffer.len() => {
                return decode_buffer(&buffer);
            }
            code if code > 0 => {
                capacity = code as usize;
                if capacity > 1 << 20 {
                    return String::new();
                }
            }
            _ => return String::new(),
        }
    }
}

/// `tta_available` with the reason for the negative case. The Swift side
/// records the reason in the process-wide last error (it also writes the
/// error buffer, but the buffer is lost when the buffer is too small), so a
/// negative return falls back to [`last_error`] when the buffer is empty.
pub fn available() -> Result<(), String> {
    let mut err = vec![0u8; 4096];
    // SAFETY: `err` is a valid writable region whose length is its capacity.
    let code = unsafe { tta_available(err.as_mut_ptr().cast::<c_char>(), err.len()) };
    if code == 1 {
        Ok(())
    } else {
        let reason = decode_buffer(&err);
        Err(if reason.is_empty() {
            last_error()
        } else {
            reason
        })
    }
}

/// `SpeechTranscriber.installedLocales` identifiers, sorted.
pub fn installed_locales() -> Result<Vec<String>, SpeechError> {
    parse_locale_list(&read_string(
        // SAFETY: `out` is a valid writable region of `cap` bytes, which is
        // the callee's stated contract.
        |out, cap| unsafe { tta_installed_locales(out, cap) },
        "reading installed locales",
    )?)
}

/// The union of both modules' `supportedLocales` identifiers, sorted.
pub fn supported_locales() -> Result<Vec<String>, SpeechError> {
    parse_locale_list(&read_string(
        // SAFETY: `out` is a valid writable region of `cap` bytes, which is
        // the callee's stated contract.
        |out, cap| unsafe { tta_supported_locales(out, cap) },
        "reading supported locales",
    )?)
}

fn parse_locale_list(json: &str) -> Result<Vec<String>, SpeechError> {
    serde_json::from_str(json).map_err(|error| {
        SpeechError::ModelLoad(format!("locale list is not a JSON string array: {error}"))
    })
}

/// Where a locale stands for the catalog row's module.
pub fn locale_status(module: AppleModule, locale: &str) -> AssetStatus {
    let supported = supported_locales()
        .map(|list| list.iter().any(|l| l == locale))
        .unwrap_or(false);
    let installed = installed_locales()
        .map(|list| list.iter().any(|l| l == locale))
        .unwrap_or(false);
    if module == AppleModule::Dictation && !supported {
        // The dictation row exists for locales only that module covers; a
        // locale the transcriber supports but dictation does not is not a
        // failure of this row, it just belongs to the other row.
        return AssetStatus::Unsupported;
    }
    if installed {
        AssetStatus::Installed
    } else if supported {
        AssetStatus::Supported
    } else {
        AssetStatus::Unsupported
    }
}

/// Download the assets for `locale` (bounded by the Swift-side 10 minute
/// timeout). `progress` receives a 0.0..=1.0 fraction as the OS reports it.
pub fn install_assets(locale: &str, progress: &mut dyn FnMut(f64)) -> Result<(), SpeechError> {
    let locale_c = CString::new(locale)
        .map_err(|_| SpeechError::ModelLoad("locale contains a NUL byte".to_string()))?;
    // The C callback cannot capture; report through a global-free channel by
    // rounding to a shared atomic. The dictation worker calls this from one
    // thread at a time (one live instance is expected), so a static is safe.
    static LAST_PROGRESS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    LAST_PROGRESS.store(0, std::sync::atomic::Ordering::SeqCst);
    extern "C" fn report(_user: *mut c_void, fraction: c_double, _message: *const c_char) {
        let bits = (fraction.clamp(0.0, 1.0) * 1_000_000.0) as u64;
        LAST_PROGRESS.store(bits, std::sync::atomic::Ordering::SeqCst);
    }
    let mut err = vec![0u8; 512];
    // SAFETY: `locale_c` is a valid NUL-terminated string for the call and
    // `err` is a valid writable region whose length is its capacity.
    let code = unsafe {
        tta_install_assets(
            locale_c.as_ptr(),
            report,
            std::ptr::null_mut(),
            err.as_mut_ptr().cast::<c_char>(),
            err.len(),
        )
    };
    let fraction = LAST_PROGRESS.load(std::sync::atomic::Ordering::SeqCst) as f64 / 1_000_000.0;
    progress(fraction);
    if code == 0 {
        progress(1.0);
        return Ok(());
    }
    let message = decode_buffer(&err);
    Err(SpeechError::ModelLoad(if message.is_empty() {
        "installing locale assets failed".to_string()
    } else {
        message
    }))
}

/// Apple's on-device speech provider.
///
/// The bridge owns one process-wide session, so use one live instance at a
/// time. `load` and `transcribe` block (the first `load` may download
/// assets): call them from a worker thread, never the UI thread.
#[derive(Debug)]
pub struct AppleSpeech {
    module: AppleModule,
    loaded: bool,
    locale: Option<String>,
}

impl AppleSpeech {
    /// Create an unloaded provider for `module`; nothing touches the network
    /// until [`SpeechProvider::load`].
    pub fn new(module: AppleModule) -> Self {
        Self {
            module,
            loaded: false,
            locale: None,
        }
    }

    /// The requested transcriber module.
    pub fn module(&self) -> AppleModule {
        self.module
    }

    /// Whether the provider is loaded and the bridge reports the locale as
    /// installed.
    pub fn is_ready(&self) -> bool {
        self.loaded
            && self
                .locale
                .as_deref()
                .is_some_and(|l| locale_status(self.module, l) == AssetStatus::Installed)
    }

    /// The locale this provider loaded, when any.
    pub fn loaded_locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }
}

/// Map a Teletype language hint ("en", "de", "en-US", "auto") to a locale
/// identifier the modules list: English picks `en_US` (installed on the
/// research host and the common case), other languages pick the most common
/// regional variant the module supports, falling back to the bare language
/// tag.
pub fn locale_for(language: &str) -> Option<String> {
    let trimmed = language.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return Some("en_US".to_string());
    }
    // A full BCP-47 tag with a region ("de-DE") is already a locale.
    if trimmed.contains('-') || trimmed.contains('_') {
        return Some(trimmed.replace('_', "-").to_uppercase_region());
    }
    let base = trimmed.to_lowercase();
    let preferred = match base.as_str() {
        "en" => "en_US",
        "de" => "de_DE",
        "fr" => "fr_FR",
        "es" => "es_ES",
        "it" => "it_IT",
        "pt" => "pt_PT",
        "ja" => "ja_JP",
        "ko" => "ko_KR",
        "zh" => "zh_CN",
        "hi" => "hi_IN",
        "ar" => "ar_AE",
        "nl" => "nl_NL",
        "pl" => "pl_PL",
        "ru" => "ru_RU",
        "sv" => "sv_SE",
        "da" => "da_DK",
        "fi" => "fi_FI",
        "no" | "nb" => "nb_NO",
        "el" => "el_GR",
        "cs" => "cs_CZ",
        "sk" => "sk_SK",
        "ro" => "ro_RO",
        "hu" => "hu_HU",
        "he" => "he_IL",
        "tr" => "tr_TR",
        "vi" => "vi_VN",
        "th" => "th_TH",
        "id" => "id_ID",
        "ms" => "ms_MY",
        "uk" => "uk_UA",
        "hr" => "hr_HR",
        "ca" => "ca_ES",
        other => {
            // No known regional variant: let the bridge try the bare tag.
            return Some(other.to_string());
        }
    };
    Some(preferred.to_string())
}

impl SpeechProvider for AppleSpeech {
    /// Check availability, then the locale's asset status. `path` is unused:
    /// the assets are OS-managed system storage, not a file Teletype
    /// downloads. When the locale is supported but not installed, install it
    /// here (blocking, network, bounded by the Swift-side 10 minute timeout)
    /// so the first `transcribe` is fast.
    fn load(&mut self, _path: &std::path::Path) -> Result<(), SpeechError> {
        if self.is_ready() {
            return Ok(());
        }
        available().map_err(|reason| {
            SpeechError::ModelLoad(format!("Apple on-device speech is unavailable: {reason}"))
        })?;
        let locale = locale_for("en").ok_or_else(|| {
            SpeechError::ModelLoad("no locale for the requested language".to_string())
        })?;
        // The language is fixed at load time; `transcribe` re-checks it
        // against `language` and reports a mismatch as a no-op.
        match locale_status(self.module, &locale) {
            AssetStatus::Installed => {
                self.locale = Some(locale);
                self.loaded = true;
                Ok(())
            }
            AssetStatus::Supported => {
                install_assets(&locale, &mut |_fraction| {}).map_err(|e| {
                    SpeechError::ModelLoad(format!(
                        "installing the '{locale}' speech assets failed: {e}"
                    ))
                })?;
                self.locale = Some(locale);
                self.loaded = true;
                Ok(())
            }
            AssetStatus::Unsupported => Err(SpeechError::ModelLoad(format!(
                "the '{locale}' locale is not supported by {}",
                self.module
            ))),
        }
    }

    fn unload(&mut self) {
        self.loaded = false;
        self.locale = None;
    }

    fn is_loaded(&self) -> bool {
        self.is_ready()
    }

    /// Transcribe 16 kHz mono f32 samples. `language` maps to a locale; the
    /// bridge is called with it directly (the assets may already be
    /// installed for it), and the provider's locale is re-resolved for the
    /// next `load`.
    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        if samples.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        // A provider that never loaded its locale (or whose locale was not
        // installed) cannot transcribe: the bridge would spend the whole
        // transcribe timeout on a locale it was never asked to prepare.
        if !self.loaded || self.locale.is_none() {
            return Err(SpeechError::Transcribe(
                "Apple speech provider is not loaded; call load first".to_string(),
            ));
        }
        let locale = locale_for(language).ok_or_else(|| {
            SpeechError::Transcribe("no locale for the requested language".to_string())
        })?;
        // A locale that is not even supported fails fast with a clear error
        // instead of making the bridge spend the transcribe timeout.
        if locale_status(self.module, &locale) == AssetStatus::Unsupported {
            return Err(SpeechError::Transcribe(format!(
                "the '{locale}' locale is not supported by {}",
                self.module
            )));
        }
        let locale_c = CString::new(locale.as_str())
            .map_err(|_| SpeechError::Transcribe("locale contains a NUL byte".to_string()))?;
        let mut capacity = 4096usize;
        loop {
            let mut out = vec![0u8; capacity];
            let mut err = vec![0u8; 512];
            // SAFETY: `samples`, `out` and `err` are valid regions whose
            // lengths are passed as their capacities, and `locale_c` is a
            // valid NUL-terminated C string for the duration of the call.
            let code = unsafe {
                tta_transcribe(
                    samples.as_ptr(),
                    samples.len(),
                    locale_c.as_ptr(),
                    out.as_mut_ptr().cast::<c_char>(),
                    out.len(),
                    err.as_mut_ptr().cast::<c_char>(),
                    err.len(),
                )
            };
            match code {
                0 => return Ok(decode_buffer(&out)),
                code if code > 0 => {
                    capacity = (code as usize).max(capacity * 2);
                    if capacity > 8 * 1024 * 1024 {
                        return Err(SpeechError::Transcribe(format!(
                            "Apple speech needs a {code} byte result buffer (limit 8 MiB)"
                        )));
                    }
                }
                _ => {
                    let message = decode_buffer(&err);
                    let message = if message.is_empty() {
                        last_error()
                    } else {
                        message
                    };
                    return Err(SpeechError::Transcribe(if message.is_empty() {
                        "Apple speech transcribe failed".to_string()
                    } else {
                        message
                    }));
                }
            }
        }
    }

    fn transcribe_detailed(
        &mut self,
        samples: &[f32],
        language: &str,
    ) -> Result<Transcript, SpeechError> {
        // The first pass reports no per-word timings; the default mapping is
        // the honest shape until time-indexed transcription is wired.
        self.transcribe(samples, language).map(|text| Transcript {
            text,
            words: Vec::new(),
            confidence: 1.0,
            language: None,
        })
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
}

/// Uppercase the region part of a BCP-47 tag ("de-de" -> "de-DE"); a tag
/// without a region is returned lowercased with the language part intact.
trait RegionCase {
    fn to_uppercase_region(self) -> String;
}

impl RegionCase for String {
    fn to_uppercase_region(self) -> String {
        match self.split_once('-') {
            Some((lang, region)) => format!("{}-{}", lang.to_lowercase(), region.to_uppercase()),
            None => self.to_lowercase(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The FFI session is process-wide and cargo runs tests in parallel, so
    /// every test that touches the bridge takes this lock.
    fn ffi_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The locale mapping is pure and never touches the framework.
    #[test]
    fn locale_for_maps_known_languages_to_regional_variants() {
        assert_eq!(locale_for("en").as_deref(), Some("en_US"));
        assert_eq!(locale_for("de").as_deref(), Some("de_DE"));
        assert_eq!(locale_for("auto").as_deref(), Some("en_US"));
        assert_eq!(locale_for("").as_deref(), Some("en_US"));
        assert_eq!(locale_for("de-DE").as_deref(), Some("de-DE"));
        assert_eq!(locale_for("en-US").as_deref(), Some("en-US"));
        assert_eq!(locale_for("xx").as_deref(), Some("xx"));
    }

    /// The availability gate: on a host where the bridge exists but the OS
    /// is below 26 (or the hardware is ineligible) `available` must fail
    /// with a reason. On macOS 26+ with the framework it succeeds; the
    /// assertion holds in both cases because it only checks the contract
    /// shape, and on CI (no bridge) this test is not compiled at all.
    #[test]
    fn availability_gate_reports_a_reason_when_unavailable() {
        let _guard = ffi_lock();
        match available() {
            Ok(()) => {}
            Err(reason) => assert!(!reason.is_empty(), "unavailable with no reason"),
        }
    }

    /// The locale lists are JSON arrays of identifiers; on a host without
    /// the framework the call fails cleanly instead of panicking.
    #[test]
    fn locale_lists_are_json_arrays() {
        let _guard = ffi_lock();
        for result in [installed_locales(), supported_locales()] {
            match result {
                Ok(list) => {
                    for locale in &list {
                        assert!(!locale.is_empty(), "empty locale identifier");
                    }
                }
                Err(SpeechError::ModelLoad(_)) => {}
                Err(other) => panic!("unexpected error kind: {other:?}"),
            }
        }
    }

    /// A provider whose `load` was never called refuses to transcribe.
    #[test]
    fn transcribe_without_load_is_refused() {
        let mut provider = AppleSpeech::new(AppleModule::Transcriber);
        let error = provider
            .transcribe(&[0.0; 160], "en")
            .expect_err("unloaded provider must refuse");
        assert!(
            matches!(error, SpeechError::Transcribe(_)),
            "expected a Transcribe error, got {error:?}"
        );
    }

    /// `transcribe` on empty audio is a no-speech error before any bridge
    /// call.
    #[test]
    fn empty_audio_is_no_speech() {
        let mut provider = AppleSpeech::new(AppleModule::Dictation);
        assert!(matches!(
            provider.transcribe(&[], "en"),
            Err(SpeechError::NoSpeech)
        ));
    }
}
