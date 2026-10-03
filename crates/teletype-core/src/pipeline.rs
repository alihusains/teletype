//! The unified pipeline: voice and typed input converge here.
//!
//! ```text
//! UnifiedInput
//!   ↓ ContextProvider        → ApplicationContext
//!   ↓ AutoText protect()     → {{AUTOTEXT_n}} placeholders
//!   ↓ Personalization        → PreferencePacket
//!   ↓ TransformEngine        → prompt → inference → validation
//!   ↓ AutoText restore()     → exact values back
//!   ↓ TextInjector           → clipboard + paste
//! ```
//!
//! Data safety: the raw input is preserved at every stage. If any AI stage
//! fails, the pipeline returns the original input (with AutoText expanded) —
//! never an error that loses the user's words.

use crate::{
    autotext::{self, protect, AutoTextStore},
    context::ApplicationContext,
    dictionary::{Dictionary, WordChecker},
    emoji, itn,
    llm::InferenceProvider,
    personalization::{self, UserProfile},
    platform::Platform,
    style::StyleProfileStore,
    transforms::{
        engine::{self, TransformResult},
        prompt::PromptContext,
        TransformDefinition, TransformStore,
    },
};

/// Where the input came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputSource {
    /// From the microphone (transcript).
    #[default]
    Voice,
    /// Typed directly (AutoText expansion only, no AI by default).
    Typed,
}

/// One unit of input entering the pipeline.
#[derive(Debug, Clone)]
pub struct UnifiedInput {
    pub source: InputSource,
    /// Raw text, before any processing.
    pub text: String,
}

/// The outcome of one pipeline run.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// The final text that was (or would be) injected.
    pub final_text: String,
    /// The raw input, preserved for history/debugging.
    pub raw_input: String,
    /// True when an AI transform changed the text.
    pub transformed: bool,
    /// True when AutoText expanded at least one trigger.
    pub autotext_expanded: bool,
    /// The context the pipeline saw.
    pub context: ApplicationContext,
    /// Transform metrics, if a transform ran.
    pub transform: Option<TransformResult>,
    /// True when a transform was requested but no inference provider was
    /// loaded, so the pipeline fell back to AutoText expansion only. The UI
    /// must surface this; the fallback is never supposed to be silent.
    pub transform_skipped_no_model: bool,
}

/// Everything the pipeline needs, assembled by the desktop app.
pub struct Pipeline<'a> {
    pub platform: &'a dyn Platform,
    pub autotext: &'a AutoTextStore,
    pub transforms: &'a TransformStore,
    pub profile: &'a UserProfile,
    /// The inference provider for the selected model, if ready.
    pub inference: Option<&'a dyn InferenceProvider>,
    /// The user dictionary (custom words protected from "corrections").
    pub dictionary: &'a Dictionary,
    /// The word checker that decides whether a listed dictionary word was
    /// actually spoken. Default: [`EditDistanceChecker`] (the historical
    /// edit-distance rule). Swap for a trained model later without touching
    /// the pipeline.
    pub word_checker: &'a dyn WordChecker,
    /// Style profiles; `active_style` is the id of the active profile.
    /// `active_style` is only consulted when no per-app override matches the
    /// frontmost app (see `style::resolve_style_id`).
    pub styles: &'a StyleProfileStore,
    pub active_style: &'a str,
    /// Explicit style profile id chosen for this dictation ("" = none).
    /// Wins over both per-app overrides and the global active style.
    pub explicit_style: &'a str,
    /// When true, run the auto-apply transform (voice input).
    pub auto_apply: bool,
    /// Restore the user's clipboard after injection.
    pub restore_clipboard: bool,
    /// When true, strip filler words from voice transcripts.
    pub remove_filler_words: bool,
    /// The list of filler words to remove (lowercased).
    pub filler_words: Vec<String>,
    /// When true, restore curated emoji phrases in voice transcripts after
    /// the transform (P3.2). Skipped for non-Latin-script languages.
    pub restore_emoji: bool,
    /// When true, convert spoken emoji phrases ("thumbs up emoji") to glyphs
    /// in voice transcripts after the transform (BUG-002).
    pub spoken_emoji: bool,
    /// When true, the System AutoText spoken-punctuation entries ("comma" →
    /// "," etc.) run in the pipeline. Off means those phrases stay as words;
    /// custom AutoText is unaffected (BUG-002). The gate is applied to
    /// `system_autotext` by the caller (the desktop app filters the list);
    /// the pipeline only ever sees the entries it should run.
    pub spoken_punctuation: bool,
    /// Built-in System AutoText entries (spoken-phrase → symbol). Custom
    /// entries in `autotext` override these.
    pub system_autotext: &'a [crate::autotext::AutoTextEntry],
    /// Optional sink for streaming transform tokens (T1.1). `None` (the
    /// default in tests) means no live preview; the transform still runs and
    /// returns the full text.
    pub token_sink: Option<&'a mut dyn FnMut(&str)>,
    /// P5.1: when true and a local LLM provider is active, the deterministic
    /// polish gate may skip the transform for short clean utterances.
    /// Default `false` (off until measured).
    pub polish_gate_enabled: bool,
    /// P5.1: max word count for the gate's short-clean-skip path. Default 8.
    pub polish_gate_threshold_words: usize,
    /// Vocabulary-pack terms (lowercased, folded) for the lowest-priority
    /// fuzzy correction tier. Precomputed once per dictation via
    /// `crate::vocab::terms_for`.
    pub pack_terms: &'a [crate::vocab::PackTerm],
}

