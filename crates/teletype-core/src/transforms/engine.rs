//! The transform engine: model-agnostic orchestration of one transform.
//!
//! The engine takes the *protected* input (AutoText already placeholdered) and
//! returns either transformed text (still placeholdered) or a fallback to the
//! input. AutoText restoration happens in the pipeline, not here.

use super::prompt::{
    build_eg1_messages, build_prompt, build_s1_messages, eg1_too_short, strip_eg1_tags,
    PromptContext,
};
use super::validator::{self, Failure, ValidatedOutput};
use super::TransformDefinition;

/// Why a transform was skipped or fell back to the raw input.
///
/// Lean 6-case taxonomy (P1-16): no 17-case telemetry copy. The 7 validator
/// `Failure` cases collapse under [`SkipReason::ValidationRejected`] with the
/// specific kind as detail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// No inference provider was loaded when the transform ran.
    NoModelLoaded,
    /// The model file was missing or failed to load.
    ModelLoadFailed,
    /// The inference request failed (network, HTTP error, timeout).
    InferenceError { detail: String },
    /// The model output was rejected by the validator. `kind` names the
    /// specific `Failure` variant that triggered the fallback.
    ValidationRejected { kind: String },
    /// The input was too short to polish (1-3 words); the model would treat
    /// it as a prompt to answer, not a transcript to clean.
    TooShort,
    /// The input + output budget exceeds the model's context window.
    ContextOverflow,
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::NoModelLoaded => write!(f, "No model loaded"),
            SkipReason::ModelLoadFailed => write!(f, "Model load failed"),
            SkipReason::InferenceError { detail } => write!(f, "Inference error: {detail}"),
            SkipReason::ValidationRejected { kind } => write!(f, "Polish rejected: {kind}"),
            SkipReason::TooShort => write!(f, "Too short to polish"),
            SkipReason::ContextOverflow => write!(f, "Input too large for model context"),
        }
    }
}

/// What a transform run costs/produced, for observability.
#[derive(Debug, Clone, Default)]
pub struct TransformMetrics {
    pub latency_ms: u128,
    pub fell_back: bool,
    pub failure: Option<Failure>,
    /// Why the transform fell back, when it did. `None` on a clean pass or a
    /// successful transform.
    pub skip_reason: Option<SkipReason>,
}

/// The result of running the pipeline's transform stage.
#[derive(Debug, Clone)]
pub struct TransformResult {
    /// Transformed text with placeholders intact, or the original input.
    pub text: String,
    /// True when the model output was used; false when we fell back.
    pub transformed: bool,
    pub metrics: TransformMetrics,
    /// The raw (protected) input text the transform ran on, preserved so the
    /// UI can send it back for the personalization feedback loop (T2.1).
    pub input_text: Option<String>,
}

/// EG-1 was fine-tuned on a fixed system+user split; its polish behavior
/// lives in the weights, not in per-mode prompt rules. Bypass ultra-short
/// input (LLMs treat 1–3 word inputs as prompts to answer), then run the
/// training-faithful messages with temperature 0 and a character-count cap
/// (`max(len, 256)`, CJK-safe).
fn run_eg1(
    provider: &dyn crate::llm::InferenceProvider,
    protected_input: &str,
    ctx: &PromptContext,
    on_token: &mut Option<&mut dyn FnMut(&str)>,
) -> Result<String, ()> {
    if eg1_too_short(protected_input, &ctx.language) {
        return Err(());
    }
    let (system, user) = build_eg1_messages(protected_input);
    let max_tokens = (protected_input.chars().count() as u32).max(256);
    let params = crate::llm::GenerationParams {
        max_tokens,
        temperature: 0.0,
        timeout: crate::llm::scaled_timeout(max_tokens),
        ..Default::default()
    };
    let raw = provider
        .generate_with_system_stream(&system, &user, params, &mut |tok| {
            if let Some(f) = on_token.as_mut() {
                f(tok);
            }
        })
        .map_err(|_| ())?;
    let cleaned = strip_eg1_tags(&raw);
    if cleaned.is_empty() {
        return Err(());
    }
    Ok(cleaned)
}

/// S1-mini (`superwhisper/s1-mini`) was fine-tuned on a fixed system prompt
/// plus a control line as the first user-message line. Bypass ultra-short
/// input, then run the card-faithful messages with temperature 0 and the
/// same CJK-safe character-count cap as EG-1.
fn run_s1(
    provider: &dyn crate::llm::InferenceProvider,
    protected_input: &str,
    ctx: &PromptContext,
    on_token: &mut Option<&mut dyn FnMut(&str)>,
) -> Result<String, ()> {
    if eg1_too_short(protected_input, &ctx.language) {
        return Err(());
    }
    let (system, user) = build_s1_messages(protected_input, &ctx.s1_control);
    let max_tokens = (protected_input.chars().count() as u32).max(256);
    let params = crate::llm::GenerationParams {
        max_tokens,
        temperature: 0.0,
        timeout: crate::llm::scaled_timeout(max_tokens),
        ..Default::default()
    };
    let raw = provider
        .generate_with_system_stream(&system, &user, params, &mut |tok| {
            if let Some(f) = on_token.as_mut() {
                f(tok);
            }
        })
        .map_err(|_| ())?;
    let cleaned = raw.trim().to_string();
    if cleaned.is_empty() {
        // The model card says filler-only input returns an empty string with
        // finish_reason: stop. That is a valid "nothing to clean" answer, not
        // a crash: pass the input through (it was filler anyway).
        return Err(());
    }
    Ok(cleaned)
}

