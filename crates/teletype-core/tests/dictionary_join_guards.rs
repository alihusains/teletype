//! Word-join guards for the fuzzy dictionary fixer.
//!
//! claim: text-integrity
//!
//! The fixer joins tokens before matching, which can swallow a sentence
//! boundary ("still open. AI" -> "still OpenAI") or a standalone function
//! word ("a PI" -> "API"). The two guards
//! [`slice_crosses_punctuation`](teletype_core::dictionary::slice_crosses_punctuation)
//! and [`span_joins_through_stopword`](teletype_core::dictionary::span_joins_through_stopword)
//! gate every claimed multi-word span back against the ORIGINAL token text
//! (punctuation included). They are applied at the span claim in
//! `correct_with_dictionary` (the pass-1 `pass1_consumed` loop).
//!
//! `correct_with_dictionary` is private to `pipeline.rs`, so this integration
//! test drives the exact same logic the pipeline runs: the pass-1
//! [`rescore_words`](teletype_core::rescoring::rescore_words) exact/alias
//! claim, followed by the same `pass1_consumed` gating the pipeline applies,
//! with the per-token tiers falling through to "keep the token as-is".
//!
//! Dictionary/pack-term setup reuses the exact pattern of the existing tests
//! in `pipeline.rs` (`dict_with` + [`EditDistanceChecker`]); the rescoring
//! terms are built the way `correct_with_dictionary` builds them.
//!
//! ```text
//! cargo test -p teletype-core --test dictionary_join_guards
//! ```

use teletype_core::dictionary::{
    slice_crosses_punctuation, span_joins_through_stopword, Dictionary, DictionaryWord,
};
use teletype_core::rescoring::{rescore_words, RescoreTerm};
use teletype_core::vocab::PackTerm;

/// One dictionary word per canonical spelling. Reuses the exact pattern of the
/// pipeline tests' `dict_with` helper (`DictionaryWord::new` +
/// `Dictionary::insert`; user-taught words are fuzzy-eligible).
fn dict_with(words: &[&str]) -> Dictionary {
    let mut d = Dictionary::default();
    for w in words {
        d.insert(DictionaryWord::new(*w, "")).unwrap();
    }
    d
}

/// Build rescoring terms the way `correct_with_dictionary` does: one
/// `RescoreTerm` per dictionary word, weight 1.0, plus any vocabulary-pack
/// terms.
fn rescore_terms(dictionary: &Dictionary, pack_terms: &[PackTerm]) -> Vec<RescoreTerm> {
    let mut terms: Vec<RescoreTerm> = dictionary
        .words
        .iter()
        .map(|w| RescoreTerm {
            text: w.word.clone(),
            aliases: w.aliases.clone(),
            weight: 1.0,
        })
        .collect();
    terms.extend(pack_terms.iter().map(|pt| RescoreTerm {
        text: pt.canonical.clone(),
        aliases: vec![pt.alias.clone()],
        weight: 1.0,
    }));
    terms
}

/// The bare tokens the pipeline's pass 1 matches on: the original text with
/// punctuation stripped and lowercased (exactly what
/// `correct_with_dictionary` builds for `rescore_words`).
fn bare_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        })
        .collect()
}

