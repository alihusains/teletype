//! Speech (dictation) model catalog.
//!
//! The rows live in `teletype-catalog`'s bundled `speech.json` (Phase A);
//! this module wraps each parsed [`SpeechEntry`] in a [`SpeechModel`] that
//! carries the engine predicates and path/url helpers the app and the eval
//! harness read.

use std::path::{Path, PathBuf};

use teletype_catalog::schema::{Artifact, Purpose, SpeechEntry};
use teletype_catalog::Catalog;

/// One downloadable speech model: a parsed [`SpeechEntry`] plus the engine
/// predicates and path/url helpers the consumers read.
#[derive(Debug, Clone)]
pub struct SpeechModel {
    entry: SpeechEntry,
}

impl SpeechModel {
    pub fn from_entry(entry: SpeechEntry) -> Self {
        Self { entry }
    }

    /// The parsed entry (artifacts, languages, purposes, ...).
    pub fn entry(&self) -> &SpeechEntry {
        &self.entry
    }

    pub fn id(&self) -> &str {
        &self.entry.id
    }

    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// The model file name inside the models directory (the first artifact,
    /// or the row id for rows with no files, like the Core ML and Apple rows).
    pub fn file(&self) -> &str {
        self.entry
            .artifacts
            .first()
            .map(|a| a.file.as_str())
            .unwrap_or(&self.entry.id)
    }

    /// The engine that loads this file.
    pub fn engine(&self) -> Engine {
        Engine::from_name(&self.entry.engine).unwrap_or(Engine::Whisper)
    }

    pub fn size_mb(&self) -> u32 {
        self.entry.size_mb
    }

    /// Hex SHA-256 of the primary artifact, or `None` for multi-file repos
    /// (Core ML), the two-file llama-server pair, and OS-managed rows.
    pub fn sha256(&self) -> Option<&str> {
        self.entry
            .artifacts
            .first()
            .and_then(|a| a.sha256.as_deref())
    }

    pub fn english_only(&self) -> bool {
        self.entry.english_only
    }

    /// Static compile-time flag kept for the catalog's own tests and docs.
    ///
    /// This is NOT what the app recommends at runtime: the honest
    /// recommendation is [`recommended_speech_model_id`], which is computed
    /// per host. Do not read this field to decide what to default a user to.
    pub fn recommended(&self) -> bool {
        self.entry.recommended
    }

    /// Companion model, not a dictation engine: the Core ML CTC spotter is
    /// downloaded by FluidAudio on demand for vocabulary boosting, so the
    /// model picker must not offer it as something that transcribes.
    pub fn auxiliary(&self) -> bool {
        self.entry.auxiliary
    }

    pub fn description(&self) -> &str {
        &self.entry.description
    }

    /// Languages the model can transcribe, or `None` for every Whisper
    /// language (99).
    pub fn languages(&self) -> Option<&[String]> {
        self.entry.languages.as_deref()
    }

    pub fn speed(&self) -> f64 {
        self.entry.speed
    }

    pub fn accuracy(&self) -> f64 {
        self.entry.accuracy
    }

    pub fn min_ram_gb(&self) -> u32 {
        self.entry.min_ram_gb
    }

    pub fn base_url(&self) -> Option<&str> {
        self.entry.base_url.as_deref()
    }

    /// What the row is good at, shown as chips beside the name.
    pub fn purposes(&self) -> &[Purpose] {
        &self.entry.purposes
    }

    /// The row's artifact files, with their pinned bytes and digests.
    pub fn artifacts(&self) -> &[Artifact] {
        &self.entry.artifacts
    }

    /// Whether the binary on `platform` can load this model. Listing a model
    /// a host cannot load produces a dead download, so the catalog list, the
    /// default pick, and the persisted-selection migration all consult this.
    pub fn available_on(&self, platform: Platform) -> bool {
        self.engine().available_on(platform)
    }

    /// Offer gate with runtime facts resolved by the caller (tests, previews).
    ///
    /// Same predicate as [`Self::offered_on`] plus the engine-build check:
    /// a model is only offered when this build compiled the engine that loads
    /// it (`parakeet_cpp_available` for the parakeet.cpp rows; the other
    /// engines inherit it from their platform check).
    pub fn offered_on_with(
        &self,
        platform: Platform,
        fluid_available: bool,
        parakeet_cpp_available: bool,
    ) -> bool {
        let engine = self.engine();
        let transcribe_cpp_available =
            matches!(engine, Engine::TranscribeCpp) && crate::transcribe::engine_available();
        engine.available_on(platform)
            && !self.entry.auxiliary
            && (engine != Engine::ParakeetCoreml || fluid_available)
            && engine.built_on(platform, parakeet_cpp_available, transcribe_cpp_available)
            && !(platform.is_macos() && engine == Engine::SherpaParakeet)
    }

    pub fn offered_on(&self, platform: Platform, fluid_available: bool) -> bool {
        self.offered_on_with(
            platform,
            fluid_available,
            crate::parakeet_cpp::engine_available(),
        )
    }

    /// The path handed to `SpeechProvider::load` under `models_dir`.
    ///
    /// One ggml/GGUF file for Whisper/Parakeet/Parakeet.cpp; the extracted
    /// archive directory for sherpa; the models *base* directory for the
    /// FluidAudio bridge, which appends its own repo folder.
    pub fn provider_path(&self, models_dir: &Path) -> PathBuf {
        Catalog::bundled().speech_provider_path(&self.entry, models_dir)
    }

    /// Where this model's bytes live under `models_dir`, used for installed /
    /// downloaded status. Differs from [`Self::provider_path`] only for the
    /// Core ML bridge, which receives the base directory.
    pub fn install_path(&self, models_dir: &Path) -> PathBuf {
        Catalog::bundled().speech_install_path(&self.entry, models_dir)
    }

    /// The download URL for this model's primary artifact.
    pub fn url(&self) -> String {
        Catalog::bundled().speech_url(&self.entry)
    }

    /// Whether this model can transcribe `language`.
    ///
    /// The model's own claim: `None`, empty, or `"auto"` is `true` for every
    /// row ("auto" is not a request for a language, it is a request for the
    /// engine's own default); a non-English row (Whisper, Parakeet TDT v3)
    /// honours any language; an English-only row honours only `"en"` (after
    /// normalization, so `"en-US"` counts).
    ///
    /// This is the single language-capability *predicate*: the take path and
    /// the Settings engine picker both route through it, so a future engine
    /// gets the check by construction. It deliberately cannot see per-row
    /// limitations narrower than `english_only` (a Parakeet build that lists
    /// only 25 of Whisper's 99 languages): [`entry_supports`] layers the row's
    /// `languages` table on top for that case, and [`pick_engine`]'s detour
    /// consults the candidate row directly.
    pub fn honours_language(&self, language: Option<&str>) -> bool {
        match normalize_language(language) {
            None => true,
            Some("en") => true,
            Some(_) => !self.english_only(),
        }
    }

    /// Human-readable language label.
    pub fn language_label(&self) -> String {
        if self.english_only() {
            "English only".to_string()
        } else if let Some(langs) = self.languages() {
            format!("{} languages", langs.len())
        } else {
            "99 languages".to_string()
        }
    }
}

/// Human-readable name for a normalized two-letter language code, used in
/// user-facing notices ("cannot transcribe Polish"). Unknown codes fall
/// back to the code itself.
///
/// Covers the languages the settings picker offers; a small table is the
/// right size, and the fallback keeps a new entry from ever producing a
/// blank message.
pub fn language_name(code: &str) -> &str {
    match code {
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "ca" => "Catalan",
        "cs" => "Czech",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "es" => "Spanish",
        "et" => "Estonian",
        "fa" => "Persian",
        "fi" => "Finnish",
        "fr" => "French",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "it" => "Italian",
        "ja" => "Japanese",
        "ko" => "Korean",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "mt" => "Maltese",
        "nl" => "Dutch",
        "no" => "Norwegian",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sv" => "Swedish",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "ur" => "Urdu",
        "zh" => "Chinese",
        other => other,
    }
}

/// The speech rows of the bundled catalog, wrapped for the consumers.
pub fn catalog() -> Vec<SpeechModel> {
    Catalog::bundled()
        .speech()
        .iter()
        .cloned()
        .map(SpeechModel::from_entry)
        .collect()
}

/// Finds a catalog entry by id.
pub fn find(id: &str) -> Option<SpeechModel> {
    catalog().into_iter().find(|m| m.id() == id)
}

/// Normalizes a Settings language for routing: `"en-US"`/`"en_US"` become
/// `"en"`, and `"auto"`/empty/whitespace become `None` ("the engine decides"),
/// which selects the platform default instead of routing to Whisper.
///
/// `Settings::language` ships as `"auto"`, so every caller that threads the
/// stored value through here treats a fresh install as "no language set".
fn normalize_language(language: Option<&str>) -> Option<&str> {
    let base = language
        .map(|l| l.trim())
        .map(|l| l.split(['-', '_']).next().unwrap_or(l))?;
    if base.is_empty() || base.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(base)
    }
}