impl<'a> Pipeline<'a> {
    /// Runs the pipeline and returns the final text. Does NOT inject —
    /// the caller decides (tests assert on the returned text).
    pub fn run(
        &mut self,
        input: UnifiedInput,
        explicit_transform: Option<&TransformDefinition>,
    ) -> PipelineResult {
        let raw_input = input.text.clone();
        let context = self.platform.active_application().unwrap_or_default();
        let is_voice = input.source == InputSource::Voice;

        // 0. For voice input, correct ASR mishearings using the user's
        //    dictionary (and enabled vocabulary packs, as a lower-priority
        //    fuzzy tier). This runs before any transform so the AI sees the
        //    corrected text.
        let input_text =
            if is_voice && (!self.dictionary.words.is_empty() || !self.pack_terms.is_empty()) {
                correct_with_dictionary(
                    &input.text,
                    self.dictionary,
                    self.pack_terms,
                    self.word_checker,
                )
            } else {
                input.text.clone()
            };

        // 0.5. Remove filler words from the transcript, before the transform
        //      (same pattern as dictionary correction at step 0), so the AI
        //      sees filler-free text and any newlines the model produces
        //      survive untouched (the pass must never run after the LLM).
        let input_text = if is_voice && self.remove_filler_words && !self.filler_words.is_empty() {
            remove_filler_words(&input_text, &self.filler_words)
        } else {
            input_text
        };

        // 0.6. Deterministic ITN (numbers, dates, phones, money, units).
        //      Voice-only, English-gated. Runs before AutoText protect so
        //      ITN never sees placeholders, and before the transform so the
        //      model sees formatted numbers. Works with no model loaded.
        let input_text = if is_voice && itn::should_run(&self.profile.language) {
            itn::normalize(&input_text)
        } else {
            input_text
        };

        // 1. Protect AutoText values (typed `/trigger`s) and, for voice,
        //    spoken snippet phrases, so the exact values survive a transform.
        //    The two namespaces are disjoint, so protect each on the original
        //    text and keep the pass that matched anything.
        let protected = if is_voice {
            let typed = protect::protect(&input_text, self.autotext, &context);
            let spoken = protect::protect_snippets_with(
                &input_text,
                self.autotext,
                &context,
                self.system_autotext,
            );
            if spoken.has_placeholders() {
                spoken
            } else {
                typed
            }
        } else {
            protect::protect(&input_text, self.autotext, &context)
        };
        let autotext_expanded = protected.has_placeholders();

        // 2. Choose the transform (owned, so the match arms have a uniform type).
        let transform: Option<TransformDefinition> = match explicit_transform {
            Some(t) => Some(t.clone()),
            None if input.source == InputSource::Voice && self.auto_apply => {
                self.transforms.auto_apply().cloned()
            }
            None => None,
        };

        // 3. Run the transform (if any and if a provider is ready).
        //
        // P5.1: deterministic polish gate. When a local LLM provider is
        // active and the gate is on, short clean utterances skip the
        // 500 ms – 3 s polish pass. AutoText expansion is unaffected (it ran
        // at step 1 and is restored at step 4 regardless).
        let gate_skip = (transform.is_some()
            && self.inference.is_some_and(|p| p.is_local())
            && self.polish_gate_enabled)
            .then(|| {
                crate::transforms::gate::should_polish(
                    &protected.text,
                    &self.profile.language,
                    self.polish_gate_threshold_words,
                )
            });
        if let Some(decision) = &gate_skip {
            if !decision.should_polish {
                tracing::info!(reason = decision.reason, "transform_skipped_gate");
            }
        }

        let (text_after_transform, transform_result) = match transform.as_ref() {
            Some(t) => match self.inference {
                Some(provider) => {
                    if let Some(decision) = gate_skip.as_ref().filter(|d| !d.should_polish) {
                        // Gate decided to skip: produce the same
                        // TransformResult shape the existing skip paths use
                        // (transformed: false, skip_reason set) so the pill,
                        // tray and toast all surface the reason via the
                        // existing channel.
                        let result = engine::TransformResult {
                            text: protected.text.clone(),
                            transformed: false,
                            input_text: Some(input_text.clone()),
                            metrics: engine::TransformMetrics {
                                latency_ms: 0,
                                fell_back: true,
                                failure: None,
                                skip_reason: Some(engine::SkipReason::GateSkip {
                                    detail: decision.reason.to_string(),
                                }),
                            },
                        };
                        (result.text.clone(), Some(result))
                    } else {
                        let packet = personalization::packet::resolve(self.profile, &context);
                        let mut style = packet.style;
                        // Style resolution order (T2.2):
                        // 1. explicit per-dictation selection (self.explicit_style),
                        // 2. per-app override for the frontmost app,
                        // 3. default style for the app category (e.g. Email →
                        //    professional, Chat → casual),
                        // 4. the user's global active style, 5. default (none).
                        let category_default = crate::style::default_style_for_app(
                            &context.application_type,
                            self.active_style,
                        );
                        let style_id = crate::style::resolve_style_id(
                            &self.styles.app_style_overrides,
                            &context.application_name,
                            &category_default,
                            self.explicit_style,
                        );
                        for p in self.styles.active_phrases(&style_id) {
                            if !style.contains(&p) {
                                style.push(p);
                            }
                        }
                        let mut known = self
                            .dictionary
                            .known_words()
                            .into_iter()
                            .map(|w| format!("keep '{w}' as written"))
                            .collect::<Vec<_>>();
                        known.sort();
                        let ctx = PromptContext {
                            app: Some(context.clone()),
                            preferences: style,
                            preferred_terms: known,
                            user_instruction: None,
                            language: self.profile.language.clone(),
                            s1_control: self.profile.s1_control,
                        };
                        let mut result = engine::run_transform_blocking(
                            provider,
                            t,
                            &protected.text,
                            &ctx,
                            &mut self.token_sink,
                        );
                        if result.input_text.is_none() {
                            result.input_text = Some(input_text.clone());
                        }
                        (result.text.clone(), Some(result))
                    }
                }
                None => {
                    // No model: expand AutoText (and snippets for voice).
                    let mut expanded =
                        autotext::expand::expand(&input_text, self.autotext, &context);
                    if is_voice {
                        expanded = autotext::expand::expand_snippets_with(
                            &expanded,
                            self.autotext,
                            &context,
                            self.system_autotext,
                        );
                    }
                    // P1-16: flag the skip so the UI can tell the user why
                    // nothing was polished.
                    let result = engine::TransformResult {
                        text: expanded.clone(),
                        transformed: false,
                        metrics: engine::TransformMetrics {
                            latency_ms: 0,
                            fell_back: true,
                            failure: None,
                            skip_reason: Some(engine::SkipReason::NoModelLoaded),
                        },
                        input_text: Some(input_text.clone()),
                    };
                    (expanded, Some(result))
                }
            },
            None => (protected.text.clone(), None),
        };

        // 4. Restore AutoText values, or expand directly when no transform ran.
        let final_text = if autotext_expanded {
            protect::restore(&text_after_transform, &protected)
        } else if transform.is_none() {
            // No transform: expand triggers directly (typed input path), plus
            // spoken snippets for voice.
            let mut expanded = autotext::expand::expand(&input_text, self.autotext, &context);
            if is_voice {
                expanded = autotext::expand::expand_snippets_with(
                    &expanded,
                    self.autotext,
                    &context,
                    self.system_autotext,
                );
            }
            expanded
        } else {
            text_after_transform
        };

        // 5. Emoji restore (P3.2): spoken emoji phrases survive the LLM
        //    polish as words, so restore them AFTER the transform. Voice-only
        //    (typed text is the user's own), English-gated like ITN, and
        //    behind the `restore_emoji` setting. Idempotent by construction.
        let final_text =
            if is_voice && self.restore_emoji && itn::should_run(&self.profile.language) {
                emoji::restore(&final_text)
            } else {
                final_text
            };

        // 5.5. Spoken-emoji formatting (BUG-002): "thumbs up emoji" → "👍".
        //      Voice-only, after the transform for the same reason as step 5,
        //      and behind the `spoken_emoji` setting. Idempotent.
        let final_text = if is_voice && self.spoken_emoji {
            crate::transforms::spoken_emoji::format_spoken_emoji(&final_text, true)
        } else {
            final_text
        };

        // A transform was selected but no provider was loaded: the pipeline
        // produced AutoText-only output. Flag it so the caller can tell the
        // user why nothing was rewritten.
        //
        // This used to be `transform.is_some() && transform_result.is_none()`,
        // which can never be true here: the no-provider arm returns
        // `Some(TransformResult { .., skip_reason: NoModelLoaded })`, so the
        // flag was false on exactly the path it was written for and the
        // `Warn` log line in `dictation.rs` could never fire. Read the reason
        // the arm recorded instead of inferring it from an Option.
        let transform_skipped_no_model = transform.is_some()
            && transform_result.as_ref().is_some_and(|t| {
                t.metrics.skip_reason.as_ref() == Some(&engine::SkipReason::NoModelLoaded)
            });

        PipelineResult {
            final_text,
            raw_input,
            transformed: transform_result.as_ref().is_some_and(|r| r.transformed),
            autotext_expanded,
            context,
            transform: transform_result,
            transform_skipped_no_model,
        }
    }
}

