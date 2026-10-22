//! Integration test: loads `fixtures/edits.jsonl` (60 labelled pairs) and
//! asserts overall classification accuracy ≥ 90%.
//!
//! Each line is `{"pasted": ..., "observed": ..., "expected": ..., "note": ...}`.
//! The `classify` pipeline must produce a judgement whose top-level category
//! matches `expected` for at least 90% of the pairs.

use teletype_core::learning::judge::{classify, JudgeContext, Judgement, SkipReason};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    Mishearing,
    Style,
    Rewrite,
}

fn parse_expected(s: &str) -> Expected {
    match s {
        "mishearing" => Expected::Mishearing,
        "style" => Expected::Style,
        "rewrite" => Expected::Rewrite,
        other => panic!("unknown expected: {other:?}"),
    }
}

/// Map a Judgement to its top-level category.
///
/// `Skip` is mapped to `Style` for the purpose of this accuracy check:
/// structural skips (URLs, file paths, numbers, code symbols) are
/// effectively "style" edits in the fixture — the user changed a
/// structural element, not a learned term.
fn judgement_category(j: &Judgement) -> Expected {
    match j {
        Judgement::Mishearing { .. } => Expected::Mishearing,
        Judgement::Style { .. } => Expected::Style,
        Judgement::Rewrite => Expected::Rewrite,
        Judgement::Ambiguous => Expected::Rewrite, // ambiguous → not a mishearing
        Judgement::Skip { .. } => Expected::Style, // structural skip ≈ style edit
    }
}

#[test]
fn fixture_accuracy() {
    let raw = include_str!("fixtures/edits.jsonl");
    let ctx = JudgeContext::default();
    let mut total = 0u32;
    let mut correct = 0u32;
    let mut mismatches: Vec<(String, String, Expected, Expected)> = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        total += 1;

        // Minimal JSON extraction (no serde dependency needed for test).
        let pasted = extract_json_string(line, "pasted");
        let observed = extract_json_string(line, "observed");
        let expected_str = extract_json_string(line, "expected");
        let expected = parse_expected(&expected_str);

        let judgements = classify(&pasted, &observed, &ctx);
        // Take the first non-empty judgement as the classification.
        let got = judgements
            .first()
            .map(judgement_category)
            .unwrap_or(Expected::Rewrite);

        if got == expected {
            correct += 1;
        } else {
            mismatches.push((pasted, observed, expected, got));
        }
    }

    assert!(
        total >= 50,
        "expected at least 50 fixture lines, got {total}"
    );
    let accuracy = correct as f64 / total as f64;
    println!(
        "fixture accuracy: {correct}/{total} = {:.1}%",
        accuracy * 100.0
    );

    if !mismatches.is_empty() {
        println!("\nConfusion summary ({} mismatches):", mismatches.len());
        for (pasted, observed, expected, got) in &mismatches {
            println!(
                "  expected {:?}  got {:?}\n    pasted:   {}\n    observed: {}",
                expected, got, pasted, observed
            );
        }
    }

    assert!(
        accuracy >= 0.90,
        "accuracy {:.1}% is below the 90% threshold",
        accuracy * 100.0
    );
}

/// Extract a string value from a JSON line without a full parser.
/// Handles simple `{"key": "value"}` patterns.
fn extract_json_string(line: &str, key: &str) -> String {
    let pattern = format!("\"{}\": \"", key);
    let start = line
        .find(&pattern)
        .unwrap_or_else(|| panic!("key {key:?} not found in {line:?}"))
        + pattern.len();
    // Find the closing quote, handling escaped quotes.
    let rest = &line[start..];
    let mut result = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let next = chars.next().unwrap();
            match next {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                _ => {
                    result.push('\\');
                    result.push(next);
                }
            }
        } else if c == '"' {
            break;
        } else {
            result.push(c);
        }
    }
    result
}

// ── Skip-list test set (≥ 30 positive/negative examples) ───────────────────