/// Runs one transform through a provider. Model-agnostic and synchronous in
/// its contract: the caller decides threading.
///
/// `on_token` is an optional sink for streaming tokens (T1.1): when `Some`,
/// each token the provider emits is forwarded to it. `None` (all existing
/// callers) means no live preview; the transform still runs and returns the
/// full text unchanged.
/// Runs one transform over a possibly long transcript.
///
/// T1.3: very long transcripts are split into sentence-aligned chunks of at
/// most [`super::splitter::MAX_CHUNK_WORDS`] words, each chunk is transformed
/// on its own, and the results are re-joined with a single space. Short input
/// (the common case) returns a single chunk, so behaviour is unchanged. The
/// split happens on the *protected* input, so AutoText placeholders stay
/// atomic and survive the round-trip; the pipeline restores them afterwards.
pub fn run_transform_blocking(
    provider: &dyn crate::llm::InferenceProvider,
    transform: &TransformDefinition,
    protected_input: &str,
    ctx: &PromptContext,
    on_token: &mut Option<&mut dyn FnMut(&str)>,
) -> TransformResult {
    let chunks = super::splitter::split_for_polish(protected_input);
    if chunks.len() == 1 {
        return run_single_chunk(provider, transform, protected_input, ctx, on_token);
    }

    let mut full = String::new();
    let mut any_transformed = false;
    let mut total_latency = 0u128;
    for chunk in &chunks {
        let r = run_single_chunk(provider, transform, chunk, ctx, on_token);
        if !full.is_empty() {
            full.push(' ');
        }
        full.push_str(&r.text);
        any_transformed |= r.transformed;
        total_latency += r.metrics.latency_ms;
    }
    TransformResult {
        text: full,
        transformed: any_transformed,
        metrics: TransformMetrics {
            latency_ms: total_latency,
            fell_back: !any_transformed,
            failure: None,
            skip_reason: None,
        },
        input_text: None,
    }
}

