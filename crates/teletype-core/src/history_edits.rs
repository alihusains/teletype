//! What each text stage changed in a dictation, as inline spans for the
//! Dictation tab.
//!
//! The pipeline records each stage's own before/after pair
//! ([`crate::pipeline::StageText`]). [`compose`] replays those pairs over the
//! raw ASR tokens and returns the final text as an ordered list of spans,
//! each one either unchanged or attributed to the stage that changed it.
//!
//! Filler removals are never inferred from a raw-to-final diff: they come
//! from the filler stage's own pair, so a filler can never be blamed on
//! polish. When a stage's `before` does not match what the earlier stages
//! produced, [`compose`] returns `None`. No diff is better than a wrong one.

use serde::{Deserialize, Serialize};

use crate::learning::diff::{diff_words, EditOp};
use crate::pipeline::{EditKind, StageText};

/// Raw texts longer than this many tokens get no diff (same limit as the
/// UI's `wordDiff.ts`).
pub const MAX_DIFF_TOKENS: usize = 400;

/// Which stage made a span, or `Same` for unchanged text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpanKind {
    Same,
    Filler,
    Dictionary,
    Autotext,
    Polish,
    Format,
    Correction,
}

impl From<&EditKind> for SpanKind {
    fn from(k: &EditKind) -> Self {
        match k {
            EditKind::Filler => SpanKind::Filler,
            EditKind::Dictionary => SpanKind::Dictionary,
            EditKind::Autotext => SpanKind::Autotext,
            EditKind::Polish => SpanKind::Polish,
            EditKind::Format => SpanKind::Format,
            EditKind::Correction => SpanKind::Correction,
        }
    }
}

/// One run of the final text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    /// `same` for unchanged text, otherwise the stage that made the change.
    pub kind: SpanKind,
    /// Original words ("" for a pure insertion).
    pub from: String,
    /// New words ("" for a removal; unused for `same`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    /// Only case and/or punctuation differ.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub minor: bool,
}

/// Working segment. For `Same`, `to == from` (one raw token).
#[derive(Debug, Clone)]
struct Seg {
    kind: SpanKind,
    from: Vec<String>,
    to: Vec<String>,
}

/// One change a stage made, over the visible token stream.
struct Group {
    /// Index of the first visible token it touches.
    start: usize,
    /// Visible tokens it removes or replaces (0 for a pure insertion).
    len: usize,
    /// Tokens it puts in their place.
    new: Vec<String>,
}

fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

/// Replays `stages` over the raw tokens. `None` when the chain does not
/// line up, or the raw text is over [`MAX_DIFF_TOKENS`].
pub fn compose(raw: &str, stages: &[StageText]) -> Option<Vec<TextSpan>> {
    let tokens = tokenize(raw);
    if tokens.len() > MAX_DIFF_TOKENS {
        return None;
    }
    let mut segs: Vec<Seg> = tokens
        .into_iter()
        .map(|t| Seg {
            kind: SpanKind::Same,
            from: vec![t.clone()],
            to: vec![t],
        })
        .collect();
    for stage in stages {
        segs = apply_stage(
            &segs,
            SpanKind::from(&stage.kind),
            &stage.before,
            &stage.after,
        )?;
    }
    Some(finish(segs))
}

