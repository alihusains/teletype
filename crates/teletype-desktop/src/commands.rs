//! Tauri commands: the IPC surface the React UI calls.

use serde::{Deserialize, Serialize};

use teletype_core::{
    platform::Permission,
    state::{PillPosition as CorePillPosition, RecordingMode as CoreRecordingMode},
    transforms::ListStyle as CoreListStyle,
};

/// Structured IPC error (roadmap S3): a stable machine-readable `code` the
/// UI can branch on, plus the human-readable `message` shown to the user.
/// Messages are unchanged from the `String`-error era, so logs and toasts
/// read exactly as before; only the shape gained a code.
#[derive(Debug, Clone, PartialEq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    /// Caller-fixable input/validation failure (bad id, empty word, ...).
    pub fn user(message: impl Into<String>) -> Self {
        Self {
            code: "E_REQUEST".into(),
            message: message.into(),
        }
    }

    /// Named resource that does not exist (word, pack, preference, ...).
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: "E_NOT_FOUND".into(),
            message: message.into(),
        }
    }

    /// Model/catalog/download failure.
    pub fn model(message: impl Into<String>) -> Self {
        Self {
            code: "E_MODEL".into(),
            message: message.into(),
        }
    }

    /// Bug, I/O, or infrastructure failure the caller cannot fix.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "E_INTERNAL".into(),
            message: message.into(),
        }
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::internal(message)
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::internal(message.to_string())
    }
}

impl From<CommandError> for String {
    fn from(err: CommandError) -> Self {
        err.message
    }
}

pub(crate) type CommandResult<T> = Result<T, CommandError>;

/// Hosts that are allowed to fall back to the legacy `openai` keychain
/// account (see [`read_api_key`]).
const OPENAI_HOSTS: &[&str] = &[
    "api.openai.com",
    "openai.com",
    "www.openai.com",
    "eu.api.openai.com",
];

/// Derives the keychain account id from a base URL (P0-9).
/// `https://openrouter.ai/api/v1` -> `openrouter.ai`
/// `http://127.0.0.1:11434/v1` -> `127.0.0.1`
///
/// Parsed with `url::Url` rather than by splitting on `:` and `/`. The manual
/// version read the *username* of a URL with userinfo as the host, so
/// `https://user:pass@evil.example/v1` resolved to the account `user`, found
/// no key for it, and fell through to the OpenAI key, which was then sent to
/// `evil.example`.
///
/// The port is deliberately not part of the account, so existing stored keys
/// keep resolving. Two local services on different ports therefore share one
/// key slot; that is a loopback-only exposure and changing it would silently
/// orphan every stored key.
fn secret_account_for_url(base_url: &str) -> String {
    match url::Url::parse(base_url) {
        Ok(u) => u
            .host_str()
            .map(|h| h.to_ascii_lowercase())
            .unwrap_or_else(|| "unknown".to_string()),
        Err(_) => "unknown".to_string(),
    }
}

/// True when `base_url` actually points at OpenAI.
fn is_openai_base_url(base_url: &str) -> bool {
    let host = secret_account_for_url(base_url);
    host == "openai" || OPENAI_HOSTS.contains(&host.as_str())
}

/// Reads the API key for the given base URL.
///
/// A pre-P0-9 install stored its OpenAI key under the account `openai`, so
/// that account is still honoured -- but **only when the configured base URL
/// really is OpenAI**. The fallback used to be unconditional, which sent the
/// user's OpenAI key to whatever third-party host they had pointed the
/// connector at (Groq, OpenRouter, an arbitrary pasted URL), on every app
/// launch, every connection test and every provider probe.
fn read_api_key(base_url: &str) -> Option<String> {
    let host = secret_account_for_url(base_url);
    if let Some(key) = crate::secrets::get_secret(&host).ok().flatten() {
        return Some(key);
    }
    if is_openai_base_url(base_url) {
        return crate::secrets::get_secret("openai").ok().flatten();
    }
    None
}

pub mod autotext;
pub mod dictionary;
pub mod history;
pub mod models;
pub mod personalization;
pub mod scratchpad;
pub mod settings;
pub mod system;
pub mod transforms;

pub use autotext::*;
pub use dictionary::*;
pub use history::*;
pub use models::*;
pub use personalization::*;
pub use scratchpad::*;
pub use settings::*;
pub use system::*;
pub use transforms::*;

// ---- Settings ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
/// One user-added Polish instruction (Wispr-style "Customize your Polish
/// prompt" list). The base prompt is the first, locked list item and is not
/// stored here; only user-added fragments are.
pub struct PolishInstruction {
    pub id: String,
    pub text: String,
    pub enabled: bool,
}

/// Which local ASR engine loads the selected dictation model. Replaces the
/// old runtime `use_parakeet` boolean (roadmap S9): the engine is now a
/// first-class, persisted setting. The selected model always implies its own
/// engine; this field is the fallback when the model id is not in the
/// catalog (e.g. a custom file the user pointed the app at).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpeechEngine {
    #[default]
    Whisper,
    Parakeet,
    ParakeetCpp,
}

impl SpeechEngine {
    /// The engine label used in log lines and the runtime status UI.
    pub fn label(self) -> &'static str {
        match self {
            SpeechEngine::Whisper => "whisper",
            SpeechEngine::Parakeet => "parakeet",
            SpeechEngine::ParakeetCpp => "parakeet.cpp",
        }
    }
}

// ---------------------------------------------------------------------------
// Typed settings enums (roadmap S3).
//
// These fields used to be free-form `String`s parsed with `from_str_lenient`,
// so a typo silently became the default. Each enum serializes to the exact
// wire values the UI already sends (byte-identical JSON), and an
// unrecognized value deserializes to `Unknown` instead of failing the whole
// settings load — a future value can never brick a settings file. `Unknown`
// always resolves to the previous default behavior. Command inputs that take
// a raw string (`set_app_icon`, `set_theme`) parse via `FromStr` and reject
// unknown values with `E_REQUEST` instead.
// ---------------------------------------------------------------------------

/// How the user controls recording. Wire values `"hold"` / `"toggle"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingModeSetting {
    #[default]
    Hold,
    Toggle,
    #[serde(other)]
    Unknown,
}

impl RecordingModeSetting {
    pub fn resolve(self) -> CoreRecordingMode {
        match self {
            RecordingModeSetting::Toggle => CoreRecordingMode::Toggle,
            RecordingModeSetting::Hold | RecordingModeSetting::Unknown => CoreRecordingMode::Hold,
        }
    }
}

/// Where the floating pill sits (3×3 grid). Wire values like `"bottomCenter"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PillPositionSetting {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
    #[serde(other)]
    Unknown,
}

impl PillPositionSetting {
    pub fn resolve(self) -> CorePillPosition {
        match self {
            PillPositionSetting::TopLeft => CorePillPosition::TopLeft,
            PillPositionSetting::TopCenter => CorePillPosition::TopCenter,
            PillPositionSetting::TopRight => CorePillPosition::TopRight,
            PillPositionSetting::CenterLeft => CorePillPosition::CenterLeft,
            PillPositionSetting::Center => CorePillPosition::Center,
            PillPositionSetting::CenterRight => CorePillPosition::CenterRight,
            PillPositionSetting::BottomLeft => CorePillPosition::BottomLeft,
            PillPositionSetting::BottomRight => CorePillPosition::BottomRight,
            // Matches the old `parse_position` fallback.
            PillPositionSetting::BottomCenter | PillPositionSetting::Unknown => {
                CorePillPosition::BottomCenter
            }
        }
    }
}

/// How lists are formatted in AI-polished output. Wire values `"bullets"` /
/// `"numbered"` (plus the legacy `"numbered-list"` alias the lenient parser
/// accepted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListStyleSetting {
    #[default]
    Bullets,
    #[serde(alias = "numbered-list")]
    Numbered,
    #[serde(other)]
    Unknown,
}

impl ListStyleSetting {
    pub fn resolve(self) -> CoreListStyle {
        match self {
            ListStyleSetting::Numbered => CoreListStyle::Numbered,
            ListStyleSetting::Bullets | ListStyleSetting::Unknown => CoreListStyle::Bullets,
        }
    }

    pub fn as_id(self) -> &'static str {
        match self {
            ListStyleSetting::Numbered => "numbered",
            ListStyleSetting::Bullets | ListStyleSetting::Unknown => "bullets",
        }
    }
}

/// Dictionary match strictness. Wire values `"loose"` / `"standard"` / `"strict"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchStrictness {
    Loose,
    #[default]
    Standard,
    Strict,
    #[serde(other)]
    Unknown,
}

impl MatchStrictness {
    /// The similarity floor for this level. `Standard` (and `Unknown`)
    /// resolve to `None` so the caller keeps the pipeline's own default
    /// rather than a restated number — the two cannot drift.
    pub fn floor(self) -> Option<f32> {
        match self {
            MatchStrictness::Loose | MatchStrictness::Strict => MATCH_STRICTNESS_LEVELS
                .iter()
                .find(|(name, _)| *name == self.as_id())
                .map(|(_, v)| *v),
            MatchStrictness::Standard | MatchStrictness::Unknown => None,
        }
    }

    pub fn as_id(self) -> &'static str {
        match self {
            MatchStrictness::Loose => "loose",
            MatchStrictness::Standard => "standard",
            MatchStrictness::Strict => "strict",
            MatchStrictness::Unknown => "standard",
        }
    }
}

impl std::str::FromStr for MatchStrictness {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "loose" => Ok(MatchStrictness::Loose),
            "standard" => Ok(MatchStrictness::Standard),
            "strict" => Ok(MatchStrictness::Strict),
            _ => Err(CommandError::user(format!(
                "Unknown match strictness '{s}'"
            ))),
        }
    }
}

/// Selected app icon. Wire values `"white"` / `"purple"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppIcon {
    #[default]
    White,
    Purple,
    #[serde(other)]
    Unknown,
}

impl AppIcon {
    pub fn as_id(self) -> &'static str {
        match self {
            AppIcon::Purple => "purple",
            AppIcon::White => "white",
            AppIcon::Unknown => "white",
        }
    }
}