/// Whether the catalog entry `id` can transcribe `language` (the raw
/// Settings value - BCP-47 tags and whitespace included - are normalized
/// here; `None` matches every model).
///
/// The single list predicate for the Settings engine picker: the row's
/// own `languages` table (when present - `None` = the engine's full
/// table, i.e. Whisper's 99) must agree with
/// [`SpeechModel::honours_language`], so the two cannot drift apart.
fn entry_supports(entry: &SpeechModel, language: Option<&str>) -> bool {
    match normalize_language(language) {
        None => true,
        Some(lang) => {
            entry
                .languages()
                .is_none_or(|langs| langs.iter().any(|l| l == lang))
                && entry.honours_language(Some(lang))
        }
    }
}

/// Flip to `true` only if the P3.8 bake-off shows Parakeet v2 beating Ultra
/// on the English human corpus; both English default branches honor it.
pub const V2_ENGLISH_PREFERRED: bool = false;

/// The host platform facts for [`pick_engine`].
pub use teletype_catalog::engine::Platform;

/// The speech engine enum, re-exported for consumers.
pub use teletype_catalog::Engine;
/// Detects the host platform for [`pick_engine`].
///
/// The macOS major version comes from `sw_vers` (no extra dependency). A
/// failed probe reports major 0, which selects the ggml build - the safe
/// fallback on every Mac.
pub fn host_platform() -> Platform {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            let major = std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| parse_macos_major(&s))
                .unwrap_or(0);
            Platform::MacosAppleSilicon { major }
        } else {
            Platform::MacosIntel
        }
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Other
    }
}

fn parse_macos_major(version: &str) -> Option<u16> {
    version.trim().split('.').next()?.parse().ok()
}

/// The id of a [`SpeechModel`] in the catalog.
pub type ModelId = String;

/// Picks the default speech-model id for the current host and language.
///
/// Selection order (plan §3.2):
/// 1. an explicit user pin wins when this host can load it (unknown and
///    unloadable ids are ignored);
/// 2. a language outside Parakeet's 25 routes to Whisper (99 languages),
///    where Whisper ships;
/// 3. macOS 14+ Apple Silicon uses the Core ML Ultra build (or v2 for
///    English once [`V2_ENGLISH_PREFERRED`] is flipped by the P3.8 bake-off);
/// 4. Windows uses the sherpa-onnx int8 build;
/// 5. Intel Macs and older macOS keep the ggml Parakeet v3; every other
///    host uses sherpa-onnx, the only cross-platform engine.
///
/// The returned model is always loadable on `platform`; that invariant is
/// what keeps a Windows or Linux install from selecting a macOS-only engine.
///
/// `language` is the Settings language ("en", "de", …, "auto", or a BCP-47
/// tag such as "en-US"); `None`/`"auto"` mean "engine decides", which selects
/// the platform default rather than routing to Whisper.
pub fn pick_engine(pinned: Option<&str>, platform: Platform, language: Option<&str>) -> String {
    // 1. Pin first, but never a model this host cannot load: a selection
    //    carried over from another platform must not brick dictation.
    if let Some(id) = pinned {
        if let Some(entry) = find(id) {
            if entry.available_on(platform) {
                return entry.id().to_string();
            }
        }
    }

    // Normalize "en-US" to "en"; treat "auto"/empty as "no language set".
    let lang = normalize_language(language);

    // 2. Whisper covers every language the best platform candidate cannot
    //    transcribe, where it ships. The candidate is the same row the
    //    platform match below would pick, so the detour and the platform
    //    answer cannot disagree about the language: its `languages` table
    //    (25 for Parakeet, `None` = the engine's full 99 for Whisper) is the
    //    row's own claim, layered on `honours_language` (the English-only
    //    half) by `entry_supports`. Where Whisper does not ship (Windows),
    //    the platform default below is the closest loadable engine.
    let platform_default: &str = match platform {
        Platform::MacosAppleSilicon { major } if major >= 14 => {
            if V2_ENGLISH_PREFERRED {
                "parakeet-v2-coreml"
            } else {
                "parakeet-ultra-coreml"
            }
        }
        Platform::Windows => {
            if V2_ENGLISH_PREFERRED {
                "sherpa-parakeet-v2-int8"
            } else {
                "sherpa-parakeet-v3-int8"
            }
        }
        Platform::MacosAppleSilicon { .. } | Platform::MacosIntel => "parakeet-tdt-v3",
        Platform::Other => "sherpa-parakeet-v3-int8",
    };
    if let Some(l) = lang {
        let detours = find("large-v3-turbo-q5").is_some_and(|m| m.available_on(platform))
            && !find(platform_default).is_some_and(|m| entry_supports(&m, Some(l)));
        if detours {
            return "large-v3-turbo-q5".to_string();
        }
    }

    match platform {
        // 3. macOS 14+ on Apple Silicon runs Core ML on the Neural Engine.
        //    The candidate is the same row step 2 consulted, so the detour
        //    and this answer cannot disagree about the language.
        Platform::MacosAppleSilicon { major } if major >= 14 => {
            if V2_ENGLISH_PREFERRED && lang == Some("en") {
                "parakeet-v2-coreml"
            } else {
                platform_default
            }
        }
        // 4. Windows runs sherpa-onnx on CPU.
        Platform::Windows => {
            if V2_ENGLISH_PREFERRED && lang == Some("en") {
                "sherpa-parakeet-v2-int8"
            } else {
                platform_default
            }
        }
        // 5. Intel Macs and older macOS keep the ggml build; other hosts
        //    (Linux, …) can only load sherpa-onnx.
        Platform::MacosAppleSilicon { .. } | Platform::MacosIntel => platform_default,
        Platform::Other => platform_default,
    }
    .to_string()
}

/// Dictation models tried by [`recommended_speech_model_id`] when
/// [`pick_engine`]'s answer cannot be offered on this build, best first.
///
/// Every entry is a real dictation model, so the fallback can never name a
/// row the list hides (the CTC spotter is deliberately absent).
const RECOMMENDED_FALLBACKS: &[&str] = &[
    "parakeet-tdt-v3",
    "parakeet-tdt-v3-q4",
    "large-v3-turbo-q5",
    "sherpa-parakeet-v3-int8",
];

/// The id of the model this build should recommend for `platform` and
/// `language`, guaranteed to be a model [`SpeechModel::offered_on`] accepts.
///
/// [`pick_engine`] alone is not enough: it routes on platform, OS version and
/// language but cannot see the binary it is running inside, so on an Apple
/// Silicon macOS 14+ host whose build did not link the FluidAudio bridge (CI
/// builds, a build configured without the Swift toolchain) it still answers
/// `parakeet-ultra-coreml`. The model list filters those rows out, which used
/// to leave onboarding with a recommendation that matched no row: no card
/// rendered and no way to continue. Intel Macs and a failed macOS version
/// probe are already handled inside [`pick_engine`] (they answer the ggml
/// build), so this function leaves them alone.
///
/// `language` is the Settings value ("en", "en-US", "auto", `""`, `None`);
/// "auto" and empty behave like `None`, i.e. "the engine decides".
///
/// The result is always a catalog entry, never an empty id, so a host with no
/// offerable model at all (impossible today: sherpa-onnx is unconditional)
/// still reports [`pick_engine`]'s platform default instead of nothing.
pub fn recommended_speech_model_id(
    platform: Platform,
    language: Option<&str>,
    fluid_available: bool,
) -> String {
    let offerable = |id: &str| {
        find(id)
            .is_some_and(|m| m.offered_on(platform, fluid_available))
    };

    let pick = pick_engine(None, platform, language);
    if offerable(&pick) {
        return pick;
    }

    let lang = normalize_language(language);
    for id in RECOMMENDED_FALLBACKS {
        if find(id).is_some_and(|m| {
            m.offered_on(platform, fluid_available) && entry_supports(&m, lang)
        }) {
            return id.to_string();
        }
    }

    // Last resort: the first offered dictation model that covers the language,
    // then any offered model. Keeps the contract "the recommendation is always
    // in the list" even if the catalog grows an unusual engine.
    let all = catalog();
    if let Some(m) = all
        .iter()
        .find(|m| m.offered_on(platform, fluid_available) && entry_supports(m, lang))
    {
        return m.id().to_string();
    }
    if let Some(m) = all.iter().find(|m| m.offered_on(platform, fluid_available)) {
        return m.id().to_string();
    }
    pick
}

/// How many model rows a first-run surface may show.
pub const SHORTLIST_MAX: usize = 3;

