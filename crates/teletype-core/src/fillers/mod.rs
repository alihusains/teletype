//! Tiered filler removal with a must-pass safety corpus.
//!
//! Replaces the flat word-list filler removal with a positional, tiered pass
//! that only removes a filler word when its context proves it is a spoken
//! hesitation. See [`tiers::remove_filler_words_tiered`] and
//! [`corpus`] for the rules and the must-pass safety corpus.
//!
//! The public entry point for the pipeline is
//! [`remove_filler_words`], which wraps the tiered pass and applies the
//! English-only language gate. It keeps the historical `Vec<String>` user
//! list signature so the pipeline call site stays minimal.

pub mod corpus;
pub mod tiers;

use tiers::{remove_filler_words_tiered, FillerInput, FillerStrength};

/// Removes filler words from a voice transcript using the tiered positional
/// rules.
///
/// `user_fillers` is the user's custom list (lowercased); each entry is
/// treated as Tier 1. `dictionary_terms` and `autotext_triggers` are
/// protected terms that are never removed even if they look like fillers.
///
/// The pass is English-only: for non-Latin-script languages (and empty
/// legacy is treated as English) it is a no-op, matching the ITN and
/// polish-gate gates.
pub fn remove_filler_words(
    text: &str,
    user_fillers: &[String],
    dictionary_terms: &[String],
    autotext_triggers: &[String],
    language: &str,
    strength: FillerStrength,
) -> String {
    remove_filler_words_tiered(
        &FillerInput {
            text,
            user_fillers,
            dictionary_terms,
            autotext_triggers,
        },
        language,
        strength,
    )
}
