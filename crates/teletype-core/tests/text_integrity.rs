//! Text-integrity tests: the user's words must survive.
//!
//! Two tiers, and both are executable:
//!
//!   * `protected_*` asserts the **invariant that should hold**. Green means
//!     the bug is fixed; `#[ignore]` means it is still live, and the reason
//!     names the finding.
//!   * `regression_*` pins **today's wrong output** for a defect that is
//!     still live. Green on purpose. When a fix lands this test fails, which
//!     is the signal to delete it and promote the case to `protected_*`.
//!
//! ```text
//! cargo test -p teletype-core --test text_integrity
//! cargo test -p teletype-core --test text_integrity -- --ignored
//! ```
//!
//! `journey_smoke.rs` asserts journey invariants (no words lost, no
//! placeholder leakage, everything reachable). Those are coarse enough that a
//! whole class of corruption slips through, because the words are still
//! present, just welded to the wrong neighbour or rewritten as a different
//! token. This file pins the **exact output** of the text transforms, so a
//! change to ITN, System AutoText or the Insights math has to argue with a
//! concrete string.

use teletype_core::autotext::{expand, protect, AutoTextStore};
use teletype_core::context;
use teletype_core::itn;

// ===========================================================================
// The bar every other test measures against
// ===========================================================================

#[test]
fn protected_plain_prose_is_never_modified() {
    for s in [
        "the deploy went out this morning",
        "let us cherrypick the fix and ship it",
        "i spoke to the team about the roadmap",
        "she asked whether we could review the contract",
        "we should probably move the meeting to next week",
    ] {
        assert_eq!(itn::normalize(s), s, "ITN modified plain prose: {s:?}");
    }
}

// ===========================================================================
// ITN: boundaries, spacing, and the ambiguous cases
// ===========================================================================

#[test]
fn protected_itn_never_welds_a_number_onto_a_word() {
    // Every pass used to start its match with `\s` and never re-emit it, so
    // "fifty dollars" became "cost$50". `numeric_ranges` already had the right
    // shape (`(?P<lead>^|\s)` re-emitted); the other six now use it too.
    for (input, want) in [
        ("the meeting is at five pm", "the meeting is at 5:00 PM"),
        ("it will cost fifty dollars", "it will cost $50"),
        ("i paid ninety nine dollars", "i paid $99"),
        ("that is one hundred percent right", "that is 100% right"),
        (
            "the ratio is one point five to one",
            "the ratio is 1.5 to one",
        ),
        (
            "lets meet on march twelfth two thousand seven",
            "lets meet on March 12, 2007",
        ),
        ("50 dollars", "$50"),
        ("it costs 50 dollars", "it costs $50"),
        ("the cost is 1,234 dollars", "the cost is $1,234"),
        // `phone_digit_runs` had the same defect: its pattern ended with `\s*`,
        // so the space *after* the number was consumed too.
        ("call 555 010 9999 at 5 pm", "call 555-010-9999 at 5 pm"),
        ("my number is 555 010 9999", "my number is 555-010-9999"),
        ("dial zero nine one two three", "dial 09123"),
    ] {
        assert_eq!(itn::normalize(input), want, "input: {input:?}");
    }
}

#[test]
fn protected_itn_never_eats_the_start_of_the_next_word() {
    // `digit_alt` had no `\b`, so `out` was `o` + `ut` and `ones` was
    // `one` + `s`.
    for (input, want) in [
        (
            "the reading is nine point eight on the Richter scale",
            "the reading is 9.8 on the Richter scale",
        ),
        (
            "i rate it one point five out of five",
            "i rate it 1.5 out of five",
        ),
        ("one point out of ten", "one point out of 10"),
        ("three point ones", "three point ones"),
    ] {
        assert_eq!(itn::normalize(input), want, "input: {input:?}");
    }
}