impl std::str::FromStr for AppIcon {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "purple" => Ok(AppIcon::Purple),
            "white" => Ok(AppIcon::White),
            _ => Err(CommandError::user(format!("Unknown app icon '{s}'"))),
        }
    }
}

/// UI color theme. Wire values `"system"` / `"light"` / `"dark"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiTheme {
    #[default]
    System,
    Light,
    Dark,
    #[serde(other)]
    Unknown,
}

impl UiTheme {
    pub fn as_id(self) -> &'static str {
        match self {
            UiTheme::System => "system",
            UiTheme::Light => "light",
            UiTheme::Dark => "dark",
            UiTheme::Unknown => "system",
        }
    }
}

impl std::str::FromStr for UiTheme {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "system" => Ok(UiTheme::System),
            "light" => Ok(UiTheme::Light),
            "dark" => Ok(UiTheme::Dark),
            _ => Err(CommandError::user(format!("Unknown theme '{s}'"))),
        }
    }
}

/// Typed AutoText replacement timing. Wire values `"delimiter"` / `"immediate"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutotextTiming {
    #[default]
    Delimiter,
    Immediate,
    #[serde(other)]
    Unknown,
}

impl AutotextTiming {
    pub fn as_id(self) -> &'static str {
        match self {
            AutotextTiming::Delimiter => "delimiter",
            AutotextTiming::Immediate => "immediate",
            AutotextTiming::Unknown => "delimiter",
        }
    }
}

/// How much filler the tiered pass removes. Wire values `"light"` /
/// `"standard"` / `"strict"`; the shipped default is `standard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillerStrengthSetting {
    Light,
    #[default]
    Standard,
    Strict,
    #[serde(other)]
    Unknown,
}

impl FillerStrengthSetting {
    pub fn as_id(self) -> &'static str {
        match self {
            FillerStrengthSetting::Light => "light",
            FillerStrengthSetting::Standard => "standard",
            FillerStrengthSetting::Strict => "strict",
            // A value from a newer build degrades to the shipped behavior
            // rather than to "remove nothing" or a panic.
            FillerStrengthSetting::Unknown => "standard",
        }
    }

    /// The core tier set this level enables. Light is Tier 1 only, Standard
    /// adds the punctuation-gated discourse markers, Strict adds the softeners
    /// that commas set off.
    pub fn to_core(self) -> teletype_core::fillers::tiers::FillerStrength {
        match self {
            FillerStrengthSetting::Light => {
                teletype_core::fillers::tiers::FillerStrength::Hesitations
            }
            FillerStrengthSetting::Standard | FillerStrengthSetting::Unknown => {
                teletype_core::fillers::tiers::FillerStrength::Fillers
            }
            FillerStrengthSetting::Strict => teletype_core::fillers::tiers::FillerStrength::Full,
        }
    }
}

/// Recording pill style. Wire values `"default"` / `"classic"` /
/// `"levelRail"` / `"well"` / `"dotGrid"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PillStyle {
    Default,
    #[default]
    Classic,
    LevelRail,
    Well,
    DotGrid,
    #[serde(other)]
    Unknown,
}

impl PillStyle {
    pub fn as_id(self) -> &'static str {
        match self {
            PillStyle::Default => "default",
            PillStyle::Classic => "classic",
            PillStyle::LevelRail => "levelRail",
            PillStyle::Well => "well",
            PillStyle::DotGrid => "dotGrid",
            PillStyle::Unknown => "default",
        }
    }
}

/// When to load the speech engine ahead of the hotkey press. Wire values
/// `"off"` / `"10"` / `"30"` / `"60"` / `"always"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WarmEnginePolicy {
    Off,
    #[serde(rename = "10")]
    Sec10,
    #[serde(rename = "30")]
    #[default]
    Sec30,
    #[serde(rename = "60")]
    Sec60,
    #[serde(rename = "always")]
    Always,
    #[serde(other)]
    Unknown,
}