/// Remove filler words from a transcript. Only removes whole-word, case-
/// insensitive matches that are NOT part of a longer word (e.g. "er" is
/// removed but "error" is not). Cleans up double spaces left behind.
///
/// Line-aware: newlines are preserved (a filler word at a line boundary is
/// dropped along with its surrounding whitespace), so this pass is safe to
/// run on the raw transcript before a transform.
fn remove_filler_words(text: &str, words: &[String]) -> String {
    if words.is_empty() {
        return text.to_string();
    }
    let filler: std::collections::HashSet<String> =
        words.iter().map(|w| w.to_lowercase()).collect();

    let mut out = String::new();
    let mut pending: Vec<String> = Vec::new();
    let mut prev_blank = true; // start of output: no leading blank line

    for raw_line in text.split_inclusive('\n') {
        let line: String = raw_line.chars().filter(|c| *c != '\r').collect();
        let trimmed = line.trim_end_matches('\n');
        let is_blank = trimmed.trim().is_empty();
        // Tokens that are exactly a filler word (punctuation allowed around
        // it) are dropped; everything else is kept verbatim.
        let kept: Vec<&str> = trimmed
            .split_whitespace()
            .filter(|tok| {
                let bare = tok
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                !filler.contains(&bare)
            })
            .collect();
        if is_blank {
            if !prev_blank {
                out.push('\n');
                prev_blank = true;
            }
        } else if kept.is_empty() {
            // Line held only filler: collapse it into the surrounding
            // whitespace so it cannot leave a stray blank line.
            if !prev_blank {
                out.push(' ');
            }
        } else {
            if !prev_blank {
                out.push(' ');
            }
            out.push_str(&kept.join(" "));
            if line.ends_with('\n') {
                out.push('\n');
                prev_blank = true;
            } else {
                prev_blank = false;
            }
        }
        pending.clear();
    }
    out.trim_end_matches(' ')
        .trim_start_matches(' ')
        .to_string()
}