/// Transforms a single (already <= [`super::splitter::MAX_CHUNK_WORDS`] word)
/// chunk. This is the original single-pass body.
fn run_single_chunk(
    provider: &dyn crate::llm::InferenceProvider,
    transform: &TransformDefinition,
    protected_input: &str,
    ctx: &PromptContext,
    on_token: &mut Option<&mut dyn FnMut(&str)>,
) -> TransformResult {
    let started = std::time::Instant::now();
    let is_eg1 = provider.model_id() == "eg-1";
    let is_s1 = provider.model_id() == "s1-mini";

    // Context preflight: refuse to send a request that cannot fit (prompt +
    // output cap + margin) in the model's window. Sending one anyway just
    // truncates and the fallback pastes raw text, which is the same outcome
    // with a wasted 20-60s of generation. Prompt tokens are estimated at
    // ~4 chars/token, the same shape the reference app uses.
    let prompt_chars = protected_input.chars().count() + 400; // system prompt + wrapper overhead
    if let Some(window) = provider.context_tokens() {
        let output_cap: u32 = if is_eg1 || is_s1 {
            (protected_input.chars().count() as u32).max(256)
        } else {
            ((protected_input.chars().count() / 3 + 100) as u32)
                .max(256)
                .min(2048)
        };
        let est_prompt_tokens = (prompt_chars as u32) / 4;
        if est_prompt_tokens + output_cap + 256 > window {
            tracing::info!(
                window,
                est_prompt_tokens,
                output_cap,
                "polish input too large for model context; passing through"
            );
            return TransformResult {
                text: protected_input.to_string(),
                transformed: false,
                metrics: TransformMetrics {
                    latency_ms: started.elapsed().as_millis(),
                    fell_back: true,
                    failure: None,
                    skip_reason: Some(SkipReason::ContextOverflow),
                },
                input_text: Some(protected_input.to_string()),
            };
        }
    }

    let mut outcome = if is_eg1 {
        match run_eg1(provider, protected_input, ctx, on_token) {
            Ok(cleaned) => {
                // Shared validator: preambles, fences, echo, growth, and
                // dropped placeholders still fall back as usual.
                let prompt = format!("{}\n{}", super::prompt::EG1_SYSTEM_PROMPT, protected_input);
                match validator::validate(&cleaned, protected_input, transform, &prompt) {
                    ValidatedOutput::Transformed(text) => TransformResult {
                        text,
                        transformed: true,
                        metrics: TransformMetrics {
                            latency_ms: started.elapsed().as_millis(),
                            ..Default::default()
                        },
                        input_text: Some(protected_input.to_string()),
                    },
                    ValidatedOutput::Fallback(orig, failure) => TransformResult {
                        text: orig,
                        transformed: false,
                        metrics: TransformMetrics {
                            latency_ms: started.elapsed().as_millis(),
                            fell_back: true,
                            failure,
                            skip_reason: failure.map(|f| SkipReason::ValidationRejected {
                                kind: f.to_string(),
                            }),
                        },
                        input_text: Some(protected_input.to_string()),
                    },
                }
            }
            Err(()) => {
                let reason = if eg1_too_short(protected_input, &ctx.language) {
                    Some(SkipReason::TooShort)
                } else {
                    Some(SkipReason::InferenceError {
                        detail: "EG-1 generation failed".into(),
                    })
                };
                TransformResult {
                    text: protected_input.to_string(),
                    transformed: false,
                    metrics: TransformMetrics {
                        latency_ms: started.elapsed().as_millis(),
                        fell_back: true,
                        failure: None,
                        skip_reason: reason,
                    },
                    input_text: Some(protected_input.to_string()),
                }
            }
        }
    } else if is_s1 {
        match run_s1(provider, protected_input, ctx, on_token) {
            Ok(cleaned) => {
                let prompt = format!("{}\n{}", super::prompt::S1_SYSTEM_PROMPT, protected_input);
                match validator::validate(&cleaned, protected_input, transform, &prompt) {
                    ValidatedOutput::Transformed(text) => TransformResult {
                        text,
                        transformed: true,
                        metrics: TransformMetrics {
                            latency_ms: started.elapsed().as_millis(),
                            ..Default::default()
                        },
                        input_text: Some(protected_input.to_string()),
                    },
                    ValidatedOutput::Fallback(orig, failure) => TransformResult {
                        text: orig,
                        transformed: false,
                        metrics: TransformMetrics {
                            latency_ms: started.elapsed().as_millis(),
                            fell_back: true,
                            failure,
                            skip_reason: failure.map(|f| SkipReason::ValidationRejected {
                                kind: f.to_string(),
                            }),
                        },
                        input_text: Some(protected_input.to_string()),
                    },
                }
            }
            Err(()) => {
                let reason = if eg1_too_short(protected_input, &ctx.language) {
                    Some(SkipReason::TooShort)
                } else {
                    Some(SkipReason::InferenceError {
                        detail: "S1 generation failed".into(),
                    })
                };
                TransformResult {
                    text: protected_input.to_string(),
                    transformed: false,
                    metrics: TransformMetrics {
                        latency_ms: started.elapsed().as_millis(),
                        fell_back: true,
                        failure: None,
                        skip_reason: reason,
                    },
                    input_text: Some(protected_input.to_string()),
                }
            }
        }
    } else {
        let prompt = build_prompt(transform, protected_input, ctx);
        // Both providers return Err when generation stops at the cap, and an
        // Err makes the engine fall back to the input: an undersized cap
        // silently discarded whole transformations. Scale the cap with the
        // input (~3 chars per token, CJK under-budgets so use the raw count
        // as the floor), matching the reference app's `max(len/3+100, 256)`
        // shape, bounded so prompt + output fit the local server's 4096-token
        // context.
        let max_tokens = ((protected_input.chars().count() / 3 + 100) as u32)
            .max(256)
            .min(2048);
        let params = crate::llm::GenerationParams {
            max_tokens,
            timeout: crate::llm::scaled_timeout(max_tokens),
            ..Default::default()
        };

        let raw = provider.generate_stream(&prompt, params, &mut |tok| {
            if let Some(f) = on_token.as_mut() {
                f(tok);
            }
        });
        match raw {
            Ok(raw) => match validator::validate(&raw, protected_input, transform, &prompt) {
                ValidatedOutput::Transformed(text) => TransformResult {
                    text,
                    transformed: true,
                    metrics: TransformMetrics {
                        latency_ms: started.elapsed().as_millis(),
                        ..Default::default()
                    },
                    input_text: Some(protected_input.to_string()),
                },
                ValidatedOutput::Fallback(orig, failure) => TransformResult {
                    text: orig,
                    transformed: false,
                    metrics: TransformMetrics {
                        latency_ms: started.elapsed().as_millis(),
                        fell_back: true,
                        failure,
                        skip_reason: failure.map(|f| SkipReason::ValidationRejected {
                            kind: f.to_string(),
                        }),
                    },
                    input_text: Some(protected_input.to_string()),
                },
            },
            Err(e) => {
                tracing::warn!(error = %e, "transform inference failed; falling back to input");
                TransformResult {
                    text: protected_input.to_string(),
                    transformed: false,
                    metrics: TransformMetrics {
                        latency_ms: started.elapsed().as_millis(),
                        fell_back: true,
                        failure: None,
                        skip_reason: Some(SkipReason::InferenceError { detail: e }),
                    },
                    input_text: Some(protected_input.to_string()),
                }
            }
        }
    };
    // Deterministic spoken-list formatting for the polish transforms: a
    // spoken enumeration must render as points whether or not the model
    // cooperates (EG-1's fixed training template cannot carry a list rule,
    // and its weights only bullet enumerations that have a lead-in). Runs on
    // whatever text survives this stage: model output, or the input on
    // fallback / too-short bypass.
    if matches!(
        transform.id.as_str(),
        "builtin-polish" | "builtin-professional"
    ) {
        outcome.text = format_spoken_lists(&outcome.text);
    }
    outcome
}

// ---------------------------------------------------------------------------
// Deterministic spoken-list formatting
//
// Formatting is application behavior, not model behavior: whether a spoken
// enumeration renders as points must not depend on prompt rules or model
// compliance. Two conservative, idempotent patterns are recognized:
//
// 1. Ordinal narration: >= 3 markers ("first", "second", ... plus "then" /
//    "finally") where the first marker is "first" becomes a bulleted list
//    with the markers dropped. This is EnviousWispr's "Spoken lists"
//    promise: its cloud/Apple prompts carry the rule, EG-1 does not.
// 2. Announced lists: a list-opener phrase before a colon ("bring the
//    following ...: apple, grapes, banana and onion") becomes one item per
//    line.
//
// Prose that merely mentions "first"/"then", sentence groups after a colon,
// and anything already bulleted is left untouched.
// ---------------------------------------------------------------------------

/// Longest ordinal segment we are willing to bullet (prose safety valve).
const MAX_SEGMENT_WORDS: usize = 20;
/// Longest item in an announced list ("the following: ...").
const MAX_ANNOUNCED_ITEM_WORDS: usize = 6;
/// Announced-list openers: the lead-in must contain one of these before the
/// colon, so an arbitrary colon ("Meeting notes: John will lead") never
/// fires.
const LIST_OPENERS: &[&str] = &[
    "the following",
    "as follows",
    "the items are",
    "the list is",
    "what i need",
    "things i need",
    "three things",
    "the steps are",
    "here's what",
    "here is what",
];
/// Function words that mark a punctuated-less tail as prose rather than bare
/// items: the word-splitting path runs only when every word is content
/// ("apple grapes banana onion"), never through these.
const LIST_STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "in", "on", "at", "for", "with", "from", "by", "and", "or",
    "but", "is", "are", "was", "were", "be", "been", "am", "it", "this", "that", "these", "those",
    "my", "your", "our", "their", "we", "i", "you", "he", "she", "they", "do", "did", "does",
    "will", "would", "can", "could", "have", "has", "had", "not", "no", "so", "as", "if", "then",
];
const ORDINAL_MARKERS: &[&str] = &[
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
];
const CHAIN_MARKERS: &[&str] = &["then", "finally"];