impl WarmEnginePolicy {
    pub fn as_id(self) -> &'static str {
        match self {
            WarmEnginePolicy::Off => "off",
            WarmEnginePolicy::Sec10 => "10",
            WarmEnginePolicy::Sec30 => "30",
            WarmEnginePolicy::Sec60 => "60",
            WarmEnginePolicy::Always => "always",
            WarmEnginePolicy::Unknown => "30",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkey: String,
    pub recording_mode: RecordingModeSetting,
    pub selected_speech_model: String,
    /// Which engine loads the dictation model (S9). The selected model's own
    /// engine wins at runtime; this is the fallback for ids outside the
    /// catalog. Migrated one-time at load from the model id.
    #[serde(default = "default_speech_engine")]
    pub speech_engine: SpeechEngine,
    pub language: String,
    pub input_device: String,
    pub restore_clipboard: bool,
    /// Leave the dictated text on the clipboard as an extra item, so it is
    /// added to the system clipboard history instead of being lost with the
    /// paste. Independent of `restore_clipboard`: the user's own copy is put
    /// back either way, and nothing is ever destroyed.
    #[serde(default = "default_true")]
    pub keep_text_on_clipboard: bool,
    pub auto_apply_transform: bool,
    pub show_tray_icon: bool,
    pub has_completed_onboarding: bool,
    pub selected_llm_model: String,
    /// Typed AutoText: expand a stored trigger (e.g. `/email`) as you type it
    /// followed by space/enter, in any app. On by default so the feature works
    /// out of the box; requires Accessibility permission on macOS.
    #[serde(default = "default_true")]
    pub typing_autotext_enabled: bool,
    /// AutoText replacement timing: "delimiter" (expand when space/tab/enter
    /// completes a trigger, the default) or "immediate" (expand the moment the
    /// buffer ends with a trigger).
    #[serde(default = "default_autotext_timing")]
    pub autotext_timing: AutotextTiming,
    pub remove_filler_words: bool,
    pub filler_words: Vec<String>,
    /// How aggressively the tiered filler pass removes words: "light" /
    /// "standard" (the shipped default) / "strict".
    #[serde(default = "default_filler_strength")]
    pub filler_strength: FillerStrengthSetting,
    /// Where the floating pill sits: a 3×3 grid, e.g. "bottomCenter".
    pub pill_position: PillPositionSetting,
    /// Show the pill's idle bars even when not dictating.
    pub always_show_pill: bool,
    /// Id of the active style profile ("" = none). New installs start on the
    /// built-in Concise; see [`default_active_style_profile`].
    pub active_style_profile: String,
    /// When true and the scratchpad window is frontmost, dictation is
    /// appended to the scratchpad instead of being injected.
    pub scratchpad_enabled: bool,
    /// Selected app icon id: "white" (default) or "purple".
    #[serde(default = "default_app_icon")]
    pub app_icon: AppIcon,
    /// Folder where day-wise transcript files are written. Empty = default
    /// (a `transcripts` subfolder in the app config dir).
    #[serde(default)]
    pub transcripts_dir: String,
    /// Active LLM provider: "local-server" | "openai-compat" | "" (none).
    #[serde(default)]
    pub selected_llm_provider: String,
    /// Base URL for the OpenAI-compatible connector (includes `/v1`).
    #[serde(default = "default_openai_base_url")]
    pub openai_base_url: String,
    /// Remote model id for the OpenAI-compatible connector.
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    /// Show the Developer tab (live app logs) in the sidebar.
    #[serde(default)]
    pub enable_developer_tab: bool,
    /// Command Mode master switch. When off, extracted commands are never
    /// executed and the take is delivered as plain dictation instead.
    #[serde(default)]
    pub command_mode_enabled: bool,
    /// Recording pill style: "default" | "classic" | "levelRail" | "well" | "dotGrid".
    #[serde(default = "default_pill_style")]
    pub pill_style: PillStyle,
    /// Stop recording automatically once the user pauses after speaking.
    /// Active only in hands-free hold mode (Hold mode + a double-tap to start),
    /// where the pause is the only stop signal. In plain hold-to-talk the key
    /// release already stops the take, and in push-to-talk (toggle) mode the
    /// second tap does, so both preempt VAD.
    #[serde(default)]
    pub vad_auto_stop: bool,
    /// How long the pause must last before auto-stop triggers, in
    /// milliseconds (the "stop after a pause of N ms" slider).
    #[serde(default = "default_vad_silence_ms")]
    pub vad_silence_ms: u64,
    /// P5.1: when true and a local LLM provider is active, a deterministic
    /// gate may skip the polish pass for short clean utterances. On by
    /// default: it only skips when a local LLM is active and the take is
    /// short and clean, so it is a pure latency win with no quality risk
    /// (roadmap P5.1).
    #[serde(default = "default_true")]
    pub polish_gate_enabled: bool,
    /// P5.1: max word count for the gate's short-clean-skip path.
    #[serde(default = "default_polish_gate_threshold_words")]
    pub polish_gate_threshold_words: usize,
    /// How much of a polished take the model may replace, as a percentage of
    /// its words. 40 is the default; 100 turns the ceiling off. A rewrite
    /// over the ceiling falls back to the words the user actually said,
    /// alongside the fixed off-topic floor that always applies.
    #[serde(default = "default_polish_max_change_percent")]
    pub polish_max_change_percent: u8,
    /// Per-app ASR language overrides (P3.3): normalized app key -> language
    /// code (e.g. "com.google.gmail" -> "en"). An override for the frontmost
    /// app wins over the global `language` setting.
    #[serde(default)]
    pub app_language_overrides: std::collections::BTreeMap<String, String>,
    /// When true, restore curated emoji phrases in voice transcripts after
    /// the transform (P3.2).
    #[serde(default = "default_true_emoji")]
    pub restore_emoji: bool,
    /// When true, convert spoken emoji phrases ("thumbs up emoji") to
    /// glyphs in voice transcripts after the transform (BUG-002, ported
    /// from EW's EmojiFormatterStep).
    #[serde(default = "default_true")]
    pub spoken_emoji: bool,
    /// When true, the System AutoText spoken-punctuation entries ("comma" →
    /// "," etc.) run in the pipeline. Off means those phrases stay as
    /// words; custom AutoText is unaffected (BUG-002).
    #[serde(default = "default_true")]
    pub spoken_punctuation: bool,
    #[serde(default)]
    pub model_unload_delay_secs: u64,
    /// Ids of enabled vocabulary packs ("tech", "medical", "legal", "brands",
    /// "names"). Off by default; each pack is toggled independently.
    #[serde(default)]
    pub enabled_packs: Vec<String>,
    /// UI color theme: "system" (default, follows the OS) | "light" | "dark".
    /// Applies to the main window only; the floating pill stays always-dark.
    #[serde(default = "default_theme")]
    pub theme: UiTheme,
    /// Reduce Motion: when true, force near-instant animation/transition
    /// durations in the UI. When false, the OS Reduce Motion preference still
    /// applies (P3.19).
    #[serde(default)]
    pub reduce_motion: bool,
    /// Show the live transcript in the recording pill while speaking
    /// (T1.2). On by default; the interim loop costs a re-decode per tick,
    /// so users who only want the animation can switch it off.
    #[serde(default = "default_true")]
    pub live_preview_enabled: bool,
    /// Play a short cue when a dictation starts and when it stops, so the
    /// user knows the hotkey registered without watching the pill.
    #[serde(default = "default_true")]
    pub play_recording_sounds: bool,
    /// Which cue to play: a macOS system sound name from
    /// [`RECORDING_SOUND_NAMES`]. Kept separate from the toggle so the choice
    /// survives the user switching cues off and back on.
    #[serde(default = "default_recording_sound")]
    pub recording_sound: String,
    /// Separate binding that aborts the recording in progress and injects
    /// nothing. Empty means disabled; the default is Escape.
    #[serde(default = "default_cancel_hotkey")]
    pub cancel_hotkey: String,
    /// Make injected text match the surroundings: reuse the leading space
    /// that is already there instead of adding another, and carry the
    /// capitalisation of the character the dictation follows.
    #[serde(default = "default_true")]
    pub smart_insertion: bool,
    /// When to load the speech engine ahead of the hotkey press, as a policy
    /// string from `"off"`, `"10"`, `"30"`, `"60"`, `"always"`. Warming trades
    /// battery and memory for a faster first word.
    #[serde(default = "default_warm_engine_policy")]
    pub warm_engine_policy: WarmEnginePolicy,
    /// Dictionary match strictness: one of `loose` | `standard` | `strict`
    /// (`MATCH_STRICTNESS_LEVELS`).
    #[serde(default = "default_match_strictness")]
    pub match_strictness: MatchStrictness,
    /// Master switch for the dictionary correction pass. Off by nothing —
    /// on by default so existing behaviour is unchanged; the Dictionary
    /// page's banner exposes the toggle (reference-app parity).
    #[serde(default = "default_true")]
    pub dictionary_enabled: bool,
    /// AI Polish rule toggles: which prompt fragments are active. All five
    /// are on by default (the full polish prompt). The UI composes the
    /// instruction from these + the user's custom text; the engine just
    /// sends the stored instruction verbatim.
    #[serde(default = "default_polish_rules")]
    pub polish_rules: std::collections::BTreeMap<String, bool>,
    /// How lists are formatted in AI-polished output: "bullets" ("- item")
    /// or "numbered" ("1. item"). Default is bullets (matches CORE_RULES).
    #[serde(default = "default_list_style")]
    pub list_style: ListStyleSetting,
    /// Wispr-style "Customize your Polish prompt" list: each entry is a
    /// user-added instruction fragment with an on/off toggle. Appended to the
    /// assembled Polish instruction (only the enabled ones). The base prompt
    /// is the first, locked item in the UI and is not stored here.
    #[serde(default)]
    pub polish_custom_instructions: Vec<PolishInstruction>,
    /// Quick Add (item 8): global keybinding that adds the currently
    /// selected word in any app to the dictionary. Empty = disabled.
    #[serde(default)]
    pub quick_add_hotkey: String,
    /// T20: show the strong-language coach card on the Insights screen.
    /// On by default; the card is hidden entirely when off.
    #[serde(default = "default_true")]
    pub show_language_coach: bool,
    /// Dedicated Command Mode hotkey (P2.4): starts a command take instead
    /// of dictation. Empty = disabled. macOS default "Ctrl+Option+Space",
    /// Windows default "Ctrl+Alt+Space".
    #[serde(default = "default_command_hotkey")]
    pub command_hotkey: String,
    /// P8.4: Paste-Last hotkey. Re-delivers the last final text through the
    /// same delivery path as a normal paste. macOS default "Cmd+Ctrl+V",
    /// other platforms "Ctrl+Alt+V". Empty = disabled.
    #[serde(default = "default_paste_last_hotkey")]
    pub paste_last_hotkey: String,
    /// Spoken trigger for Command Mode (P2.5): a take starting with
    /// "teletype" / "hey teletype" is stripped and routed as a command.
    #[serde(default = "default_true")]
    pub command_wake_phrase: bool,
    /// Enable RNNoise noise suppression on the microphone input.
    #[serde(default = "default_true")]
    pub noise_reduction_enabled: bool,
    /// One-time migration notice for the UI (P3.9), serialized as
    /// `pendingNotice`. Set when something had to be swapped under the user: a
    /// removed LLM model (EG-1) or a dictation model this host cannot run
    /// ([`migrate_unavailable_speech_model`], which names old and new in the
    /// sentence). The Models screen shows it as a banner and clears it through
    /// the `dismiss_pending_notice` command, so it can only appear once.
    #[serde(default)]
    pub pending_notice: Option<String>,
    /// Phase 5/6: polish the entire take as one unit at the end instead of
    /// per committed segment. Slower but more consistent output.
    #[serde(default)]
    pub polish_whole_text_at_end: bool,
    /// Phase 5/6: skip the AI polish pass when the take is already clean
    /// (the P6.2 gate constant, now user-controllable). On by default.
    #[serde(default = "default_true")]
    pub polish_skip_when_clean: bool,
    /// Phase 5/6: when to start recording ahead of the hotkey press.
    /// "off" | "while_app_active" | "always".
    #[serde(default = "default_instant_start")]
    pub instant_start: String,
    /// Phase 5/6: prefer the built-in microphone while a Bluetooth headset
    /// is connected (music keeps playing over the headset; the narrowband
    /// HFP mic is what hurts accuracy). On by default.
    #[serde(default = "default_true")]
    pub prefer_builtin_mic_with_bt: bool,
    /// Phase 5/6: run the experimental audio conditioning stage (prewarm +
    /// front-end pipeline, see `teletype_core::audio::condition`). Off by
    /// default; the engine wiring consumes this in a later unit.
    pub audio_conditioning: bool,
    /// Phase 7.8: learn personal terms from the user's post-dictation edits
    /// (the edit-watch loop). On by default; the pipeline consumption of
    /// this flag is wired in a later unit.
    #[serde(default = "default_true")]
    pub learn_from_edits: bool,
    /// Phase 7.8: promote names seen on screen (screen-context ephemeral
    /// terms) into the learned lexicon. On by default.
    #[serde(default = "default_true")]
    pub learn_names_from_screen: bool,
    /// Phase 7.8: keep recorded audio for 7 days / 200 clips so self-tests
    /// can replay it. Off by default (privacy); the retention sweep is a
    /// later unit, this toggle only records the user's choice.
    #[serde(default)]
    pub keep_audio_for_self_tests: bool,
    /// Phase 5.6: lower the default output device's volume while a take is
    /// recording, so playback audio stops bleeding into the microphone.
    /// On by default; the level is restored on stop (and on crash, via the
    /// recovery spool state file).
    #[serde(default = "default_true")]
    pub duck_other_audio: bool,
    /// Phase 5.6: the output volume the ducker applies while recording
    /// (0.0-1.0 scalar; 0.2 = the plan's 20%).
    #[serde(default = "default_duck_level")]
    pub duck_level: f32,
    /// Phase 5.6: send the system play/pause media key when a take starts
    /// and again when it stops. macOS only, opt-in: there is no public API
    /// to tell whether anything is playing, so it is off by default.
    #[serde(default)]
    pub pause_media_while_dictating: bool,
}

/// A partial settings update: every field is `Option<...>`, and only the
/// `Some(...)` fields are applied. This is the non-clobbering alternative to
/// saving the whole `Settings` document: a caller that read a stale snapshot
/// can change one field without reverting every other field another save
/// changed in the meantime (the lost-update bug, B8).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsPatch {
    pub hotkey: Option<String>,
    pub recording_mode: Option<RecordingModeSetting>,
    pub selected_speech_model: Option<String>,
    pub speech_engine: Option<SpeechEngine>,
    pub language: Option<String>,
    pub input_device: Option<String>,
    pub restore_clipboard: Option<bool>,
    pub keep_text_on_clipboard: Option<bool>,
    pub auto_apply_transform: Option<bool>,
    pub show_tray_icon: Option<bool>,
    pub has_completed_onboarding: Option<bool>,
    pub selected_llm_model: Option<String>,
    pub typing_autotext_enabled: Option<bool>,
    pub autotext_timing: Option<AutotextTiming>,
    pub remove_filler_words: Option<bool>,
    pub filler_words: Option<Vec<String>>,
    pub filler_strength: Option<FillerStrengthSetting>,
    pub pill_position: Option<PillPositionSetting>,
    pub always_show_pill: Option<bool>,
    pub active_style_profile: Option<String>,
    pub scratchpad_enabled: Option<bool>,
    pub app_icon: Option<AppIcon>,
    pub transcripts_dir: Option<String>,
    pub selected_llm_provider: Option<String>,
    pub openai_base_url: Option<String>,
    pub openai_model: Option<String>,
    pub enable_developer_tab: Option<bool>,
    pub command_mode_enabled: Option<bool>,
    pub pill_style: Option<PillStyle>,
    pub vad_auto_stop: Option<bool>,
    pub vad_silence_ms: Option<u64>,
    pub polish_gate_enabled: Option<bool>,
    pub polish_gate_threshold_words: Option<usize>,
    pub polish_max_change_percent: Option<u8>,
    pub app_language_overrides: Option<std::collections::BTreeMap<String, String>>,
    pub restore_emoji: Option<bool>,
    pub spoken_emoji: Option<bool>,
    pub spoken_punctuation: Option<bool>,
    pub model_unload_delay_secs: Option<u64>,
    pub enabled_packs: Option<Vec<String>>,
    pub theme: Option<UiTheme>,
    pub reduce_motion: Option<bool>,
    pub live_preview_enabled: Option<bool>,
    pub play_recording_sounds: Option<bool>,
    pub recording_sound: Option<String>,
    pub cancel_hotkey: Option<String>,
    pub smart_insertion: Option<bool>,
    pub warm_engine_policy: Option<WarmEnginePolicy>,
    pub match_strictness: Option<MatchStrictness>,
    pub dictionary_enabled: Option<bool>,
    pub polish_rules: Option<std::collections::BTreeMap<String, bool>>,
    pub list_style: Option<ListStyleSetting>,
    pub polish_custom_instructions: Option<Vec<PolishInstruction>>,
    pub quick_add_hotkey: Option<String>,
    pub show_language_coach: Option<bool>,
}

