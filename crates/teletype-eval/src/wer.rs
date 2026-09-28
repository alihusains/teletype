//! Word error rate over word tokens.
//!
//! Tokenisation: lowercase, strip punctuation, split on whitespace, drop
//! empty tokens. Digits and internal apostrophes are kept (e.g. `don't`,
//! `2026`).
//!
//! Alignment is a standard Levenshtein DP over word tokens using a two-row
//! rolling buffer, so memory is O(min(n, m)) in the row dimension rather than
//! a full n*m matrix. The shorter sequence is used as the row axis.

use std::collections::HashMap;

/// One aligned word edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EditKind {
    Substitution,
    Deletion,
    Insertion,
}

/// A single aligned edit with its position in the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Edit {
    pub kind: EditKind,
    /// 0-based index into the reference token stream (insertions carry the
    /// index of the reference token they follow).
    pub ref_index: usize,
}

/// Result of aligning one reference/hypothesis pair.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wer {
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
    /// N: number of reference tokens.
    pub reference_words: usize,
    /// `(S + D + I) / N`. `0.0` when `N == 0` (guarded).
    pub wer: f64,
    /// The full aligned edit list, in reference order.
    pub edits: Vec<Edit>,
}

/// Aggregate WER over many cases: micro- and macro-average plus the worst
/// cases, so a single bad utterance is visible in the report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WerSummary {
    pub cases: usize,
    /// Total reference words across all cases.
    pub total_reference_words: usize,
    /// Total errors (S + D + I) across all cases.
    pub total_errors: usize,
    /// Micro-average: total errors / total reference words.
    pub micro_wer: f64,
    /// Macro-average: mean of the per-case WERs.
    pub macro_wer: f64,
    /// Up to `worst_n` cases with the highest per-case WER, worst first.
    pub worst: Vec<WorstCase>,
}

/// One entry in [`WerSummary::worst`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorstCase {
    pub id: String,
    pub reference: String,
    pub hypothesis: String,
    pub wer: f64,
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
}

/// Lowercase, strip punctuation, split on whitespace, drop empties.
///
/// "Punctuation" means any character that is not a letter, a digit, or an
/// apostrophe. Digits and internal apostrophes survive: `don't` stays `don't`,
/// `3` stays `3`.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        let cleaned: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .collect();
        let lowered = cleaned.to_lowercase();
        if !lowered.is_empty() {
            out.push(lowered);
        }
    }
    out
}

/// Align `reference` to `hypothesis` with Levenshtein DP over word tokens and
/// return the error breakdown plus WER.
///
/// `wer = (S + D + I) / N` where N is the reference word count. When N is 0
/// the rate is defined as 0.0 (there is nothing to be wrong about); the edit
/// counts still report the insertions.
///
/// Memory: the DP values use a two-row rolling buffer (O(min(n, m)) in the
/// row dimension). The edit sequence is reconstructed from a decision grid
/// of 1 byte per (i, j) cell — the standard trade for obtaining the
/// per-edit breakdown, which is what the report's worst-case section needs.
pub fn word_error_rate(reference: &str, hypothesis: &str) -> Wer {
    let ref_tokens = tokenize(reference);
    let hyp_tokens = tokenize(hypothesis);
    let (substitutions, deletions, insertions, edits) = align(&ref_tokens, &hyp_tokens);
    let n = ref_tokens.len();
    let wer = if n == 0 {
        0.0
    } else {
        (substitutions + deletions + insertions) as f64 / n as f64
    };
    Wer {
        substitutions,
        deletions,
        insertions,
        reference_words: n,
        wer,
        edits,
    }
}