/// Word-boundary marker positions over an already-lowercased text.
fn find_marker_hits(lower: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(needle) {
        let abs = from + rel;
        let before_ok = lower[..abs]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after = abs + needle.len();
        let after_ok = lower[after..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            out.push(abs);
        }
        from = abs + needle.len();
    }
    out
}

/// Trims a segment sitting between two ordinal markers: surrounding
/// whitespace/punctuation, the "First of all" idiom's "of all", and dangling
/// conjunctions the marker split left behind ("... review and", "and then
/// ..."), since clauses joined by "and"/"but"/"so" are not list material
/// (EnviousWispr's local-prompt restraint).
fn clean_list_segment(raw: &str) -> String {
    let mut s: &str = raw.trim();
    loop {
        let before = s.len();
        s = s.trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '-')
        });
        if let Some(prefix) = s.get(..6) {
            if prefix.eq_ignore_ascii_case("of all") {
                s = &s[6..];
                continue;
            }
        }
        if s.len() == before {
            break;
        }
    }
    let s = s.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | ';' | ':'));

    const JOINERS: &[&str] = &["and", "or", "but", "so", "then"];
    let mut words: Vec<&str> = s.split_whitespace().collect();
    while words
        .first()
        .is_some_and(|w| JOINERS.contains(&w.to_ascii_lowercase().as_str()))
    {
        words.remove(0);
    }
    while words
        .last()
        .is_some_and(|w| JOINERS.contains(&w.to_ascii_lowercase().as_str()))
    {
        words.pop();
    }
    words.join(" ")
}

/// Uppercases the first letter, leaving camelCase brand starts ("iPhone")
/// alone.
fn capitalize_first(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut done = false;
    for (i, c) in s.char_indices() {
        if !done && c.is_alphabetic() {
            let second = s[i + c.len_utf8()..].chars().next();
            if i == 0 && matches!(second, Some(n) if n.is_uppercase()) {
                out.push(c);
            } else {
                out.extend(c.to_uppercase());
            }
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// Splits a chunk on the conjunctions " and "/" or " (case-insensitive).
fn split_on_and(chunk: &str) -> Vec<&str> {
    let lower = chunk.to_ascii_lowercase();
    let mut parts = Vec::new();
    let mut start = 0;
    loop {
        let mut next: Option<(usize, usize)> = None;
        for needle in [" and ", " or "] {
            if let Some(p) = lower[start..].find(needle) {
                let abs = start + p;
                if next.is_none_or(|(np, _)| abs < np) {
                    next = Some((abs, needle.len()));
                }
            }
        }
        match next {
            Some((pos, nlen)) => {
                parts.push(&chunk[start..pos]);
                start = pos + nlen;
            }
            None => {
                parts.push(&chunk[start..]);
                break;
            }
        }
    }
    parts
}

/// Cleans one announced-list item: leading/trailing punctuation and a
/// conjunction that opened the chunk ("and onion" -> "onion").
fn clean_announced_item(raw: &str) -> String {
    let mut s: &str = raw.trim();
    loop {
        let before = s.len();
        s = s.trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '-')
        });
        if let Some(p) = s.get(..4) {
            if p.eq_ignore_ascii_case("and ") {
                s = &s[4..];
                continue;
            }
        }
        if let Some(p) = s.get(..3) {
            if p.eq_ignore_ascii_case("or ") {
                s = &s[3..];
                continue;
            }
        }
        if s.len() == before {
            break;
        }
    }
    s.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | ';' | ':'))
        .to_string()
}

/// True when every word is content (no function words): safe to split a
/// punctuation-less tail into one item per word.
fn is_bare_content_words(words: &[&str]) -> bool {
    (3..=8).contains(&words.len())
        && words.iter().all(|w| {
            let key = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            !key.is_empty() && !LIST_STOPWORDS.contains(&key.as_str())
        })
}

/// Splits announced-list text (after the colon) into items. Returns None
/// when the shape is prose rather than a list.
fn split_announced_items(rest: &str) -> Option<Vec<String>> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }

    // Marker-shaped output: the model already wrote "- " markers but crammed
    // them onto one line, the exact failure EnviousWispr's judge scores as
    // major_fail "wrong_format" ("merges several items onto one line"). A tail
    // that opens with a marker is that shape, so split on the markers instead
    // of commas, conjunctions, or words. Hyphen runs that do not open the tail
    // ("bring - laptop - charger") stay prose.
    let opened_with_marker = rest.starts_with("- ") || rest.starts_with("\u{2022} ");
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("\u{2022} "))
        .unwrap_or(rest);

    let raw_chunks: Vec<&str> = if opened_with_marker && rest.contains("- ") {
        rest.split("- ").collect()
    } else if rest.contains(',') {
        rest.split(',').collect()
    } else {
        let lower = rest.to_ascii_lowercase();
        if lower.contains(" and ") || lower.contains(" or ") {
            split_on_and(rest)
        } else {
            // No punctuation at all: split on words only when every word is
            // content.
            let words: Vec<&str> = rest.split_whitespace().collect();
            if !is_bare_content_words(&words) {
                return None;
            }
            return Some(words.into_iter().map(clean_announced_item).collect());
        }
    };

    // Split chunks further on conjunctions, clean, drop empties.
    let mut items: Vec<String> = Vec::new();
    for chunk in raw_chunks {
        for part in split_on_and(chunk) {
            let item = clean_announced_item(part);
            if !item.is_empty() {
                items.push(item);
            }
        }
    }

    // Fewer than three items: promote multi-word content items to their own
    // words before giving up ("apple grapes banana and onion" conjunct-splits
    // into two chunks; only the word split reaches three).
    if items.len() < 3 {
        let mut expanded: Vec<String> = Vec::new();
        let mut grew = false;
        for item in &items {
            let words: Vec<&str> = item.split_whitespace().collect();
            if is_bare_content_words(&words) {
                for w in words {
                    expanded.push(clean_announced_item(w));
                }
                grew = true;
            } else {
                expanded.push(item.clone());
            }
        }
        if grew {
            items = expanded;
        }
    }

    if items.len() < 3 {
        return None;
    }
    if items
        .iter()
        .any(|i| i.split_whitespace().count() > MAX_ANNOUNCED_ITEM_WORDS)
    {
        return None;
    }
    Some(items)
}