impl SettingsPatch {
    /// Applies every `Some(...)` field onto `target`; `None` fields are left
    /// untouched. Returns `true` when anything changed.
    pub fn apply_to(&self, target: &mut Settings) -> bool {
        let mut changed = false;
        macro_rules! apply {
            ($field:ident) => {
                if let Some(v) = self.$field.clone() {
                    if target.$field != v {
                        target.$field = v;
                        changed = true;
                    }
                }
            };
        }
        apply!(hotkey);
        apply!(recording_mode);
        apply!(selected_speech_model);
        apply!(speech_engine);
        apply!(language);
        apply!(input_device);
        apply!(restore_clipboard);
        apply!(keep_text_on_clipboard);
        apply!(auto_apply_transform);
        apply!(show_tray_icon);
        apply!(has_completed_onboarding);
        apply!(selected_llm_model);
        apply!(typing_autotext_enabled);
        apply!(autotext_timing);
        apply!(remove_filler_words);
        apply!(filler_words);
        apply!(filler_strength);
        apply!(pill_position);
        apply!(always_show_pill);
        apply!(active_style_profile);
        apply!(scratchpad_enabled);
        apply!(app_icon);
        apply!(transcripts_dir);
        apply!(selected_llm_provider);
        apply!(openai_base_url);
        apply!(openai_model);
        apply!(enable_developer_tab);
        apply!(command_mode_enabled);
        apply!(pill_style);
        apply!(vad_auto_stop);
        apply!(vad_silence_ms);
        apply!(polish_gate_enabled);
        apply!(polish_gate_threshold_words);
        apply!(polish_max_change_percent);
        apply!(app_language_overrides);
        apply!(restore_emoji);
        apply!(spoken_emoji);
        apply!(spoken_punctuation);
        apply!(model_unload_delay_secs);
        apply!(enabled_packs);
        apply!(theme);
        apply!(reduce_motion);
        apply!(live_preview_enabled);
        apply!(play_recording_sounds);
        apply!(recording_sound);
        apply!(cancel_hotkey);
        apply!(smart_insertion);
        apply!(warm_engine_policy);
        apply!(match_strictness);
        apply!(dictionary_enabled);
        apply!(polish_rules);
        apply!(list_style);
        apply!(polish_custom_instructions);
        apply!(quick_add_hotkey);
        apply!(show_language_coach);
        changed
    }
}

fn default_polish_rules() -> std::collections::BTreeMap<String, bool> {
    ["concise", "clarity", "reorder", "structure", "tone"]
        .into_iter()
        .map(|k| (k.to_string(), true))
        .collect()
}

/// The default list style is dash bullets, matching the CORE_RULES prompt.
fn default_list_style() -> ListStyleSetting {
    ListStyleSetting::Bullets
}

/// Default word cap for the P5.1 polish gate (short-clean-skip path).
fn default_polish_gate_threshold_words() -> usize {
    8
}

/// Default ceiling on how much of a polished take may be replaced. Matches
/// the core validator's default so the setting and the guard cannot drift.
fn default_polish_max_change_percent() -> u8 {
    teletype_core::transforms::validator::DEFAULT_MAX_CHANGE_PERCENT
}

/// Styles are on out of the box: a new install polishes with the Concise style
/// instead of waiting for the user to discover the Style tab. An empty string
/// is the user's own "no style", and a saved settings file always carries the
/// key, so this only ever reaches a fresh install. It is inert on the shipped
/// default model, which takes its style from its own weights; `capability` on
/// `style_resolution` is what tells the Style tab that.
fn default_active_style_profile() -> String {
    "style-concise".into()
}

fn default_openai_base_url() -> String {
    "https://api.openai.com/v1".into()
}

fn default_openai_model() -> String {
    "gpt-4o-mini".into()
}

/// The default app icon is the light tile with the purple mark.
fn default_app_icon() -> AppIcon {
    AppIcon::White
}

/// The default UI theme follows the OS preference.
fn default_theme() -> UiTheme {
    UiTheme::System
}

/// The default recording pill style is the original Teletype design.
fn default_pill_style() -> PillStyle {
    PillStyle::Classic
}

/// Default pause duration before VAD auto-stop triggers.
fn default_vad_silence_ms() -> u64 {
    800
}

/// The default ASR engine. Whisper is the portable default; Parakeet is
/// macOS-only, so a Parakeet selection is never the cross-platform default.
fn default_speech_engine() -> SpeechEngine {
    SpeechEngine::Whisper
}

/// The default dictation model for one host, from facts instead of probes.
///
/// `fluid_available` must be `teletype_fluid::bridge_built()`: a build without
/// the Core ML bridge cannot load the Neural Engine rows, so a default that
/// names one is a default that cannot transcribe anything. Routing through
/// [`teletype_speech::catalog::recommended_speech_model_id`] keeps this default,
/// the model list's recommended row, and the migration below on one predicate.
///
/// `language` is `Settings::language` ("auto"/empty mean "no language set").
fn default_speech_model_id_for(
    platform: teletype_speech::catalog::Platform,
    language: Option<&str>,
    fluid_available: bool,
) -> &'static str {
    teletype_speech::catalog::recommended_speech_model_id(platform, language, fluid_available)
}

/// The default dictation model for this host.
///
/// The catalog's platform pick is the single source of truth: Core ML Ultra
/// on macOS 14+ Apple Silicon (only when this build links the FluidAudio
/// bridge), sherpa-onnx on Windows, ggml Parakeet on Intel and older macOS.
/// Delegating keeps the default from naming an engine the local binary cannot
/// load, which matters because this value is *written to disk* on a fresh
/// install: a default the host cannot load would brick the first dictation
/// before the user ever opened the Models screen.
///
/// `pub` so the model-delete path can reset the selection back to a model the
/// host can actually load after the user removes their current pick.
pub fn default_speech_model_id() -> &'static str {
    default_speech_model_id_for(
        teletype_speech::catalog::host_platform(),
        None,
        teletype_fluid::bridge_built(),
    )
}

/// Emoji restore is on by default (P3.2): the curated list is small and the
/// matching rule is conservative (word-boundary, ambiguity-safe).
fn default_true_emoji() -> bool {
    true
}

/// Default AutoText replacement timing: delimiter-completed.
fn default_autotext_timing() -> AutotextTiming {
    AutotextTiming::Delimiter
}

/// The default filler strength is Standard: hesitation fillers plus the
/// punctuation-gated discourse markers, the behavior the pipeline shipped
/// with before the setting existed.
fn default_filler_strength() -> FillerStrengthSetting {
    FillerStrengthSetting::Standard
}

/// Generic "on by default" serde default.
fn default_true() -> bool {
    true
}
/// Phase 5.6: the duck level default is the plan's 20%.
fn default_duck_level() -> f32 {
    0.2
}

/// Phase 5/6: instant start is off by default; the prewarm arming is wired
/// in a later unit and off is the safe no-op.
fn default_instant_start() -> String {
    "off".into()
}

/// P2.4: the Command Mode hotkey default is per-OS because the modifiers
/// differ (Option on macOS, Alt on Windows).
fn default_command_hotkey() -> String {
    if cfg!(target_os = "windows") {
        "Ctrl+Alt+Space".to_string()
    } else {
        "Ctrl+Option+Space".to_string()
    }
}

/// P8.4: the Paste-Last hotkey default is per-OS.
fn default_paste_last_hotkey() -> String {
    if cfg!(target_os = "windows") {
        "Ctrl+Alt+V".to_string()
    } else {
        "Cmd+Ctrl+V".to_string()
    }
}

