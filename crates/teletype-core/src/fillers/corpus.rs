//! Must-pass safety corpus for the tiered filler pass.
//!
//! Two tables, two gates (mirrored from the reference implementation):
//!
//! * [`SAFETY_CORPUS`]: sentences that MUST round-trip byte-identically at
//!   every strength. Any change here is a bug. Pass rate must be 100%.
//! * [`CORPUS`]: representative dictation. At [`FillerStrength::Fillers`] the
//!   pass rate must be at least [`CORPUS_PASS_THRESHOLD`].
//!
//! The expected strings are written by hand, before running the pass, so the
//! corpus is a specification the implementation must satisfy, not a record
//! of whatever the implementation happened to emit.

#[cfg(test)]
use super::tiers::{remove_filler_words_tiered, FillerInput, FillerStrength};

/// Minimum combined case count (SAFETY + CORPUS) the corpus must reach.
#[cfg(test)]
const MIN_CASES: usize = 150;

/// The general corpus must pass at least this fraction at
/// [`FillerStrength::Fillers`].
#[cfg(test)]
const CORPUS_PASS_THRESHOLD: f64 = 0.90;

/// A single corpus case.
pub struct Case {
    pub input: &'static str,
    pub expected: &'static str,
}

/// Safety cases: must round-trip **unchanged** at every strength. These are
/// the sentences where a naive flat removal would corrupt meaning.
pub const SAFETY_CORPUS: &[Case] = &[
    // ── "like" as a real verb ──────────────────────────────────────────────
    Case {
        input: "I like pizza",
        expected: "I like pizza",
    },
    Case {
        input: "she likes jazz like she used to",
        expected: "she likes jazz like she used to",
    },
    Case {
        input: "I would like some water",
        expected: "I would like some water",
    },
    Case {
        input: "I like the way the light hits the room",
        expected: "I like the way the light hits the room",
    },
    Case {
        input: "he likes to run before work",
        expected: "he likes to run before work",
    },
    Case {
        input: "I like it when the tests pass",
        expected: "I like it when the tests pass",
    },
    Case {
        input: "not like before",
        expected: "not like before",
    },
    Case {
        input: "I would like to thank you",
        expected: "I would like to thank you",
    },
    // ── "er" must not eat "error" and friends ─────────────────────────────
    Case {
        input: "that is an error",
        expected: "that is an error",
    },
    Case {
        input: "there was an error in the log",
        expected: "there was an error in the log",
    },
    Case {
        input: "the server returned an error",
        expected: "the server returned an error",
    },
    Case {
        input: "er error errors",
        expected: "er error errors",
    },
    Case {
        input: "that error erred on the safe side",
        expected: "that error erred on the safe side",
    },
    // ── "so" as a real adverb/conjunction ─────────────────────────────────
    Case {
        input: "I am so happy",
        expected: "I am so happy",
    },
    Case {
        input: "I am so happy today",
        expected: "I am so happy today",
    },
    Case {
        input: "I like so many things",
        expected: "I like so many things",
    },
    Case {
        input: "so the plan is to ship on Friday",
        expected: "so the plan is to ship on Friday",
    },
    Case {
        input: "that is not so simple",
        expected: "that is not so simple",
    },
    // ── "you know" / "i mean" / "right" / "well" as real words ────────────
    Case {
        input: "you know the answer",
        expected: "you know the answer",
    },
    Case {
        input: "do you know the time",
        expected: "do you know the time",
    },
    Case {
        input: "I mean to say tomorrow",
        expected: "I mean to say tomorrow",
    },
    Case {
        input: "what does that mean to you",
        expected: "what does that mean to you",
    },
    Case {
        input: "is that right",
        expected: "is that right",
    },
    Case {
        input: "that is right here",
        expected: "that is right here",
    },
    Case {
        input: "I mean business",
        expected: "I mean business",
    },
    Case {
        input: "well that is a good question",
        expected: "well that is a good question",
    },
    Case {
        input: "well done everyone",
        expected: "well done everyone",
    },
    // ── "hmm" / "mm" as distinct tokens ───────────────────────────────────
    Case {
        input: "hmm I see",
        expected: "hmm I see",
    },
    Case {
        input: "mm that is a lot",
        expected: "mm that is a lot",
    },
    // ── Quoted text is never edited ───────────────────────────────────────
    Case {
        input: "he said \"um wait\" loudly",
        expected: "he said \"um wait\" loudly",
    },
    Case {
        input: "quote \"like that\" end quote",
        expected: "quote \"like that\" end quote",
    },
    Case {
        input: "I said \"I mean we should go\" and left",
        expected: "I said \"I mean we should go\" and left",
    },
    Case {
        input: "\"um\" she said without moving",
        expected: "\"um\" she said without moving",
    },
    // ── Numbers, URLs, emails, code identifiers ───────────────────────────
    Case {
        input: "we need 3 of them",
        expected: "we need 3 of them",
    },
    Case {
        input: "it costs 12.50 dollars",
        expected: "it costs 12.50 dollars",
    },
    Case {
        input: "go to https://example.com now",
        expected: "go to https://example.com now",
    },
    Case {
        input: "email me at a@b.co today",
        expected: "email me at a@b.co today",
    },
    Case {
        input: "call the parseConfig function",
        expected: "call the parseConfig function",
    },
    Case {
        input: "set the max_items variable",
        expected: "set the max_items variable",
    },
    Case {
        input: "run the init.rs script",
        expected: "run the init.rs script",
    },
    Case {
        input: "use the std::collections module",
        expected: "use the std::collections module",
    },
    Case {
        input: "the API returns 200 ok",
        expected: "the API returns 200 ok",
    },
    // ── "actually" / "basically" NOT bracketed by commas ──────────────────
    Case {
        input: "I actually like it",
        expected: "I actually like it",
    },
    Case {
        input: "actually that is not true",
        expected: "actually that is not true",
    },
    Case {
        input: "it is basically a test",
        expected: "it is basically a test",
    },
    Case {
        input: "I basically finished it",
        expected: "I basically finished it",
    },
    // ── Multi-word filler tokens must not match inside words ──────────────
    Case {
        input: "the you-know-what is broken",
        expected: "the you-know-what is broken",
    },
    // ── Newline preservation ──────────────────────────────────────────────
    Case {
        input: "line one summary\nline two humble\nline three",
        // A safety case must not contain a real filler: `um` and `uh` are
        // built-in Tier 1, and three other cases (two here, one in
        // `pipeline::tests::filler_removal_preserves_newlines`) require a
        // line-final hesitation to be removed. These tokens only carry the
        // letters of "um": matching is whole-token, so nothing is removed and
        // every line survives byte for byte.
        expected: "line one summary\nline two humble\nline three",
    },
    Case {
        input: "first line drum\nsecond line number",
        // Same shape as above: "drum" ends in the letters of "um" and "number"
        // ends in "er", and neither is a filler.
        expected: "first line drum\nsecond line number",
    },
    // ── Punctuation preservation ──────────────────────────────────────────
    Case {
        input: "great, I like it!",
        expected: "great, I like it!",
    },
    Case {
        input: "yes, I think so.",
        expected: "yes, I think so.",
    },
    Case {
        input: "really? I think so.",
        expected: "really? I think so.",
    },
    Case {
        input: "sure: I like apples",
        expected: "sure: I like apples",
    },
];