/// Pattern 1: ordinal narration ("first X, then Y, finally Z") -> bullets.
fn format_ordinal_list(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    // (start, len, is_ordinal, is_first)
    let mut hits: Vec<(usize, usize, bool, bool)> = Vec::new();
    for m in ORDINAL_MARKERS {
        for p in find_marker_hits(&lower, m) {
            hits.push((p, m.len(), true, *m == "first"));
        }
    }
    for m in CHAIN_MARKERS {
        for p in find_marker_hits(&lower, m) {
            hits.push((p, m.len(), false, false));
        }
    }
    hits.sort_by_key(|h| h.0);
    if hits.len() < 3 || !hits[0].3 {
        return None;
    }

    let lead = text[..hits[0].0].trim();
    let mut segments = Vec::with_capacity(hits.len());
    for (i, &(start, len, _, _)) in hits.iter().enumerate() {
        let seg_end = hits.get(i + 1).map(|h| h.0).unwrap_or(text.len());
        segments.push(clean_list_segment(&text[start + len..seg_end]));
    }

    if segments.iter().all(|s| s.is_empty()) {
        // Bare "first, second, third": the markers are the content. Only
        // when there is no lead-in and every marker is an ordinal, so prose
        // like "I came first then second then third in line" (which has a
        // lead-in) never turns into bullets.
        if !lead.is_empty() || !hits.iter().all(|h| h.2) {
            return None;
        }
        let items: Vec<String> = hits
            .iter()
            .map(|&(start, len, _, _)| {
                format!("- {}.", capitalize_first(&text[start..start + len]))
            })
            .collect();
        return Some(items.join("\n"));
    }
    if segments.iter().any(|s| s.is_empty()) {
        return None;
    }
    for s in &segments {
        if s.split_whitespace().count() > MAX_SEGMENT_WORDS {
            return None;
        }
        if s.starts_with("- ") || s.starts_with("\u{2022} ") {
            return None;
        }
    }

    let mut out = String::new();
    if !lead.is_empty() {
        // The lead-in is the speaker's words, kept verbatim (EnviousWispr's
        // rule: "The lead-in is their words and is never dropped"), only
        // capitalized like the sentence it is.
        let lead = capitalize_first(lead.trim_end());
        out.push_str(&lead);
        if !matches!(
            lead.chars().last(),
            Some(':') | Some('.') | Some('?') | Some('!')
        ) {
            out.push(':');
        }
        out.push('\n');
    }
    let lines: Vec<String> = segments
        .iter()
        .map(|s| {
            let mut item = capitalize_first(s);
            if !matches!(item.chars().last(), Some('.' | '!' | '?' | ':' | ';')) {
                item.push('.');
            }
            format!("- {item}")
        })
        .collect();
    out.push_str(&lines.join("\n"));
    Some(out)
}

/// Pattern 2: announced lists ("bring the following ...: apple, grapes") ->
/// one item per line.
fn format_announced_list(text: &str) -> Option<String> {
    let trigger = text.char_indices().find_map(|(i, c)| {
        if c != ':' {
            return None;
        }
        // Skip time-like colons ("10:30"): digit on both sides.
        let prev = text[..i].chars().next_back();
        let next = text[i + 1..].chars().next();
        if matches!(prev, Some(d) if d.is_ascii_digit())
            && matches!(next, Some(d) if d.is_ascii_digit())
        {
            return None;
        }
        let lead = text[..i].to_ascii_lowercase();
        LIST_OPENERS.iter().any(|o| lead.contains(o)).then_some(i)
    })?;
    let lead = text[..trigger].trim();
    let items = split_announced_items(&text[trigger + 1..])?;

    let mut out = String::with_capacity(text.len());
    out.push_str(&capitalize_first(lead));
    out.push_str(":\n");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str("- ");
        // Every EnviousWispr worked example ends each item with a period
        // ("- Call the supplier."); their judge treats punctuation as an
        // allowed variant, so only add it when the item lacks terminal marks.
        let mut line = capitalize_first(item);
        if !matches!(line.chars().last(), Some('.' | '!' | '?' | ':' | ';')) {
            line.push('.');
        }
        out.push_str(&line);
    }
    Some(out)
}

