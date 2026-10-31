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
//! polish. When a later stage rewrites words an earlier stage already
//! changed, the earlier span is kept (marked `superseded`) and the later
//! span starts from the words that stage received. When a stage's `before`
//! does not match what the earlier stages produced, or the spans do not
//! spell the final text, [`compose`] returns `None`. No diff is better
//! than a wrong one.

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
    /// The words this stage received ("" for a pure insertion). For the
    /// first stage to touch a word that is the raw ASR word.
    pub from: String,
    /// New words ("" for a removal; unused for `same`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    /// Only case and/or punctuation differ.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub minor: bool,
    /// A later stage rewrote this change's `to`. The span stays so the
    /// earlier stage keeps its credit, but its `to` is not in the final
    /// text: the next span starts from it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub superseded: bool,
}

/// Working segment. For `Same`, `to == from` (one raw token).
#[derive(Debug, Clone)]
struct Seg {
    kind: SpanKind,
    from: Vec<String>,
    to: Vec<String>,
    /// See [`TextSpan::superseded`].
    superseded: bool,
}

impl Seg {
    fn change(kind: SpanKind, to: Vec<String>) -> Self {
        Self {
            kind,
            from: vec![],
            to,
            superseded: false,
        }
    }

    /// The tokens this segment puts in the current text.
    fn visible(&self) -> &[String] {
        if self.superseded {
            &[]
        } else {
            &self.to
        }
    }
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
/// line up, the raw text is over [`MAX_DIFF_TOKENS`], or the spans do not
/// spell `final_text` (whitespace aside): a wrong diff is never stored.
pub fn compose(raw: &str, stages: &[StageText], final_text: &str) -> Option<Vec<TextSpan>> {
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
            superseded: false,
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
    let spans = finish(segs);
    (visible_text(&spans) == tokenize(final_text).join(" ")).then_some(spans)
}

/// Pushes the cluster part built so far, if any, and starts a new one.
fn flush_part(out: &mut Vec<Seg>, part: &mut Seg) {
    let kind = part.kind;
    let mut done = std::mem::replace(part, Seg::change(kind, vec![]));
    if done.from.is_empty() && done.to.is_empty() {
        return;
    }
    if done.from == done.to {
        done.kind = SpanKind::Same;
    }
    out.push(done);
}

/// Applies one stage's word diff to the segments.
fn apply_stage(segs: &[Seg], kind: SpanKind, before: &str, after: &str) -> Option<Vec<Seg>> {
    // Visible token index -> (segment, position in its visible tokens).
    let vis: Vec<(usize, usize)> = segs
        .iter()
        .enumerate()
        .flat_map(|(i, s)| (0..s.visible().len()).map(move |p| (i, p)))
        .collect();
    let visible: Vec<&str> = vis
        .iter()
        .map(|&(i, p)| segs[i].visible()[p].as_str())
        .collect();
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
    // Visible index range of each segment.
    let mut seg_vis = Vec::with_capacity(segs.len());
    let mut at = 0;
    for s in segs {
        seg_vis.push((at, at + s.visible().len()));
        at += s.visible().len();
    }
    // Segment ranges [lo, hi] that a change touches: every replaced or
    // removed token, plus an insertion strictly inside a multi-token
    // segment (that segment is split by it). Ranges that share a segment
    // fold into one cluster.
    let mut ranges: Vec<(usize, usize)> = groups
        .iter()
        .filter(|g| g.len > 0)
        .map(|g| (vis[g.start].0, vis[g.start + g.len - 1].0))
        .collect();
    for g in groups.iter().filter(|g| g.len == 0) {
        if let Some(i) = seg_vis
            .iter()
            .position(|&(a, b)| a < g.start && g.start < b)
        {
            ranges.push((i, i));
        }
    }
    ranges.sort_unstable();
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    for (lo, hi) in ranges {
        match clusters.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => clusters.push((lo, hi)),
        }
    }