/// Levenshtein edit distance between two strings (case-insensitive).
pub(crate) fn edit_distance_public(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Punctuation-folded lowercase form for exact dictionary comparison:
/// keeps letters, digits and spaces, drops everything else, and collapses
/// whitespace runs. "EG-1" folds to "eg1", "VS Code" to "vs code", so a
/// dictated token matches the canonical word without the near-miss pass.
fn fold_lower(s: &str) -> String {
    let kept: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    kept.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Corrects words in a transcript using the user's dictionary.
///
/// Exact matches (punctuation-folded, case-insensitive) always rewrite to the
/// canonical spelling. Words the user taught additionally allow edit-distance
/// <= 2 near-miss correction, fixing ASR mishearings of proper nouns and
/// jargon. Builtin seeds are exact-only: common acronyms sit within distance
/// 2 of ordinary words ("apt" -> "API", "its" -> "iOS"), so they must never
/// take the near-miss path (see `DictionaryWord::fuzzy`).
///
/// Words shorter than 3 characters are skipped (too risky to correct).
///
/// Pack terms (vocabulary packs) run as a final, lowest-priority FUZZY-only
/// tier: they only fire when none of the passes above matched the token, and
/// under a stricter bar than the dictation passes (similarity >= 0.85, alias
/// length >= 7) because pack data is curated, not user-taught.
fn correct_with_dictionary(
    text: &str,
    dictionary: &Dictionary,
    pack_terms: &[crate::vocab::PackTerm],
    checker: &dyn WordChecker,
) -> String {
    if dictionary.words.is_empty() && pack_terms.is_empty() {
        return text.to_string();
    }

    // Build lookup entries: (lowercased, canonical, word_count).
    struct Entry {
        lower: String,
        canonical: String,
        count: usize,
        /// User words allow distance-2 near-miss correction; builtin seeds
        /// are exact-only (see `DictionaryWord::fuzzy`).
        fuzzy: bool,
    }
    let entries: Vec<Entry> = dictionary
        .words
        .iter()
        .map(|w| {
            let lower = w.word.to_lowercase();
            let count = lower.split_whitespace().count();
            Entry {
                lower,
                canonical: w.word.clone(),
                count,
                fuzzy: w.fuzzy,
            }
        })
        .collect();

    // Split into tokens, preserving punctuation.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut result: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;

    while i < tokens.len() {
        let tok = tokens[i];
        let leading: String = tok.chars().take_while(|c| !c.is_alphanumeric()).collect();
        let trailing: String = tok
            .chars()
            .rev()
            .take_while(|c| !c.is_alphanumeric())
            .collect();
        let bare: String = tok.chars().filter(|c| c.is_alphanumeric()).collect();
        let bare_lower = bare.to_lowercase();

        // Try multi-word matches first (longest first), then single-word.
        let mut matched = false;

        for count in (2..=4).rev() {
            if i + count > tokens.len() {
                continue;
            }
            // Build the phrase from `count` consecutive tokens.
            let phrase_parts: Vec<String> = (i..i + count)
                .map(|j| {
                    tokens[j]
                        .chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                        .to_lowercase()
                })
                .collect();
            let phrase = phrase_parts.join(" ");

            if phrase.len() < 3 {
                continue;
            }

            // Check exact match first (punctuation-folded, so "EG-1" exactly
            // matches the token "eg1" without needing the near-miss pass).
            let mut found: Option<&Entry> = None;
            for e in &entries {
                if e.count == count && fold_lower(&e.lower) == phrase {
                    found = Some(e);
                    break;
                }
            }
            if found.is_none() {
                // Near match via the word checker (edit distance by default).
                for e in &entries {
                    if e.count != count || !e.fuzzy {
                        continue;
                    }
                    if checker.probability(&e.lower, &phrase, true) >= checker.cutoff() {
                        found = Some(e);
                        break;
                    }
                }
            }

            if let Some(e) = found {
                // Replace the `count` tokens with the canonical phrase.
                // Preserve leading punctuation of the first token and
                // trailing punctuation of the last token.
                let last_tok = tokens[i + count - 1];
                let last_trailing: String = last_tok
                    .chars()
                    .rev()
                    .take_while(|c| !c.is_alphanumeric())
                    .collect();
                result.push(format!("{leading}{}{last_trailing}", e.canonical));
                i += count;
                matched = true;
                break;
            }
        }

        if matched {
            continue;
        }

        // Single-word match.
        if bare_lower.len() >= 3 {
            let mut found: Option<&Entry> = None;
            for e in &entries {
                if e.count != 1 {
                    continue;
                }
                if fold_lower(&e.lower) == bare_lower {
                    found = Some(e);
                    break;
                }
            }
            if found.is_none() {
                for e in &entries {
                    if e.count != 1 || !e.fuzzy {
                        continue;
                    }
                    if checker.probability(&e.lower, &bare_lower, true) >= checker.cutoff() {
                        found = Some(e);
                        break;
                    }
                }
            }
            if let Some(e) = found {
                result.push(format!("{leading}{}{trailing}", e.canonical));
                i += 1;
                continue;
            }

            // Final tier: vocabulary packs. Fuzzy-only, lowest authority —
            // only reached when the dictionary passes found no match for
            // this token. Stricter than the dictation passes:
            // similarity >= 0.85 (0.80 + the reference's 0.05 bump) and
            // alias length >= 7 (short aliases are too risky to rewrite
            // dictated text on).
            if !pack_terms.is_empty() {
                let mut pack_hit = false;
                for pt in pack_terms {
                    let alias = fold_lower(&pt.alias);
                    if alias.len() < 7 {
                        continue;
                    }
                    let len_diff = (alias.len() as i32 - bare_lower.len() as i32).unsigned_abs();
                    if len_diff > 2 {
                        continue;
                    }
                    let dist = edit_distance_public(&bare_lower, &alias);
                    let max_len = bare_lower.len().max(alias.len());
                    if dist <= 2 && (1.0 - dist as f32 / max_len as f32) >= 0.85 {
                        result.push(format!("{leading}{}{trailing}", pt.canonical));
                        i += 1;
                        pack_hit = true;
                        break;
                    }
                }
                if pack_hit {
                    continue;
                }
            }
        }

        // No match: keep the token as-is.
        result.push(tok.to_string());
        i += 1;
    }

    result.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{context::normalize, platform::MockPlatform};

    static DICT: std::sync::LazyLock<crate::dictionary::Dictionary> =
        std::sync::LazyLock::new(crate::dictionary::Dictionary::default);
    static STYLES: std::sync::LazyLock<crate::style::StyleProfileStore> =
        std::sync::LazyLock::new(crate::style::StyleProfileStore::with_built_ins);
    static CHECKER: crate::dictionary::EditDistanceChecker =
        crate::dictionary::EditDistanceChecker::new();

    /// A mock inference provider that returns a fixed string.
    struct MockLlm {
        output: String,
    }
    impl InferenceProvider for MockLlm {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn is_local(&self) -> bool {
            true
        }
        fn generate(
            &self,
            _prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            Ok(self.output.clone())
        }
    }

    /// A mock inference provider that records the prompt it saw and returns
    /// a fixed string.
    struct CaptureLlm {
        output: String,
        captured: std::sync::Mutex<Option<String>>,
    }
    impl InferenceProvider for CaptureLlm {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn is_local(&self) -> bool {
            true
        }
        fn generate(
            &self,
            prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            *self.captured.lock().unwrap() = Some(prompt.to_string());
            Ok(self.output.clone())
        }
    }

    /// Builds a minimal pipeline with a capturing mock LLM and runs one
    /// voiced sentence through the auto-apply transform. Returns the prompt
    /// the model saw, so tests can assert which style phrases were injected.
    fn prompt_seen_for(
        app: ApplicationContext,
        active_style: &str,
        explicit_style: &str,
    ) -> String {
        let platform = MockPlatform::with_app(app);
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = CaptureLlm {
            output: "ok".into(),
            captured: std::sync::Mutex::new(None),
        };
        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            dictionary: &DICT,
            styles: &STYLES,
            active_style,
            explicit_style,
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };
        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "hello there friend how are you doing today".into(),
        };
        pipeline.run(input, None);
        let captured = llm
            .captured
            .lock()
            .unwrap()
            .take()
            .expect("prompt captured");
        captured
    }

    #[test]
    fn email_app_gets_professional_default_without_override() {
        // Gmail (Email), global active style is concise, no override:
        // the category default (professional) must be injected, not concise.
        let prompt = prompt_seen_for(normalize("com.google.gmail", "Gmail"), "style-concise", "");
        assert!(
            prompt.contains("use a professional tone"),
            "professional default missing: {prompt}"
        );
        assert!(!prompt.contains("be concise"));
    }

    #[test]
    fn manual_app_override_beats_category_default() {
        // Gmail (Email, category default = professional) with a manual
        // override to casual: casual wins.
        let mut styles = crate::style::StyleProfileStore::with_built_ins();
        styles
            .set_app_style_override("gmail", "style-casual")
            .unwrap();
        let platform = MockPlatform::with_app(normalize("com.google.gmail", "Gmail"));
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = CaptureLlm {
            output: "ok".into(),
            captured: std::sync::Mutex::new(None),
        };
        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            dictionary: &DICT,
            styles: &styles,
            active_style: "style-concise",
            explicit_style: "",
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };
        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "hello there friend how are you doing today".into(),
        };
        pipeline.run(input, None);
        let prompt = llm.captured.lock().unwrap().take().unwrap();
        assert!(
            prompt.contains("keep a friendly, casual tone"),
            "override (casual) missing: {prompt}"
        );
        assert!(!prompt.contains("use a professional tone"));
    }

    #[test]
    fn explicit_style_beats_override_and_category_default() {
        // Gmail (Email): explicit per-dictation concise beats both the
        // category default (professional) and the global active style.
        let prompt = prompt_seen_for(
            normalize("com.google.gmail", "Gmail"),
            "style-professional",
            "style-concise",
        );
        assert!(prompt.contains("be concise"), "explicit missing: {prompt}");
        assert!(!prompt.contains("use a professional tone"));
    }

    #[test]
    fn unknown_app_type_keeps_global_active_style() {
        // Regression guard: an unclassified app with no override uses the
        // global active style exactly as before.
        let prompt = prompt_seen_for(ApplicationContext::unknown(), "style-concise", "");
        assert!(
            prompt.contains("be concise"),
            "active style missing: {prompt}"
        );
        assert!(!prompt.contains("use a professional tone"));
    }

    #[test]
    fn voice_with_transform_and_autotext() {
        let platform = MockPlatform::with_app(normalize("com.google.gmail", "Gmail"));
        let mut autotext = AutoTextStore::default();
        autotext
            .insert(crate::autotext::AutoTextEntry::new(
                "/email",
                "user@example.com",
            ))
            .unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = MockLlm {
            output: "Please send the updated proposal to {{AUTOTEXT_0}} and let me know when you receive it.".into(),
        };

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "hey john can you send the updated proposal to /email and let me know when you get a chance".into(),
        };
        let result = pipeline.run(input, None);

        // The email must survive intact.
        assert!(
            result.final_text.contains("user@example.com"),
            "email lost: {}",
            result.final_text
        );
        // The transform ran.
        assert!(result.transformed);
        // Context was detected.
        assert_eq!(
            result.context.application_type,
            crate::context::AppType::Email
        );
    }

    #[test]
    fn typed_input_expands_autotext_without_ai() {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let mut autotext = AutoTextStore::default();
        autotext
            .insert(crate::autotext::AutoTextEntry::new("/name", "Ali Husain"))
            .unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None,
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Typed,
            text: "regards /name".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "regards Ali Husain");
        assert!(!result.transformed);
        assert!(result.autotext_expanded);
    }

    #[test]
    fn no_model_falls_back_to_expanded_input() {
        let platform = MockPlatform::with_app(normalize("com.google.gmail", "Gmail"));
        let mut autotext = AutoTextStore::default();
        autotext
            .insert(crate::autotext::AutoTextEntry::new(
                "/email",
                "user@example.com",
            ))
            .unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None, // no model
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "send to /email".into(),
        };
        let result = pipeline.run(input, None);
        // No model: AutoText still expands, no transform.
        assert_eq!(result.final_text, "send to user@example.com");
        assert!(!result.transformed);
    }

    #[test]
    fn voice_expands_spoken_snippets() {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let mut autotext = AutoTextStore::default();
        let mut e = crate::autotext::AutoTextEntry::new("/email", "abcd@gmail.com");
        e.snippet = "my email".into();
        autotext.insert(e).unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None,
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        // Voice: the spoken phrase expands even though there's no `/trigger`.
        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "my email please".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "abcd@gmail.com please");
        assert!(!result.transformed);

        // Typed input does NOT expand snippets (only spoken).
        let input = UnifiedInput {
            source: InputSource::Typed,
            text: "my email please".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "my email please");
    }

    #[test]
    fn inference_failure_preserves_input() {
        struct FailLlm;
        impl InferenceProvider for FailLlm {
            fn model_id(&self) -> &str {
                "fail"
            }
            fn model_name(&self) -> &str {
                "fail"
            }
            fn generate(&self, _: &str, _: crate::llm::GenerationParams) -> Result<String, String> {
                Err("model crashed".into())
            }
        }
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = FailLlm;

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "my important words".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "my important words");
        assert!(!result.transformed);
    }

    #[test]
    fn explicit_instruction_overrides_context() {
        // The Rewriter's user_instruction is the explicit instruction.
        // Here we just verify the pipeline passes it through.
        let platform = MockPlatform::with_app(normalize("com.google.gmail", "Gmail"));
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let mut rewriter = transforms.get("builtin-rewriter").unwrap().clone();
        rewriter.auto_apply = false;
        let profile = UserProfile::default();

        let llm = MockLlm {
            output: "Make it super casual, bro!".into(),
        };

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "formal text here".into(),
        };
        let result = pipeline.run(input, Some(&rewriter));
        assert!(result.transformed);
        assert_eq!(result.final_text, "Make it super casual, bro!");
    }

    // ---- Dictionary correction tests ----

    fn dict_with(words: &[&str]) -> crate::dictionary::Dictionary {
        let mut d = crate::dictionary::Dictionary::default();
        for w in words {
            d.insert(crate::dictionary::DictionaryWord::new(*w, ""))
                .unwrap();
        }
        d
    }

    #[test]
    fn edit_distance_basic() {
        assert_eq!(edit_distance_public("kitten", "sitting"), 3);
        assert_eq!(edit_distance_public("flaw", "lawn"), 2);
        assert_eq!(edit_distance_public("same", "same"), 0);
        assert_eq!(edit_distance_public("", "abc"), 3);
    }

    #[test]
    fn corrects_misheard_proper_noun() {
        let d = dict_with(&["Rida Fatema"]);
        assert_eq!(
            correct_with_dictionary("Rida Fattama is here", &d, &[], &CHECKER),
            "Rida Fatema is here"
        );
    }

    #[test]
    fn corrects_single_char_typo() {
        let d = dict_with(&["Teletype"]);
        assert_eq!(
            correct_with_dictionary("I use Teletype daily", &d, &[], &CHECKER),
            "I use Teletype daily"
        );
    }

    #[test]
    fn corrects_close_mishearing() {
        let d = dict_with(&["IC Markets"]);
        // "IC Margets" is edit distance 2 from "IC Markets"
        assert_eq!(
            correct_with_dictionary("trading on IC Margets", &d, &[], &CHECKER),
            "trading on IC Markets"
        );
    }

    #[test]
    fn preserves_punctuation() {
        let d = dict_with(&["Ridha"]);
        // "Ridha" is an exact match for the taught word "Ridha", so it
        // stays as-is. Punctuation is preserved.
        assert_eq!(
            correct_with_dictionary("Hi, Ridha!", &d, &[], &CHECKER),
            "Hi, Ridha!"
        );
    }

    #[test]
    fn does_not_correct_short_words() {
        let d = dict_with(&["ab"]);
        // "ab" is only 2 chars, so it's skipped
        assert_eq!(
            correct_with_dictionary("ab cd ef", &d, &[], &CHECKER),
            "ab cd ef"
        );
    }

    #[test]
    fn does_not_correct_unrelated_words() {
        let d = dict_with(&["Rida"]);
        // "hello" is too far from "Rida"
        assert_eq!(
            correct_with_dictionary("hello world", &d, &[], &CHECKER),
            "hello world"
        );
    }

    #[test]
    fn empty_dictionary_is_noop() {
        let d = crate::dictionary::Dictionary::default();
        assert_eq!(
            correct_with_dictionary("any text here", &d, &[], &CHECKER),
            "any text here"
        );
    }

    // ---- Vocabulary-pack tier tests ----

    /// One pack term: cherrypick <- cherapak (from the tech pack data).
    fn cherrypick_terms() -> Vec<crate::vocab::PackTerm> {
        crate::vocab::terms_for(&["tech".to_string()])
    }

    #[test]
    fn pack_term_corrects_mishearing_when_enabled() {
        let d = crate::dictionary::Dictionary::default();
        let terms = cherrypick_terms();
        assert!(!terms.is_empty(), "tech pack should contribute terms");
        assert_eq!(
            correct_with_dictionary("let us cherapak the fix", &d, &terms, &CHECKER),
            "let us cherrypick the fix",
            "tech pack enabled: cherapak should become cherrypick"
        );
    }

    #[test]
    fn pack_term_not_corrected_when_disabled() {
        let d = crate::dictionary::Dictionary::default();
        // No packs enabled: the mishearing survives.
        assert_eq!(
            correct_with_dictionary("let us cherapak the fix", &d, &[], &CHECKER),
            "let us cherapak the fix",
            "pack disabled: cherapak must not be rewritten"
        );
    }

    #[test]
    fn pack_tier_runs_with_empty_dictionary() {
        // The guard must let pack terms run even when the dictionary is empty.
        let d = crate::dictionary::Dictionary::default();
        assert!(d.words.is_empty());
        let terms = cherrypick_terms();
        assert_eq!(
            correct_with_dictionary("cherapak it", &d, &terms, &CHECKER),
            "cherrypick it"
        );
    }

    #[test]
    fn user_dictionary_beats_pack_term() {
        // The same canonical is taught by the user with different casing;
        // the dictionary pass runs first and must win over the pack tier.
        // "cherrypick" exactly matches the user word "CherryPick"
        // (punctuation-folded, case-insensitive), so the dictionary rewrites
        // it to the user's casing before the pack tier ever sees it.
        let d = dict_with(&["CherryPick"]);
        let terms = cherrypick_terms();
        assert_eq!(
            correct_with_dictionary("let us cherrypick the fix", &d, &terms, &CHECKER),
            "let us CherryPick the fix",
            "user dictionary must win over the pack tier"
        );
        // Without the dictionary the same token still becomes "cherrypick"
        // via the pack tier (lowercase canonical) — proving the pack tier
        // runs when the dictionary finds no match.
        let empty = crate::dictionary::Dictionary::default();
        assert_eq!(
            correct_with_dictionary("let us cherrypick the fix", &empty, &terms, &CHECKER),
            "let us cherrypick the fix"
        );
        // And a true pack mishearing (cherapak -> cherrypick, dist 1, sim
        // 0.91) IS corrected by the pack tier when the dictionary is empty.
        assert_eq!(
            correct_with_dictionary("let us cherapak the fix", &empty, &terms, &CHECKER),
            "let us cherrypick the fix"
        );
    }

    #[test]
    fn pack_fuzzy_rejects_short_or_low_similarity_aliases() {
        let d = crate::dictionary::Dictionary::default();
        let terms = vec![
            // Alias shorter than 7 chars: must never fire.
            crate::vocab::PackTerm {
                canonical: "cherrypick".into(),
                alias: "cherp".into(),
            },
            // Distance 3 (and similarity 0.67 < 0.85): must not fire.
            crate::vocab::PackTerm {
                canonical: "cherrypick".into(),
                alias: "cherpick".into(),
            },
        ];
        // "cherpick" (6 chars) is within len_diff 2 of the token "cherp"
        // (dist 3), but the reference's packFuzzyMinLength = 7 must keep it
        // from firing. With no qualifying alias the token survives.
        assert_eq!(
            correct_with_dictionary("cherp here", &d, &terms, &CHECKER),
            "cherp here",
            "short / low-similarity pack aliases must not rewrite"
        );
        // A 7+ char alias at dist 2 but similarity 0.75 < 0.85: must not
        // fire either (the bump above the dictation floor is the point).
        // "cherrypika" vs "cherrypick": dist 1, sim 0.888 — the real
        // low-similarity case needs dist 2 at length 8: "cherrypik" vs
        // "cherryppik" would be dist 1 too; use "cherrypick" vs "cherrywick"
        // (dist 2, sim 0.75).
        let low_sim = vec![crate::vocab::PackTerm {
            canonical: "cherrypick".into(),
            alias: "cherrywick".into(), // "cherrywick" vs "cherrypick": dist 2, sim 0.75
        }];
        assert_eq!(
            correct_with_dictionary("cherrypick here", &d, &low_sim, &CHECKER),
            "cherrypick here",
            "low-similarity (0.75 < 0.85) alias must not rewrite"
        );
        // But a qualifying alias (dist 1, sim 0.91 >= 0.85, len 7+) fires.
        let good = vec![crate::vocab::PackTerm {
            canonical: "cherrypick".into(),
            alias: "cherrypik".into(),
        }];
        assert_eq!(
            correct_with_dictionary("cherrypik it", &d, &good, &CHECKER),
            "cherrypick it"
        );
    }

    #[test]
    fn pack_tier_preserves_punctuation() {
        let d = crate::dictionary::Dictionary::default();
        let terms = cherrypick_terms();
        // Trailing punctuation is preserved exactly like the existing
        // single-word fuzzy pass (leading punctuation on the first token,
        // trailing on the last).
        assert_eq!(
            correct_with_dictionary("(cherapak)", &d, &terms, &CHECKER),
            "(cherrypick)",
            "leading/trailing punctuation must survive the pack tier"
        );
    }

    #[test]
    fn similarity_floor_rejects_short_word_near_miss() {
        // P0-11: a taught 3-letter word "apt" must not rewrite "app"
        // (dist=1, sim=0.67 < 0.80).
        let d = dict_with(&["apt"]);
        assert_eq!(
            correct_with_dictionary("I use the app daily", &d, &[], &CHECKER),
            "I use the app daily",
            "short word near-miss should not rewrite"
        );
        // But a longer word at dist=2 with sim >= 0.80 should still correct.
        let d2 = dict_with(&["OpenAI"]);
        assert_eq!(
            correct_with_dictionary("I use openai daily", &d2, &[], &CHECKER),
            "I use OpenAI daily",
            "exact match should still work"
        );
    }

    #[test]
    fn similarity_floor_allongs_long_word_near_miss() {
        // P0-11: "Rida Fatema" (11 chars) vs "Rida Fattama" (12 chars):
        // dist=2, sim = 1 - 2/12 = 0.83 >= 0.80. Should correct.
        let d = dict_with(&["Rida Fatema"]);
        assert_eq!(
            correct_with_dictionary("Rida Fattama is here", &d, &[], &CHECKER),
            "Rida Fatema is here",
            "long phrase near-miss should still correct"
        );
    }

    #[test]
    fn seeded_builtins_do_not_falsely_correct_common_words() {
        let mut d = crate::dictionary::Dictionary::default();
        d.seed_builtins();
        // Words a speaker plausibly says that sit within edit distance 2 of a
        // seeded builtin: none of these may be rewritten.
        let untouched = [
            "I am apt to opt in",
            "he ate a pie and a spa day",
            "fix its behavior",
            "clip the club video",
            "his cloud macro egg ego",
            "the clause says so",
            "works with his code",
            "billions of ions",
        ];
        for input in untouched {
            assert_eq!(
                correct_with_dictionary(input, &d, &[], &CHECKER),
                input,
                "builtin falsely corrected: {input}"
            );
        }
        // Exact (case-insensitive) matches must still be corrected.
        assert_eq!(
            correct_with_dictionary("we use github and macos daily", &d, &[], &CHECKER),
            "we use GitHub and macOS daily"
        );
    }

    #[test]
    fn pipeline_corrects_voice_with_dictionary() {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let dict = dict_with(&["Rida Fatema"]);

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None,
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &dict,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "Rida Fattama is coming".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "Rida Fatema is coming");
    }

    /// Runs one voice dictation through the pipeline with auto-apply on and
    /// returns **the prompt the model was given**.
    ///
    /// This used to return `final_text` on the assumption that the echo mock's
    /// output was also the prompt. That stopped being true when
    /// `validator::Failure::OffTopic` (2026-09-28) started rejecting a
    /// response that shares no content with the input, which is the correct
    /// behaviour: the mock interleaved style instructions into the sentence,
    /// so its output was not a rewrite. The four callers want to know which
    /// style phrases reached the prompt, so capture the prompt directly and
    /// keep the mock's output a faithful echo.
    fn run_prompt_for(
        app: ApplicationContext,
        styles: &crate::style::StyleProfileStore,
        active_style: &str,
        explicit_style: &str,
    ) -> String {
        let platform = MockPlatform::with_app(app);
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = PromptCapturingLlm {
            captured: std::sync::Mutex::new(None),
        };
        {
            let mut pipeline = Pipeline {
                platform: &platform,
                autotext: &autotext,
                transforms: &transforms,
                profile: &profile,
                inference: Some(&llm),
                dictionary: &DICT,
                styles,
                active_style,
                explicit_style,
                auto_apply: true,
                restore_clipboard: true,
                remove_filler_words: false,
                filler_words: vec![],
                token_sink: None,
                polish_gate_enabled: false,
                polish_gate_threshold_words: 8,
                restore_emoji: false,
                spoken_emoji: false,
                spoken_punctuation: true,
                system_autotext: &[],
                pack_terms: &[],
                word_checker: &CHECKER,
            };
            pipeline.run(
                UnifiedInput {
                    source: InputSource::Voice,
                    text: "hello, good to hear from you after this long time, how is the \
                           family doing, and when are we meeting next week"
                        .into(),
                },
                None,
            );
        }
        // Bind the guard to a local: returning straight off the tail
        // expression keeps the MutexGuard alive past the end of `llm`.
        let captured = llm.captured.lock().unwrap().take();
        captured.expect("the transform must have reached the provider")
    }

    /// Records the prompt and returns the user's own words, so the transform
    /// is accepted and the caller can assert on what the model was asked.
    struct PromptCapturingLlm {
        captured: std::sync::Mutex<Option<String>>,
    }
    impl InferenceProvider for PromptCapturingLlm {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn is_local(&self) -> bool {
            true
        }
        fn generate(
            &self,
            prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            *self.captured.lock().unwrap() = Some(prompt.to_string());
            // A pass-through is a legitimate no-op transform.
            let start = prompt.rfind("<<<\n").map(|i| i + 4);
            let end = prompt.rfind("\n>>>");
            match (start, end) {
                (Some(a), Some(b)) if b > a => Ok(prompt[a..b].to_string()),
                _ => Ok(String::new()),
            }
        }
    }

    #[test]
    fn app_style_override_applies_when_app_is_frontmost() {
        let mut store = crate::style::StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("gmail", "style-professional")
            .unwrap();
        // No global active style; Gmail is frontmost → professional wins.
        let prompt = run_prompt_for(normalize("com.google.gmail", "Gmail"), &store, "", "");
        assert!(
            prompt.contains("use a professional tone"),
            "professional phrases missing: {prompt}"
        );
        assert!(!prompt.contains("contractions are fine"));
    }

    #[test]
    fn explicit_style_beats_app_override_in_pipeline() {
        let mut store = crate::style::StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("gmail", "style-casual")
            .unwrap();
        let prompt = run_prompt_for(
            normalize("com.google.gmail", "Gmail"),
            &store,
            "",
            "style-concise",
        );
        assert!(prompt.contains("be concise"));
        assert!(!prompt.contains("contractions are fine"));
    }

    #[test]
    fn unknown_app_uses_global_active_style_in_pipeline() {
        let mut store = crate::style::StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("gmail", "style-casual")
            .unwrap();
        let prompt = run_prompt_for(
            normalize("com.apple.terminal", "Terminal"),
            &store,
            "style-professional",
            "",
        );
        assert!(prompt.contains("use a professional tone"));
        assert!(!prompt.contains("contractions are fine"));
    }

    #[test]
    fn pipeline_does_not_correct_typed_input() {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let dict = dict_with(&["Rida Fatema"]);

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None,
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &dict,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Typed,
            text: "Rida Fattama is coming".into(),
        };
        let result = pipeline.run(input, None);
        // Typed input is NOT corrected (only voice).
        assert_eq!(result.final_text, "Rida Fattama is coming");
    }

    #[test]
    fn filler_removal_preserves_newlines() {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let dict = crate::dictionary::Dictionary::default();

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None,
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: true,
            filler_words: vec!["um".into(), "uh".into(), "like".into()],
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            dictionary: &dict,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "Greeting line um\nBody line uh\nSign-off line like".into(),
        };
        let result = pipeline.run(input, None);
        // Newlines must survive filler removal (P0-3: filler pass runs before
        // the transform, on the raw input, and is line-aware).
        assert_eq!(
            result.final_text, "Greeting line\nBody line\nSign-off line",
            "newlines flattened: {:?}",
            result.final_text
        );
    }

    // ---- P5.1 polish gate pipeline tests ----

    /// Capitalises the text it is given, so the mock behaves like a real
    /// (very timid) polish rather than returning a placeholder string.
    ///
    /// `MockLlm` returned the fixed string "Polished by the model!" for every
    /// input, which shares no content with the utterance.
    /// `validator::Failure::OffTopic` (2026-09-28) correctly rejects that: a
    /// response unrelated to the input is a refusal or a non-sequitur, not a
    /// rewrite. Returning a faithful rewrite keeps these tests testing the
    /// gate, which is what they are for.
    struct GateLlm;
    impl InferenceProvider for GateLlm {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn is_local(&self) -> bool {
            true
        }
        fn generate(
            &self,
            prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            let start = prompt.rfind("<<<\n").map(|i| i + 4).ok_or("no marker")?;
            let end = prompt.rfind("\n>>>").ok_or("no end marker")?;
            let body = prompt[start..end].trim();
            let mut chars = body.chars();
            let first = chars
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            Ok(format!("{first}{}", chars.collect::<String>()))
        }
    }

    fn run_gate_pipeline(gate_enabled: bool, text: &str) -> PipelineResult {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = GateLlm;

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: gate_enabled,
            polish_gate_threshold_words: 8,
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: text.into(),
        };
        pipeline.run(input, None)
    }

    #[test]
    fn gate_on_skips_clean_short_utterance() {
        let result = run_gate_pipeline(true, "the file is in downloads");
        // Gate skipped the LLM: no transform, input preserved verbatim.
        assert!(!result.transformed);
        assert_eq!(result.final_text, "the file is in downloads");
        match result
            .transform
            .as_ref()
            .and_then(|t| t.metrics.skip_reason.as_ref())
        {
            Some(engine::SkipReason::GateSkip { detail }) => {
                assert_eq!(detail, "short_clean");
            }
            other => panic!("expected GateSkip, got: {other:?}"),
        }
    }

    #[test]
    fn gate_off_always_runs_transform() {
        let result = run_gate_pipeline(false, "the file is in downloads");
        // Gate off: the model output is used even for a clean short input.
        // The mock capitalises, so the capital "The" proves the model ran.
        assert!(result.transformed);
        assert_eq!(result.final_text, "The file is in downloads");
    }

    #[test]
    fn gate_on_still_polishes_filler_utterance() {
        let result = run_gate_pipeline(true, "um so like the thing you know with the client");
        // Filler triggers the gate: the LLM runs. The mock capitalises the
        // first letter, so a capitalised "Um" proves the model produced the
        // text rather than the pipeline passing the input through.
        assert!(result.transformed);
        assert_eq!(
            result.final_text, "Um so like the thing you know with the client",
            "the model's rewrite must be what lands, not the raw input"
        );
    }

    #[test]
    fn gate_on_skips_autotext_still_expands() {
        // AutoText expansion is upstream of the gate and must survive a skip.
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let mut autotext = AutoTextStore::default();
        autotext
            .insert(crate::autotext::AutoTextEntry::new(
                "/email",
                "user@example.com",
            ))
            .unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = GateLlm;

        let mut pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
            explicit_style: "",
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: true,
            polish_gate_threshold_words: 8,
            restore_emoji: false,
            spoken_emoji: true,
            spoken_punctuation: true,
            pack_terms: &[],
            word_checker: &CHECKER,
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "send it to /email ok".into(),
        };
        let result = pipeline.run(input, None);
        // The gate skipped the LLM, but the AutoText value was restored.
        assert!(!result.transformed);
        assert_eq!(result.final_text, "send it to user@example.com ok");
        assert!(result.autotext_expanded);
    }
}