impl Settings {
    /// One-time S9 migration: when a settings file predates the `speech_engine`
    /// field, infer the engine from the selected model's catalog entry and
    /// record it, so the persisted file carries the field on the next save.
    /// Returns `true` when a migration was applied.
    pub fn migrate(&mut self) -> bool {
        let entry = teletype_speech::catalog::find(&self.selected_speech_model);
        if let Some(m) = entry {
            let engine = if m.engine == teletype_speech::catalog::Engine::Parakeet
                || m.engine == teletype_speech::catalog::Engine::ParakeetCpp
            {
                SpeechEngine::Parakeet
            } else {
                SpeechEngine::Whisper
            };
            if self.speech_engine != engine {
                self.speech_engine = engine;
                return true;
            }
        }
        false
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: String::new(),
            recording_mode: RecordingModeSetting::Hold,
            selected_speech_model: default_speech_model_id().into(),
            speech_engine: default_speech_engine(),
            language: "auto".into(),
            input_device: String::new(),
            restore_clipboard: true,
            keep_text_on_clipboard: true,
            auto_apply_transform: true,
            show_tray_icon: true,
            has_completed_onboarding: false,
            selected_llm_model: "speakoflow-mini".into(),
            typing_autotext_enabled: default_true(),
            autotext_timing: default_autotext_timing(),
            remove_filler_words: true,
            filler_words: default_filler_words(),
            filler_strength: FillerStrengthSetting::Standard,
            pill_position: PillPositionSetting::BottomCenter,
            always_show_pill: false,
            active_style_profile: default_active_style_profile(),
            scratchpad_enabled: false,
            app_icon: default_app_icon(),
            transcripts_dir: String::new(),
            // SpeakoFlow Mini is the default transform model (local-server);
            // S1-mini remains available as an alternative in the Models screen.
            selected_llm_provider: "local-server".into(),
            openai_base_url: default_openai_base_url(),
            openai_model: default_openai_model(),
            enable_developer_tab: false,
            command_mode_enabled: false,
            pill_style: default_pill_style(),
            vad_auto_stop: false,
            vad_silence_ms: default_vad_silence_ms(),
            polish_gate_enabled: default_true(),
            polish_gate_threshold_words: default_polish_gate_threshold_words(),
            polish_max_change_percent: default_polish_max_change_percent(),
            app_language_overrides: std::collections::BTreeMap::new(),
            restore_emoji: true,
            spoken_emoji: default_true(),
            spoken_punctuation: default_true(),
            model_unload_delay_secs: 0,
            enabled_packs: Vec::new(),
            theme: default_theme(),
            reduce_motion: false,
            live_preview_enabled: default_true(),
            play_recording_sounds: default_true(),
            recording_sound: default_recording_sound(),
            cancel_hotkey: default_cancel_hotkey(),
            smart_insertion: default_true(),
            warm_engine_policy: default_warm_engine_policy(),
            match_strictness: default_match_strictness(),
            dictionary_enabled: default_true(),
            polish_rules: default_polish_rules(),
            list_style: default_list_style(),
            polish_custom_instructions: Vec::new(),
            quick_add_hotkey: String::new(),
            show_language_coach: default_true(),
            command_hotkey: default_command_hotkey(),
            paste_last_hotkey: default_paste_last_hotkey(),
            command_wake_phrase: default_true(),
            noise_reduction_enabled: default_true(),
            pending_notice: None,
            polish_whole_text_at_end: false,
            polish_skip_when_clean: default_true(),
            instant_start: default_instant_start(),
            prefer_builtin_mic_with_bt: default_true(),
            learn_from_edits: default_true(),
            learn_names_from_screen: default_true(),
            keep_audio_for_self_tests: false,
            duck_other_audio: default_true(),
            duck_level: default_duck_level(),
            pause_media_while_dictating: false,
            audio_conditioning: false,
        }
    }
}

/// Replaces a persisted speech-model selection this host cannot offer.
///
/// The same config directory can be synced or copied between machines, and
/// the catalog is platform-specific (ggml engines are macOS-only, Core ML
/// needs macOS 14+ Apple Silicon *and* a build that links the FluidAudio
/// bridge, sherpa is the Windows default). Keeping a pick the binary cannot
/// load turns the first dictation into a load error, so the selection is reset
/// to what [`teletype_speech::catalog::recommended_speech_model_id`] offers
/// this host, honoring the user's language. Picks of auxiliary models (the CTC
/// spotter) are replaced too: they cannot transcribe.
///
/// The swap is *told*, not silently made: the previous id is returned and
/// [`Settings::pending_notice`] is filled with a sentence naming both models,
/// which the Models screen shows until `dismiss_pending_notice` clears it. A
/// user whose careful choice changed under them must be able to see what
/// happened and pick it again on a machine that can run it.
///
/// `fluid_available` must be `teletype_fluid::bridge_built()`, otherwise a
/// Core ML pick on a bridge-less Mac counts as loadable and the first
/// dictation fails on load.
pub fn migrate_unavailable_speech_model(
    settings: &mut Settings,
    platform: teletype_speech::catalog::Platform,
    fluid_available: bool,
) -> Option<String> {
    let loadable = teletype_speech::catalog::find(&settings.selected_speech_model)
        .is_some_and(|m| m.offered_on(platform, fluid_available));
    if loadable {
        return None;
    }
    let next =
        default_speech_model_id_for(platform, Some(settings.language.as_str()), fluid_available);
    let previous = std::mem::replace(&mut settings.selected_speech_model, next.to_string());
    let notice = speech_migration_notice(&previous, next, platform, fluid_available);
    settings.pending_notice = Some(match settings.pending_notice.take() {
        // Keep an earlier migration notice (EG-1) instead of overwriting it:
        // two facts happened, the banner can carry two sentences.
        Some(existing) => format!("{existing} {notice}"),
        None => notice,
    });
    Some(previous)
}

/// One human sentence for [`Settings::pending_notice`], naming the model the
/// user had and the one Teletype switched to, with the reason that is actually
/// true for this host. Catalog labels, never raw ids, because the user picked
/// the label.
pub fn speech_migration_notice(
    previous: &str,
    next: &str,
    platform: teletype_speech::catalog::Platform,
    fluid_available: bool,
) -> String {
    use teletype_speech::catalog::Engine;
    let new_name = teletype_speech::catalog::find(next)
        .map(|m| m.name)
        .unwrap_or(next);
    let Some(old) = teletype_speech::catalog::find(previous) else {
        // Empty or unknown id: a damaged or older settings file, not a choice
        // the user made.
        return format!(
            "No dictation model was selected that this computer can run, so Teletype is using {new_name}."
        );
    };
    let old_name = old.name;
    let reason = if old.auxiliary {
        format!("{old_name} is a vocabulary helper, not a dictation model")
    } else if !old.available_on(platform) {
        format!("{old_name} cannot run on this computer")
    } else if old.engine == Engine::ParakeetCoreml && !fluid_available {
        format!("{old_name} needs the Neural Engine build this Teletype does not include")
    } else {
        format!("{old_name} is not available in this version of Teletype")
    };
    format!("{reason}, so Teletype switched dictation to {new_name}.")
}

/// Clears the one-time [`Settings::pending_notice`] banner after the user has
/// read it, and persists the clear. Without this the migration sentence would
/// come back on every launch.
#[tauri::command]
pub async fn dismiss_pending_notice(state: tauri::State<'_, crate::AppState>) -> CommandResult<()> {
    let mut settings = state.settings();
    settings.pending_notice = None;
    state.replace_settings(settings)
}

/// P3.9 migration: EG-1 is gone from the catalog (its license forbids use in
/// competing products). A user who had it selected moves to SpeakoFlow Mini,
/// the default cleanup model, with a one-time notice the Models screen shows
/// and clears. Returns true when the settings were changed.
pub fn migrate_removed_models(settings: &mut Settings) -> bool {
    if settings.selected_llm_model == "eg-1" {
        settings.selected_llm_model = "speakoflow-mini".to_string();
        settings.pending_notice =
            Some("EG-1 was removed from Teletype. You are now using SpeakoFlow Mini.".to_string());
        return true;
    }
    // Until the Models rail split on `polish_capable`, NeuDecide (the Command
    // Mode decider) and Needle 3 (extraction) could be picked as the polish
    // engine, and NeuDecide even wore the Recommended badge. One of those left
    // in `selected_llm_model` loads without complaint and then returns every
    // preset's input untouched, which reads as "polish is broken". Move it to
    // the shipped default, the same target the EG-1 migration uses.
    //
    // Needle 3 is no longer in the catalog, so the lookup misses for it; the
    // id stays in the list so a saved needle3 selection is still refused as
    // a stale pick rather than loaded.
    const REMOVED_DECIDERS: &[&str] = &["needle3"];
    let stale_id = settings.selected_llm_model.as_str();
    let entry = teletype_inference::catalog::find(stale_id);
    let removed_decider = REMOVED_DECIDERS.contains(&stale_id);
    if let Some(entry) = entry {
        if !entry.polish_capable {
            settings.pending_notice = Some(format!(
                "{} is a Command Mode model, not a polish engine. Teletype switched text cleanup \
                 to SpeakoFlow Mini.",
                entry.name
            ));
            settings.selected_llm_model = "speakoflow-mini".to_string();
            return true;
        }
    } else if removed_decider {
        settings.pending_notice = Some(
            "Needle 3 is no longer offered by Teletype and was not a \
             polish engine. Teletype switched text cleanup to SpeakoFlow Mini."
                .to_string(),
        );
        settings.selected_llm_model = "speakoflow-mini".to_string();
        return true;
    }
    false
}

/// macOS system sounds offered as dictation cues.
///
/// These are files that ship with every Mac (`/System/Library/Sounds`), so the
/// picker has real, working audio with no bundled assets and nothing to keep
/// in sync with an app release. They are short and distinct enough to tell
/// start from stop by ear, which is the only thing the cue has to do.
pub const RECORDING_SOUND_NAMES: &[&str] = &[
    "Submarine",
    "Tink",
    "Pop",
    "Ping",
    "Glass",
    "Hero",
    "Funk",
    "Blow",
];

fn default_recording_sound() -> String {
    "Tink".into()
}

/// The cancel binding, separate from the record binding.
///
/// Escape is the shipped default because it is the one chord a user reaches
/// for mid-recording without looking, and cancelling is the only action safe
/// to bind that way.
fn default_cancel_hotkey() -> String {
    "Escape".into()
}

/// `off` | `10` | `30` | `60` | `always`
fn default_warm_engine_policy() -> WarmEnginePolicy {
    WarmEnginePolicy::Sec30
}

/// `loose` | `standard` | `strict`
fn default_match_strictness() -> MatchStrictness {
    MatchStrictness::Standard
}

pub const MATCH_STRICTNESS_LEVELS: &[(&str, f32)] =
    &[("loose", 0.72), ("standard", 0.80), ("strict", 0.92)];

/// Resolves the similarity floor for a stored strictness level.
///
/// `Standard` (and `Unknown`) map to the pipeline's existing default rather
/// than restating it, so changing the default in one place cannot leave this
/// returning a stale number. Prefer [`MatchStrictness::floor`]; this free
/// function stays for the IPC-adjacent call sites that still pass ids.
pub fn match_strictness_floor(level: MatchStrictness) -> Option<f32> {
    level.floor()
}