/// The rows worth showing someone who has never dictated here, best first.
///
/// The first element is always [`recommended_speech_model_id`], so a caller
/// can render the recommended card and the shortlist without them
/// disagreeing. It is exempt from the language check because it is exempt
/// there too: an uncovered language on Windows has no row that transcribes
/// it, and showing nothing would strand the user in onboarding.
pub fn shortlist_ids(
    platform: Platform,
    language: Option<&str>,
    fluid_available: bool,
) -> Vec<String> {
    let mut ids: Vec<String> = Vec::with_capacity(SHORTLIST_MAX);
    let recommended = recommended_speech_model_id(platform, language, fluid_available);
    ids.push(recommended.clone());

    let mut push = |id: &str| {
        if id == recommended.as_str() || ids.iter().any(|i| i == id) {
            return;
        }
        if let Some(entry) = find(id) {
            if entry.offered_on(platform, fluid_available) && entry_supports(&entry, language) {
                ids.push(id.to_string());
            }
        }
    };

    match platform {
        Platform::MacosAppleSilicon { major } if major >= 14 => {
            push("parakeet-tdt-v3");
            push("large-v3-turbo-q5");
        }
        Platform::MacosAppleSilicon { .. } | Platform::MacosIntel => {
            // The recommendation is already the ggml Parakeet build here, so the
            // only other row a first-run Mac user needs is the language fallback.
            push("large-v3-turbo-q5");
        }
        Platform::Windows | Platform::Other => {
            push("sherpa-parakeet-v3-int8");
            push("sherpa-parakeet-v2-int8");
        }
    }

    ids.truncate(SHORTLIST_MAX);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chip vocabulary the UI may show. A label outside this list is a
    /// catalog bug, not a wording choice, so the test pins the set.
    const PURPOSE_LABELS: &[&str] = &[
        "Everyday dictation",
        "Fastest",
        "Most accurate",
        "Multilingual",
        "English only",
        "Live streaming",
        "Context-aware",
        "Low memory",
        "No download",
        "Long recordings",
        "Indian languages",
        "Experimental",
    ];

    /// The 19 transcription-ready locales of Nemotron 3.5 ASR streaming 0.6B
    /// (the 15 base languages the speech.json row lists).
    const NEMOTRON35_READY_LANGUAGES: &[&str] = &[
        "ar", "de", "en", "es", "fr", "hi", "it", "ja", "ko", "nl", "pt", "ru", "tr", "uk", "vi",
    ];

    /// The 25 Parakeet TDT v3 languages the speech.json rows list.
    const PARAKEET_V3_LANGUAGES: &[&str] = &[
        "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv",
        "mt", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
    ];

    /// The 30 Qwen3-ASR languages the speech.json row lists.
    const QWEN3_ASR_LANGUAGES: &[&str] = &[
        "ar", "bg", "cs", "da", "de", "el", "en", "es", "fi", "fr", "hr", "hi", "hu", "id", "it",
        "ja", "ko", "ms", "nl", "no", "pl", "pt", "ro", "ru", "sk", "sv", "tr", "uk", "vi", "zh",
    ];

    /// The streaming-ASR pins the offline shape gate checks (R6: the data
    /// lives in speech.json artifacts; this table is the test's reference).
    const PINNED_SPEECH_ARTIFACTS: &[(&str, &str, &str, &str)] = &[
        (
            "nemotron-3.5-asr-streaming-q4",
            "nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf",
            "41c99fa5fb6f3d35f68e79adc3e755eca2232a8d921178bd647b71194792b8fd",
            "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/resolve/53ce288f69326b415aeef030cfcb02a08a8096dd",
        ),
        (
            "moonshine-streaming-medium-q8",
            "moonshine-streaming-medium-Q8_0.gguf",
            "f7c9564249b508f6012927ec4f9e536087da53a7047f858ca9975bea5f75299e",
            "https://huggingface.co/handy-computer/moonshine-streaming-medium-gguf/resolve/56302cde2ae162d1357a833d91775bcd06e91e1b",
        ),
        (
            "granite-speech-470m-q4",
            "granite-speech-5.0-470m-turboctc-Q4_K_M.gguf",
            "3f30525381d72246b7f99e49b6cb66b249cf1b990c6c3d0e031a7765735f513e",
            "https://huggingface.co/handy-computer/granite-speech-5.0-470m-turboctc-gguf/resolve/0cf5be9f4bcd479989e6a24d142094d34d552daa",
        ),
    ];

    /// The two GGUFs a Qwen3-ASR install needs, main model first (the
    /// speech.json reference the offline gate checks against).
    const QWEN3_ASR_ARTIFACTS: &[(&str, &str)] = &[
        ("Qwen3-ASR-1.7B-Q8_0.gguf", "58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57"),
        (
            "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf",
            "46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d",
        ),
    ];

    /// The download base URLs the speech.json rows carry (engine-default
    /// engines carry none).
    const WHISPER_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
    const PARAKEET_BASE_URL: &str = "https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main";
    const SHERPA_MODELS_BASE_URL: &str =
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

    /// Every host fact the recommendation and the model list must agree on.
    const PLATFORMS: &[Platform] = &[
        Platform::MacosAppleSilicon { major: 26 },
        Platform::MacosAppleSilicon { major: 15 },
        Platform::MacosAppleSilicon { major: 14 },
        Platform::MacosAppleSilicon { major: 13 },
        Platform::MacosIntel,
        Platform::Windows,
        Platform::Other,
    ];
    const LANGUAGES: &[Option<&str>] = &[
        None,
        Some(""),
        Some("auto"),
        Some("en"),
        Some("en-US"),
        Some("de"),
        Some("ar"),
        Some("ja-JP"),
        Some("und"),
    ];

    /// The speech list is the parsed catalog, row for row (R6 smoke test).
    #[test]
    fn speech_model_list_is_backed_by_the_parsed_catalog() {
        let parsed = Catalog::bundled();
        assert_eq!(parsed.speech().len(), catalog().len());
    }

    #[test]
    fn apple_speech_rows_are_offered_only_on_macos_26_and_carry_no_download() {
        let mac26 = Platform::MacosAppleSilicon { major: 26 };
        let mac27 = Platform::MacosAppleSilicon { major: 27 };
        let mac25 = Platform::MacosAppleSilicon { major: 25 };
        for id in ["apple-speech-transcriber", "apple-dictation-transcriber"] {
            let row = find(id).unwrap_or_else(|| panic!("{id} missing from catalog"));
            assert_eq!(row.engine(), Engine::AppleSpeech, "{id}");
            // No download: zero size, no pinned bytes, and no URL to fetch.
            assert_eq!(row.size_mb(), 0, "{id} must be zero-download");
            assert!(row.sha256().is_none(), "{id} must not pin bytes");
            assert!(!row.recommended(), "{id} stays opt-in");
            assert!(!row.auxiliary(), "{id} is a dictation model");
            assert_eq!(row.speed(), 0.0, "{id} has no measured speed");
            assert_eq!(row.accuracy(), 0.0, "{id} has no measured accuracy");
            assert!(
                row.description().contains("Experimental"),
                "{id} must be marked experimental"
            );
            // Offered on macOS 26+ Apple Silicon, nowhere else.
            assert!(row.available_on(mac26), "{id} on macOS 26");
            assert!(row.available_on(mac27), "{id} on macOS 27");
            assert!(!row.available_on(mac25), "{id} not on macOS 25");
            assert!(
                !row.available_on(Platform::MacosAppleSilicon { major: 14 }),
                "{id} not on macOS 14"
            );
            assert!(!row.available_on(Platform::MacosIntel), "{id} not on Intel");
            assert!(!row.available_on(Platform::Windows), "{id} not on Windows");
            assert!(!row.available_on(Platform::Other), "{id} not on other hosts");
            // The offer gate (the list predicate) agrees with available_on.
            assert!(row.offered_on(mac26, true), "{id} offered on macOS 26");
            assert!(!row.offered_on(mac25, true), "{id} hidden on macOS 25");
        }
    }

    /// The three transcribe.cpp rows: pinned bytes, the right language
    /// tables, a purpose chip each (including `Experimental`), and the
    /// macOS-only offer gate (the dependency is wired macOS-only in this
    /// tree; the Windows release build re-enables it).
    #[test]
    fn transcribe_cpp_rows_are_well_formed() {
        let mac = Platform::MacosAppleSilicon { major: 14 };
        let rows = [
            (
                "nemotron-3.5-asr-streaming-q4",
                "nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf",
                "41c99fa5fb6f3d35f68e79adc3e755eca2232a8d921178bd647b71194792b8fd",
                "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/resolve/53ce288f69326b415aeef030cfcb02a08a8096dd",
            ),
            (
                "moonshine-streaming-medium-q8",
                "moonshine-streaming-medium-Q8_0.gguf",
                "f7c9564249b508f6012927ec4f9e536087da53a7047f858ca9975bea5f75299e",
                "https://huggingface.co/handy-computer/moonshine-streaming-medium-gguf/resolve/56302cde2ae162d1357a833d91775bcd06e91e1b",
            ),
            (
                "granite-speech-470m-q4",
                "granite-speech-5.0-470m-turboctc-Q4_K_M.gguf",
                "3f30525381d72246b7f99e49b6cb66b249cf1b990c6c3d0e031a7765735f513e",
                "https://huggingface.co/handy-computer/granite-speech-5.0-470m-turboctc-gguf/resolve/0cf5be9f4bcd479989e6a24d142094d34d552daa",
            ),
        ];
        for (id, file, sha, base) in rows {
            let m = find(id).unwrap_or_else(|| panic!("{id} missing from catalog"));
            assert_eq!(m.engine(), Engine::TranscribeCpp, "{id}");
            assert_eq!(m.file(), file, "{id}");
            assert_eq!(m.sha256(), Some(sha), "{id} must pin the HF lfs oid");
            assert!(m.base_url().is_some_and(|b| b == base), "{id} base_url");
            assert_eq!(m.url(), format!("{base}/{file}"), "{id} url");
            assert!(!m.recommended(), "{id} is never the default");
            assert!(!m.auxiliary(), "{id} is a dictation model");
            assert!(
                m.purposes().iter().any(|p| p.label == "Experimental"),
                "{id} must carry the Experimental chip"
            );
            assert!(m.size_mb() > 0 && m.size_mb() < 2048, "{id} under the ceiling");
            // Single-file layout, like the ggml engines.
            let dir = std::path::Path::new("/models");
            assert_eq!(m.provider_path(dir), dir.join(file), "{id}");
            assert_eq!(m.install_path(dir), dir.join(file), "{id}");
            // Offered on macOS (where the dependency ships), nowhere else.
            assert!(m.available_on(mac), "{id} on macOS");
            assert!(
                !m.available_on(Platform::Windows),
                "{id} not on Windows (this build)"
            );
            assert!(!m.available_on(Platform::Other), "{id} not on other hosts");
        }
        // The language tables: 15 Nemotron locales (the 19 ready locales
        // collapsed to their 15 base languages), English-only Moonshine,
        // 7 Granite languages.
        let nemotron = find("nemotron-3.5-asr-streaming-q4").unwrap();
        let nemotron_langs: Vec<&str> = nemotron
            .languages()
            .map(|l| l.iter().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(
            nemotron_langs,
            NEMOTRON35_READY_LANGUAGES,
            "nemotron carries the ready-locale table"
        );
        assert_eq!(NEMOTRON35_READY_LANGUAGES.len(), 15);
        assert!(!nemotron.languages().unwrap().iter().any(|l| l == "pl"));
        assert!(
            nemotron.honours_language(Some("pl")),
            "nemotron is not English-only"
        );
        let moonshine = find("moonshine-streaming-medium-q8").unwrap();
        assert!(moonshine.english_only(), "moonshine is English-only");
        assert!(!moonshine.honours_language(Some("de")));
        let granite = find("granite-speech-470m-q4").unwrap();
        let granite_langs: &[&str] = &["en", "fr", "de", "es", "pt", "it", "ja"];
        let granite_actual: Vec<&str> = granite
            .languages()
            .map(|l| l.iter().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(
            granite_actual, granite_langs,
            "granite carries its 7-language table"
        );
        // Granite is not English-only: it honours French, German, etc.
        assert!(granite.honours_language(Some("fr")));
        assert!(!granite.english_only());
    }

    #[test]
    fn transcribe_cpp_rows_are_not_offered_outside_their_platform() {
        for id in [
            "nemotron-3.5-asr-streaming-q4",
            "moonshine-streaming-medium-q8",
            "granite-speech-470m-q4",
        ] {
            let m = find(id).unwrap();
            assert!(!m.offered_on(Platform::Windows, true), "{id} on Windows");
            assert!(!m.offered_on(Platform::Other, true), "{id} on other");
            // On macOS the offer gate adds the build check: a build without
            // the compiled engine offers no row (mirrors parakeet.cpp).
            let offered = m.offered_on_with(Platform::MacosAppleSilicon { major: 14 }, true, true);
            assert_eq!(
                offered,
                crate::transcribe::engine_available(),
                "{id} offer on macOS follows the build fact"
            );
        }
    }

    #[test]
    fn every_single_file_model_pins_its_bytes() {
        for m in catalog() {
            match m.engine() {
                // The Core ML bridge fetches a multi-file checkpoint repo,
                // and Apple speech is OS-managed system storage; no single
                // artifact exists whose hash could be pinned.
                Engine::ParakeetCoreml | Engine::AppleSpeech => assert!(
                    m.sha256().is_none(),
                    "{} is a multi-file repo and must not carry a single-file hash",
                    m.id()
                ),
                _ => {
                    let sha = m
                        .sha256()
                        .unwrap_or_else(|| panic!("{} has no SHA-256 pin", m.id()));
                    assert_eq!(sha.len(), 64, "{}: {sha}", m.id());
                    assert!(
                        sha.chars()
                            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                        "{} is not lowercase hex: {sha}",
                        m.id()
                    );
                }
            }
        }
    }

    /// Offline shape gate for the pinned artifact table: every entry names a
    /// row that exists, carries a 40-hex commit ref in its URL, a 64-hex
    /// lowercase digest, and a positive byte count, and the three streaming
    /// ASR rows are all present. The ignored live test (teletype-desktop)
    /// checks these pins against Hugging Face; this one runs in the normal
    /// suite so a malformed pin or a dropped row breaks the build.
    #[test]
    fn pinned_speech_artifact_table_is_well_formed() {
        for (model_id, file, sha, base) in PINNED_SPEECH_ARTIFACTS {
            let m = find(model_id).unwrap_or_else(|| panic!("{model_id} not in catalog"));
            let url = m.url();
            assert!(
                url.starts_with(*base),
                "{model_id} url should start with its base: {url}"
            );
            // The ref between /resolve/ and the trailing /<file> must be a
            // 40-hex commit, not a branch name.
            let Some(after_resolve) = url.split("/resolve/").nth(1) else {
                panic!("{model_id} url has no /resolve/ ref: {url}");
            };
            let ref_part = after_resolve.split('/').next().unwrap_or(after_resolve);
            assert_eq!(
                ref_part.len(),
                40,
                "{model_id} ref is not a 40-hex commit: {ref_part}"
            );
            assert!(
                ref_part
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "{model_id} ref is not lowercase hex: {ref_part}"
            );
            // The digest must match the row's own pin and be 64-hex lowercase.
            assert_eq!(
                m.sha256(),
                Some(*sha),
                "{model_id} table digest disagrees with the row"
            );
            assert_eq!(sha.len(), 64, "{model_id} digest length");
            assert!(
                sha.chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "{model_id} digest is not lowercase hex"
            );
            let art = m
                .artifacts()
                .iter()
                .find(|a| a.file == *file)
                .unwrap_or_else(|| panic!("{model_id} has no artifact {file}"));
            assert!(art.bytes > 0, "{model_id} byte count is zero");
            assert_eq!(
                m.file(),
                *file,
                "{model_id} table file disagrees with the row"
            );
        }
        // All three streaming ASR rows must be pinned.
        let ids: Vec<&str> = PINNED_SPEECH_ARTIFACTS.iter().map(|a| a.0).collect();
        for id in [
            "nemotron-3.5-asr-streaming-q4",
            "moonshine-streaming-medium-q8",
            "granite-speech-470m-q4",
        ] {
            assert!(ids.contains(&id), "{id} is missing from the pinned table");
        }
    }

    #[test]
    fn catalog_ids_are_unique() {
        let all = catalog();
        let mut ids: Vec<&str> = all.iter().map(|m| m.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
    }

    #[test]
    fn exactly_one_recommended() {
        // The static flag is the catalog's compile-time reference default only.
        // The runtime recommendation is `recommended_speech_model_id`, which is
        // per host; `recommended_speech_model_id_is_always_an_offered_row`
        // guards that one instead.
        assert_eq!(catalog().iter().filter(|m| m.recommended()).count(), 1);
    }

    #[test]
    fn find_returns_entries() {
        assert!(find("large-v3-turbo-q5").is_some());
        assert!(find("no-such-model").is_none());
        // The publisher never shipped a v2 ggml build (ggml-org/parakeet-GGUF
        // holds v3 only), so the old entry was a guaranteed 404 download.
        assert!(find("parakeet-tdt-v2").is_none());
    }

    #[test]
    fn urls_point_at_the_right_repos() {
        for m in catalog() {
            match m.engine() {
                Engine::Whisper => assert!(m.url().starts_with(WHISPER_BASE_URL)),
                Engine::Parakeet => assert!(m.url().starts_with(PARAKEET_BASE_URL)),
                Engine::ParakeetCoreml => {
                    // Core ML rows have no artifacts in speech.json (the
                    // FluidAudio bridge fetches the repo on demand), so the
                    // URL is empty. The bridge knows the repo slug itself.
                    assert!(m.url().is_empty(), "{} Core ML URL should be empty", m.id());
                }
                Engine::SherpaParakeet => assert!(m.url().starts_with(SHERPA_MODELS_BASE_URL)),
                Engine::ParakeetCpp => {
                    if m.base_url().is_none() {
                        assert!(m.url().starts_with(PARAKEET_BASE_URL));
                    }
                }
                Engine::Qwen3Asr => {
                    let base = m
                        .base_url()
                        .unwrap_or("https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main");
                    assert!(m.url().starts_with(base), "{}", m.id());
                }
                // OS-managed: no URL exists, so the row is skipped here.
                Engine::AppleSpeech => {}
                // Each row carries a `base_url` (one handy-computer repo per
                // model), so the generic branch built the URL; assert it is
                // non-empty and points at the repo the row names.
                Engine::TranscribeCpp => {
                    assert!(m.base_url().is_some(), "{} needs a base_url", m.id());
                    assert!(!m.url().is_empty(), "{} has no URL", m.id());
                    assert!(
                        m.url().starts_with(m.base_url().unwrap()),
                        "{}",
                        m.id()
                    );
                }
                Engine::Onnx => unreachable!("Onnx decider is not a dictation engine"),
            }
            // The Apple and Core ML rows are the only ones without a URL:
            // the OS owns the assets (Apple) or the bridge fetches the repo
            // on demand (Core ML), so there is nothing to point at.
            if m.engine() != Engine::AppleSpeech && m.engine() != Engine::ParakeetCoreml {
                assert!(m.url().ends_with(m.file()));
            }
        }
    }

    #[test]
    fn fluid_speech_mini_entry() {
        let m = find("fluid-speech-mini").expect("fluid-speech-mini entry");
        assert_eq!(m.engine(), Engine::ParakeetCpp);
        assert_eq!(
            m.url(),
            "https://huggingface.co/FluidVoice-Labs/fluid-speech-mini-gguf/resolve/main/fluid-speech-mini-q2k-q4k.gguf"
        );
        assert_eq!(
            m.sha256(),
            Some("f0796bf40274cc49d3db58563c23cd024b52ca7e2ae4749ccf27af85841726b5")
        );
        assert!(m.english_only());
        assert!(!m.recommended());
    }

    #[test]
    fn parakeet_is_recommended() {
        // Static catalog flag only: this is the cross-platform reference build.
        // Runtime recommendation lives in `recommended_speech_model_id`.
        assert!(find("parakeet-tdt-v3").unwrap().recommended());
        assert_eq!(find("parakeet-tdt-v3").unwrap().engine(), Engine::Parakeet);
    }

    #[test]
    fn language_labels() {
        assert_eq!(
            find("parakeet-tdt-v3").unwrap().language_label(),
            "25 languages"
        );
        assert_eq!(find("small-en").unwrap().language_label(), "English only");
        assert_eq!(find("base").unwrap().language_label(), "99 languages");
    }

    #[test]
    fn coreml_and_sherpa_entries_are_well_formed() {
        // The `recommended` assertions here check the static catalog flag
        // (exactly one entry carries it), not the runtime pick: Ultra *is* the
        // runtime recommendation on an Apple Silicon macOS 14+ build that links
        // the bridge, as `recommended_speech_model_id` proves.
        let ultra = find("parakeet-ultra-coreml").unwrap();
        assert_eq!(ultra.engine(), Engine::ParakeetCoreml);
        let ultra_langs: Vec<&str> = ultra
            .languages()
            .map(|l| l.iter().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(ultra_langs, PARAKEET_V3_LANGUAGES);
        assert!(!ultra.recommended());
        assert_eq!(ultra.language_label(), "25 languages");

        let v2 = find("parakeet-v2-coreml").unwrap();
        assert_eq!(v2.engine(), Engine::ParakeetCoreml);
        assert!(v2.english_only());
        assert_eq!(
            find("parakeet-v3-coreml").unwrap().engine(),
            Engine::ParakeetCoreml
        );
        let ctc = find("parakeet-ctc-110m-coreml").unwrap();
        assert_eq!(ctc.engine(), Engine::ParakeetCoreml);
        assert!(!ctc.recommended());

        let sherpa = find("sherpa-parakeet-v3-int8").unwrap();
        assert_eq!(sherpa.engine(), Engine::SherpaParakeet);
        let sherpa_langs: Vec<&str> = sherpa
            .languages()
            .map(|l| l.iter().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(sherpa_langs, PARAKEET_V3_LANGUAGES);
        assert!(find("sherpa-parakeet-v2-int8").unwrap().english_only());
    }

    #[test]
    fn pick_engine_defaults_by_platform() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(pick_engine(None, mac14, None), "parakeet-ultra-coreml");
        // German on an Apple Silicon macOS 15 Mac stays on the Neural Engine
        // build: the Core ML Ultra row lists the 25 Parakeet v3 languages
        // (German included), so the Whisper detour does not fire. This is the
        // deliberate behaviour of routing the detour through the candidate
        // row's table + honours_language instead of the old free-standing
        // 25-language table; a Core ML row that lists fewer languages than
        // the engine claims (e.g. no `ar`) still routes to Whisper, per
        // `uncovered_languages_route_to_whisper_only_where_whisper_ships`.
        assert_eq!(
            pick_engine(None, Platform::MacosAppleSilicon { major: 15 }, Some("de")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            pick_engine(None, Platform::MacosAppleSilicon { major: 14 }, Some("de")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            pick_engine(None, Platform::MacosAppleSilicon { major: 13 }, None),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            pick_engine(None, Platform::MacosIntel, Some("en")),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            pick_engine(None, Platform::Windows, None),
            "sherpa-parakeet-v3-int8"
        );
        // Linux and other hosts can only load sherpa-onnx, not ggml.
        assert_eq!(
            pick_engine(None, Platform::Other, Some("fr")),
            "sherpa-parakeet-v3-int8"
        );
    }

    /// The core bug this function exists to prevent: the recommendation named
    /// a Core ML row that the list filter had already stripped, so onboarding
    /// rendered no model card and Continue stayed disabled. The recommendation
    /// must always be a row the list emits.
    #[test]
    fn recommended_speech_model_id_is_always_an_offered_row() {
        for platform in PLATFORMS {
            for language in LANGUAGES {
                for fluid in [true, false] {
                    let id = recommended_speech_model_id(*platform, *language, fluid);
                    let entry = find(&id).unwrap_or_else(|| {
                        panic!("{id} from {platform:?}/{language:?} is not in the catalog")
                    });
                    assert!(
                        entry.offered_on(*platform, fluid),
                        "recommended {id} on {platform:?} for {language:?} with bridge={fluid}, \
                         but the model list filters that row out"
                    );
                    // The same predicate the list uses, spelled out here so a
                    // future change to `offered_on` cannot silently drift.
                    assert!(entry.available_on(*platform), "{id} cannot load");
                    assert!(!entry.auxiliary(), "{id} is not a dictation model");
                    if entry.engine() == Engine::ParakeetCoreml {
                        assert!(fluid, "{id} needs the bridge that is absent");
                    }
                }
            }
        }
    }

    /// Exactly one row carries the recommendation on every host: a list with
    /// none is the onboarding deadlock, a list with two is a coin flip.
    #[test]
    fn recommended_speech_model_id_is_unique_per_host() {
        for platform in PLATFORMS {
            for language in LANGUAGES {
                for fluid in [true, false] {
                    let id = recommended_speech_model_id(*platform, *language, fluid);
                    let matches = catalog()
                        .iter()
                        .filter(|m| m.offered_on(*platform, fluid) && m.id() == id)
                        .count();
                    assert_eq!(
                        matches, 1,
                        "{platform:?}/{language:?}/bridge={fluid} -> {id}"
                    );
                }
            }
        }
    }

    #[test]
    fn recommended_speech_model_id_requires_the_fluid_bridge_for_core_ml() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        // The bridge is what makes the Neural Engine rows real.
        assert_eq!(
            recommended_speech_model_id(mac14, None, true),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            recommended_speech_model_id(mac14, Some("en"), true),
            "parakeet-ultra-coreml"
        );
        // Without it (CI build, a bridge-less configuration, or a failed
        // version probe) the ggml Parakeet v3 takes over: the same 25
        // languages, on the engine an Intel or macOS 13 Mac also gets.
        assert_eq!(
            recommended_speech_model_id(mac14, None, false),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            recommended_speech_model_id(mac14, Some("de"), false),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            recommended_speech_model_id(
                Platform::MacosAppleSilicon { major: 0 },
                Some("en"),
                false
            ),
            "parakeet-tdt-v3"
        );
    }

    #[test]
    fn recommended_speech_model_id_matches_the_platform_defaults() {
        // Hosts that never touch Core ML are unaffected by the bridge flag.
        for fluid in [true, false] {
            assert_eq!(
                recommended_speech_model_id(Platform::MacosAppleSilicon { major: 13 }, None, fluid),
                "parakeet-tdt-v3"
            );
            assert_eq!(
                recommended_speech_model_id(Platform::MacosIntel, Some("en"), fluid),
                "parakeet-tdt-v3"
            );
            assert_eq!(
                recommended_speech_model_id(Platform::Windows, None, fluid),
                "sherpa-parakeet-v3-int8"
            );
            assert_eq!(
                recommended_speech_model_id(Platform::Other, Some("fr"), fluid),
                "sherpa-parakeet-v3-int8"
            );
        }
    }

    /// An Arabic user must never land on Ultra: it cannot transcribe Arabic,
    /// and the fallback chain keeps the language route instead of silently
    /// handing back a Parakeet build.
    #[test]
    fn recommended_speech_model_id_respects_the_language() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        for lang in ["ar", "hi", "ur", "ja", "zh", "ko", "tr", "fa"] {
            for fluid in [true, false] {
                assert_eq!(
                    recommended_speech_model_id(mac14, Some(lang), fluid),
                    "large-v3-turbo-q5",
                    "macOS 14/{lang} with bridge={fluid}"
                );
            }
            // Whisper does not ship for Windows, so the recommendation stays on
            // the engine the binary can actually load.
            for fluid in [true, false] {
                assert_eq!(
                    recommended_speech_model_id(Platform::Windows, Some(lang), fluid),
                    "sherpa-parakeet-v3-int8",
                    "Windows/{lang} with bridge={fluid}"
                );
            }
        }
        // A covered language keeps the platform default.
        assert_eq!(
            recommended_speech_model_id(Platform::Windows, Some("uk"), false),
            "sherpa-parakeet-v3-int8"
        );
        // "auto", empty, and None all mean "the engine decides".
        for unset in [None, Some(""), Some("auto"), Some("AUTO"), Some("  auto  ")] {
            assert_eq!(
                recommended_speech_model_id(mac14, unset, true),
                "parakeet-ultra-coreml",
                "{unset:?} must not route to Whisper"
            );
        }
        // BCP-47 tags normalize to the base language.
        assert_eq!(
            recommended_speech_model_id(Platform::Windows, Some("pt-BR"), false),
            "sherpa-parakeet-v3-int8"
        );
        assert_eq!(
            recommended_speech_model_id(mac14, Some("ja-JP"), true),
            "large-v3-turbo-q5"
        );
    }

    #[test]
    fn offered_on_is_the_list_predicate() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        let ultra = find("parakeet-ultra-coreml").unwrap();
        assert!(ultra.offered_on(mac14, true));
        assert!(!ultra.offered_on(mac14, false), "no bridge, no Core ML row");
        assert!(!ultra.offered_on(Platform::MacosAppleSilicon { major: 13 }, true));
        assert!(!ultra.offered_on(Platform::Windows, true));

        // The CTC spotter is a boosting companion: never offered, even though
        // the host can load it.
        let ctc = find("parakeet-ctc-110m-coreml").unwrap();
        assert!(ctc.available_on(mac14));
        assert!(!ctc.offered_on(mac14, true));
        assert!(!ctc.offered_on(mac14, false));

        // sherpa-onnx loads everywhere, but its Windows/Linux CPU rows are
        // not surfaced on macOS: every native macOS engine beats them.
        let sherpa = find("sherpa-parakeet-v3-int8").unwrap();
        for p in PLATFORMS {
            if p.is_macos() {
                assert!(
                    !sherpa.offered_on(*p, true) && !sherpa.offered_on(*p, false),
                    "sherpa must not be offered on {p:?}"
                );
            } else {
                assert!(sherpa.offered_on(*p, true) && sherpa.offered_on(*p, false));
            }
        }
    }

    #[test]
    fn uncovered_languages_route_to_whisper_only_where_whisper_ships() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        for lang in ["ar", "hi", "ur", "ja", "zh", "ko", "tr", "fa"] {
            assert_eq!(
                pick_engine(None, mac14, Some(lang)),
                "large-v3-turbo-q5",
                "macOS/{lang}"
            );
            // Whisper is macOS-only, so Windows keeps its loadable engine
            // rather than selecting a model the binary cannot load.
            assert_eq!(
                pick_engine(None, Platform::Windows, Some(lang)),
                "sherpa-parakeet-v3-int8",
                "Windows/{lang}"
            );
        }
        // "auto" means "no language set": platform default, not Whisper.
        assert_eq!(
            pick_engine(None, mac14, Some("auto")),
            "parakeet-ultra-coreml"
        );
        // BCP-47 tags normalize to the base language: en-US is covered.
        assert_eq!(
            pick_engine(None, mac14, Some("en-US")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(pick_engine(None, mac14, Some("ja-JP")), "large-v3-turbo-q5");
    }

    #[test]
    fn pin_wins_only_when_the_host_can_load_it() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(
            pick_engine(Some("large-v3-turbo"), mac14, Some("ar")),
            "large-v3-turbo"
        );
        assert_eq!(
            pick_engine(Some("sherpa-parakeet-v2-int8"), mac14, None),
            "sherpa-parakeet-v2-int8"
        );
        // A pin from another platform is not loadable here; automatic
        // selection continues instead of selecting a dead engine.
        assert_eq!(
            pick_engine(Some("parakeet-tdt-v3"), Platform::Windows, Some("ja")),
            "sherpa-parakeet-v3-int8"
        );
        assert_eq!(
            pick_engine(Some("large-v3-turbo"), Platform::Windows, Some("en")),
            "sherpa-parakeet-v3-int8"
        );
        // Unknown ids are ignored (they cannot be loaded), so automatic
        // selection continues.
        assert_eq!(
            pick_engine(Some("no-such-model"), mac14, None),
            "parakeet-ultra-coreml"
        );
    }

    #[test]
    fn parakeet_cpp_unavailable_hides_fluid() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        let mini = find("fluid-speech-mini").unwrap();
        assert!(
            !mini.offered_on_with(mac14, true, false),
            "no compiled parakeet.cpp dylib, no Fluid Speech Mini row"
        );
        assert!(mini.offered_on_with(mac14, true, true));

        // The platform gate still wins: parakeet.cpp is macOS-only, so a
        // Windows host with a "built" engine gets no row either.
        assert!(!mini.offered_on_with(Platform::Windows, true, true));
    }

    #[test]
    fn engine_labels_are_stable() {
        assert_eq!(Engine::Whisper.label(), "whisper.cpp");
        assert_eq!(Engine::Whisper.platform_label(), "macOS only");
        assert_eq!(Engine::Parakeet.label(), "parakeet (ggml)");
        assert_eq!(Engine::Parakeet.platform_label(), "macOS only");
        assert_eq!(Engine::ParakeetCoreml.label(), "Neural Engine (Core ML)");
        assert_eq!(
            Engine::ParakeetCoreml.platform_label(),
            "Apple Silicon, macOS 14+"
        );
        assert_eq!(Engine::SherpaParakeet.label(), "sherpa-onnx (CPU)");
        assert_eq!(
            Engine::SherpaParakeet.platform_label(),
            "Windows, macOS, Linux"
        );
        assert_eq!(Engine::ParakeetCpp.label(), "parakeet.cpp");
        assert_eq!(Engine::ParakeetCpp.platform_label(), "macOS only");
        assert_eq!(Engine::Qwen3Asr.label(), "Qwen3-ASR (llama.cpp)");
        assert_eq!(
            Engine::Qwen3Asr.platform_label(),
            "macOS only (llama-server)"
        );
    }

    #[test]
    fn qwen3_asr_row_is_well_formed() {
        let m = find("qwen3-asr").expect("qwen3-asr entry");
        assert_eq!(m.engine(), Engine::Qwen3Asr);
        // macOS only, and only where llama-server was built with the mtmd
        // tools; the Windows release build has not been shown to load it.
        assert!(m.available_on(Platform::MacosAppleSilicon { major: 14 }));
        assert!(m.available_on(Platform::MacosIntel));
        assert!(!m.available_on(Platform::Windows));
        assert!(!m.available_on(Platform::Other));
        // Built like the ggml engines: no extra build fact.
        assert!(m.engine().built_on(
            Platform::MacosIntel,
            false,
            false
        ));
        // Two-file artifact: the first artifact's sha256 is the row's pin.
        assert!(!m.recommended());
        assert!(!m.auxiliary());
        assert!(!m.english_only());
        let langs: Vec<&str> = m
            .languages()
            .map(|l| l.iter().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(langs, QWEN3_ASR_LANGUAGES);
        assert_eq!(m.language_label(), "30 languages");
        let dir = std::path::Path::new("/models");
        assert_eq!(m.provider_path(dir), dir.join("qwen3-asr"));
        assert_eq!(m.install_path(dir), dir.join("qwen3-asr"));
        assert_eq!(
            m.url(),
            "https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main/Qwen3-ASR-1.7B-Q8_0.gguf"
        );
        // The two pinned GGUFs the installer verifies against, from the row.
        let arts = m.artifacts();
        assert_eq!(arts.len(), 2);
        assert_eq!(arts[0].file, m.file(), "the row names the main artifact");
        assert_eq!(arts[0].file, QWEN3_ASR_ARTIFACTS[0].0);
        assert_eq!(arts[1].file, QWEN3_ASR_ARTIFACTS[1].0);
        for (a, (file, sha)) in arts.iter().zip(QWEN3_ASR_ARTIFACTS) {
            assert_eq!(&a.file, file);
            assert_eq!(a.sha256.as_deref(), Some(*sha));
            assert_eq!(a.sha256.as_deref().unwrap().len(), 64, "{file}");
            assert!(
                a.sha256.as_deref().unwrap().chars().all(|c| c.is_ascii_hexdigit()),
                "{file}"
            );
            assert!(a.bytes > 0, "{file}");
        }
        // The row's sha256() is the first artifact's pin (the main model).
        assert_eq!(
            m.sha256(),
            Some(QWEN3_ASR_ARTIFACTS[0].1),
            "the row's sha256 is the main model's pin"
        );
        // `size_mb` is the disk pre-flight figure, so it may round up but must
        // never under-report the two files it stands for.
        let total_mb = (arts.iter().map(|a| a.bytes).sum::<u64>() / 1_048_576) as u32;
        assert!(
            (total_mb..=(total_mb as usize * 120 / 100) as u32).contains(&m.size_mb()),
            "size_mb {} vs {} bytes of artifacts",
            m.size_mb(),
            total_mb
        );
    }

    #[test]
    fn sherpa_is_never_offered_on_macos() {
        // sherpa-onnx loads on macOS (available_on is true), but its rows are
        // not surfaced to Mac users: every native macOS engine beats it and
        // the "Default on Windows" models confused onboarding. They remain
        // offered on Windows/Linux, where sherpa IS the default engine.
        let v3 = find("sherpa-parakeet-v3-int8").unwrap();
        let v2 = find("sherpa-parakeet-v2-int8").unwrap();
        for p in [
            Platform::MacosAppleSilicon { major: 27 },
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosIntel,
        ] {
            assert!(v3.available_on(p), "sherpa still loads on {p:?}");
            assert!(
                !v3.offered_on(p, true) && !v2.offered_on(p, true),
                "sherpa must not be offered on {p:?}"
            );
        }
        for p in [Platform::Windows, Platform::Other] {
            assert!(
                v3.offered_on(p, true) && v2.offered_on(p, true),
                "sherpa must stay offered on {p:?}"
            );
        }
        // The macOS recommendation must still be a row the list emits (the
        // historical onboarding bug: a recommendation that was filtered out).
        let mac = Platform::MacosAppleSilicon { major: 27 };
        let rec = recommended_speech_model_id(mac, Some("en"), true);
        let rec_model = find(&rec).unwrap();
        assert!(
            rec_model.offered_on(mac, true),
            "recommended {rec} is not offered on macOS"
        );
    }

    #[test]
    fn engine_availability_follows_what_ships() {
        // whisper-rs, parakeet-sys, and parakeet-cpp-sys are macOS-only
        // dependencies; sherpa-onnx is unconditional.
        for p in [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosIntel,
        ] {
            for id in [
                "parakeet-tdt-v3",
                "large-v3-turbo-q5",
                "fluid-speech-mini",
                "sherpa-parakeet-v3-int8",
            ] {
                assert!(find(id).unwrap().available_on(p), "{id} on {p:?}");
            }
        }
        assert!(find("parakeet-ultra-coreml")
            .unwrap()
            .available_on(Platform::MacosAppleSilicon { major: 14 }));

        // Core ML needs the Neural Engine: Apple Silicon on macOS 14+.
        let ultra = find("parakeet-ultra-coreml").unwrap();
        assert!(!ultra.available_on(Platform::MacosAppleSilicon { major: 13 }));
        assert!(!ultra.available_on(Platform::MacosIntel));

        for p in [Platform::Windows, Platform::Other] {
            for id in [
                "parakeet-tdt-v3",
                "large-v3-turbo-q5",
                "fluid-speech-mini",
                "parakeet-ultra-coreml",
            ] {
                assert!(!find(id).unwrap().available_on(p), "{id} on {p:?}");
            }
            assert!(find("sherpa-parakeet-v3-int8").unwrap().available_on(p));
            assert!(find("sherpa-parakeet-v2-int8").unwrap().available_on(p));
        }
    }

    #[test]
    fn pick_engine_never_returns_an_unavailable_model() {
        let platforms = [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosAppleSilicon { major: 13 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ];
        for p in platforms {
            for lang in [None, Some("en"), Some("de"), Some("ar"), Some("auto")] {
                let id = pick_engine(None, p, lang);
                let model = find(&id).unwrap_or_else(|| panic!("{id} missing from catalog"));
                assert!(
                    model.available_on(p),
                    "pick_engine chose {id} on {p:?} for {lang:?}, but the host cannot load it"
                );
            }
        }
    }

    #[test]
    fn honours_language_arms() {
        let en_only = find("fluid-speech-mini").unwrap();
        assert!(en_only.english_only());
        // None / empty / "auto" are not a request for a language: every row
        // honours them (the engine uses its own default).
        assert!(en_only.honours_language(None));
        assert!(en_only.honours_language(Some("")));
        assert!(en_only.honours_language(Some("auto")));
        assert!(en_only.honours_language(Some("AUTO")));
        assert!(en_only.honours_language(Some("   ")));
        // English in any BCP-47 spelling is honoured.
        assert!(en_only.honours_language(Some("en")));
        assert!(en_only.honours_language(Some("en-US")));
        // A non-English lock on an English-only row is the disagreement this
        // predicate exists to answer.
        assert!(!en_only.honours_language(Some("pl")));
        assert!(!en_only.honours_language(Some("de")));

        let multilingual = find("parakeet-tdt-v3").unwrap();
        assert!(!multilingual.english_only());
        assert!(multilingual.honours_language(None));
        assert!(multilingual.honours_language(Some("en")));
        assert!(multilingual.honours_language(Some("pl")));
        assert!(multilingual.honours_language(Some("de")));

        // Whisper carries no language table: it transcribes every language.
        let whisper = find("large-v3-turbo").unwrap();
        assert!(!whisper.english_only());
        assert!(whisper.honours_language(Some("pl")));
        assert!(whisper.honours_language(Some("ar")));

        // An English-only Whisper build honours English but not more.
        let small_en = find("small-en").unwrap();
        assert!(small_en.english_only());
        assert!(small_en.honours_language(Some("en")));
        assert!(!small_en.honours_language(Some("de")));
    }

    #[test]
    fn catalog_rows_agree_on_language_support() {
        // The catalog's `languages` table and `honours_language` must not
        // contradict each other for any row. A table that lists a language
        // is the row's claim of support, so the predicate must agree; and a
        // predicate that claims a concrete non-English language is an
        // English-only row's negation, so the table (when present) must not
        // withhold it. The `languages: None` shape (Whisper's full 99)
        // carries no table to check.
        for m in catalog() {
            for l in ["en", "de", "pl", "ar", "ja", "fr", "ru", "uk"] {
                match m.languages() {
                    Some(langs) => {
                        if langs.iter().any(|x| x == l) {
                            assert!(
                                m.honours_language(Some(l)),
                                "{} lists `{l}` but honours_language says no",
                                m.id()
                            );
                        }
                    }
                    None => {
                        if m.english_only() && l != "en" {
                            assert!(
                                !m.honours_language(Some(l)),
                                "{} has no table but honours `{l}` despite english_only",
                                m.id()
                            );
                        }
                    }
                }
            }
            // The take path's question, directly: an English-only row never
            // honours a non-English language.
            if m.english_only() {
                assert!(
                    !m.honours_language(Some("pl")),
                    "{} is english_only yet honours Polish",
                    m.id()
                );
            }
        }
    }

    #[test]
    fn pick_engine_language_detour_agrees_with_row_capability() {
        // The detour must agree with the platform candidate's own claim: where
        // Whisper ships, detour iff the candidate's table + predicate
        // (`entry_supports`) say it cannot transcribe the language; where
        // Whisper does not, the candidate always stands. A future flag flip
        // (V2_ENGLISH_PREFERRED), new engine, or edited table cannot make the
        // two disagree without this failing. `entry_supports` is the same
        // predicate the Settings engine picker uses to list a model's
        // languages, so this also keeps the picker and `pick_engine` honest
        // against each other.
        let platforms = [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosAppleSilicon { major: 13 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ];
        for p in platforms {
            for lang in ["en", "en-US", "de", "pl", "ar", "ja-JP", "uk", "auto"] {
                let candidate: &str = match p {
                    Platform::MacosAppleSilicon { major } if major >= 14 => "parakeet-ultra-coreml",
                    Platform::Windows => "sherpa-parakeet-v3-int8",
                    Platform::MacosAppleSilicon { .. } | Platform::MacosIntel => "parakeet-tdt-v3",
                    Platform::Other => "sherpa-parakeet-v3-int8",
                };
                // `entry_supports` is the single list predicate the Settings
                // engine picker uses to decide whether a model's language
                // list covers a language, so the detour must agree with it
                // directly (a row's `honours_language` alone cannot see a
                // 25-language table that withholds `en-US`). The test passes
                // the raw language - BCP-47 tags included - exactly as the
                // picker does; the row normalizes it, so "en-US" is
                // covered by a table that lists "en".
                let can_support = find(candidate).is_some_and(|m| entry_supports(&m, Some(lang)));
                let whisper_ships = find("large-v3-turbo-q5").is_some_and(|m| m.available_on(p));
                let picked = pick_engine(None, p, Some(lang));
                assert!(
                    picked == candidate || (whisper_ships && picked == "large-v3-turbo-q5"),
                    "{p:?}/{lang}: picked {picked}, expected {candidate} or Whisper"
                );
                assert_eq!(
                    picked == "large-v3-turbo-q5",
                    whisper_ships && !can_support,
                    "{p:?}/{lang}: Whisper detour disagrees with the candidate row"
                );
            }
        }
    }

    #[test]
    fn only_the_ctc_spotter_is_auxiliary() {
        let ctc = find("parakeet-ctc-110m-coreml").expect("ctc entry");
        assert!(
            ctc.auxiliary(),
            "the CTC spotter is a boosting companion, not a dictation model"
        );
        assert_eq!(catalog().iter().filter(|m| m.auxiliary()).count(), 1);
    }

    #[test]
    fn install_and_provider_paths_follow_engine_layout() {
        let dir = std::path::Path::new("/models");

        let ggml = find("parakeet-tdt-v3").unwrap();
        assert_eq!(ggml.provider_path(dir), dir.join(ggml.file()));
        assert_eq!(ggml.install_path(dir), dir.join(ggml.file()));

        // The sherpa release archive extracts to a directory named after the
        // archive without its `.tar.bz2` suffix.
        let sherpa = find("sherpa-parakeet-v3-int8").unwrap();
        let sherpa_dir = dir
            .join("sherpa")
            .join("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8");
        assert_eq!(sherpa.provider_path(dir), sherpa_dir);
        assert_eq!(sherpa.install_path(dir), sherpa_dir);

        // The FluidAudio bridge takes the base directory and appends the
        // repo folder itself; the installed bytes live in that repo folder.
        // `fluid_dir_name` strips `-coreml` from the repo slug, so Ultra
        // lives in `parakeet-ultra`, not `parakeet-ultra-coreml`, and the
        // v3 row (file `parakeet-v3-coreml`) lives in `parakeet-v3`.
        let fluid = find("parakeet-ultra-coreml").unwrap();
        assert_eq!(fluid.provider_path(dir), dir.join("fluid"));
        assert_eq!(
            fluid.install_path(dir),
            dir.join("fluid").join("parakeet-ultra")
        );
        let v3 = find("parakeet-v3-coreml").unwrap();
        assert_eq!(
            v3.install_path(dir),
            dir.join("fluid").join("parakeet-v3")
        );
    }

    #[test]
    fn parses_macos_major_versions() {
        assert_eq!(parse_macos_major("14.5"), Some(14));
        assert_eq!(parse_macos_major("15.0.1"), Some(15));
        assert_eq!(parse_macos_major("13"), Some(13));
        assert_eq!(parse_macos_major(" 26.1\n"), Some(26));
        assert_eq!(parse_macos_major(""), None);
        assert_eq!(parse_macos_major("not-a-version"), None);
    }

    #[test]
    fn host_platform_matches_the_build_target() {
        let p = host_platform();
        if cfg!(target_os = "macos") {
            match p {
                Platform::MacosAppleSilicon { major } => {
                    assert!(cfg!(target_arch = "aarch64"));
                    // 0 means the version probe failed; otherwise the value
                    // must be a real macOS major.
                    assert!(major == 0 || major >= 10, "detected macOS {major}");
                }
                Platform::MacosIntel => assert!(!cfg!(target_arch = "aarch64")),
                other => panic!("macOS host reported {other:?}"),
            }
        } else if cfg!(target_os = "windows") {
            assert_eq!(p, Platform::Windows);
        } else {
            assert_eq!(p, Platform::Other);
        }
    }

    #[test]
    fn english_stays_ultra_while_the_v2_flag_is_off() {
        // `V2_ENGLISH_PREFERRED` is const-false until the P3.8 bake-off, so
        // the English branches must not produce v2. (If the flag flips, this
        // test flips with it and the two assertions below become v2 picks.)
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(
            pick_engine(None, mac14, Some("en")),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            pick_engine(None, Platform::Windows, Some("en")),
            "sherpa-parakeet-v3-int8"
        );
    }

    #[test]
    fn every_pick_returns_a_catalog_entry() {
        let platforms = [
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ];
        for p in platforms {
            for lang in [None, Some("en"), Some("ar"), Some("auto")] {
                let id = pick_engine(None, p, lang);
                assert!(find(&id).is_some(), "{id} missing from catalog");
            }
        }
    }

    /// The first-run list is one card per decision, and the card that leads it
    /// is the card the rest of the app recommends.
    #[test]
    fn shortlist_leads_with_the_recommendation_and_stays_small() {
        for p in PLATFORMS {
            for fluid in [true, false] {
                for lang in [
                    None,
                    Some("auto"),
                    Some("en"),
                    Some("de"),
                    Some("ar"),
                    Some("ja"),
                ] {
                    let ids = shortlist_ids(*p, lang, fluid);
                    assert!(
                        !ids.is_empty(),
                        "{p:?}/lang={lang:?}/fluid={fluid} shortlisted nothing"
                    );
                    assert!(
                        ids.len() <= SHORTLIST_MAX,
                        "{p:?}/{lang:?} shortlisted {} rows: {ids:?}",
                        ids.len()
                    );
                    assert_eq!(
                        ids[0],
                        recommended_speech_model_id(*p, lang, fluid),
                        "{p:?}/{lang:?}/fluid={fluid} must lead with its recommendation"
                    );
                    for (i, id) in ids.iter().enumerate() {
                        let entry = find(id).expect("shortlist names a catalog entry");
                        assert!(
                            entry.offered_on(*p, fluid),
                            "{p:?}/fluid={fluid} shortlisted an unoffered row {id}"
                        );
                        // Row 0 is the recommendation, which is allowed to be a
                        // row that cannot do the language when nothing else on
                        // this host can (Windows + Arabic).
                        if i > 0 {
                            assert!(
                                entry_supports(&entry, lang),
                                "{p:?}/{lang:?} shortlisted {id}, which cannot transcribe \
                                 that language"
                            );
                        }
                    }
                }
            }
        }
    }

    /// English on Apple Silicon is the case Ali complained about: 13 rows for
    /// one decision. Three is the answer - Neural Engine, portable ggml, and
    /// the many-language build - and it survives losing the bridge.
    #[test]
    fn shortlist_is_the_measured_mac_triple() {
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(
            shortlist_ids(mac14, Some("en"), true),
            [
                "parakeet-ultra-coreml",
                "parakeet-tdt-v3",
                "large-v3-turbo-q5"
            ]
        );

        // No Neural Engine build: the recommendation moves and the list follows.
        assert_eq!(
            shortlist_ids(mac14, Some("en"), false),
            ["parakeet-tdt-v3", "large-v3-turbo-q5"]
        );

        // macOS 13 has no Core ML row either, so ggml Parakeet leads.
        assert_eq!(
            shortlist_ids(Platform::MacosAppleSilicon { major: 13 }, Some("en"), true),
            ["parakeet-tdt-v3", "large-v3-turbo-q5"]
        );
        assert_eq!(
            shortlist_ids(Platform::MacosIntel, Some("en"), true),
            ["parakeet-tdt-v3", "large-v3-turbo-q5"]
        );

        // A language Parakeet cannot do routes to Whisper alone: no other row
        // on this host transcribes it, so showing three would be two dead ends.
        assert_eq!(
            shortlist_ids(mac14, Some("ja"), true),
            ["large-v3-turbo-q5"]
        );

        // Windows: the coverage build first, the English-only build second, and
        // no macOS engine at all.
        assert_eq!(
            shortlist_ids(Platform::Windows, Some("en"), true),
            ["sherpa-parakeet-v3-int8", "sherpa-parakeet-v2-int8"]
        );
        assert_eq!(
            shortlist_ids(Platform::Windows, Some("ar"), true),
            ["sherpa-parakeet-v3-int8"]
        );
    }

    #[test]
    fn every_speech_model_has_a_purpose() {
        for m in catalog() {
            assert!(
                !m.purposes().is_empty(),
                "{} has no purpose chips; every row must say what it is good for",
                m.id()
            );
        }
    }

    #[test]
    fn measured_purposes_name_their_evidence() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .canonicalize()
            .expect("repo root must resolve");
        for m in catalog() {
            for p in m.purposes() {
                match p.basis.as_str() {
                    "measured" => {
                        let evidence = p.evidence.as_deref().unwrap_or_else(|| {
                            panic!("{}: a measured chip must name its evidence", m.id())
                        });
                        let path = root.join(evidence);
                        assert!(
                            path.exists(),
                            "{}: evidence path {} does not exist",
                            m.id(),
                            evidence
                        );
                    }
                    "inferred" => {
                        assert!(
                            p.evidence.is_none(),
                            "{}: an inferred chip must not carry evidence",
                            m.id()
                        );
                    }
                    other => panic!("{}: unknown purpose basis {other:?}", m.id()),
                }
            }
        }
    }

    #[test]
    fn purpose_labels_are_from_the_fixed_vocabulary() {
        for m in catalog() {
            for p in m.purposes() {
                assert!(
                    PURPOSE_LABELS.contains(&p.label.as_str()),
                    "{} uses label {:?}, which is not in the fixed chip vocabulary",
                    m.id(),
                    p.label
                );
            }
        }
    }
}