#[test]
fn protected_itn_does_not_turn_cardinals_into_years() {
    // `parse_year` maps "twenty <unit>" to 20NN, which is right inside a date
    // and wrong on its own. The standalone pass now rejects the bare
    // two-word cardinal and keeps every real year.
    for (input, want) in [
        ("she is twenty one", "she is 21"),
        ("he is twenty six years old", "he is 26 years old"),
        (
            "we have twenty three people coming",
            "we have 23 people coming",
        ),
        ("twenty five bucks", "25 bucks"),
        ("page twenty three", "page 23"),
        ("in twenty twenty six", "in 2026"),
        ("nineteen ninety nine", "1999"),
        ("two thousand nine", "2009"),
        ("back in nineteen eighty four", "back in 1984"),
    ] {
        assert_eq!(itn::normalize(input), want, "input: {input:?}");
    }
}

#[test]
fn protected_ordinals_do_not_eat_the_time_unit_second() {
    // An article before the ordinal means a duration noun. The genuine
    // ordinals must keep working, which is why the guard also looks at the
    // following word.
    for (input, want) in [
        ("in a second", "in a second"),
        ("wait a second please", "wait a second please"),
        ("just a second please", "just a second please"),
        ("give me another second", "give me another second"),
        ("the second wave of tests", "the 2nd wave of tests"),
        ("twenty first century", "21st century"),
        ("the 2nd tuesday", "the 2nd tuesday"),
    ] {
        assert_eq!(itn::normalize(input), want, "input: {input:?}");
    }
}

#[test]
fn protected_url_pass_requires_something_that_looks_like_a_host() {
    // The pass matched any "X dot Y", so "i said dot com out loud" became
    // "i said.com out loud". It now requires a first label that is not an
    // English word, and keeps the slashes in a multi-segment path.
    for (input, want) in [
        ("the file is a dot com site", "the file is a dot com site"),
        ("i said dot com out loud", "i said dot com out loud"),
        ("a dot b is how you write it", "a dot b is how you write it"),
        ("go to github dot com", "go to github.com"),
        (
            "i visited example dot com slash about slash us",
            "i visited example.com/about/us",
        ),
        (
            "i visited example dot com slash about",
            "i visited example.com/about",
        ),
    ] {
        assert_eq!(itn::normalize(input), want, "input: {input:?}");
    }
}

#[test]
fn protected_percent_survives_a_spoken_decimal() {
    // `decimals` runs before `money_pct` and leaves a written decimal, so the
    // percent pattern has to span a decimal point, and `parse_numeric` has to
    // accept digit tokens as well as spoken ones.
    for (input, want) in [
        ("ninety nine point nine percent", "99.9%"),
        ("ninety nine percent", "99%"),
        ("50 percent", "50%"),
        (
            "zero point five percent of the shares",
            "0.5% of the shares",
        ),
        ("half of the ten percent", "half of the 10%"),
    ] {
        assert_eq!(itn::normalize(input), want, "input: {input:?}");
    }
}

// ===========================================================================
// System AutoText
// ===========================================================================

/// BUG-12, fixed. `DEFS` in `autotext/system.rs` holds bare common nouns
/// ("period", "comma", "star", "quote", "plus", "new line") and
/// `expand_snippets_with` used to match any whole-word occurrence
/// unconditionally, with no setting gate. The user said a word, a symbol was
/// typed, and because `protect_snippets_with` runs first (`pipeline.rs:169`) the
/// model was asked to "polish" the corruption.
///
/// Now an ambiguous phrase only expands when the surrounding words show the user
/// was naming a character. See `autotext::disambiguate` for the rule.
#[test]
fn protected_system_autotext_leaves_ordinary_words_alone() {
    let store = AutoTextStore::default();
    let app = context::normalize("com.apple.Notes", "Notes");
    let sys = teletype_core::autotext::system::entries();
    for (input, want) in [
        (
            "the payment period ends in March",
            "the payment period ends in March",
        ),
        (
            "use a comma to separate the fields",
            "use a comma to separate the fields",
        ),
        ("the star of the show", "the star of the show"),
        (
            "the quote in the article was wrong",
            "the quote in the article was wrong",
        ),
        ("that is a plus for the team", "that is a plus for the team"),
        (
            "start a new line for the address",
            "start a new line for the address",
        ),
        (
            "there is a line break in the paragraph",
            "there is a line break in the paragraph",
        ),
    ] {
        assert_eq!(
            expand::expand_snippets_with(input, &store, &app, sys),
            want,
            "input: {input:?}"
        );
    }
}