    let inserts_at = |k: usize| groups.iter().filter(move |g| g.len == 0 && g.start == k);

    let mut out = Vec::with_capacity(segs.len() + groups.len());
    for g in inserts_at(0) {
        out.push(Seg::change(kind, g.new.clone()));
    }
    let mut i = 0;
    while i < segs.len() {
        let Some(&(lo, hi)) = clusters.iter().find(|c| c.0 == i) else {
            out.push(segs[i].clone());
            let (_, end) = seg_vis[i];
            if !segs[i].visible().is_empty() {
                for g in inserts_at(end) {
                    out.push(Seg::change(kind, g.new.clone()));
                }
            }
            i += 1;
            continue;
        };
        let (cluster_start, _) = seg_vis[lo];
        let (_, cluster_end) = seg_vis[hi];
        let mut part = Seg::change(kind, vec![]);
        for (j, seg) in segs.iter().enumerate().take(hi + 1).skip(lo) {
            if seg.visible().is_empty() {
                // A removal (or an earlier change already rewritten) keeps
                // its own attribution and its place among the raw words, so
                // the part is split around it.
                flush_part(&mut out, &mut part);
                out.push(seg.clone());
                continue;
            }
            if seg.kind != SpanKind::Same {
                // An earlier stage changed these words. Its span stays, and
                // this stage is credited only with what it did to the text
                // it actually received.
                flush_part(&mut out, &mut part);
                out.push(Seg {
                    superseded: true,
                    ..seg.clone()
                });
            }
            part.from.extend(seg.visible().iter().cloned());
            let (a, _) = seg_vis[j];
            for (k, token) in seg.visible().iter().enumerate() {
                let k = a + k;
                if k > cluster_start {
                    for g in inserts_at(k) {
                        part.to.extend(g.new.iter().cloned());
                    }
                }
                match owner_group[k] {
                    // A change's new words land after the last word it
                    // replaces, so a split part never shows them early.
                    Some(g) if groups[g].start + groups[g].len - 1 == k => {
                        part.to.extend(groups[g].new.iter().cloned())
                    }
                    Some(_) => {}
                    None => part.to.push(token.clone()),
                }
            }
        }
        flush_part(&mut out, &mut part);
        for g in inserts_at(cluster_end) {
            out.push(Seg::change(kind, g.new.clone()));
        }
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
            Some(last) if last.kind == seg.kind && last.superseded == seg.superseded => {
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
                    superseded: false,
                };
            }
            let to = s.to.join(" ");
            TextSpan {
                kind: s.kind,
                minor: is_minor(&from, &to),
                from,
                to,
                superseded: s.superseded,
            }
        })
        .collect()
}