/// Applies one stage's word diff to the segments.
fn apply_stage(segs: &[Seg], kind: SpanKind, before: &str, after: &str) -> Option<Vec<Seg>> {
    // Visible token index -> (segment, position in its `to`).
    let vis: Vec<(usize, usize)> = segs
        .iter()
        .enumerate()
        .flat_map(|(i, s)| (0..s.to.len()).map(move |p| (i, p)))
        .collect();
    let visible: Vec<&str> = vis.iter().map(|&(i, p)| segs[i].to[p].as_str()).collect();
    let before_tokens = tokenize(before);
    if before_tokens.len() != visible.len()
        || before_tokens.iter().zip(&visible).any(|(a, b)| a != b)
    {
        return None;
    }

    let mut groups = Vec::new();
    let mut vi = 0;
    for op in diff_words(before, after) {
        match op {
            EditOp::Equal(w) => vi += w.len(),
            EditOp::Delete(w) => {
                groups.push(Group {
                    start: vi,
                    len: w.len(),
                    new: vec![],
                });
                vi += w.len();
            }
            EditOp::Insert(w) => groups.push(Group {
                start: vi,
                len: 0,
                new: w,
            }),
            EditOp::Replace { from, to } => {
                groups.push(Group {
                    start: vi,
                    len: from.len(),
                    new: to,
                });
                vi += from.len();
            }
        }
    }
    if groups.is_empty() {
        return Some(segs.to_vec());
    }

    // The group (if any) that covers each visible token.
    let mut owner_group: Vec<Option<usize>> = vec![None; vis.len()];
    for (g, group) in groups.iter().enumerate() {
        for slot in owner_group.iter_mut().skip(group.start).take(group.len) {
            *slot = Some(g);
        }
    }
    // Segment ranges [lo, hi] that a change touches. Groups that share a
    // segment fold into one cluster.
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    for group in groups.iter().filter(|g| g.len > 0) {
        let lo = vis[group.start].0;
        let hi = vis[group.start + group.len - 1].0;
        match clusters.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => clusters.push((lo, hi)),
        }
    }

    // Visible index range of each segment.
    let mut seg_vis = Vec::with_capacity(segs.len());
    let mut at = 0;
    for s in segs {
        seg_vis.push((at, at + s.to.len()));
        at += s.to.len();
    }
    let inserts_at = |k: usize| groups.iter().filter(move |g| g.len == 0 && g.start == k);

    let mut out = Vec::with_capacity(segs.len() + groups.len());
    for g in inserts_at(0) {
        out.push(Seg {
            kind,
            from: vec![],
            to: g.new.clone(),
        });
    }
    let mut i = 0;
    while i < segs.len() {
        let Some(&(lo, hi)) = clusters.iter().find(|c| c.0 == i) else {
            out.push(segs[i].clone());
            let (_, end) = seg_vis[i];
            if !segs[i].to.is_empty() {
                for g in inserts_at(end) {
                    out.push(Seg {
                        kind,
                        from: vec![],
                        to: g.new.clone(),
                    });
                }
            }
            i += 1;
            continue;
        };
        let (cluster_start, _) = seg_vis[lo];
        let (_, cluster_end) = seg_vis[hi];
        let mut merged = Seg {
            kind,
            from: vec![],
            to: vec![],
        };
        let mut skipped = Vec::new();
        for (j, seg) in segs.iter().enumerate().take(hi + 1).skip(lo) {
            if seg.to.is_empty() {
                // A removal inside the cluster keeps its own attribution.
                skipped.push(seg.clone());
                continue;
            }
            merged.from.extend(seg.from.iter().cloned());
            let (a, b) = seg_vis[j];
            for k in a..b {
                if k > cluster_start {
                    for g in inserts_at(k) {
                        merged.to.extend(g.new.iter().cloned());
                    }
                }
                match owner_group[k] {
                    Some(g) if groups[g].start == k => {
                        merged.to.extend(groups[g].new.iter().cloned())
                    }
                    Some(_) => {}
                    None => merged.to.push(segs[j].to[k - a].clone()),
                }
            }
        }
        if merged.from == merged.to {
            merged.kind = SpanKind::Same;
        }
        out.push(merged);
        for g in inserts_at(cluster_end) {
            out.push(Seg {
                kind,
                from: vec![],
                to: g.new.clone(),
            });
        }
        out.extend(skipped);
        i = hi + 1;
    }
    Some(out)
}

/// True when two word runs differ only in case and ASCII punctuation.
fn is_minor(from: &str, to: &str) -> bool {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_ascii_punctuation())
            .flat_map(char::to_lowercase)
            .collect()
    };
    norm(from) == norm(to)
}

/// Merges adjacent segments of the same kind and renders the spans.
fn finish(segs: Vec<Seg>) -> Vec<TextSpan> {
    let mut merged: Vec<Seg> = Vec::new();
    for seg in segs {
        if seg.from.is_empty() && seg.to.is_empty() {
            continue;
        }
        match merged.last_mut() {
            Some(last) if last.kind == seg.kind => {
                last.from.extend(seg.from);
                last.to.extend(seg.to);
            }
            _ => merged.push(seg),
        }
    }
    merged
        .into_iter()
        .map(|s| {
            let from = s.from.join(" ");
            if s.kind == SpanKind::Same {
                return TextSpan {
                    kind: SpanKind::Same,
                    from,
                    to: String::new(),
                    minor: false,
                };
            }
            let to = s.to.join(" ");
            TextSpan {
                kind: s.kind,
                minor: is_minor(&from, &to),
                from,
                to,
            }
        })
        .collect()
}