/// Drive the pass-1 exact/alias span claim through [`rescore_words`], then
/// apply the SAME `pass1_consumed` gating `correct_with_dictionary` uses:
/// a multi-word claim that crosses punctuation or joins through a stopword is
/// released back to the per-token tiers (which then keep the token as-is),
/// instead of being spliced in as a joined canonical word. Single-token
/// claims always stand.
///
/// The tokens carry low confidence (0.5), the way a misheard ASR token does:
/// `rescore_words` only attempts the exact/alias span match for low-confidence
/// words (the `LOW_CONF` gate), which is exactly the situation the guards
/// gate. Confidence is the same for every token.
///
/// Returns the corrected token stream (the pipeline joins it with single
/// spaces) and the claims that actually survived the guard.
fn correct_span_claim(
    text: &str,
    dictionary: &Dictionary,
    pack_terms: &[PackTerm],
) -> (Vec<String>, Vec<teletype_core::rescoring::Replacement>) {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let bare = bare_tokens(text);
    let terms = rescore_terms(dictionary, pack_terms);
    if terms.is_empty() || bare.is_empty() {
        return (
            tokens.into_iter().map(|t| t.to_string()).collect(),
            Vec::new(),
        );
    }
    let (pass1_words, pass1_reps) = rescore_words(&bare, &vec![0.5f32; bare.len()], &terms);

    // Which token positions pass 1 consumed — the EXACT gating the pipeline
    // applies (multi-word claims are re-checked against the original text).
    let mut pass1_consumed = vec![false; tokens.len()];
    let mut kept_reps = Vec::new();
    for rep in &pass1_reps {
        let (start, end) = (rep.start, rep.end);
        if end - start >= 2
            && (slice_crosses_punctuation(&tokens, start, end, text)
                || span_joins_through_stopword(&tokens, start, end, &rep.replacement))
        {
            continue;
        }
        for slot in pass1_consumed.iter_mut().take(end).skip(start) {
            *slot = true;
        }
        kept_reps.push(rep.clone());
    }

    // Emit the span as its canonical word (first consumed token keeps leading
    // punctuation, last keeps trailing); unconsumed tokens are kept as-is
    // (the per-token tiers find no match and preserve the original).
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut j = 0;
    while j < tokens.len() {
        if pass1_consumed[j] {
            let start = j;
            while j < tokens.len() && pass1_consumed[j] {
                j += 1;
            }
            let last = j - 1;
            let canonical = pass1_words
                .get(start)
                .filter(|w| !w.is_empty())
                .cloned()
                .unwrap_or_default();
            let leading: String = tokens[start]
                .chars()
                .take_while(|c| !c.is_alphanumeric())
                .collect();
            let trailing: String = tokens[last]
                .chars()
                .rev()
                .take_while(|c| !c.is_alphanumeric())
                .collect();
            out.push(format!("{leading}{canonical}{trailing}"));
            continue;
        }
        out.push(tokens[j].to_string());
        j += 1;
    }
    (out, kept_reps)
}

// ===========================================================================
// Task cases 1-3: the rewrite must NOT happen
// ===========================================================================

#[test]
fn case1_sentence_boundary_is_not_rewritten() {
    // "still open. AI" is not rewritten to "still OpenAI".
    let d = dict_with(&["OpenAI"]);
    let text = "still open. AI";
    let (out, kept) = correct_span_claim(text, &d, &[]);
    assert!(
        kept.iter().all(|r| r.end - r.start < 2),
        "the 2-word claim across the sentence boundary must be released, not spliced in"
    );
    assert_eq!(
        out.join(" "),
        text,
        "no rewrite across the sentence boundary; punctuation and tokens are preserved"
    );
    // The guard sees the original token text (with punctuation) and blocks.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    assert!(
        slice_crosses_punctuation(&tokens, 1, 3, text),
        "the \".\" glued to \"open\" must block the span on the original token text"
    );
}

#[test]
fn case2_stopword_is_not_rewritten() {
    // "a PI" is not rewritten to "API".
    let d = dict_with(&["API"]);
    let text = "a PI";
    let (out, kept) = correct_span_claim(text, &d, &[]);
    assert!(
        kept.iter().all(|r| r.end - r.start < 2),
        "the 2-word claim that swallows the stopword \"a\" must be released"
    );
    assert_eq!(
        out.join(" "),
        text,
        "no rewrite when a standalone stopword would be absorbed"
    );
    let tokens: Vec<&str> = text.split_whitespace().collect();
    assert!(
        span_joins_through_stopword(&tokens, 0, 2, "API"),
        "\"a\" is a standalone stopword the span would absorb"
    );
}

#[test]
fn case3_comma_is_not_rewritten() {
    // "open, AI" is not rewritten to "openAI".
    let d = dict_with(&["openAI"]);
    let text = "open, AI";
    let (out, kept) = correct_span_claim(text, &d, &[]);
    assert!(
        kept.iter().all(|r| r.end - r.start < 2),
        "the 2-word claim across the comma must be released"
    );
    assert_eq!(
        out.join(" "),
        text,
        "no rewrite across the comma; tokens are preserved"
    );
    let tokens: Vec<&str> = text.split_whitespace().collect();
    assert!(
        slice_crosses_punctuation(&tokens, 0, 2, text),
        "the \",\" glued to \"open\" must block the span on the original token text"
    );
}

// ===========================================================================
// Task cases 4-6: genuine hits must still fire
// ===========================================================================

#[test]
fn case4_genuine_multiword_hit_still_fires() {
    // A user term "open ai" written plainly in one sentence is corrected to
    // its canonical spelling.
    let d = dict_with(&["Open AI"]);
    let text = "we use open ai for the model";
    let (out, kept) = correct_span_claim(text, &d, &[]);
    assert!(
        kept.iter()
            .any(|r| r.end - r.start >= 2 && r.replacement == "Open AI"),
        "the genuine multi-word hit must be claimed and spliced in"
    );
    assert_eq!(
        out.join(" "),
        "we use Open AI for the model",
        "canonical spelling is spliced in for the genuine hit"
    );
    // No punctuation or stopword in between: the guard lets the span stand.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    assert!(!slice_crosses_punctuation(&tokens, 2, 4, text));
    assert!(!span_joins_through_stopword(&tokens, 2, 4, "Open AI"));
}