/// The final text the spans spell out, words joined by single spaces.
pub fn visible_text(spans: &[TextSpan]) -> String {
    spans
        .iter()
        .filter(|s| !s.superseded)
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
            superseded: false,
        }
    }

    fn span(kind: SpanKind, from: &str, to: &str) -> TextSpan {
        TextSpan {
            kind,
            from: from.into(),
            to: to.into(),
            minor: false,
            superseded: false,
        }
    }

    fn superseded(kind: SpanKind, from: &str, to: &str) -> TextSpan {
        TextSpan {
            superseded: true,
            ..span(kind, from, to)
        }
    }

    /// Composes against the last stage's output (or the raw text).
    fn compose_chain(raw: &str, stages: &[StageText]) -> Option<Vec<TextSpan>> {
        let last = stages.last().map_or(raw, |s| s.after.as_str());
        compose(raw, stages, last)
    }

    #[test]
    fn filler_removal_matches_the_reference_card() {
        let raw = "hey sam um quick update the";
        let spans = compose_chain(
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
        let spans = compose_chain("need option two.", &[]).unwrap();
        assert_eq!(spans, vec![same("need option two.")]);
    }

    #[test]
    fn a_later_rewrite_keeps_the_earlier_stage_credited() {
        let raw = "call jon tomorrow";
        let spans = compose_chain(
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
        // The dictionary fix stays on the card, and polish is credited with
        // what it actually received ("John"), not the raw ASR word.
        assert_eq!(
            spans,
            vec![
                same("call"),
                superseded(SpanKind::Dictionary, "jon", "John"),
                span(SpanKind::Polish, "John", "Jonathan"),
                same("tomorrow"),
            ]
        );
        assert_eq!(visible_text(&spans), "call Jonathan tomorrow");
    }

    #[test]
    fn case_only_polish_change_is_minor() {
        let raw = "hello there";
        let spans = compose_chain(raw, &[stage(EditKind::Polish, raw, "Hello there.")]).unwrap();
        assert_eq!(spans.len(), 1);
        assert!(spans[0].minor, "{spans:?}");
        assert_eq!(spans[0].kind, SpanKind::Polish);
        assert_eq!(spans[0].to, "Hello there.");
    }

    #[test]
    fn broken_chain_is_none() {
        let raw = "a b c";
        assert!(compose_chain(raw, &[stage(EditKind::Polish, "a b x", "a b y")]).is_none());
        assert!(compose_chain(
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
        assert!(compose_chain(&raw, &[]).is_none());
        let ok = vec!["w"; MAX_DIFF_TOKENS].join(" ");
        assert!(compose_chain(&ok, &[]).is_some());
    }

    #[test]
    fn insertion_only_stage_has_empty_from() {
        let raw = "see you";
        let spans = compose_chain(raw, &[stage(EditKind::Polish, raw, "see you soon")]).unwrap();
        assert_eq!(
            spans,
            vec![same("see you"), span(SpanKind::Polish, "", "soon")]
        );
        let front = compose_chain(raw, &[stage(EditKind::Format, raw, "ok see you")]).unwrap();
        assert_eq!(front[0], span(SpanKind::Format, "", "ok"));
        assert_eq!(visible_text(&front), "ok see you");
    }

    #[test]
    fn filler_keeps_its_attribution_and_its_place_through_a_later_polish() {
        let raw = "so um tuesday works";
        let stages = [
            stage(EditKind::Filler, raw, "so tuesday works"),
            stage(EditKind::Polish, "so tuesday works", "Thursday works."),
        ];
        let spans = compose_chain(raw, &stages).unwrap();
        // The struck "um" sits between "so" and "tuesday", where it was
        // spoken, not after the sentence polish rewrote. Polish's new words
        // land after the last word they replace.
        assert_eq!(
            spans,
            vec![
                span(SpanKind::Polish, "so", ""),
                span(SpanKind::Filler, "um", ""),
                span(SpanKind::Polish, "tuesday works", "Thursday works."),
            ]
        );
        assert_eq!(visible_text(&spans), "Thursday works.");
    }

    #[test]
    fn an_insertion_inside_an_earlier_change_is_kept() {
        let raw = "march fifth twenty twenty six";
        let stages = [
            stage(EditKind::Format, raw, "March 5 2026"),
            stage(EditKind::Polish, "March 5 2026", "March 5 in 2026"),
        ];
        let spans = compose_chain(raw, &stages).unwrap();
        assert_eq!(
            spans,
            vec![
                superseded(SpanKind::Format, raw, "March 5 2026"),
                span(SpanKind::Polish, "March 5 2026", "March 5 in 2026"),
            ]
        );
        assert_eq!(visible_text(&spans), "March 5 in 2026");
    }

    #[test]
    fn spans_that_do_not_spell_the_final_text_are_none() {
        let raw = "see you";
        let stages = [stage(EditKind::Polish, raw, "see you soon")];
        assert!(compose(raw, &stages, "see you soon").is_some());
        // Whitespace aside: a newline in the pasted text still matches.
        assert!(compose(raw, &stages, "see\nyou  soon").is_some());
        // A pass after the recorded stages changed the text.
        assert!(compose(raw, &stages, "see you soon!").is_none());
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