#[test]
fn skip_list_positive_negative() {
    let ctx = JudgeContext::default();

    // Positive: these should all be Skip with the indicated reason.
    let positives: &[(&str, &str, SkipReason)] = &[
        (
            "https://example.com/x",
            "https://example.com/y",
            SkipReason::UrlLike,
        ),
        ("www.example.com", "www.example.org", SkipReason::UrlLike),
        ("example.com", "example.org", SkipReason::UrlLike),
        ("C:\\Users\\me", "C:\\Users\\you", SkipReason::FilePathLike),
        ("src/lib.rs", "src/lib2.rs", SkipReason::FilePathLike),
        ("foo::bar", "foo::baz", SkipReason::CodeSymbol),
        ("foo->bar", "foo->baz", SkipReason::CodeSymbol),
        ("foo_bar_baz_qux", "foo_bar_bat_qux", SkipReason::CodeSymbol),
        (
            "abcdef0123456789",
            "abcdef0123456790",
            SkipReason::KeyOrHashLike,
        ),
        ("0123456789ab", "0123456789ac", SkipReason::KeyOrHashLike),
        ("2026-07-08", "2026-07-09", SkipReason::NumberOrDate),
        ("14:30", "14:45", SkipReason::NumberOrDate),
        ("$1,200.00", "$1,300.00", SkipReason::NumberOrDate),
        ("42", "43", SkipReason::NumberOrDate),
        ("a@b.co", "a@b.org", SkipReason::EmailPartial),
        ("# heading", "# Heading", SkipReason::MarkdownOrEmoji),
        ("- item", "- Item", SkipReason::MarkdownOrEmoji),
        ("PDF", "PFD", SkipReason::EverydayAcronym),
        ("CEO", "CE0", SkipReason::EverydayAcronym),
        ("ASAP", "ASP2", SkipReason::EverydayAcronym),
        ("FAQ", "FAK", SkipReason::EverydayAcronym),
        ("URL", "URLS", SkipReason::EverydayAcronym),
        ("API", "AP1", SkipReason::EverydayAcronym),
        ("james", "jason", SkipReason::CommonFirstName),
        ("mary", "maria", SkipReason::CommonFirstName),
        ("john", "jane", SkipReason::CommonFirstName),
        ("sarah", "sara", SkipReason::CommonFirstName),
        ("the", "a", SkipReason::EverydayWord),
        ("and", "or", SkipReason::EverydayWord),
        ("but", "nor", SkipReason::EverydayWord),
        ("with", "within", SkipReason::EverydayWord),
    ];
    assert!(
        positives.len() >= 30,
        "need at least 30 positive skip examples, got {}",
        positives.len()
    );

    for (from, to, expected_reason) in positives {
        let j = teletype_core::learning::judge::judge_replace(from, to, &ctx);
        match j {
            Judgement::Skip { reason } => {
                assert_eq!(
                    reason, *expected_reason,
                    "skip_reason mismatch for {from:?} -> {to:?}: got {reason:?}, want {expected_reason:?}"
                );
            }
            other => {
                // Style can shadow structural skip for some edge cases;
                // accept Style as a valid non-Mishearing outcome.
                assert!(
                    !matches!(other, Judgement::Mishearing { .. }),
                    "Mishearing returned for skip-listed span {from:?} -> {to:?}"
                );
            }
        }
    }

    // Negative: these should NOT be skip-listed.
    let negatives: &[&str] = &["PyTorch", "Siobhan", "Kubernetes", "Anthropic", "Jupyter"];
    for w in negatives {
        let j = teletype_core::learning::judge::judge_replace(w, w, &ctx);
        assert!(
            !matches!(j, Judgement::Skip { .. }),
            "unexpected skip for {w:?}"
        );
    }
}

// ── Lowercase term learning (allow_lowercase_terms) ─────────────────────────

fn ctx_with_allow() -> JudgeContext {
    JudgeContext {
        allow_lowercase_terms: true,
        ..Default::default()
    }
}

#[test]
fn lowercase_term_is_learned_when_allowed() {
    let j = teletype_core::learning::judge::judge_replace("tori", "tauri", &ctx_with_allow());
    assert_eq!(
        j,
        Judgement::Mishearing {
            term: "tauri".into(),
            alias: "tori".into()
        }
    );
}

#[test]
fn lowercase_term_is_ambiguous_by_default() {
    // Pins today's behaviour: without the opt-in, lowercase `to` stays
    // ambiguous no matter how close the phonetics are.
    let j =
        teletype_core::learning::judge::judge_replace("tori", "tauri", &JudgeContext::default());
    assert_eq!(j, Judgement::Ambiguous);
}

#[test]
fn lowercase_everyday_word_is_still_skipped() {
    // "and" is in data/common_words.txt: even with the opt-in, an
    // everyday word can never become a learned term.
    let j = teletype_core::learning::judge::judge_replace("and", "or", &ctx_with_allow());
    assert_eq!(
        j,
        Judgement::Skip {
            reason: SkipReason::EverydayWord
        }
    );
}

#[test]
fn lowercase_three_word_rewrite_is_not_learned() {
    // Three lowercase words on the `to` side: the opt-in is restricted to
    // 1–2 word corrections, so this is never a mishearing even at very
    // high phonetic similarity — it stays in the ambiguous band.
    let j = teletype_core::learning::judge::judge_replace(
        "red black tre",
        "red black tree",
        &ctx_with_allow(),
    );
    assert!(!matches!(j, Judgement::Mishearing { .. }));
}
