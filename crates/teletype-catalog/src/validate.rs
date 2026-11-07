//! Validator: re-runs the invariants the existing catalog tests assert, now
//! against parsed data. One API used by both CI (build-time) and the update
//! path (runtime).
//!
//! R2: an empty slice is VALID for every domain. R4: the recommended rule is
//! "at least one" per non-empty speech/llm domain, no upper bound; command
//! is exempt.

use crate::engine::Engine;
use crate::schema::{Catalog, CommandEntry, LlmEntry, SpeechEntry};
use crate::CatalogError;

/// The chip vocabulary the UI may show. A label outside this list is a
/// catalog bug, not a wording choice.
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

fn is_64_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn check_ids<'a>(ids: impl Iterator<Item = &'a str>, domain: &str) -> Result<(), CatalogError> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(CatalogError::Validation(format!(
                "{domain}: duplicate id {id}"
            )));
        }
    }
    Ok(())
}

/// Invariant 7 (R7): `size_mb * 1_000_000 + 1024 >= sum(artifact bytes)`.
fn size_covers_artifacts(id: &str, size_mb: u32, total_bytes: u64) -> Result<(), CatalogError> {
    let budget = u64::from(size_mb)
        .saturating_mul(1_000_000)
        .saturating_add(1024);
    if budget < total_bytes {
        return Err(CatalogError::Validation(format!(
            "{id}: size_mb {size_mb} does not cover {total_bytes} artifact bytes"
        )));
    }
    Ok(())
}

fn check_runs_on(id: &str, runs_on: &[String]) -> Result<(), CatalogError> {
    if runs_on.is_empty() {
        return Err(CatalogError::Validation(format!("{id}: runs_on is empty")));
    }
    for p in runs_on {
        if !matches!(p.as_str(), "macos" | "windows" | "linux") {
            return Err(CatalogError::Validation(format!(
                "{id}: unknown platform in runs_on: {p}"
            )));
        }
    }
    Ok(())
}

fn check_artifact_hashes(
    id: &str,
    artifacts: &[crate::schema::Artifact],
) -> Result<(), CatalogError> {
    for a in artifacts {
        if let Some(sha) = &a.sha256 {
            if !is_64_hex(sha) {
                return Err(CatalogError::Validation(format!(
                    "{id}: artifact {} sha256 is not 64 lowercase hex",
                    a.file
                )));
            }
        }
    }
    Ok(())
}

pub fn validate_speech(entries: &[SpeechEntry]) -> Result<(), CatalogError> {
    check_ids(entries.iter().map(|e| e.id.as_str()), "speech")?;
    if !entries.is_empty() && !entries.iter().any(|e| e.recommended) {
        return Err(CatalogError::Validation(
            "speech: no recommended row (R4: at least one required for a non-empty domain)".into(),
        ));
    }
    for e in entries {
        Engine::from_name(&e.engine).map_err(|_| {
            CatalogError::Validation(format!("speech {}: unknown engine {}", e.id, e.engine))
        })?;
        for p in &e.purposes {
            if !PURPOSE_LABELS.contains(&p.label.as_str()) {
                return Err(CatalogError::Validation(format!(
                    "speech {}: purpose label not in vocabulary: {}",
                    e.id, p.label
                )));
            }
        }
        // Invariant 5: sha256 may be absent only for the Core ML repo shape
        // (parakeet-coreml) and the OS-managed Apple-speech shape (empty
        // artifacts). Any other artifact must pin its hash.
        let engine = Engine::from_name(&e.engine).unwrap_or(Engine::Whisper);
        let sha_exempt = matches!(engine, Engine::ParakeetCoreml | Engine::AppleSpeech);
        if !sha_exempt {
            for a in &e.artifacts {
                if a.sha256.is_none() {
                    return Err(CatalogError::Validation(format!(
                        "speech {}: artifact {} has no sha256",
                        e.id, a.file
                    )));
                }
            }
        }
        check_artifact_hashes(&e.id, &e.artifacts)?;
        // Invariant 6: languages agree with english_only.
        if e.english_only {
            match &e.languages {
                Some(langs) if langs == &["en".to_string()] => {}
                _ => {
                    return Err(CatalogError::Validation(format!(
                        "speech {}: english_only row must list exactly [\"en\"]",
                        e.id
                    )))
                }
            }
        }
        // Invariant 7 (R7).
        let total: u64 = e.artifacts.iter().map(|a| a.bytes).sum();
        size_covers_artifacts(&e.id, e.size_mb, total)?;
        // Invariant 8.
        check_runs_on(&e.id, &e.runs_on)?;
    }
    Ok(())
}

pub fn validate_llm(entries: &[LlmEntry]) -> Result<(), CatalogError> {
    check_ids(entries.iter().map(|e| e.id.as_str()), "llm")?;
    if !entries.is_empty() && !entries.iter().any(|e| e.recommended) {
        return Err(CatalogError::Validation(
            "llm: no recommended row (R4: at least one required for a non-empty domain)".into(),
        ));
    }
    for e in entries {
        check_artifact_hashes(&e.id, &e.shards)?;
        let total: u64 = e.shards.iter().map(|a| a.bytes).sum();
        size_covers_artifacts(&e.id, e.size_mb, total)?;
        check_runs_on(&e.id, &e.runs_on)?;
    }
    Ok(())
}

pub fn validate_command(entries: &[CommandEntry]) -> Result<(), CatalogError> {
    check_ids(entries.iter().map(|e| e.id.as_str()), "command")?;
    // Command is exempt from the recommended rule (R4).
    for e in entries {
        if !e
            .capabilities
            .contains(&crate::schema::Capability::ToolCalling)
        {
            return Err(CatalogError::Validation(format!(
                "command {}: must carry the tool_calling capability",
                e.id
            )));
        }
        Engine::from_name(&e.engine).map_err(|_| {
            CatalogError::Validation(format!("command {}: unknown engine {}", e.id, e.engine))
        })?;
        check_artifact_hashes(&e.id, &e.shards)?;
        let total: u64 = e.shards.iter().map(|a| a.bytes).sum();
        size_covers_artifacts(&e.id, e.size_mb, total)?;
        check_runs_on(&e.id, &e.runs_on)?;
    }
    Ok(())
}

pub fn validate_catalog(catalog: &Catalog) -> Result<(), CatalogError> {
    validate_speech(&catalog.speech)?;
    validate_llm(&catalog.llm)?;
    validate_command(&catalog.command)?;
    Ok(())
}
