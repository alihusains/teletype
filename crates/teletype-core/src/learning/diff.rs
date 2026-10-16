//! Word-level diffing for the learning loop (Phase 7, P7.3).
//!
//! Diffs `pasted` (what the engine dictated) against `observed` (what the
//! user left after editing) at word granularity, so the judge can classify
//! each replacement span. Tokenization is plain whitespace splitting —
//! punctuation stays attached to its token ("hi." is one token).
//!
//! Implementation note: `similar` 3.x's `TextDiff` only diffs `str`
//! (line/word/byte modes), not arbitrary token sequences, so we run its
//! `diff()` algorithm directly over the token slices. That is exactly the
//! Myers diff the task asks for, with stable output.

use similar::{algorithms::Capture, Algorithm};

/// One op in a word-level diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOp {
    /// Consecutive identical words.
    Equal(Vec<String>),
    /// Words in `pasted` replaced by (possibly different-length) words in `observed`.
    Replace { from: Vec<String>, to: Vec<String> },
    /// Words present only in `observed`.
    Insert(Vec<String>),
    /// Words present only in `pasted`.
    Delete(Vec<String>),
}

/// Tokenize on whitespace; empty strings yield no tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

/// Diff `pasted` vs `observed` at word level using `similar`'s Myers
/// algorithm over the token slices. Output is deterministic for a given
/// input pair (`similar`'s Myers diff is stable for equal inputs).
pub fn diff_words(pasted: &str, observed: &str) -> Vec<EditOp> {
    let left = tokenize(pasted);
    let right = tokenize(observed);
    let mut capture = Capture::new();
    similar::algorithms::diff(
        Algorithm::Myers,
        &mut capture,
        &left,
        0..left.len(),
        &right,
        0..right.len(),
    )
    .expect("similar diff over finite slices");
    merge_replace(to_edit_ops(&left, &right, capture.ops()))
}

/// Map raw `similar` ops (old/new index + length) onto our `EditOp`s.
fn to_edit_ops(left: &[String], right: &[String], ops: &[similar::DiffOp]) -> Vec<EditOp> {
    ops.iter()
        .map(|op| match *op {
            similar::DiffOp::Equal {
                old_index,
                new_index: _,
                len,
            } => EditOp::Equal(left[old_index..old_index + len].to_vec()),
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => EditOp::Delete(left[old_index..old_index + old_len].to_vec()),
            similar::DiffOp::Insert {
                new_index, new_len, ..
            } => EditOp::Insert(right[new_index..new_index + new_len].to_vec()),
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => EditOp::Replace {
                from: left[old_index..old_index + old_len].to_vec(),
                to: right[new_index..new_index + new_len].to_vec(),
            },
        })
        .collect()
}

/// Merge adjacent non-Equal ops into a single `Replace`. `similar`'s raw
/// ops can emit the two halves of a replacement as separate Delete/Insert
/// ops (even interleaved); the judge expects one Replace per changed span.
fn merge_replace(ops: Vec<EditOp>) -> Vec<EditOp> {
    let mut out: Vec<EditOp> = Vec::with_capacity(ops.len());
    for op in ops {
        match op {
            EditOp::Equal(_) => out.push(op),
            other => {
                if out.last().is_some_and(|o| !matches!(o, EditOp::Equal(_))) {
                    // absorb into the trailing non-Equal op
                    let last = out.pop().unwrap();
                    let (mut lf, mut lt) = match last {
                        EditOp::Replace { from, to } => (from, to),
                        EditOp::Delete(w) => (w, vec![]),
                        EditOp::Insert(w) => (vec![], w),
                        EditOp::Equal(_) => unreachable!(),
                    };
                    let (a, b) = match other {
                        EditOp::Delete(w) => (w, vec![]),
                        EditOp::Insert(w) => (vec![], w),
                        EditOp::Replace { from, to } => (from, to),
                        EditOp::Equal(_) => unreachable!(),
                    };
                    lf.extend(a);
                    lt.extend(b);
                    out.push(EditOp::Replace { from: lf, to: lt });
                } else {
                    out.push(other);
                }
            }
        }
    }
    out
}

/// The `Replace` ops that are candidate spans for the judge: 1–4 words on
/// each side. Longer replacements are handled by the judge as rewrites.
pub fn changed_pairs(ops: &[EditOp]) -> Vec<(Vec<String>, Vec<String>)> {
    ops.iter()
        .filter_map(|op| match op {
            EditOp::Replace { from, to }
                if (1..=4).contains(&from.len()) && (1..=4).contains(&to.len()) =>
            {
                Some((from.clone(), to.clone()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_is_one_equal() {
        let ops = diff_words("hello world", "hello world");
        assert_eq!(
            ops,
            vec![EditOp::Equal(vec!["hello".into(), "world".into()])]
        );
    }

    #[test]
    fn single_word_replace() {
        let ops = diff_words("call john", "call sarah");
        assert_eq!(
            ops,
            vec![
                EditOp::Equal(vec!["call".into()]),
                EditOp::Replace {
                    from: vec!["john".into()],
                    to: vec!["sarah".into()]
                },
            ]
        );
        assert_eq!(
            changed_pairs(&ops),
            vec![(vec!["john".into()], vec!["sarah".into()])]
        );
    }

    #[test]
    fn insert_op() {
        let ops = diff_words("hi there", "hi there mate");
        assert!(matches!(&ops[1], EditOp::Insert(w) if w == &["mate".to_string()]));
    }

    #[test]
    fn delete_op() {
        let ops = diff_words("hi there mate", "hi there");
        assert!(matches!(&ops[1], EditOp::Delete(w) if w == &["mate".to_string()]));
    }

    #[test]
    fn four_word_replace_is_a_candidate() {
        // One Replace span of 3 words per side (4th side-word "w" is the
        // equal context "e" shifted; similar's stable output keeps the
        // span at 3+4, still within the 1..=4 candidate window).
        let ops = diff_words("a b c d e", "a x y z w e");
        let pairs = changed_pairs(&ops);
        assert!(
            pairs
                .iter()
                .any(|(f, _)| f == &["b".to_string(), "c".into(), "d".into()]),
            "expected a 3-word from-side candidate, got {pairs:?}"
        );
    }

    #[test]
    fn five_word_replace_is_not_a_candidate() {
        let ops = diff_words("a b c d e f g", "a x y z w v g");
        assert!(changed_pairs(&ops).is_empty());
    }

    #[test]
    fn punctuation_stays_attached() {
        let ops = diff_words("hi, john.", "hi, sarah.");
        assert_eq!(
            ops,
            vec![
                EditOp::Equal(vec!["hi,".into()]),
                EditOp::Replace {
                    from: vec!["john.".into()],
                    to: vec!["sarah.".into()]
                },
            ]
        );
    }

    #[test]
    fn unicode_safety() {
        let ops = diff_words("café naïve", "café naïveté");
        let pairs = changed_pairs(&ops);
        assert_eq!(pairs, vec![(vec!["naïve".into()], vec!["naïveté".into()])]);
    }

    #[test]
    fn empty_strings() {
        assert!(diff_words("", "").is_empty());
        assert_eq!(
            diff_words("", "hi"),
            vec![EditOp::Insert(vec!["hi".into()])]
        );
        assert_eq!(
            diff_words("hi", ""),
            vec![EditOp::Delete(vec!["hi".into()])]
        );
    }

    #[test]
    fn changed_pairs_ignores_non_replace_ops() {
        let ops = diff_words("a b", "a c d");
        assert_eq!(
            changed_pairs(&ops),
            vec![(vec!["b".into()], vec!["c".into(), "d".into()])]
        );
    }
}