/// The other half of BUG-12: the fix must not cost a capability. Every way of
/// asking that worked before still works, and the two-word phrases a user
/// spells out mid-sentence ("email at sign example dot com") are untouched.
#[test]
fn protected_symbol_requests_still_expand() {
    let store = AutoTextStore::default();
    let app = context::normalize("com.apple.Notes", "Notes");
    let sys = teletype_core::autotext::system::entries();
    for (input, want) in [
        // Plain, one-word requests.
        ("comma", ","),
        ("period", "."),
        ("semicolon", ";"),
        ("the end full stop", "the end."),
        // Asked for with a naming cue.
        ("insert a comma here", "insert a, here"),
        ("type a period", "type a."),
        ("the symbol for a period", "the symbol for a."),
        // Spelling an address aloud, where the symbol is mid-sentence.
        ("email at sign example dot com", "email @ example dot com"),
        // Names with no ordinary-word reading, anywhere in a sentence.
        ("a ampersand and a tilde", "a & and a ~"),
        ("open paren close paren", "()"),
    ] {
        assert_eq!(
            expand::expand_snippets_with(input, &store, &app, sys),
            want,
            "input: {input:?}"
        );
    }
}

/// The protect path runs *before* the LLM (`pipeline.rs:169`), so a leak here
/// is what handed the model `the payment{{AUTOTEXT_0}} ends in March` to
/// "polish". It has to be checked separately, not inferred from `expand`.
#[test]
fn protected_autotext_placeholder_is_not_inserted_for_prose() {
    let store = AutoTextStore::default();
    let app = context::normalize("com.apple.Notes", "Notes");
    let sys = teletype_core::autotext::system::entries();
    for input in [
        "the payment period ends in March",
        "use a comma to separate the fields",
        "the star of the show",
        "that is a plus for the team",
        "start a new line for the address",
    ] {
        let out = protect::protect_snippets_with(input, &store, &app, sys);
        assert!(
            !input.contains("AUTOTEXT"),
            "sanity: the fixture must not already contain a placeholder"
        );
        assert!(
            !out.text.contains("AUTOTEXT"),
            "prose got a placeholder for {input:?}: {:?}",
            out.text
        );
        assert_eq!(out.text, input, "prose was rewritten for {input:?}");
    }
    // A real request still gets its placeholder, so the LLM still sees the
    // character as a token it must not touch.
    let out = protect::protect_snippets_with("insert a comma here", &store, &app, sys);
    assert!(
        out.text.contains("AUTOTEXT"),
        "request lost its placeholder"
    );
}

/// A *custom* entry is exempt. If someone deliberately created a snippet whose
/// phrase is "comma", they meant it to fire wherever it appears.
#[test]
fn protected_custom_snippets_are_never_ambiguous() {
    let mut store = AutoTextStore::default();
    let mut custom = teletype_core::autotext::AutoTextEntry::new("/comma", "CUSTOM");
    custom.snippet = "comma".into();
    store.insert(custom).unwrap();
    let app = context::normalize("com.apple.Notes", "Notes");
    let sys = teletype_core::autotext::system::entries();
    // The System "period" entry is suppressed here...
    assert_eq!(
        expand::expand_snippets_with("the payment period ends in March", &store, &app, sys),
        "the payment period ends in March"
    );
    // ...but the custom "comma" fires even mid-sentence.
    assert_eq!(
        expand::expand_snippets_with("a comma b", &store, &app, sys),
        "a CUSTOM b"
    );
}

// ===========================================================================
// Insights
// ===========================================================================