/// The final text the spans spell out, words joined by single spaces.
pub fn visible_text(spans: &[TextSpan]) -> String {
    spans
        .iter()
        .map(|s| {
            if s.kind == SpanKind::Same {
                &s.from
            } else {
                &s.to
            }
        })
        .filter(|t| !t.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(kind: EditKind, before: &str, after: &str) -> StageText {
        StageText {
            kind,
            before: before.into(),
            after: after.into(),
        }
    }

    fn same(t: &str) -> TextSpan {
        TextSpan {
            kind: SpanKind::Same,
            from: t.into(),
            to: String::new(),
            minor: false,
        }
    }

    fn span(kind: SpanKind, from: &str, to: &str) -> TextSpan {
        TextSpan {
            kind,
            from: from.into(),
            to: to.into(),
            minor: false,
        }
    }

    #[test]
    fn filler_removal_matches_the_reference_card() {
        let raw = "hey sam um quick update the";
        let spans = compose(
            raw,
            &[stage(EditKind::Filler, raw, "hey sam quick update the")],
        )
        .unwrap();
        assert_eq!(
            spans,
            vec![
                same("hey sam"),
                span(SpanKind::Filler, "um", ""),
                same("quick update the"),
            ]
        );
        assert_eq!(visible_text(&spans), "hey sam quick update the");
    }

    #[test]
    fn no_stages_is_one_same_span() {
        let spans = compose("need option two.", &[]).unwrap();
        assert_eq!(spans, vec![same("need option two.")]);
    }

    #[test]
    fn last_writer_wins_and_from_keeps_the_raw_word() {
        let raw = "call jon tomorrow";
        let spans = compose(
            raw,
            &[
                stage(EditKind::Dictionary, raw, "call John tomorrow"),
                stage(
                    EditKind::Polish,
                    "call John tomorrow",
                    "call Jonathan tomorrow",
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            spans,
            vec![
                same("call"),
                span(SpanKind::Polish, "jon", "Jonathan"),
                same("tomorrow"),
            ]
        );
    }

    #[test]
    fn case_only_polish_change_is_minor() {
        let raw = "hello there";
        let spans = compose(raw, &[stage(EditKind::Polish, raw, "Hello there.")]).unwrap();
        assert_eq!(spans.len(), 1);
        assert!(spans[0].minor, "{spans:?}");
        assert_eq!(spans[0].kind, SpanKind::Polish);
        assert_eq!(spans[0].to, "Hello there.");
    }

    #[test]
    fn broken_chain_is_none() {
        let raw = "a b c";
        assert!(compose(raw, &[stage(EditKind::Polish, "a b x", "a b y")]).is_none());
        assert!(compose(
            raw,
            &[
                stage(EditKind::Filler, raw, "a c"),
                stage(EditKind::Polish, raw, "a c d"),
            ]
        )
        .is_none());
    }

    #[test]
    fn too_many_tokens_is_none() {
        let raw = vec!["w"; MAX_DIFF_TOKENS + 1].join(" ");
        assert!(compose(&raw, &[]).is_none());
        let ok = vec!["w"; MAX_DIFF_TOKENS].join(" ");
        assert!(compose(&ok, &[]).is_some());
    }

    #[test]
    fn insertion_only_stage_has_empty_from() {
        let raw = "see you";
        let spans = compose(raw, &[stage(EditKind::Polish, raw, "see you soon")]).unwrap();
        assert_eq!(
            spans,
            vec![same("see you"), span(SpanKind::Polish, "", "soon")]
        );
        let front = compose(raw, &[stage(EditKind::Format, raw, "ok see you")]).unwrap();
        assert_eq!(front[0], span(SpanKind::Format, "", "ok"));
        assert_eq!(visible_text(&front), "ok see you");
    }

    #[test]
    fn filler_keeps_its_attribution_through_a_later_polish() {
        let raw = "so um tuesday works";
        let stages = [
            stage(EditKind::Filler, raw, "so tuesday works"),
            stage(EditKind::Polish, "so tuesday works", "So Thursday works."),
        ];
        let spans = compose(raw, &stages).unwrap();
        assert!(spans
            .iter()
            .any(|s| s.kind == SpanKind::Filler && s.from == "um" && s.to.is_empty()));
        assert!(spans
            .iter()
            .all(|s| s.kind != SpanKind::Polish || !s.from.contains("um")));
        assert_eq!(visible_text(&spans), "So Thursday works.");
    }

    #[test]
    fn spans_round_trip_through_json() {
        let spans = vec![same("a"), span(SpanKind::Filler, "um", "")];
        let json = serde_json::to_string(&spans).unwrap();
        assert_eq!(
            json,
            r#"[{"kind":"same","from":"a"},{"kind":"filler","from":"um"}]"#
        );
        let back: Vec<TextSpan> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spans);
    }
}