#[test]
fn case5_stopword_term_still_matches() {
    // "bank of america" still matches when written exactly, even though the
    // canonical term itself contains the stopword "of".
    let d = dict_with(&["Bank of America"]);
    let text = "wire it to bank of america now";
    let (out, kept) = correct_span_claim(text, &d, &[]);
    assert!(
        kept.iter()
            .any(|r| r.replacement == "Bank of America" && r.end - r.start == 3),
        "the exact 3-word \"bank of america\" must be claimed"
    );
    // The canonical term contains its own stopword, so the stopword guard
    // never blocks it.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let bof = |s: usize, e: usize| span_joins_through_stopword(&tokens, s, e, "Bank of America");
    // The 3-word span "bank of america" (positions 3..6) is not blocked.
    assert!(
        !bof(3, 6),
        "a term containing its own stopword is its own authority"
    );
    // The output still contains the canonical spelling.
    assert!(
        out.join(" ").contains("Bank of America"),
        "canonical spelling present in output, got: {}",
        out.join(" ")
    );
}

#[test]
fn case6_single_token_correction_unaffected() {
    // A single-token correction is unaffected: the guards only gate spans of
    // two or more tokens.
    let d = dict_with(&["Ridha"]);
    let text = "hi ridha";
    let (out, kept) = correct_span_claim(text, &d, &[]);
    assert!(
        kept.iter()
            .any(|r| r.end - r.start == 1 && r.replacement == "Ridha"),
        "the single-token exact correction must stand"
    );
    assert_eq!(
        out.join(" "),
        "hi Ridha",
        "single-token correction is preserved"
    );
    // The guards themselves refuse single-token spans.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    assert!(!slice_crosses_punctuation(&tokens, 1, 2, text));
    assert!(!span_joins_through_stopword(&tokens, 1, 2, "Ridha"));
}

// ===========================================================================
// Predicate unit pins (the exact shapes the guards must see)
// ===========================================================================

#[test]
fn punctuation_guard_sees_original_token_text() {
    // The guard must see the ORIGINAL token text (with punctuation), not the
    // stripped form: the pipeline feeds `text.split_whitespace()`.
    let text = "still open. AI now";
    let tokens: Vec<&str> = text.split_whitespace().collect();
    assert!(
        slice_crosses_punctuation(&tokens, 1, 3, text),
        "the \".\" between \"open\" and \"AI\" must block the span even though it is glued to \"open\""
    );
    // A stripped token stream carries no boundary: confirming the guard
    // needs the original text.
    let stripped_owned = bare_tokens(text);
    let stripped: Vec<&str> = stripped_owned.iter().map(String::as_str).collect();
    assert!(
        !slice_crosses_punctuation(&stripped, 1, 3, stripped.join(" ").as_str()),
        "a stripped stream carries no boundary, so the guard needs the original text"
    );
    // No boundary -> no block.
    let plain = "still open ai now";
    let ptokens: Vec<&str> = plain.split_whitespace().collect();
    assert!(!slice_crosses_punctuation(&ptokens, 1, 3, plain));
}

#[test]
fn stopword_guard_pins() {
    let tokens: Vec<&str> = "a PI".split_whitespace().collect();
    assert!(
        span_joins_through_stopword(&tokens, 0, 2, "API"),
        "\"a\" is a standalone stopword the span would absorb"
    );
    assert!(
        !span_joins_through_stopword(&tokens, 0, 2, "a PI"),
        "a canonical term that contains the stopword as one of its own words is its own authority"
    );

    // "Bank of America" is allowlisted: its "of" never blocks.
    let b: Vec<&str> = "bank of america".split_whitespace().collect();
    assert!(!span_joins_through_stopword(&b, 0, 3, "Bank of America"));

    // A genuine multi-word phrase with no stopword in between is not blocked.
    let o: Vec<&str> = "open ai".split_whitespace().collect();
    assert!(!span_joins_through_stopword(&o, 0, 2, "Open AI"));

    // A punctuation-attached stopword still counts ("a," is still "a").
    let c: Vec<&str> = "a, PI".split_whitespace().collect();
    assert!(span_joins_through_stopword(&c, 0, 2, "API"));
}