/// Levenshtein DP with backtrace.
///
/// The shorter token list is used as the row axis, and the DP values are
/// rolled through two rows of length `min(n, m) + 1` — the row dimension is
/// O(min(n, m)), never a full n*m matrix of values. Decisions (which of the
/// three moves was taken) are kept as a 1-byte-per-cell grid so the edit
/// sequence can be reconstructed.
fn align(ref_tokens: &[String], hyp_tokens: &[String]) -> (usize, usize, usize, Vec<Edit>) {
    // Row axis = shorter sequence. If the hypothesis is shorter we transpose
    // (swap the roles) and translate the edit kinds back afterwards:
    // a deletion against the swapped reference is an insertion against the
    // real reference, and vice versa.
    let (rows, cols, swapped) = if ref_tokens.len() <= hyp_tokens.len() {
        (ref_tokens, hyp_tokens, false)
    } else {
        (hyp_tokens, ref_tokens, true)
    };
    let m = rows.len();
    let n = cols.len();

    // Decision bits: 1 = match/substitution (consume both), 2 = deletion
    // from `rows` (consume row), 4 = insertion (consume col).
    // Row 0 of the DP values is (0, 1, 2, ...); it is never overwritten.
    let mut prev: Vec<u32> = (0..=n as u32).collect();
    let mut curr = vec![0u32; n + 1];
    // Decision grid: decisions[i][j] records the move taken to reach cell
    // (i, j). Row 0 and column 0 are pure insertions/deletions by
    // construction and are not recorded.
    let mut decisions: Vec<Vec<u8>> = vec![vec![0u8; n + 1]; m + 1];

    for i in 1..=m {
        let next = &mut curr[..];
        let above = &prev[..];
        next[0] = i as u32;
        for j in 0..n {
            let cost = if rows[i - 1] == cols[j] { 0 } else { 1 };
            let sub = above[j] + cost;
            let del = above[j + 1] + 1;
            let ins = next[j] + 1;
            let (best, bit) = if sub <= del && sub <= ins {
                (sub, 1u8)
            } else if del <= ins {
                (del, 2u8)
            } else {
                (ins, 4u8)
            };
            next[j + 1] = best;
            decisions[i][j + 1] = bit;
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    // Backtrace from (m, n) to (0, 0).
    let mut edits: Vec<Edit> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        let bit = decisions[i][j];
        if i > 0 && (bit & 1) != 0 {
            let is_sub = rows[i - 1] != cols[j - 1];
            if is_sub {
                edits.push(Edit {
                    kind: EditKind::Substitution,
                    ref_index: if swapped { j - 1 } else { i - 1 },
                });
            }
            i -= 1;
            j -= 1;
        } else if i > 0 && (bit & 2) != 0 {
            // Consumed a row token.
            edits.push(Edit {
                kind: if swapped {
                    EditKind::Insertion
                } else {
                    EditKind::Deletion
                },
                ref_index: if swapped { j.saturating_sub(1) } else { i - 1 },
            });
            i -= 1;
        } else {
            // Consumed a col token.
            edits.push(Edit {
                kind: if swapped {
                    EditKind::Deletion
                } else {
                    EditKind::Insertion
                },
                ref_index: if swapped {
                    i.saturating_sub(1)
                } else {
                    j.saturating_sub(1)
                },
            });
            j -= 1;
        }
    }
    edits.reverse();

    let (mut substitutions, mut deletions, mut insertions) = (0usize, 0usize, 0usize);
    for e in &edits {
        match e.kind {
            EditKind::Substitution => substitutions += 1,
            EditKind::Deletion => deletions += 1,
            EditKind::Insertion => insertions += 1,
        }
    }
    (substitutions, deletions, insertions, edits)
}

/// Aggregate per-case [`Wer`]s (with their case metadata) into a
/// [`WerSummary`].
pub fn summarize(cases: &[(String, String, String, Wer)]) -> WerSummary {
    let total_words: usize = cases.iter().map(|(_, _, _, w)| w.reference_words).sum();
    let total_errors: usize = cases
        .iter()
        .map(|(_, _, _, w)| w.substitutions + w.deletions + w.insertions)
        .sum();
    let micro_wer = if total_words == 0 {
        0.0
    } else {
        total_errors as f64 / total_words as f64
    };
    let macro_wer = if cases.is_empty() {
        0.0
    } else {
        cases.iter().map(|(_, _, _, w)| w.wer).sum::<f64>() / cases.len() as f64
    };

    let mut worst: Vec<WorstCase> = cases
        .iter()
        .map(|(id, reference, hypothesis, w)| WorstCase {
            id: id.clone(),
            reference: reference.clone(),
            hypothesis: hypothesis.clone(),
            wer: w.wer,
            substitutions: w.substitutions,
            deletions: w.deletions,
            insertions: w.insertions,
        })
        .collect();
    worst.sort_by(|a, b| {
        b.wer
            .partial_cmp(&a.wer)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    worst.truncate(5);

    WerSummary {
        cases: cases.len(),
        total_reference_words: total_words,
        total_errors,
        micro_wer,
        macro_wer,
        worst,
    }
}

/// Per-tag WER grouping, keyed by tag, sorted by tag name.
pub fn by_tag(cases: &[(String, String, Vec<String>, Wer)]) -> Vec<(String, TagWer)> {
    #[derive(Default)]
    struct Acc {
        words: usize,
        errors: usize,
        cases: usize,
    }
    let mut acc: HashMap<String, Acc> = HashMap::new();
    for (id, _ref, tags, w) in cases {
        let _ = id;
        let errors = w.substitutions + w.deletions + w.insertions;
        for tag in tags {
            let a = acc.entry(tag.clone()).or_default();
            a.words += w.reference_words;
            a.errors += errors;
            a.cases += 1;
        }
    }
    let mut out: Vec<(String, TagWer)> = acc
        .into_iter()
        .map(|(tag, a)| {
            (
                tag,
                TagWer {
                    cases: a.cases,
                    reference_words: a.words,
                    errors: a.errors,
                    wer: if a.words == 0 {
                        0.0
                    } else {
                        a.errors as f64 / a.words as f64
                    },
                },
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Micro-WER for one tag group.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TagWer {
    pub cases: usize,
    pub reference_words: usize,
    pub errors: usize,
    pub wer: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_is_zero() {
        let w = word_error_rate("the cat sat on the mat", "the cat sat on the mat");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 0));
        assert_eq!(w.wer, 0.0);
    }

    #[test]
    fn case_and_punctuation_insensitive() {
        let w = word_error_rate("The Cat, Sat! On the Mat.", "the cat sat on the mat");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 0));
        assert_eq!(w.wer, 0.0);
    }

    #[test]
    fn pure_substitution() {
        // "cat" -> "dog": 1 substitution over 4 reference words.
        let w = word_error_rate("the cat sat on", "the dog sat on");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (1, 0, 0));
        assert_eq!(w.reference_words, 4);
        assert!((w.wer - 0.25).abs() < 1e-12);
    }

    #[test]
    fn deletion() {
        // "the cat sat" -> "the sat": 1 deletion over 3 reference words.
        let w = word_error_rate("the cat sat", "the sat");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 1, 0));
        assert!((w.wer - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn insertion() {
        // "the cat" -> "the fat cat": 1 insertion over 2 reference words.
        let w = word_error_rate("the cat", "the fat cat");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 1));
        assert!((w.wer - 0.5).abs() < 1e-12);
    }

    #[test]
    fn empty_reference() {
        let w = word_error_rate("", "the cat");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 2));
        assert_eq!(w.reference_words, 0);
        assert_eq!(w.wer, 0.0, "N == 0 must be guarded to 0.0");
    }

    #[test]
    fn empty_hypothesis_is_all_deletions() {
        let w = word_error_rate("the cat sat", "");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 3, 0));
        assert_eq!(w.wer, 1.0);
    }

    #[test]
    fn both_empty() {
        let w = word_error_rate("", "");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 0));
        assert_eq!(w.wer, 0.0);
    }

    #[test]
    fn digits_and_apostrophes_survive() {
        // "don't" must align as one token, not "don" + "t".
        let w = word_error_rate("don't do it", "do not do it");
        // ref: [don't, do, it]  hyp: [do, not, do, it]
        // optimal: sub don't->do, ins not, match do, match it => S=1 I=1, WER 2/3
        assert_eq!((w.substitutions, w.deletions, w.insertions), (1, 0, 1));
        assert!((w.wer - 2.0 / 3.0).abs() < 1e-12);
    }

    /// Hand-computed realistic multi-sentence example (not a snapshot):
    ///
    /// ref:  "the deploy went out this morning and it is all good"  (12 words)
    /// hyp:  "the deploy went out this morning and everything is good"
    ///
    /// Hand-computed: "it" (ref) vs "everything" (hyp) at the same
    /// alignment position. The kind split between substitution and
    /// deletion+insertion is not unique, but the total error count is
    /// always the optimal edit distance: 2 errors, WER = 2/12 = 1/6.
    #[test]
    fn realistic_multi_word_example_hand_computed() {
        let w = word_error_rate(
            "the deploy went out this morning and it is all good",
            "the deploy went out this morning and everything is good",
        );
        assert_eq!(
            w.substitutions + w.deletions + w.insertions,
            2,
            "one word swapped for a different word is exactly 2 edit-distance errors"
        );
        assert_eq!(w.reference_words, 11);
        assert!((w.wer - 2.0 / 11.0).abs() < 1e-12);
    }

    /// Second hand-computed example mixing all three edit kinds:
    ///
    /// ref: "please call me after lunch"            (6 words)
    /// hyp: "please call me after the lunch break"
    ///
    /// ref: [please, call, me, after, lunch]  (5 words)
    /// hyp: [please, call, me, after, the, lunch, break]
    ///
    /// Optimal alignment: match x4, insert "the", match "lunch",
    /// insert "break" => S=0, D=0, I=2, WER = 2/5 = 0.4.
    #[test]
    fn realistic_mixed_edits_hand_computed() {
        let w = word_error_rate(
            "please call me after lunch",
            "please call me after the lunch break",
        );
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 2));
        assert_eq!(w.reference_words, 5);
        assert!((w.wer - 0.4).abs() < 1e-12);
    }

    /// Third hand-computed example with a substitution AND a deletion:
    ///
    /// ref: "the quick brown fox"        (4 words)
    /// hyp: "the slow fox"
    ///
    /// Optimal: match "the", sub "quick"->"slow", del "brown", match "fox"
    /// => S=1, D=1, I=0, WER = 2/4 = 0.5.
    #[test]
    fn realistic_sub_and_del_hand_computed() {
        let w = word_error_rate("the quick brown fox", "the slow fox");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (1, 1, 0));
        assert_eq!(w.reference_words, 4);
        assert_eq!(w.wer, 0.5);
    }

    /// The alignment math must actually drive the counts: mutating a
    /// reference word must change the error counts, not just the string.
    #[test]
    fn mutating_reference_changes_error_counts() {
        let base = word_error_rate("the cat sat on the mat", "the cat sat on the mat");
        assert_eq!(
            (base.substitutions, base.deletions, base.insertions),
            (0, 0, 0)
        );

        // Mutate one reference word: "cat" -> "dog" must register exactly one
        // substitution.
        let mut1 = word_error_rate("the dog sat on the mat", "the cat sat on the mat");
        assert_eq!(
            (mut1.substitutions, mut1.deletions, mut1.insertions),
            (1, 0, 0),
            "mutating a reference word must produce exactly one substitution"
        );

        // Delete a reference word: the total error count must rise by
        // exactly one. (The kind decomposition is not unique for adjacent
        // tokens — Levenshtein may report it as a substitution — but the
        // total S+D+I is always the optimal edit distance.)
        let mut2 = word_error_rate("the mat sat on the mat", "the cat sat on the mat");
        assert_eq!(
            mut2.substitutions + mut2.deletions + mut2.insertions,
            1,
            "deleting a reference word must add exactly one error"
        );

        // Add a hypothesis word: must register exactly one insertion.
        let mut3 = word_error_rate("the cat sat on the mat", "the cat sat on the big mat");
        assert_eq!(
            (mut3.substitutions, mut3.deletions, mut3.insertions),
            (0, 0, 1),
            "adding a hypothesis word must produce exactly one insertion"
        );
    }

    #[test]
    fn summarize_micro_and_macro() {
        // Case A: "a b" vs "a c" -> S=1, N=2, wer=0.5
        // Case B: "c d e" vs "c d e" -> 0 errors, N=3, wer=0.0
        // micro = 1 / 5 = 0.2 ; macro = (0.5 + 0.0) / 2 = 0.25
        let a = word_error_rate("a b", "a c");
        let b = word_error_rate("c d e", "c d e");
        let s = summarize(&[
            ("a".into(), "a b".into(), "a c".into(), a),
            ("b".into(), "c d e".into(), "c d e".into(), b),
        ]);
        let _ = "hypothesis is the 3rd tuple element";
        assert_eq!(s.cases, 2);
        assert_eq!(s.total_reference_words, 5);
        assert_eq!(s.total_errors, 1);
        assert!((s.micro_wer - 0.2).abs() < 1e-12);
        assert!((s.macro_wer - 0.25).abs() < 1e-12);
        assert_eq!(s.worst.len(), 2);
        assert_eq!(s.worst[0].id, "a", "worst case must be first");
    }

    #[test]
    fn summarize_empty() {
        let s = summarize(&[]);
        assert_eq!(s.cases, 0);
        assert_eq!(s.micro_wer, 0.0);
        assert_eq!(s.macro_wer, 0.0);
    }
}