/// Pattern 3: model output that committed to a list but left every "- "
/// marker on one line ("... three tasks that you need to do. - get the
/// groceries - call the doctor - fill the petrol"). EnviousWispr's judge
/// scores that shape as major_fail ("merges several items onto one line");
/// their fix is prompt-side (their L1 wants the lead-in "ending with a
/// colon"), ours is repair: keep the lead-in, normalize a terminal period to
/// a colon, one item per line. Gated on a punctuation-terminated lead-in plus
/// at least three markers with short items, so prose hyphen runs ("stunned -
/// truly - speechless") never fire.
fn format_inline_marker_run(text: &str) -> Option<String> {
    if text.contains('\n') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut markers: Vec<usize> = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'-'
            || bytes.get(i + 1) != Some(&b' ')
            || !(i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            continue;
        }
        markers.push(i);
    }
    if markers.len() < 3 {
        return None;
    }
    let lead = text[..markers[0]].trim_end();
    if lead.is_empty() || !matches!(lead.chars().last(), Some('.' | ':' | '?' | '!' | ';')) {
        return None;
    }
    let mut items: Vec<String> = Vec::with_capacity(markers.len());
    for (k, &m) in markers.iter().enumerate() {
        let start = m + 2; // skip the "- " marker itself
        let end = markers.get(k + 1).copied().unwrap_or(text.len());
        let item = clean_announced_item(&text[start..end]);
        if item.is_empty() || item.split_whitespace().count() > MAX_ANNOUNCED_ITEM_WORDS {
            return None;
        }
        items.push(item);
    }

    let mut lead = capitalize_first(lead);
    if lead.ends_with('.') {
        // EnviousWispr's local prompt: the lead-in sits on its own line
        // "ending with a colon"; the model wrote a period instead.
        lead.pop();
        lead.push(':');
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&lead);
    out.push('\n');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut line = capitalize_first(item);
        if !matches!(line.chars().last(), Some('.' | '!' | '?' | ':' | ';')) {
            line.push('.');
        }
        out.push_str("- ");
        out.push_str(&line);
    }
    Some(out)
}

