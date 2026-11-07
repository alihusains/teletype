//! Entry schema for the data-driven model catalog.
//!
//! The schema is the union of the old `SpeechModel` and `CatalogEntry`
//! structs plus the capability set, field for field so nothing is lost in
//! the migration to JSON.

use serde::{Deserialize, Serialize};

/// The user-facing capability set a model declares.
///
/// `asr` transcribes voice to text, `polish` cleans up transcribed text,
/// `tool_calling` can emit structured tool calls (command mode), and `omni`
/// marks a one-model-does-everything model that runs its own pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Asr,
    Polish,
    ToolCalling,
    Omni,
}

/// One file of a model artifact, with the exact published bytes and the
/// publisher's SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub file: String,
    pub bytes: u64,
    pub sha256: Option<String>,
}

/// Why a speech row is worth choosing, shown as a chip beside the name.
/// `measured` separates a role this repo has measured from one inferred
/// from the model card or engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Purpose {
    pub label: String,
    pub basis: String,
    /// Eval file or report that proves a measured chip, relative to the repo
    /// root. Required when `basis` is `measured`; `null` for `inferred`.
    pub evidence: Option<String>,
}

/// One ASR (dictation) model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechEntry {
    pub id: String,
    pub name: String,
    /// Engine registry name (see `teletype_catalog::engine::Engine`).
    pub engine: String,
    pub capabilities: Vec<Capability>,
    /// Languages the model can transcribe, or `null` for every engine
    /// language (Whisper's 99).
    pub languages: Option<Vec<String>>,
    pub english_only: bool,
    /// The model's files. Single-file, two-file (llama-server pair), and
    /// shard shapes all collapse into this list. Empty for the Core ML
    /// repo shape and the OS-managed Apple-speech shape.
    pub artifacts: Vec<Artifact>,
    /// Optional download base URL override. `null` means the engine's
    /// default base URL is used.
    pub base_url: Option<String>,
    /// Approximate download size in decimal MB.
    pub size_mb: u32,
    pub min_ram_gb: u32,
    pub speed: f64,
    pub accuracy: f64,
    pub recommended: bool,
    /// Companion model, not a dictation engine.
    pub auxiliary: bool,
    /// Platform set the row is offered on: `macos`, `windows`, `linux`.
    pub runs_on: Vec<String>,
    pub purposes: Vec<Purpose>,
    pub description: String,
}

/// One polish/LLM model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmEntry {
    pub id: String,
    pub name: String,
    /// Approximate parameter class, for display.
    pub size_label: String,
    /// Replaces the old `polish_capable` bool; a polish row carries
    /// `["polish"]`.
    pub capabilities: Vec<Capability>,
    pub url: String,
    /// Approximate download size in decimal MB.
    pub size_mb: u32,
    pub description: String,
    /// What this row is worth choosing for, shown as a chip beside the name.
    pub specialty: String,
    pub sha256: Option<String>,
    pub backup_url: Option<String>,
    pub attribution: Option<String>,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub requires_license_accept: bool,
    pub recommended: bool,
    pub runs_on_cpu: bool,
    pub verdict: String,
    pub verdict_note: String,
    pub sort_order: u32,
    /// Non-empty for multi-file (split) GGUFs.
    pub shards: Vec<Artifact>,
    /// Platform set the row is offered on: `macos`, `windows`, `linux`.
    pub runs_on: Vec<String>,
}

/// One command-mode decider model.
///
/// Carries the FULL display field set the UI reads for the decider row (R3),
/// in addition to the base fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEntry {
    pub id: String,
    pub name: String,
    /// Engine registry name (the decider is `onnx`).
    pub engine: String,
    /// Always includes `tool_calling`.
    pub capabilities: Vec<Capability>,
    pub artifacts: Vec<Artifact>,
    pub base_url: Option<String>,
    pub size_mb: u32,
    pub description: String,
    pub runs_on: Vec<String>,
    pub runs_on_cpu: bool,
    pub size_label: String,
    pub specialty: String,
    pub sha256: Option<String>,
    pub backup_url: Option<String>,
    pub attribution: Option<String>,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub requires_license_accept: bool,
    pub recommended: bool,
    pub verdict: String,
    pub verdict_note: String,
    pub sort_order: u32,
    pub shards: Vec<Artifact>,
}

/// The three pipeline domains, parsed from the three JSON files.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Catalog {
    pub speech: Vec<SpeechEntry>,
    pub llm: Vec<LlmEntry>,
    pub command: Vec<CommandEntry>,
}