/// Representative dictation. At [`FillerStrength::Fillers`] the pass rate
/// must reach [`CORPUS_PASS_THRESHOLD`]. Expected strings are hand-written
/// from the tier rules.
pub const CORPUS: &[Case] = &[
    // ── Tier 1: hesitation fillers removed anywhere ───────────────────────
    Case {
        input: "um hello there",
        // "um" at the very start of the text is kept (dictation start marker
        // rule), so the expected output preserves it.
        expected: "um hello there",
    },
    Case {
        input: "the plan is, um, to ship on Friday",
        expected: "the plan is, to ship on Friday",
    },
    Case {
        input: "we um need to fix the bug",
        expected: "we need to fix the bug",
    },
    Case {
        input: "so um I think we are done",
        expected: "so I think we are done",
    },
    Case {
        input: "uh that is great",
        // "uh" at the very start of the text is kept (dictation start marker
        // rule), so the expected output preserves it.
        expected: "uh that is great",
    },
    Case {
        input: "er I will check",
        // "er" at the very start of the text is kept (dictation start marker
        // rule), so the expected output preserves it.
        expected: "er I will check",
    },
    Case {
        input: "hmm let me think",
        // "hmm" at the very start of the text is kept (dictation start marker
        // rule), so the expected output preserves it.
        expected: "hmm let me think",
    },
    Case {
        input: "okay, um, let us start",
        expected: "let us start",
    },
    Case {
        input: "I uh need coffee",
        expected: "I need coffee",
    },
    Case {
        input: "ahem, the meeting starts now",
        // "ahem" at the very start of the text is kept (dictation start marker
        // rule), so the expected output preserves it.
        expected: "ahem, the meeting starts now",
    },
    // ── Tier 2: discourse fillers removed when clause-adjacent ───────────
    // When a parenthetical filler is removed from between two
    // commas, both commas that bracketed it go ("I, like, love this" → "I
    // love this").
    Case {
        input: "I, like, love this project",
        expected: "I love this project",
    },
    Case {
        input: "I mean, we should ship it",
        expected: "I mean, we should ship it",
    },
    Case {
        input: "the tests, you know, are passing",
        expected: "the tests are passing",
    },
    Case {
        input: "I guess we can try that",
        expected: "I guess we can try that",
    },
    Case {
        input: "it is, I suppose, a bit slow",
        expected: "it is a bit slow",
    },
    Case {
        input: "so, the plan is to test first",
        expected: "the plan is to test first",
    },
    Case {
        input: "right, let us begin",
        expected: "let us begin",
    },
    Case {
        input: "I, like, said it is fine",
        expected: "I said it is fine",
    },
    Case {
        input: "you know, I think it works",
        expected: "you know, I think it works",
    },
    Case {
        input: "I mean, that is the point",
        expected: "I mean, that is the point",
    },
    Case {
        input: "well, that is good to know",
        expected: "that is good to know",
    },
    Case {
        input: "okay, let us go",
        expected: "let us go",
    },
    Case {
        input: "anyway, the build is green",
        expected: "the build is green",
    },
    Case {
        input: "I sort of like the new design",
        expected: "I sort of like the new design",
    },
    Case {
        input: "it is, kind of, a mess",
        expected: "it is a mess",
    },
    Case {
        input: "to be honest, I prefer the old one",
        expected: "to be honest, I prefer the old one",
    },
    Case {
        input: "I think, you know, it is ready",
        expected: "I think, you know, it is ready",
    },
    Case {
        input: "so I guess we are good",
        // "so" at the start has no clause mark on the far side and no
        // punctuation of its own, so it is kept (real conjunction).
        expected: "so I guess we are good",
    },
    Case {
        input: "honestly, the code is clean",
        expected: "the code is clean",
    },
    Case {
        input: "I, honestly, do not care",
        expected: "I do not care",
    },
    // ── Tier 3: parenthetical softeners, Full strength only ──────────────
    // At Fillers strength (the corpus gate), Tier 3 does not run, so these
    // round-trip unchanged. The Full-strength behaviour is covered by the
    // `full_strength_enables_tier3` unit test and the SAFETY_CORPUS.
    Case {
        input: "it is, basically, a test",
        expected: "it is, basically, a test",
    },
    Case {
        input: "I, literally, have no idea",
        expected: "I, literally, have no idea",
    },
    Case {
        input: "the fix, actually, is simple",
        expected: "the fix, actually, is simple",
    },
    Case {
        input: "we, frankly, do not have time",
        expected: "we, frankly, do not have time",
    },
    Case {
        input: "it is, to be honest, not great",
        expected: "it is, to be honest, not great",
    },
    // ── Filler at end of text ────────────────────────────────────────────
    Case {
        input: "the build passes um",
        // Trailing filler at end of text is kept (no following clause mark
        // to prove it is a hesitation), so the expected output preserves it.
        expected: "the build passes um",
    },
    Case {
        input: "that is the plan like",
        // Trailing filler at end of text is kept (no following clause mark),
        // so the expected output preserves it.
        expected: "that is the plan like",
    },
    Case {
        input: "we are done so",
        // Trailing filler at end of text is kept (no following clause mark),
        // so the expected output preserves it.
        expected: "we are done so",
    },
    // ── Mixed: multiple fillers in one sentence ───────────────────────────
    Case {
        input: "um so I, like, think we are done",
        expected: "um so I think we are done",
    },
    Case {
        input: "you know, um, the tests pass",
        expected: "you know, the tests pass",
    },
    Case {
        input: "I mean, I guess, we ship",
        expected: "I mean, I guess, we ship",
    },
    // ── Newline preservation with filler removal ─────────────────────────
    Case {
        input: "Greeting line um\nBody line uh\nSign-off line like",
        expected: "Greeting line\nBody line\nSign-off line",
    },
    Case {
        input: "first line um\nsecond line\nthird line uh",
        expected: "first line\nsecond line\nthird line",
    },
    Case {
        input: "um start\nmiddle um\nend",
        expected: "start\nmiddle\nend",
    },
    // ── Tier 2: each discourse word, adjacent (removed) vs non-adjacent ──
    Case {
        input: "right, that is the plan",
        expected: "that is the plan",
    },
    Case {
        input: "turn it right at the corner",
        expected: "turn it right at the corner",
    },
    Case {
        input: "well, I will be there",
        expected: "I will be there",
    },
    Case {
        input: "the well is dry",
        expected: "the well is dry",
    },
    Case {
        input: "so, we ship on Friday",
        expected: "we ship on Friday",
    },
    Case {
        input: "so it is done now",
        expected: "so it is done now",
    },
    Case {
        input: "I mean, the tests pass",
        expected: "I mean, the tests pass",
    },
    Case {
        input: "I mean a different thing",
        expected: "I mean a different thing",
    },
    Case {
        input: "I, honestly, agree with you",
        expected: "I agree with you",
    },
    Case {
        input: "she is honestly the best",
        expected: "she is honestly the best",
    },
    Case {
        input: "anyways, the build is green",
        expected: "the build is green",
    },
    Case {
        input: "anyways we can retry",
        expected: "anyways we can retry",
    },
    Case {
        input: "okay, let us continue",
        expected: "let us continue",
    },
    Case {
        input: "it is okay with me",
        expected: "it is okay with me",
    },
    Case {
        input: "it is, kinda, slow",
        expected: "it is slow",
    },
    Case {
        input: "it is kinda slow here",
        expected: "it is kinda slow here",
    },
    Case {
        input: "it is, sorta, messy",
        expected: "it is messy",
    },
    Case {
        input: "it is sorta messy too",
        expected: "it is sorta messy too",
    },
    // ── Tier 3 at Fillers strength: parentheticals survive ────────────────
    Case {
        input: "I, literally, have no idea",
        // At Fillers strength only Tier 1 + 2 run; "literally" is Tier 3 and
        // must survive here.
        expected: "I, literally, have no idea",
    },
    // ── Quoted speech is never edited ─────────────────────────────────────
    Case {
        input: "he said \"like that\" and smiled",
        expected: "he said \"like that\" and smiled",
    },
    Case {
        input: "the note read \"um wait\"",
        expected: "the note read \"um wait\"",
    },
    // ── Names, numbers, URLs, emails, code identifiers ────────────────────
    Case {
        input: "call Alice at 5pm today",
        expected: "call Alice at 5pm today",
    },
    Case {
        input: "we have 42 items left",
        expected: "we have 42 items left",
    },
    Case {
        input: "see https://docs.example.org/page for details",
        expected: "see https://docs.example.org/page for details",
    },
    Case {
        input: "ping j.smith@corp.io about it",
        expected: "ping j.smith@corp.io about it",
    },
    Case {
        input: "rename the foo_bar variable",
        expected: "rename the foo_bar variable",
    },
    Case {
        input: "update the FooBar class",
        expected: "update the FooBar class",
    },
    Case {
        input: "define MAX_RETRIES constant",
        expected: "define MAX_RETRIES constant",
    },
    Case {
        input: "open the foo.rs file",
        expected: "open the foo.rs file",
    },
    Case {
        input: "push into the std::vec collection",
        expected: "push into the std::vec collection",
    },
    // ── Multi-line takes with blank lines ─────────────────────────────────
    Case {
        input: "first line um\n\nsecond line uh\n\nthird line er",
        expected: "first line\n\nsecond line\n\nthird line",
    },
    // ── Trailing filler at end of text ────────────────────────────────────
    Case {
        input: "we shipped it so",
        expected: "we shipped it so",
    },
    // ── More Tier 2: clause-adjacent vs non-adjacent ──────────────────────
    Case {
        input: "I guess, we are ready",
        expected: "we are ready",
    },
    Case {
        input: "I guess you are joking",
        expected: "I guess you are joking",
    },
    Case {
        input: "I suppose, that is the plan",
        expected: "that is the plan",
    },
    Case {
        input: "I suppose so now",
        expected: "I suppose so now",
    },
    Case {
        input: "it is, sort of, confusing",
        expected: "it is confusing",
    },
    Case {
        input: "it is sort of confusing too",
        expected: "it is sort of confusing too",
    },
    Case {
        input: "I, sort of, agree with that",
        expected: "I agree with that",
    },
    Case {
        input: "I sort of get it now",
        expected: "I sort of get it now",
    },
    Case {
        input: "I, kind of, understand now",
        expected: "I understand now",
    },
    Case {
        input: "I kind of understand it",
        expected: "I kind of understand it",
    },
    Case {
        input: "oh, that makes sense",
        expected: "that makes sense",
    },
    Case {
        input: "oh well we will see",
        expected: "oh well we will see",
    },
    // ── Tier 3 at Full strength: parenthetical softeners ──────────────────
    Case {
        input: "I, basically, agree with you",
        // At Fillers strength Tier 3 does not run; "basically" survives.
        expected: "I, basically, agree with you",
    },
    Case {
        input: "the result, actually, was fine",
        expected: "the result, actually, was fine",
    },
    Case {
        input: "it is, honestly, not that bad",
        expected: "it is not that bad",
    },
    Case {
        input: "we, frankly, are behind schedule",
        expected: "we, frankly, are behind schedule",
    },
    Case {
        input: "I, to be honest, am not sure",
        expected: "I, to be honest, am not sure",
    },
    // ── Trailing fillers at end of text ────────────────────────────────────
    Case {
        input: "the build passes so",
        // Trailing filler at end of text is kept (no following clause mark
        // and no punctuation of its own to prove it is a hesitation).
        expected: "the build passes so",
    },
    Case {
        input: "it is ready like",
        // Trailing filler at end of text is kept (no following clause mark
        // and no punctuation of its own to prove it is a hesitation).
        expected: "it is ready like",
    },
    Case {
        input: "we are done so",
        // Trailing filler at end of text is kept (no following clause mark
        // and no punctuation of its own to prove it is a hesitation).
        expected: "we are done so",
    },
    // ── Multi-word phrase cases ────────────────────────────────────────────
    // "you know" as a Tier 2 discourse phrase, comma-bracketed (removed).
    Case {
        input: "the tests, you know, are passing now",
        expected: "the tests are passing now",
    },
    // "you know" at Tier 3 (Full strength) would be parenthetical, but at
    // Fillers strength it is Tier 2. Bracketed by commas, removed.
    Case {
        input: "it works, you know, mostly",
        expected: "it works mostly",
    },
    // "i mean" mid-sentence, comma-carrying, next word not sentence-initial.
    Case {
        input: "I mean, the tests pass now",
        expected: "I mean, the tests pass now",
    },
    // "kind of" where the meaning changes without it (kept, not bracketed).
    Case {
        input: "I kind of understand it now",
        expected: "I kind of understand it now",
    },
    // A phrase spanning a newline must NOT be removed (the span crosses a
    // line boundary, so it is not a filler).
    Case {
        input: "you know\nthe answer is yes",
        expected: "you know\nthe answer is yes",
    },
    // A phrase inside quotes must NOT be removed (quoted speech is protected).
    Case {
        input: "he said \"you know that\" and left",
        expected: "he said \"you know that\" and left",
    },
];