/// The default filler words removed from transcripts when
/// `remove_filler_words` is enabled.
fn default_filler_words() -> Vec<String> {
    [
        "um", "uh", "er", "ah", "eh", "umm", "uhh", "err", "ahh", "ehh", "hmm", "hm", "mm", "mmm",
        "erm", "urm", "ugh",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    os: &'static str,
    version: String,
    setup_notes: Vec<String>,
    permissions: Vec<Permission>,
}

/// The bundled app icons, keyed by the id stored in settings.
pub const APP_ICONS: &[(&str, &str, &[u8])] = &[
    (
        "purple",
        "Teletype (purple)",
        include_bytes!("../icons/app-icon-purple.png"),
    ),
    (
        "white",
        "Teletype (light)",
        include_bytes!("../icons/app-icon-white.png"),
    ),
];

/// One Ollama model as reported by `GET /api/tags`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModelInfo {
    pub name: String,
    pub size: u64,
    pub modified: String,
    pub verdict: String,
    pub verdict_note: String,
}

/// G0.5: reports whether the selected dictation model is installed and ready.
/// Used by onboarding to confirm "you can start speaking" before finishing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelReady {
    /// The selected model id.
    pub id: String,
    /// True when the model file is present on disk.
    pub downloaded: bool,
    /// True when `downloaded` and the file is non-empty (a 0-byte file is not usable).
    pub ready: bool,
    /// Short user-facing line, e.g. "Parakeet v3 is ready" or
    /// "Model not downloaded yet".
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped default must be the catalog's platform pick, not a
    /// hardcoded id: otherwise a macOS 14+ Apple Silicon Mac keeps the ggml
    /// build while the Neural Engine build sits unused, and Windows points at
    /// an engine the binary cannot load.
    #[test]
    fn default_speech_model_is_the_platform_pick() {
        let expected = teletype_speech::catalog::recommended_speech_model_id(
            teletype_speech::catalog::host_platform(),
            None,
            teletype_fluid::bridge_built(),
        );
        assert_eq!(default_speech_model_id(), expected);
        // The default is persisted on a fresh install, so it must be a model
        // this exact binary can load and list.
        let entry = teletype_speech::catalog::find(default_speech_model_id())
            .expect("the shipped default is a catalog entry");
        assert!(
            entry.offered_on(
                teletype_speech::catalog::host_platform(),
                teletype_fluid::bridge_built()
            ),
            "default_speech_model_id persisted '{}', which this host cannot offer",
            default_speech_model_id()
        );
    }

    /// The gate the whole task turns on: the persisted default must never name
    /// a model this build cannot load, and the settings default is what a
    /// fresh install writes to disk.
    #[test]
    fn default_speech_model_never_names_a_model_the_host_cannot_offer() {
        use teletype_speech::catalog::Platform;
        for platform in [
            Platform::MacosAppleSilicon { major: 26 },
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosAppleSilicon { major: 13 },
            // A failed `sw_vers` probe: must degrade to the ggml build.
            Platform::MacosAppleSilicon { major: 0 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ] {
            for language in [None, Some("auto"), Some("en"), Some("ar")] {
                for fluid in [true, false] {
                    let id = default_speech_model_id_for(platform, language, fluid);
                    assert!(
                        teletype_speech::catalog::find(id)
                            .is_some_and(|m| m.offered_on(platform, fluid)),
                        "default '{id}' on {platform:?} for {language:?} with bridge={fluid} \
                         is not a model this build can run"
                    );
                }
            }
        }

        // The specific regression: an Apple Silicon macOS 14+ machine whose
        // binary has no FluidAudio bridge used to persist `parakeet-ultra-coreml`
        // (from `pick_engine`, which cannot see the bridge) and then fail the
        // first dictation on load.
        let mac14 = Platform::MacosAppleSilicon { major: 14 };
        assert_eq!(
            default_speech_model_id_for(mac14, None, false),
            "parakeet-tdt-v3"
        );
        assert_eq!(
            default_speech_model_id_for(mac14, None, true),
            "parakeet-ultra-coreml"
        );
        assert_eq!(
            default_speech_model_id_for(Platform::Windows, None, false),
            "sherpa-parakeet-v3-int8"
        );
    }

    #[test]
    fn settings_default_selects_a_model_this_host_can_offer() {
        // `Settings::default()` is what `JsonStore::load` falls back to, so its
        // own field must satisfy the same gate.
        let s = Settings::default();
        let entry = teletype_speech::catalog::find(&s.selected_speech_model)
            .expect("the settings default is a catalog entry");
        assert!(entry.offered_on(
            teletype_speech::catalog::host_platform(),
            teletype_fluid::bridge_built()
        ));
    }

    #[test]
    fn command_mode_toggles_via_camelcase_json() {
        // The Command Mode screen writes settings.commandModeEnabled and the
        // whole-document save_settings path must round-trip it through
        // camelCase JSON. The field is default-off.
        let s = Settings::default();
        assert!(!s.command_mode_enabled, "default off");
        let mut json = serde_json::to_value(&s).unwrap();
        assert!(
            json.get("commandModeEnabled").is_some(),
            "camelCase key present"
        );
        assert_eq!(json["commandModeEnabled"], false);
        // Simulate the UI setting it true, as save_settings would deserialize.
        json["commandModeEnabled"] = serde_json::json!(true);
        let back: Settings = serde_json::from_value(json).unwrap();
        assert!(back.command_mode_enabled, "camelCase value applied");
        // An older file without the key still parses and stays off.
        let mut legacy = serde_json::to_value(Settings::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("commandModeEnabled");
        let legacy_back: Settings = serde_json::from_value(legacy).unwrap();
        assert!(!legacy_back.command_mode_enabled, "missing key -> off");
    }

    #[test]
    fn new_settings_round_trip_through_json() {
        // Every new field carries a #[serde(default)], so an old settings.json
        // must still parse — that is the whole reason the defaults exist.
        let legacy = serde_json::json!({ "hotkey": "Cmd+Shift+Space" });
        let s: Settings = serde_json::from_value(legacy).expect("legacy settings must parse");
        assert_eq!(s.cancel_hotkey.as_str(), "Escape");
        assert_eq!(s.recording_sound.as_str(), "Tink");
        assert_eq!(s.warm_engine_policy, WarmEnginePolicy::Sec30);
        assert_eq!(s.match_strictness, MatchStrictness::Standard);
        assert!(s.smart_insertion && s.play_recording_sounds);
        // Phase 5.6: the ducking fields land with their own defaults.
        assert!(s.duck_other_audio);
        assert!((s.duck_level - 0.2).abs() < f32::EPSILON);
        assert!(!s.pause_media_while_dictating);
    }

    #[test]
    fn new_settings_survive_a_save_cycle() {
        let s = Settings {
            smart_insertion: false,
            warm_engine_policy: WarmEnginePolicy::Always,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(!back.smart_insertion);
        assert_eq!(back.warm_engine_policy, WarmEnginePolicy::Always);
    }

    #[test]
    fn migration_infers_engine_from_model_id() {
        // A pre-S9 settings file predates `speech_engine`; the one-time
        // migration must infer it from the selected model's catalog entry.
        let s = Settings {
            selected_speech_model: "parakeet-tdt-v3".into(),
            // As serde would give a missing field.
            speech_engine: SpeechEngine::Whisper,
            ..Default::default()
        };
        let mut s = s;
        assert!(s.migrate());
        assert_eq!(s.speech_engine, SpeechEngine::Parakeet);

        let mut w = Settings {
            selected_speech_model: "large-v3-turbo-q5".into(),
            speech_engine: SpeechEngine::Parakeet,
            ..Default::default()
        };
        assert!(w.migrate());
        assert_eq!(w.speech_engine, SpeechEngine::Whisper);

        // No-op when the engine already matches the model.
        let s = Settings {
            selected_speech_model: "parakeet-tdt-v3".into(),
            speech_engine: SpeechEngine::Parakeet,
            ..Default::default()
        };
        let mut s = s;
        assert!(!s.migrate());
    }

    #[test]
    fn migration_keeps_setting_for_unknown_model_id() {
        // A model id outside the catalog cannot be inferred; the stored
        // `speech_engine` is the fallback and must survive untouched.
        let s = Settings {
            selected_speech_model: "my-custom-model".into(),
            speech_engine: SpeechEngine::Parakeet,
            ..Default::default()
        };
        let mut s = s;
        assert!(!s.migrate());
        assert_eq!(s.speech_engine, SpeechEngine::Parakeet);
    }

    #[test]
    fn migrate_unavailable_speech_model_resets_a_pick_the_host_cannot_load() {
        use teletype_speech::catalog::Platform;

        // A macOS pick carried to Windows: the binary cannot load ggml there.
        let mut s = Settings {
            selected_speech_model: "parakeet-ultra-coreml".into(),
            ..Default::default()
        };
        assert_eq!(
            migrate_unavailable_speech_model(&mut s, Platform::Windows, true).as_deref(),
            Some("parakeet-ultra-coreml")
        );
        assert_eq!(s.selected_speech_model, "sherpa-parakeet-v3-int8");
        // The user is told, in words that name both models.
        let notice = s.pending_notice.clone().expect("migration sets a notice");
        assert!(
            notice.contains("Parakeet Ultra (Neural Engine)"),
            "names the old model: {notice}"
        );
        assert!(
            notice.contains("Parakeet v3 (sherpa-onnx)"),
            "names the new model: {notice}"
        );

        // A loadable pin is left alone, and writes no notice.
        let mut s = Settings {
            selected_speech_model: "parakeet-tdt-v3".into(),
            ..Default::default()
        };
        assert_eq!(
            migrate_unavailable_speech_model(&mut s, Platform::MacosIntel, true),
            None
        );
        assert_eq!(s.selected_speech_model, "parakeet-tdt-v3");
        assert!(s.pending_notice.is_none());

        // The CTC spotter is not a dictation model; replace it with the pick.
        let mut s = Settings {
            selected_speech_model: "parakeet-ctc-110m-coreml".into(),
            ..Default::default()
        };
        assert!(migrate_unavailable_speech_model(
            &mut s,
            Platform::MacosAppleSilicon { major: 15 },
            true
        )
        .is_some());
        assert_eq!(s.selected_speech_model, "parakeet-ultra-coreml");
        assert!(s
            .pending_notice
            .as_deref()
            .expect("spotter notice")
            .contains("vocabulary helper"));

        // An empty selection (fresh or damaged file) gets the default.
        let mut s = Settings {
            selected_speech_model: String::new(),
            ..Default::default()
        };
        assert!(migrate_unavailable_speech_model(&mut s, Platform::Windows, true).is_some());
        assert_eq!(s.selected_speech_model, "sherpa-parakeet-v3-int8");
        assert!(s
            .pending_notice
            .as_deref()
            .expect("empty pick notice")
            .starts_with("No dictation model was selected"));
    }

    /// The bridge is a build fact `pick_engine` cannot see, so the migration
    /// must take it as an argument: a Core ML pick on a bridge-less Mac used to
    /// count as loadable and then failed on the first dictation.
    #[test]
    fn migrate_unavailable_speech_model_honors_the_fluid_bridge() {
        use teletype_speech::catalog::Platform;
        let mac14 = Platform::MacosAppleSilicon { major: 14 };

        // With the bridge the Ultra pick is honest, so it survives.
        let mut s = Settings {
            selected_speech_model: "parakeet-ultra-coreml".into(),
            ..Default::default()
        };
        assert_eq!(migrate_unavailable_speech_model(&mut s, mac14, true), None);
        assert!(s.pending_notice.is_none());

        // Without it the pick cannot load: reset to the ggml build and say why.
        let mut s = Settings {
            selected_speech_model: "parakeet-ultra-coreml".into(),
            ..Default::default()
        };
        assert_eq!(
            migrate_unavailable_speech_model(&mut s, mac14, false).as_deref(),
            Some("parakeet-ultra-coreml")
        );
        assert_eq!(s.selected_speech_model, "parakeet-tdt-v3");
        let notice = s.pending_notice.clone().expect("bridge notice");
        assert!(notice.contains("Neural Engine build"), "{notice}");
        assert!(notice.contains("Parakeet v3"), "{notice}");

        // Idempotent: the second run finds a loadable pick and rewrites nothing,
        // so the banner cannot stack up on every launch.
        let before = s.clone();
        assert_eq!(migrate_unavailable_speech_model(&mut s, mac14, false), None);
        assert_eq!(s.selected_speech_model, before.selected_speech_model);
        assert_eq!(s.pending_notice, before.pending_notice);
    }

    /// Migrating must not lose the language the user dictates in: the
    /// replacement is the same recommendation the model list shows.
    #[test]
    fn migrate_unavailable_speech_model_keeps_the_users_language_in_mind() {
        use teletype_speech::catalog::Platform;
        let mac14 = Platform::MacosAppleSilicon { major: 14 };

        // Arabic on a bridge-less Mac: Ultra was wrong twice over, and the
        // ggml Parakeet cannot do Arabic either.
        let mut s = Settings {
            selected_speech_model: "parakeet-ultra-coreml".into(),
            language: "ar".into(),
            ..Default::default()
        };
        assert!(migrate_unavailable_speech_model(&mut s, mac14, false).is_some());
        assert_eq!(s.selected_speech_model, "large-v3-turbo-q5");

        // A covered language keeps the platform fallback.
        let mut s = Settings {
            selected_speech_model: "parakeet-ultra-coreml".into(),
            language: "de".into(),
            ..Default::default()
        };
        assert!(migrate_unavailable_speech_model(&mut s, mac14, false).is_some());
        assert_eq!(s.selected_speech_model, "parakeet-tdt-v3");

        // "auto" is no language set, so the platform default stands.
        let mut s = Settings {
            selected_speech_model: "parakeet-ultra-coreml".into(),
            language: "auto".into(),
            ..Default::default()
        };
        assert!(migrate_unavailable_speech_model(&mut s, mac14, false).is_some());
        assert_eq!(s.selected_speech_model, "parakeet-tdt-v3");

        // Windows has no Whisper, so an Arabic user stays on sherpa.
        let mut s = Settings {
            selected_speech_model: "parakeet-tdt-v3".into(),
            language: "ar".into(),
            ..Default::default()
        };
        assert!(migrate_unavailable_speech_model(&mut s, Platform::Windows, false).is_some());
        assert_eq!(s.selected_speech_model, "sherpa-parakeet-v3-int8");
    }

    #[test]
    fn pending_notice_serializes_as_camelcase_and_round_trips() {
        // The UI reads `settings.pendingNotice` and clears it through
        // `dismiss_pending_notice`; the wire key is part of that contract, and
        // a file without the key must still parse (older builds).
        let mut s = Settings {
            pending_notice: Some("Teletype switched dictation to Parakeet v3.".into()),
            ..Default::default()
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json["pendingNotice"].as_str(),
            Some("Teletype switched dictation to Parakeet v3.")
        );
        assert!(json.get("pending_notice").is_none());
        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back.pending_notice, s.pending_notice);

        // Cleared notices serialize as null, and a legacy file with no key at
        // all deserializes to None.
        s.pending_notice = None;
        let mut json = serde_json::to_value(&s).unwrap();
        assert!(json["pendingNotice"].is_null());
        json.as_object_mut().unwrap().remove("pendingNotice");
        let legacy: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.pending_notice, None);
    }

    /// A notice from an earlier migration (EG-1) is kept, not overwritten: the
    /// user still needs to read both facts.
    #[test]
    fn speech_migration_appends_to_an_existing_notice() {
        let mut s = Settings {
            selected_speech_model: "parakeet-ultra-coreml".into(),
            pending_notice: Some("EG-1 was removed from Teletype.".into()),
            ..Default::default()
        };
        assert!(migrate_unavailable_speech_model(
            &mut s,
            teletype_speech::catalog::host_platform(),
            false
        )
        .is_some());
        let notice = s.pending_notice.expect("notice");
        assert!(
            notice.starts_with("EG-1 was removed from Teletype."),
            "{notice}"
        );
        assert!(notice.contains("switched dictation"), "{notice}");
    }

    #[test]
    fn speech_migration_notice_uses_catalog_names_and_never_ids() {
        use teletype_speech::catalog::Platform;
        let mac = Platform::MacosAppleSilicon { major: 15 };
        let text = speech_migration_notice(
            "parakeet-tdt-v3",
            "sherpa-parakeet-v3-int8",
            Platform::Windows,
            true,
        );
        assert_eq!(
            text,
            "Parakeet v3 cannot run on this computer, so Teletype switched dictation to Parakeet v3 (sherpa-onnx)."
        );
        // An id outside the catalog (a custom file, or a removed model) is
        // described as a missing selection rather than quoting a raw id.
        let unknown = speech_migration_notice("no-such-model", "parakeet-tdt-v3", mac, true);
        assert!(
            unknown.starts_with("No dictation model was selected"),
            "{unknown}"
        );
        assert!(!unknown.contains("no-such-model"), "{unknown}");
        // The replacement always appears under its own label.
        assert!(unknown.contains("Parakeet v3"), "{unknown}");
    }

    #[test]
    fn migrate_removed_models_switches_the_removed_pick_and_notices_once() {
        let mut s = Settings {
            selected_llm_model: "eg-1".into(),
            ..Default::default()
        };
        assert!(migrate_removed_models(&mut s));
        assert_eq!(s.selected_llm_model, "speakoflow-mini");
        assert!(s.pending_notice.is_some());

        // Idempotent: a second run changes nothing and writes no new notice.
        let notice = s.pending_notice.clone();
        assert!(!migrate_removed_models(&mut s));
        assert_eq!(s.pending_notice, notice);

        // Any other selection is left alone.
        let mut other = Settings {
            selected_llm_model: "qwen2.5-3b".into(),
            ..Default::default()
        };
        assert!(!migrate_removed_models(&mut other));
        assert!(other.pending_notice.is_none());
        assert_eq!(other.selected_llm_model, "qwen2.5-3b");
    }

    #[test]
    fn migrate_removed_models_moves_a_decider_out_of_the_polish_slot() {
        // The two rows that were selectable as polish engines before the rail
        // split. A saved pick of either has to move, or polish silently no-ops.
        for stale in ["neudecide-q4", "needle3"] {
            let mut s = Settings {
                selected_llm_model: stale.into(),
                ..Default::default()
            };
            assert!(migrate_removed_models(&mut s), "{stale} should migrate");
            assert_eq!(s.selected_llm_model, "speakoflow-mini");
            let notice = s.pending_notice.clone().unwrap_or_default();
            assert!(notice.contains("not a polish engine"), "{notice}");
            // Idempotent, and no second notice.
            assert!(!migrate_removed_models(&mut s));
            assert_eq!(s.pending_notice, notice.into());
        }
    }

    #[test]
    fn match_strictness_only_resolves_the_levels_it_names() {
        assert_eq!(MatchStrictness::Loose.floor(), Some(0.72));
        assert_eq!(MatchStrictness::Strict.floor(), Some(0.92));
        // "standard" returns None on purpose: the caller keeps the pipeline's
        // own default, so the two cannot drift apart.
        assert_eq!(MatchStrictness::Standard.floor(), None);
        // Unknown levels must not silently become a floor.
        assert_eq!(MatchStrictness::Unknown.floor(), None);
        assert_eq!(match_strictness_floor(MatchStrictness::Loose), Some(0.72));
        assert_eq!(match_strictness_floor(MatchStrictness::Standard), None);
        // Command inputs reject unknown levels instead of defaulting.
        assert!("nonsense".parse::<MatchStrictness>().is_err());
        assert!("".parse::<MatchStrictness>().is_err());
    }

    #[test]
    fn typed_settings_enums_keep_the_json_wire_values() {
        // The UI sends plain strings; the enums must serialize byte-identical.
        let s = Settings::default();
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["recordingMode"], "hold");
        assert_eq!(json["pillPosition"], "bottomCenter");
        assert_eq!(json["listStyle"], "bullets");
        assert_eq!(json["matchStrictness"], "standard");
        assert_eq!(json["appIcon"], "white");
        assert_eq!(json["theme"], "system");
        assert_eq!(json["autotextTiming"], "delimiter");
        assert_eq!(json["pillStyle"], "classic");
        assert_eq!(json["warmEnginePolicy"], "30");

        // And every documented wire value parses back to its variant.
        let back: Settings = serde_json::from_value(serde_json::json!({
            "recordingMode": "toggle",
            "pillPosition": "topRight",
            "listStyle": "numbered",
            "matchStrictness": "strict",
            "appIcon": "purple",
            "theme": "dark",
            "autotextTiming": "immediate",
            "pillStyle": "well",
            "warmEnginePolicy": "always",
        }))
        .unwrap();
        assert_eq!(back.recording_mode, RecordingModeSetting::Toggle);
        assert_eq!(back.pill_position, PillPositionSetting::TopRight);
        assert_eq!(back.list_style, ListStyleSetting::Numbered);
        assert_eq!(back.match_strictness, MatchStrictness::Strict);
        assert_eq!(back.app_icon, AppIcon::Purple);
        assert_eq!(back.theme, UiTheme::Dark);
        assert_eq!(back.autotext_timing, AutotextTiming::Immediate);
        assert_eq!(back.pill_style, PillStyle::Well);
        assert_eq!(back.warm_engine_policy, WarmEnginePolicy::Always);

        // The legacy "numbered-list" alias the lenient parser accepted.
        let alias: Settings = serde_json::from_value(serde_json::json!({
            "listStyle": "numbered-list",
        }))
        .unwrap();
        assert_eq!(alias.list_style, ListStyleSetting::Numbered);

        // A future/typo value degrades to Unknown, never fails the load, and
        // resolves to the previous default behavior.
        let future: Settings = serde_json::from_value(serde_json::json!({
            "recordingMode": "brainwaves",
            "pillPosition": "farSideOfTheMoon",
            "listStyle": "emoji",
            "matchStrictness": "super-strict",
            "appIcon": "rainbow",
            "theme": "neon",
            "autotextTiming": "yesterday",
            "pillStyle": "brutalist",
            "warmEnginePolicy": "sometimes",
        }))
        .unwrap();
        assert_eq!(future.recording_mode, RecordingModeSetting::Unknown);
        assert_eq!(future.pill_position, PillPositionSetting::Unknown);
        assert_eq!(future.list_style, ListStyleSetting::Unknown);
        assert_eq!(future.match_strictness, MatchStrictness::Unknown);
        assert_eq!(future.app_icon, AppIcon::Unknown);
        assert_eq!(future.theme, UiTheme::Unknown);
        assert_eq!(future.autotext_timing, AutotextTiming::Unknown);
        assert_eq!(future.pill_style, PillStyle::Unknown);
        assert_eq!(future.warm_engine_policy, WarmEnginePolicy::Unknown);
        assert_eq!(
            future.recording_mode.resolve(),
            teletype_core::state::RecordingMode::Hold
        );
        assert_eq!(
            future.pill_position.resolve(),
            teletype_core::state::PillPosition::BottomCenter
        );
        assert_eq!(
            future.list_style.resolve(),
            teletype_core::transforms::ListStyle::Bullets
        );
        assert_eq!(future.match_strictness.floor(), None);
        assert_eq!(future.app_icon.as_id(), "white");
        assert_eq!(future.theme.as_id(), "system");
        assert_eq!(future.pill_style.as_id(), "default");
        assert_eq!(future.warm_engine_policy.as_id(), "30");
    }

    #[test]
    fn command_error_serializes_as_code_and_message() {
        let err = CommandError::not_found("Word not found");
        assert_eq!(err.code, "E_NOT_FOUND");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "E_NOT_FOUND");
        assert_eq!(json["message"], "Word not found");
        // String conversions keep the message readable in logs.
        let from_string: CommandError = "boom".to_string().into();
        assert_eq!(from_string.code, "E_INTERNAL");
        assert_eq!(String::from(from_string), "boom");
    }

    #[test]
    fn strictness_levels_are_ordered_and_complete() {
        let names: Vec<&str> = MATCH_STRICTNESS_LEVELS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["loose", "standard", "strict"]);
        let floors: Vec<f32> = MATCH_STRICTNESS_LEVELS.iter().map(|(_, f)| *f).collect();
        assert!(floors.windows(2).all(|w| w[0] < w[1]), "must get stricter");
    }

    #[test]
    fn decision_provider_upgrades_only_when_a_new_model_appears() {
        // Complete provider: never rebuild, however often this runs.
        assert!(!transforms::decision_upgrade_needed(
            "tinydecide+needle3",
            true,
            true
        ));
        // No new files: never rebuild.
        assert!(!transforms::decision_upgrade_needed(
            "tinydecide+llm-fallback",
            true,
            false
        ));
        assert!(!transforms::decision_upgrade_needed(
            "llm-fallback",
            false,
            false
        ));
        assert!(!transforms::decision_upgrade_needed("needle3", false, true));
        // P2.7: Needle 3 downloaded mid-session must upgrade the LLM
        // fallback; a late TinyDecide must reach a provider built without it.
        assert!(transforms::decision_upgrade_needed(
            "tinydecide+llm-fallback",
            true,
            true
        ));
        assert!(transforms::decision_upgrade_needed(
            "llm-fallback",
            true,
            false
        ));
        assert!(transforms::decision_upgrade_needed(
            "llm-fallback",
            false,
            true
        ));
        assert!(transforms::decision_upgrade_needed("needle3", true, true));
    }

    #[test]
    fn only_shipped_sounds_are_offered() {
        // The picker reads these names, so a name that is not a real macOS
        // system sound would produce a preview that always fails.
        for name in RECORDING_SOUND_NAMES {
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()),
                "sound name must be a bare filesystem-safe token: {name}"
            );
        }
        let mut sorted = RECORDING_SOUND_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), RECORDING_SOUND_NAMES.len(), "no duplicates");
        assert!(
            RECORDING_SOUND_NAMES.contains(&default_recording_sound().as_str()),
            "the default cue must be one of the offered sounds"
        );
    }

    /// BUG-06. The keychain account has to be the real host, and a URL's
    /// userinfo must never be mistaken for it. The old hand-rolled parser
    /// produced the account `user` for
    /// `https://user:pass@evil.example/v1`, which then missed and fell through
    /// to the OpenAI key, sending it to `evil.example`.
    #[test]
    fn secret_account_is_the_host_not_the_userinfo() {
        for (url, want) in [
            ("https://api.openai.com/v1", "api.openai.com"),
            ("https://openrouter.ai/api/v1", "openrouter.ai"),
            ("http://127.0.0.1:11434/v1", "127.0.0.1"),
            ("http://localhost:8080/v1", "localhost"),
            ("https://user:pass@evil.example/v1", "evil.example"),
            ("https://user@evil.example/v1", "evil.example"),
            ("https://API.OpenAI.com/v1", "api.openai.com"),
        ] {
            assert_eq!(secret_account_for_url(url), want, "url: {url}");
        }
        // A malformed base URL must not silently become a usable account.
        assert_eq!(secret_account_for_url("not a url"), "unknown");
    }

    /// The legacy `openai` keychain account is only honoured when the
    /// configured base URL really is OpenAI. Unconditionally, the user's
    /// OpenAI key was sent to whatever third-party host they had configured.
    #[test]
    fn the_legacy_openai_key_is_only_used_for_openai() {
        for url in [
            "https://api.openai.com/v1",
            "https://openai.com/v1",
            "https://eu.api.openai.com/v1",
            "https://openai.com",
        ] {
            assert!(is_openai_base_url(url), "should count as OpenAI: {url}");
        }
        // Notably: a local endpoint is never OpenAI, so a key saved for Ollama
        // can never be the OpenAI key by way of a loopback base URL.
        for url in [
            "https://openrouter.ai/api/v1",
            "https://api.groq.com/openai/v1",
            "http://127.0.0.1:11434/v1",
            "http://localhost:1234/v1",
            "https://api.openai.com.evil.example/v1",
            "https://evil.example/v1",
            "not a url",
        ] {
            assert!(
                !is_openai_base_url(url),
                "must NOT be able to reach the openai key: {url}"
            );
        }
    }

    #[test]
    fn filler_strength_wire_values_round_trip() {
        // The UI sends "light" / "standard" / "strict"; the enum must
        // serialize byte-identical and parse each wire value back.
        assert_eq!(
            serde_json::to_value(FillerStrengthSetting::Light).unwrap(),
            "light"
        );
        assert_eq!(
            serde_json::to_value(FillerStrengthSetting::Standard).unwrap(),
            "standard"
        );
        assert_eq!(
            serde_json::to_value(FillerStrengthSetting::Strict).unwrap(),
            "strict"
        );
        assert_eq!(
            serde_json::from_str::<FillerStrengthSetting>("\"light\"").unwrap(),
            FillerStrengthSetting::Light
        );
        assert_eq!(
            serde_json::from_str::<FillerStrengthSetting>("\"standard\"").unwrap(),
            FillerStrengthSetting::Standard
        );
        assert_eq!(
            serde_json::from_str::<FillerStrengthSetting>("\"strict\"").unwrap(),
            FillerStrengthSetting::Strict
        );
        // An unknown string (a newer build) degrades to Unknown, never fails
        // the load, and resolves to the shipped Standard behavior.
        assert_eq!(
            serde_json::from_str::<FillerStrengthSetting>("\"yesterday\"").unwrap(),
            FillerStrengthSetting::Unknown
        );
        assert_eq!(
            FillerStrengthSetting::Unknown.to_core(),
            FillerStrengthSetting::Standard.to_core()
        );
        // The three levels are strictly ordered, so a swapped match arm would
        // change what the user hears removed, not just which label is stored.
        assert_eq!(
            FillerStrengthSetting::Light.to_core(),
            teletype_core::fillers::tiers::FillerStrength::Hesitations
        );
        assert_eq!(
            FillerStrengthSetting::Standard.to_core(),
            teletype_core::fillers::tiers::FillerStrength::Fillers
        );
        assert_eq!(
            FillerStrengthSetting::Strict.to_core(),
            teletype_core::fillers::tiers::FillerStrength::Full
        );
        // The Settings field carries a default, so a settings.json written
        // before the field existed still loads, on Standard.
        let legacy: Settings =
            serde_json::from_value(serde_json::json!({ "hotkey": "Cmd+Shift+Space" })).unwrap();
        assert_eq!(legacy.filler_strength, FillerStrengthSetting::Standard);
        // The wire key is camelCase, matching the UI's `fillerStrength`.
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["fillerStrength"], "standard");
        assert!(json.get("filler_strength").is_none());
    }
}