/// Deterministic spoken-list formatting (see module comment above). Applied
/// to the polish transforms only; rewriter/prompt-engineer output is left
/// to its own instruction.
fn format_spoken_lists(text: &str) -> String {
    // Already a list: leave it (also makes this pass idempotent).
    if text.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("- ") || t.starts_with("\u{2022} ")
    }) {
        return text.to_string();
    }
    format_ordinal_list(text)
        .or_else(|| format_announced_list(text))
        .or_else(|| format_inline_marker_run(text))
        .unwrap_or_else(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transforms::{TransformDefinition, TransformStore};

    fn polish() -> TransformDefinition {
        TransformStore::with_built_ins()
            .get("builtin-polish")
            .unwrap()
            .clone()
    }

    struct Scripted(Option<String>);
    impl crate::llm::InferenceProvider for Scripted {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn generate(
            &self,
            _prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            match &self.0 {
                Some(out) => Ok(out.clone()),
                None => Err("model crashed".into()),
            }
        }
    }

    fn scripted(out: Option<&str>) -> Scripted {
        Scripted(out.map(str::to_string))
    }

    struct StreamingMock;
    impl crate::llm::InferenceProvider for StreamingMock {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn generate(
            &self,
            _prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            Ok("Hello".into())
        }
        fn generate_stream(
            &self,
            _prompt: &str,
            _params: crate::llm::GenerationParams,
            on_token: &mut dyn FnMut(&str),
        ) -> Result<String, String> {
            on_token("Hel");
            on_token("lo");
            Ok("Hello".into())
        }
    }

    #[test]
    fn streaming_tokens_are_forwarded_in_order() {
        let input = "hey john can you send the proposal";
        let mut seen: Vec<String> = Vec::new();
        let mut push_token = |tok: &str| seen.push(tok.to_string());
        let mut sink: Option<&mut dyn FnMut(&str)> = Some(&mut push_token);
        let result = run_transform_blocking(
            &StreamingMock,
            &polish(),
            input,
            &PromptContext::default(),
            &mut sink,
        );
        assert_eq!(seen, vec!["Hel", "lo"], "tokens must arrive in order");
        assert!(
            result.transformed,
            "validator must accept the streamed output"
        );
        assert_eq!(
            result.text, "Hello",
            "full text must be the concatenated stream"
        );
    }

    #[test]
    fn long_input_is_split_and_rejoined() {
        // A transcript far over MAX_CHUNK_WORDS must be split into multiple
        // chunks and re-joined without losing or reordering content. The mock
        // returns "Hello" for every chunk, which the validator rejects against
        // a 500-word input (it is not a valid transform of it), so each chunk
        // falls back to its own raw text. The split/rejoin must therefore
        // reassemble the original words exactly.
        let words: Vec<String> = (0..1200).map(|i| format!("w{i}")).collect();
        let input = words.join(" ");
        let result = run_transform_blocking(
            &StreamingMock,
            &polish(),
            &input,
            &PromptContext::default(),
            &mut None,
        );
        // 1200 words at 500/chunk => 3 chunks.
        let chunks = super::super::splitter::split_for_polish(&input);
        assert_eq!(chunks.len(), 3, "1200 words must split into 3 chunks");
        // Every word survives, in order (chunks re-joined by single spaces).
        let out_words: Vec<&str> = result.text.split_whitespace().collect();
        assert_eq!(out_words.len(), 1200, "no word may be lost");
        assert_eq!(out_words.first().copied(), Some("w0"));
        assert_eq!(out_words.last().copied(), Some("w1199"));
        for (i, w) in out_words.iter().enumerate() {
            assert_eq!(*w, words[i].as_str(), "word at {i} reordered/lost");
        }
    }

    #[test]
    fn clean_output_is_used() {
        let input = "hey john can you send the proposal";
        let result = run_transform_blocking(
            &scripted(Some("Hi John, can you send the proposal?")),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(result.transformed);
        assert_eq!(result.text, "Hi John, can you send the proposal?");
    }

    #[test]
    fn inference_error_falls_back_to_input() {
        let input = "my text here";
        let result = run_transform_blocking(
            &scripted(None),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(!result.transformed);
        assert_eq!(result.text, input);
        assert!(result.metrics.fell_back);
    }

    #[test]
    fn rejected_output_falls_back_to_input() {
        let input = "send the report to finance";
        let result = run_transform_blocking(
            &scripted(Some(
                "Here is the polished version: Send the report to finance.",
            )),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(!result.transformed);
        assert_eq!(result.text, input);
    }

    #[test]
    fn inference_error_sets_skip_reason() {
        let input = "my text here";
        let result = run_transform_blocking(
            &scripted(None),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(!result.transformed);
        assert!(result.metrics.fell_back);
        match &result.metrics.skip_reason {
            Some(SkipReason::InferenceError { detail }) => {
                assert!(detail.contains("model crashed"), "got: {detail}");
            }
            other => panic!("expected InferenceError, got: {other:?}"),
        }
    }

    #[test]
    fn validation_rejected_sets_skip_reason() {
        let input = "send the report to finance";
        let result = run_transform_blocking(
            &scripted(Some(
                "Here is the polished version: Send the report to finance.",
            )),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(!result.transformed);
        match &result.metrics.skip_reason {
            Some(SkipReason::ValidationRejected { kind }) => {
                assert!(kind.contains("preamble"), "got: {kind}");
            }
            other => panic!("expected ValidationRejected, got: {other:?}"),
        }
    }

    #[test]
    fn clean_output_has_no_skip_reason() {
        let input = "hey john can you send the proposal";
        let result = run_transform_blocking(
            &scripted(Some("Hi John, can you send the proposal?")),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(result.transformed);
        assert!(result.metrics.skip_reason.is_none());
    }

    #[test]
    fn placeholder_survival_is_enforced() {
        let input = "send to {{AUTOTEXT_0}} ok";
        let result = run_transform_blocking(
            &scripted(Some("Send it to the address ok")),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(!result.transformed, "dropped placeholder must fall back");
        assert_eq!(result.text, input);
    }

    // --- deterministic spoken-list formatting ---------------------------

    #[test]
    fn ordinal_enumeration_becomes_bullets() {
        assert_eq!(
            format_spoken_lists("first buy the tickets second book the hotel third pack the bags"),
            "- Buy the tickets.\n- Book the hotel.\n- Pack the bags."
        );
    }

    #[test]
    fn ordinal_enumeration_keeps_lead_in() {
        assert_eq!(
            format_spoken_lists(
                "there are three jobs before we leave first call the supplier second restock the shelves third lock the back door"
            ),
            "There are three jobs before we leave:\n- Call the supplier.\n- Restock the shelves.\n- Lock the back door."
        );
    }

    #[test]
    fn website_spoken_list_example_formats() {
        assert_eq!(
            format_spoken_lists("first the installer, then the migration, finally the docs"),
            "- The installer.\n- The migration.\n- The docs."
        );
    }

    #[test]
    fn bare_ordinals_become_bullets() {
        assert_eq!(
            format_spoken_lists("first second third"),
            "- First.\n- Second.\n- Third."
        );
        assert_eq!(
            format_spoken_lists("First, second, third."),
            "- First.\n- Second.\n- Third."
        );
    }

    #[test]
    fn model_ordinal_prose_converted() {
        assert_eq!(
            format_spoken_lists(
                "First, buy the tickets. Second, book the hotel. Third, pack the bags."
            ),
            "- Buy the tickets.\n- Book the hotel.\n- Pack the bags."
        );
    }

    #[test]
    fn first_of_all_idiom_handled() {
        assert_eq!(
            format_spoken_lists("first of all buy milk second call mom third walk the dog"),
            "- Buy milk.\n- Call mom.\n- Walk the dog."
        );
    }

    #[test]
    fn ordinal_prose_untouched() {
        for input in [
            "I will review the PR first then merge it",
            "I came first then second then third in line",
            "First National Bank opened a branch downtown",
            "we should first discuss the budget carefully before committing",
            "bring milk eggs bread and butter for the party tonight",
            "first buy milk second call mom",
        ] {
            assert_eq!(format_spoken_lists(input), input, "{input}");
        }
    }

    #[test]
    fn ew_style_announced_openers_format() {
        assert_eq!(
            format_spoken_lists("The steps are: warm up, stretch, cool down"),
            "The steps are:\n- Warm up.\n- Stretch.\n- Cool down."
        );
    }

    #[test]
    fn dangling_conjunctions_stripped_from_segments() {
        assert_eq!(
            format_spoken_lists("first review the pr and then merge it and then deploy everything"),
            "- Review the pr.\n- Merge it.\n- Deploy everything."
        );
    }

    #[test]
    fn announced_grocery_list_with_commas_formats() {
        assert_eq!(
            format_spoken_lists(
                "Bring the following from the grocery market: apple, grapes, banana and onion"
            ),
            "Bring the following from the grocery market:\n- Apple.\n- Grapes.\n- Banana.\n- Onion."
        );
    }

    #[test]
    fn announced_grocery_list_without_commas_formats() {
        assert_eq!(
            format_spoken_lists(
                "Bring the following from the grocery market: apple grapes banana onion"
            ),
            "Bring the following from the grocery market:\n- Apple.\n- Grapes.\n- Banana.\n- Onion."
        );
    }

    #[test]
    fn model_inline_hyphen_run_explodes_to_lines() {
        // The exact shape EG-1 produced for a live dictation
        // (dictation.json 2026-09-23 12:45): spoken ordinals consumed, "- "
        // markers crammed onto one line. EnviousWispr's judge scores this
        // major_fail ("merges several items onto one line") but their EG-1
        // route has no deterministic repair; ours does.
        assert_eq!(
            format_spoken_lists(
                "Write an email to George asking for three things: - the report - what happened to the test - any updates on the expo"
            ),
            "Write an email to George asking for three things:\n\
             - The report.\n\
             - What happened to the test.\n\
             - Any updates on the expo."
        );
    }

    #[test]
    fn announced_lead_in_with_spoken_ordinals_formats() {
        // Same dictation with the ordinals still present (raw/fallback
        // path): the lead-in keeps its colon, ordinals become one item per
        // line, matching EnviousWispr's rule 11 worked example.
        assert_eq!(
            format_spoken_lists(
                "Write an email to George asking for three things: first the report second what happened to the test third any updates on the expo"
            ),
            "Write an email to George asking for three things:\n\
             - The report.\n\
             - What happened to the test.\n\
             - Any updates on the expo."
        );
    }

    #[test]
    fn hyphen_run_not_opening_the_tail_stays_prose() {
        let input = "The steps are: warm up - then stretch - then cool down";
        assert_eq!(format_spoken_lists(input), input);
    }

    #[test]
    fn model_marker_run_after_period_lead_in_repairs() {
        // Live dictation (2026-09-23): EG-1 ended the lead-in with a period
        // instead of a colon and left every marker on one line. The lead-in's
        // period is normalized to a colon per EnviousWispr's L1 rule.
        assert_eq!(
            format_spoken_lists(
                "Hello, there are three tasks that you need to do. - get the groceries - call the doctor - fill the petrol"
            ),
            "Hello, there are three tasks that you need to do:\n\
             - Get the groceries.\n\
             - Call the doctor.\n\
             - Fill the petrol."
        );
    }

    #[test]
    fn marker_run_without_lead_in_terminal_punctuation_stays_prose() {
        let input = "It was a - quick fix - or so - we thought in the end";
        assert_eq!(format_spoken_lists(input), input);
    }

    #[test]
    fn two_marker_run_stays_untouched() {
        let input = "Tasks for today: - get the groceries - call the doctor";
        assert_eq!(format_spoken_lists(input), input);
    }

    #[test]
    fn announced_list_with_sentences_untouched() {
        let input = "As follows: John will lead the meeting, Sarah will write minutes and Bob will book the big room today";
        assert_eq!(format_spoken_lists(input), input);
    }

    #[test]
    fn colon_without_opener_untouched() {
        let input =
            "Meeting notes: John will lead, Sarah will write the minutes, Bob will take attendance";
        assert_eq!(format_spoken_lists(input), input);
    }

    #[test]
    fn announced_two_items_untouched() {
        let input = "As follows: milk and eggs";
        assert_eq!(format_spoken_lists(input), input);
    }

    #[test]
    fn already_bulleted_text_is_idempotent() {
        let list = "- Buy the tickets.\n- Book the hotel.\n- Pack the bags.";
        assert_eq!(format_spoken_lists(list), list);
        let announced = "Bring the following:\n- Apple\n- Grapes\n- Banana";
        assert_eq!(format_spoken_lists(announced), announced);
    }

    #[test]
    fn placeholders_survive_list_formatting() {
        let input = "first reply to {{AUTOTEXT_0}} second close the ticket third update the doc";
        let out = format_spoken_lists(input);
        assert!(out.contains("{{AUTOTEXT_0}}"), "placeholder lost: {out}");
        assert!(out.starts_with("- Reply to {{AUTOTEXT_0}}."));
    }

    #[test]
    fn george_groceries_spoken_list_formats() {
        assert_eq!(
            format_spoken_lists(
                "Hey George, bring the groceries. In this, you need to bring three items: first, carrot; second, apple; third, mangoes."
            ),
            "Hey George, bring the groceries. In this, you need to bring three items:\n- Carrot.\n- Apple.\n- Mangoes."
        );
    }

    #[test]
    fn george_groceries_model_prose_is_repaired() {
        assert_eq!(
            format_spoken_lists(
                "Hey George, bring the groceries. You need to bring three items: first carrot, second apple and third mangoes."
            ),
            "Hey George, bring the groceries. You need to bring three items:\n- Carrot.\n- Apple.\n- Mangoes."
        );
    }

    #[test]
    fn polish_output_gets_spoken_list_formatting() {
        let result = run_transform_blocking(
            &scripted(Some(
                "First, buy the tickets. Second, book the hotel. Third, pack the bags.",
            )),
            &polish(),
            "first buy the tickets second book the hotel third pack the bags",
            &PromptContext::default(),
            &mut None,
        );
        assert!(result.transformed);
        assert_eq!(
            result.text,
            "- Buy the tickets.\n- Book the hotel.\n- Pack the bags."
        );
    }

    #[test]
    fn fallback_still_gets_list_formatting() {
        // Deterministic stage, not a model transform: `transformed` stays
        // false while the text still renders as points.
        let input = "first buy the tickets second book the hotel third pack the bags";
        let result = run_transform_blocking(
            &scripted(None),
            &polish(),
            input,
            &PromptContext::default(),
            &mut None,
        );
        assert!(!result.transformed);
        assert_eq!(
            result.text,
            "- Buy the tickets.\n- Book the hotel.\n- Pack the bags."
        );
    }

    #[test]
    fn rewriter_output_not_list_reformatted() {
        let rewriter = TransformStore::with_built_ins()
            .get("builtin-rewriter")
            .unwrap()
            .clone();
        let out = "First, buy the tickets. Second, book the hotel. Third, pack the bags.";
        let result = run_transform_blocking(
            &scripted(Some(out)),
            &rewriter,
            "first buy the tickets second book the hotel third pack the bags",
            &PromptContext::default(),
            &mut None,
        );
        assert_eq!(
            result.text, out,
            "only polish transforms get list formatting"
        );
    }
}