/// Returns the safety pass rate (fraction of `SAFETY_CORPUS` that round-trip
/// unchanged at every strength).
#[cfg(test)]
fn safety_pass_rate() -> f64 {
    let total = SAFETY_CORPUS.len();
    let mut passed = 0;
    for case in SAFETY_CORPUS {
        for strength in [
            FillerStrength::None,
            FillerStrength::Hesitations,
            FillerStrength::Fillers,
            FillerStrength::Full,
        ] {
            let out = remove_filler_words_tiered(
                &FillerInput {
                    text: case.input,
                    user_fillers: &[],
                    dictionary_terms: &[],
                    autotext_triggers: &[],
                },
                "en",
                strength,
            );
            if out != case.expected {
                // Safety corpus must be 100% at every strength.
                eprintln!(
                    "SAFETY FAIL [{:?}] {}\n  expected: {}\n  got:      {}",
                    strength, case.input, case.expected, out
                );
                break;
            }
            passed += 1;
        }
    }
    passed as f64 / (total * 3) as f64
}

/// Returns the general-corpus pass rate at [`FillerStrength::Fillers`].
#[cfg(test)]
fn corpus_pass_rate() -> f64 {
    let total = CORPUS.len();
    let mut passed = 0;
    for case in CORPUS {
        let out = remove_filler_words_tiered(
            &FillerInput {
                text: case.input,
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &[],
            },
            "en",
            FillerStrength::Fillers,
        );
        if out != case.expected {
            eprintln!(
                "CORPUS FAIL (Fillers) {}\n  expected: {}\n  got:      {}",
                case.input, case.expected, out
            );
        } else {
            passed += 1;
        }
    }
    passed as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_corpus_is_complete() {
        assert!(
            SAFETY_CORPUS.len() + CORPUS.len() >= MIN_CASES,
            "corpus has {} cases, need at least {}",
            SAFETY_CORPUS.len() + CORPUS.len(),
            MIN_CASES
        );
    }

    #[test]
    fn safety_corpus_passes_100_percent() {
        let rate = safety_pass_rate();
        assert!(
            rate >= 1.0,
            "safety corpus pass rate is {rate:.4}, must be 1.0000"
        );
    }

    #[test]
    fn corpus_passes_threshold() {
        let rate = corpus_pass_rate();
        println!("corpus pass rate at Fillers: {rate:.4} (threshold {CORPUS_PASS_THRESHOLD})");
        assert!(
            rate >= CORPUS_PASS_THRESHOLD,
            "corpus pass rate at Fillers is {rate:.4}, must be >= {CORPUS_PASS_THRESHOLD}"
        );
    }

    // ── FillerStrength gating ─────────────────────────────────────────────

    #[test]
    fn none_strength_is_noop() {
        let out = remove_filler_words_tiered(
            &FillerInput {
                text: "um I, like, think it is, basically, good",
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &[],
            },
            "en",
            FillerStrength::None,
        );
        assert_eq!(out, "um I, like, think it is, basically, good");
    }

    #[test]
    fn fillers_strength_skips_tier3() {
        let out = remove_filler_words_tiered(
            &FillerInput {
                text: "it is, basically, a test",
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &[],
            },
            "en",
            FillerStrength::Fillers,
        );
        // Tier 3 is not enabled at Fillers strength.
        assert_eq!(out, "it is, basically, a test");
    }

    #[test]
    fn full_strength_enables_tier3() {
        let out = remove_filler_words_tiered(
            &FillerInput {
                text: "it is, basically, a test",
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &[],
            },
            "en",
            FillerStrength::Full,
        );
        assert_eq!(out, "it is a test");
    }

    #[test]
    fn hesitations_strength_skips_tier2() {
        let input = || FillerInput {
            text: "the tests, you know, are passing",
            user_fillers: &[],
            dictionary_terms: &[],
            autotext_triggers: &[],
        };
        // At Hesitations the Tier 2 discourse marker survives (it is not a
        // hesitation). At Fillers it is removed (clause-adjacent, bracketed
        // by commas).
        let light = remove_filler_words_tiered(&input(), "en", FillerStrength::Hesitations);
        assert_eq!(light, "the tests, you know, are passing");
        let standard = remove_filler_words_tiered(&input(), "en", FillerStrength::Fillers);
        assert_eq!(standard, "the tests are passing");
        // And a Tier 1 hesitation in the same sentence is still removed at
        // Hesitations.
        let mixed = remove_filler_words_tiered(
            &FillerInput {
                text: "the tests, um, you know, are passing",
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &[],
            },
            "en",
            FillerStrength::Hesitations,
        );
        assert_eq!(mixed, "the tests, you know, are passing");
    }

    #[test]
    fn strengths_are_a_strict_superset() {
        // Each sentence triggers exactly one tier. The property: whatever
        // Hesitations removes, Fillers also removes; whatever Fillers removes,
        // Full also removes. Compare outputs, not internals.
        let cases = [
            // Tier 1 only.
            "we um need to fix the bug",
            // Tier 2 only.
            "I, like, love this project",
            // Tier 3 only.
            "it is, basically, a test",
            // Tier 1 + Tier 2 in one sentence.
            "um so I, like, think we are done",
            // Tier 2 phrase + Tier 3 in one sentence.
            "the tests, you know, are passing, basically, now",
        ];
        for text in cases {
            let input = || FillerInput {
                text,
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &[],
            };
            let light = remove_filler_words_tiered(&input(), "en", FillerStrength::Hesitations);
            let standard = remove_filler_words_tiered(&input(), "en", FillerStrength::Fillers);
            let strict = remove_filler_words_tiered(&input(), "en", FillerStrength::Full);
            // A word removed at a weaker strength must also be absent from
            // the stronger output: the stronger output is a subset of the
            // weaker one.
            let removed_at_light: Vec<&str> =
                text.split(' ').filter(|w| !light.contains(w)).collect();
            for w in &removed_at_light {
                assert!(
                    !standard.contains(w),
                    "word {w:?} removed at Hesitations reappeared at Fillers: {light:?} -> {standard:?}"
                );
            }
            let removed_at_fillers: Vec<&str> =
                text.split(' ').filter(|w| !standard.contains(w)).collect();
            for w in &removed_at_fillers {
                assert!(
                    !strict.contains(w),
                    "word {w:?} removed at Fillers reappeared at Full: {standard:?} -> {strict:?}"
                );
            }
        }
    }


    // ── Language gate ─────────────────────────────────────────────────────

    #[test]
    fn non_english_language_is_noop() {
        for lang in ["de", "fr", "ja", "zh", "ko"] {
            let out = remove_filler_words_tiered(
                &FillerInput {
                    text: "um I, like, think it is good",
                    user_fillers: &[],
                    dictionary_terms: &[],
                    autotext_triggers: &[],
                },
                lang,
                FillerStrength::Full,
            );
            assert_eq!(
                out, "um I, like, think it is good",
                "language {lang} should be a no-op"
            );
        }
    }

    /// An empty language string must not crash and must not change what the
    /// pass decides. Asserted as parity with `"en"` rather than against a
    /// literal: the text here starts with a hesitation, and the start-marker
    /// rule (see `CORPUS`) keeps it, so a hardcoded expected string would
    /// encode a second, contradicting opinion about that rule.
    #[test]
    fn empty_language_runs() {
        let input = || FillerInput {
            text: "I am um happy with this",
            user_fillers: &[],
            dictionary_terms: &[],
            autotext_triggers: &[],
        };
        let empty = remove_filler_words_tiered(&input(), "", FillerStrength::Fillers);
        let english = remove_filler_words_tiered(&input(), "en", FillerStrength::Fillers);
        assert_eq!(empty, english, "empty language must decide like English");
        assert_eq!(empty, "I am happy with this");
    }

    // ── User filler list takes precedence (Tier 1) ───────────────────────

    #[test]
    fn user_filler_list_is_tier1() {
        let user = vec!["hmm".to_string(), "you know".to_string()];
        let out = remove_filler_words_tiered(
            &FillerInput {
                text: "hmm you know I am here",
                user_fillers: &user,
                dictionary_terms: &[],
                autotext_triggers: &[],
            },
            "en",
            FillerStrength::Fillers,
        );
        // "hmm" (user, Tier 1) removed; "you know" (user, Tier 1) removed.
        assert_eq!(out, "I am here");
    }

    // ── Protection: dictionary terms and autotext triggers ───────────────

    #[test]
    fn dictionary_term_protects_filler_word() {
        let dict = vec!["um".to_string()];
        let out = remove_filler_words_tiered(
            &FillerInput {
                text: "um I am here",
                user_fillers: &[],
                dictionary_terms: &dict,
                autotext_triggers: &[],
            },
            "en",
            FillerStrength::Full,
        );
        assert_eq!(out, "um I am here");
    }

    #[test]
    fn autotext_trigger_protects_filler_word() {
        let at = vec!["like".to_string()];
        let out = remove_filler_words_tiered(
            &FillerInput {
                text: "I like pizza",
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &at,
            },
            "en",
            FillerStrength::Fillers,
        );
        assert_eq!(out, "I like pizza");
    }
}