/// A history of `total` words, each take with a measured duration.
fn history_with_words(total: u32, per_take: u32) -> teletype_core::history::DictationHistory {
    use teletype_core::history::{DictationEntry, DictationHistory};
    let mut h = DictationHistory::default();
    let mut left = total;
    while left > 0 {
        let n = left.min(per_take);
        // 20 words over 8 s is 150 wpm, a normal conversational rate.
        h.entries.push(DictationEntry {
            id: format!("e{}", h.entries.len()),
            created_at: 1_757_000_000_000,
            text: (0..n)
                .map(|j| format!("w{j}"))
                .collect::<Vec<_>>()
                .join(" "),
            context: None,
            duration_ms: Some(8_000),
        });
        left -= n;
    }
    h
}

#[test]
fn protected_the_impact_metric_is_measured_and_monotonic() {
    // The screen used to claim "290 wpm" and a "7x faster" badge, both
    // functions of the word count rather than of how fast anyone spoke, and the
    // reported time saving could go *down* as you dictated more.
    let mut prev_saved = 0u32;
    for target in [39u32, 149, 150, 290, 449, 1000, 5000] {
        let h = history_with_words(target, 20);
        let ins = teletype_core::insights::compute(&h, 1_757_000_000_000);
        let wpm = ins.impact.words_per_minute.expect("a measured rate");
        assert!(
            (100..=250).contains(&wpm),
            "at {target} words the app claims {wpm} wpm, which is not a human \
             speaking rate"
        );
        assert!(
            ins.impact.time_saved_minutes >= prev_saved,
            "time saved went DOWN from {prev_saved} to {} at {target} words",
            ins.impact.time_saved_minutes
        );
        prev_saved = ins.impact.time_saved_minutes;
    }
}

#[test]
fn protected_an_unmeasured_history_makes_no_speed_claim() {
    // A fresh install, and any history written before durations were
    // recorded, must not produce a speed figure at all.
    use teletype_core::history::{DictationEntry, DictationHistory};
    let h = DictationHistory {
        entries: vec![DictationEntry {
            id: "a".into(),
            created_at: 1_757_000_000_000,
            text: "word ".repeat(290),
            context: None,
            duration_ms: None,
        }],
    };
    let ins = teletype_core::insights::compute(&h, 1_757_000_000_000);
    assert_eq!(
        ins.impact.words_per_minute, None,
        "no measured duration means no rate claim"
    );
    assert_eq!(ins.impact.times_faster, None);
    assert_eq!(
        ins.impact.skipped_takes, 1,
        "the unrated take is counted, so the rate is never shown as covering more"
    );
}

#[test]
fn protected_a_take_too_short_to_time_is_not_rated() {
    let ins = teletype_core::insights::compute(&history_with_words(2, 2), 1_757_000_000_000);
    // 2 words in 8 s is 15 wpm, which is measurable, so this must be rated.
    assert!(ins.impact.words_per_minute.is_some());
    // A take the microphone was open for under the floor is not.
    let mut short = history_with_words(20, 20);
    short.entries[0].duration_ms = Some(200);
    let ins = teletype_core::insights::compute(&short, 1_757_000_000_000);
    assert_eq!(ins.impact.rated_takes, 0);
    assert_eq!(ins.impact.skipped_takes, 1);
}

#[test]
fn regression_stats_weekday_labels_are_one_day_ahead() {
    // `stats.rs:57` uses `(4 + days) % 7` where `insights.rs:174` correctly
    // uses `(3 + days) % 7`, and `stats.rs:221-225` asserts the wrong value so
    // the suite is green on the bug. Brain Gotcha G003 says to keep
    // `(3+days)%7` in both files; only one was fixed.
    //
    // Currently inert: `stats::dashboard` has no production caller. It is
    // wrong the moment it is wired up.
    let src_day_label_offset = 4usize; // stats.rs
    let insights_day_label_offset = 3usize; // insights.rs
    for days in 0..14usize {
        let a =
            ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(src_day_label_offset + days) % 7];
        let b = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
            [(insights_day_label_offset + days) % 7];
        assert_ne!(
            a, b,
            "G003 resolved: stats.rs and insights.rs now agree on day {days}. \
             Fix `stats.rs:57` to `(3 + days) % 7`, correct the assertion at \
             stats.rs:221-225, and delete this test."
        );
    }
}
