//! §7.4 — promote judged edits into the learning store.
//!
//! Policy layer between the judge (§7.3) and the store (§7.1): a
//! `Mishearing` judgement becomes a lexicon row (immediately when the
//! correction is proper-noun-like and phonetically close, otherwise after
//! two observations); a `Style` judgement becomes a style rule once the
//! evidence is sufficient and consistent; `undo` tombstones permanently.
//!
//! All functions here re-check the judge's verdict with the caller's exact
//! `JudgeContext` before touching the store, so a stale or foreign
//! judgement can never write a row.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::judge::{classify, judge_replace, phon_sim, JudgeContext, Judgement};
use super::learn_memory;
use super::store::{ExampleRow, LearningStore, LexiconRow, StoreResult, StyleRuleRow};

/// Phonetic similarity at or above which a proper-noun-like mishearing
/// promotes on the first observation.
pub const IMMEDIATE_PHON_SIM: f32 = 0.75;
/// Phonetic similarity at or above which a proper-noun-like mishearing
/// promotes on the first observation when the corrected term already
/// exists in the lexicon as a contacts or manual (dictionary) term: the
/// user is correcting a known name, so one correction is enough even when
/// the misheard form is phonetically farther away.
pub const KNOWN_TERM_PHON_SIM: f32 = 0.5;
/// Observations required to promote a non-immediate mishearing.
pub const OBSERVATIONS_TO_PROMOTE: i64 = 2;
/// Minimum total style observations before a rule can be applied.
pub const STYLE_MIN_EVIDENCE: i64 = 3;
/// Minimum agree/total ratio before a style rule is applied.
pub const STYLE_MIN_AGREEMENT: f64 = 0.70;
/// Maximum examples kept per category (older ones are pruned).
pub const EXAMPLE_CAP_PER_CATEGORY: i64 = 200;

/// Outcome of a mishearing promotion attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum Promotion {
    /// The term is now in the lexicon with the alias merged in.
    Promoted {
        term: String,
        alias: String,
        /// True when promoted on this observation without waiting for
        /// the observation count (proper-noun-like + `sim >= 0.75`).
        immediate: bool,
    },
    /// Not enough observations yet.
    Pending { observations: i64, needed: i64 },
    /// The span is not learnable; nothing was written.
    Refused { reason: RefusalReason },
}

/// Why a mishearing was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    /// The defensive re-judge with the caller's context did not return a
    /// `Mishearing` verdict (or the term is in the skip lists).
    JudgeDisagrees,
    /// The term was undone (tombstoned) and is never re-learned.
    Tombstoned,
}

/// Outcome of a style observation.
#[derive(Debug, Clone, PartialEq)]
pub enum StylePromotion {
    /// The style rule was (re)written to the store.
    Applied {
        category: String,
        rule: String,
        value: String,
        /// Positive observations (agree count) backing the rule.
        evidence: i64,
        /// Total observations for this (category, rule).
        total: i64,
    },
    /// Not enough evidence or agreement yet.
    Pending { evidence: i64, total: i64 },
}

/// Notify payload for the "learned a new term" toast (P7.11 wiring).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedTerm {
    pub term: String,
    pub aliases: Vec<String>,
    pub learned_at: u64,
}

/// `term` looks like a proper noun: contains a digit, is capitalized, or
/// is CamelCase. Mirrors the judge's local heuristic (kept local because
/// the judge's copy is private; §7.4 requires a re-checkable, testable
/// copy for the immediate-promotion gate).
pub(crate) fn proper_noun_like(term: &str) -> bool {
    if term.is_empty() {
        return false;
    }
    if term.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if term.chars().next().is_some_and(|c| c.is_uppercase()) && term.len() > 1 {
        return true;
    }
    // CamelCase: an interior uppercase.
    term[1..].chars().any(|c| c.is_uppercase())
}

/// Considers one judged mishearing and, per the §7.4 policy, promotes the
/// correction into the lexicon.
///
/// `alias` is what the ASR wrote; `term` is what the user corrected to.
/// Policy:
/// 1. A tombstoned `term` is never re-learned → `Refused { Tombstoned }`.
/// 2. Defensive re-judge with the caller's `ctx`: anything other than
///    `Mishearing` → `Refused { JudgeDisagrees }`, no candidate written.
/// 3. `sim >= IMMEDIATE_PHON_SIM` and `term` proper-noun-like → promote
///    immediately. Otherwise bump the candidate counter and promote once
///    it reaches `OBSERVATIONS_TO_PROMOTE`.
///
/// Promotion upserts the lexicon row: a new row gets `source = "learned"`;
/// an existing row keeps its `source`/`weight` and only gains the alias
/// (deduped, stable order). The candidate row is cleared on promotion.
pub fn consider_mishearing(
    store: &LearningStore,
    alias: &str,
    term: &str,
    ctx: &JudgeContext,
    now_ms: u64,
) -> StoreResult<Promotion> {
    if let Some(row) = store.lexicon_by_term(term)? {
        if row.tombstoned {
            return Ok(Promotion::Refused {
                reason: RefusalReason::Tombstoned,
            });
        }
    }

    // Learn memory: the user said "no" to this (alias → term) pair twice →
    // blocked permanently; never re-offer it. Reported as `Tombstoned`
    // (never re-learned) rather than adding a new variant for a case the
    // judge itself would also refuse on the second observation.
    if learn_memory::is_blocked(store, alias, term, now_ms)? {
        return Ok(Promotion::Refused {
            reason: RefusalReason::Tombstoned,
        });
    }

    // Defensive re-judge: the caller judged with this ctx; we must agree.
    if !matches!(
        judge_replace(alias, term, ctx),
        Judgement::Mishearing { .. }
    ) {
        return Ok(Promotion::Refused {
            reason: RefusalReason::JudgeDisagrees,
        });
    }

    let sim = phon_sim(alias, term);
    let immediate = proper_noun_like(term)
        && (sim >= IMMEDIATE_PHON_SIM
            || (sim >= KNOWN_TERM_PHON_SIM && is_known_term(store, term)?));

    let already_promoted = store.lexicon_by_term(term)?.is_some();
    let (count, _total) = if immediate {
        // Immediate path: no candidate row is ever created. A
        // re-observation of an already-promoted term reports the
        // threshold as reached; the promotion below is then a no-op
        // upsert with the alias deduped.
        (OBSERVATIONS_TO_PROMOTE, 0)
    } else if already_promoted {
        // Non-immediate re-observation of an already-promoted term:
        // the candidate row was cleared at promotion time, so do not
        // bump (that would reset the count); report the threshold.
        (OBSERVATIONS_TO_PROMOTE, 0)
    } else {
        store.bump_learn_candidate("mishearing", alias, term, "", true, now_ms)?
    };

    if count < OBSERVATIONS_TO_PROMOTE {
        return Ok(Promotion::Pending {
            observations: count,
            needed: OBSERVATIONS_TO_PROMOTE,
        });
    }

    // Promote: upsert the lexicon row, merging the alias.
    let existing = store.lexicon_by_term(term)?;
    let row = match existing {
        Some(mut e) => {
            if !e.aliases.iter().any(|a| a == alias) {
                e.aliases.push(alias.to_string());
            }
            // Remember where this learned term came from so an undo of the
            // term can be recorded as a pair refusal (learn memory).
            if e.source == "learned" && e.learned_from.is_none() {
                e.learned_from = Some(alias.to_string());
            }
            e
        }
        None => LexiconRow {
            term: term.to_string(),
            aliases: vec![alias.to_string()],
            source: "learned".into(),
            weight: 1.0,
            hits: 0,
            false_alarms: 0,
            created: now_ms,
            last_used: Some(now_ms),
            tombstoned: false,
            app_scope: None,
            learned_from: Some(alias.to_string()),
        },
    };
    store.upsert_lexicon(&row)?;
    // Zero the promotion counters, keep the settle tallies: the settle sweep
    // writes its survival evidence to this same row.
    store.reset_learn_candidate_counters("mishearing", alias, term, "")?;

    let final_row = store
        .lexicon_by_term(term)?
        .ok_or_else(|| store_error_term_missing(term))?;
    let _ = LearnedTerm {
        term: final_row.term.clone(),
        aliases: final_row.aliases.clone(),
        learned_at: final_row.created,
    };
    Ok(Promotion::Promoted {
        term: term.to_string(),
        alias: alias.to_string(),
        immediate,
    })
}

/// What one observed edit taught the learning store.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObservedEditLearning {
    /// One outcome per distinct mishearing span in the edit.
    pub promotions: Vec<Promotion>,
    /// Terms this edit added to the lexicon. Undo tombstones exactly these;
    /// a term that already existed (manual, contacts, an earlier take) is
    /// never listed, so undo cannot delete vocabulary the edit did not learn.
    pub created_terms: Vec<String>,
}

/// Feeds one observed edit (`pasted` → `observed`) through the §7.3 judge
/// and §7.4 promotion policy.
///
/// `classify` diffs the two texts and judges each replacement span; every
/// `Mishearing` becomes a `consider_mishearing` call, so style edits,
/// rewrites, structural spans, and synonym swaps never reach the lexicon.
/// Duplicate spans are promoted once. The caller owns the learning gates
/// (`learn_from_edits`, `learn_terminology`) and decides what to do with a
/// `Pending` outcome.
pub fn consider_observed_edit(
    store: &LearningStore,
    pasted: &str,
    observed: &str,
    ctx: &JudgeContext,
    now_ms: u64,
) -> StoreResult<ObservedEditLearning> {
    let mut out = ObservedEditLearning::default();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for judgement in classify(pasted, observed, ctx) {
        let Judgement::Mishearing { term, alias } = judgement else {
            continue;
        };
        if !seen.insert((alias.to_lowercase(), term.to_lowercase())) {
            continue;
        }
        let existed = store.lexicon_by_term(&term)?.is_some();
        let promotion = consider_mishearing(store, &alias, &term, ctx, now_ms)?;
        if matches!(promotion, Promotion::Promoted { .. })
            && !existed
            && !out.created_terms.contains(&term)
        {
            out.created_terms.push(term.clone());
        }
        out.promotions.push(promotion);
    }
    Ok(out)
}

/// Considers one style observation for `(category, rule, value)`.
///
/// `agrees` is whether this observation supports the proposed value.
/// When `total >= STYLE_MIN_EVIDENCE` and `agree / total >=
/// STYLE_MIN_AGREEMENT`, the style rule is (re)written with `evidence =
/// agree` and `updated = now_ms` → `Applied`. Else `Pending`.
pub fn consider_style(
    store: &LearningStore,
    category: &str,
    rule: &str,
    value: &str,
    agrees: bool,
    now_ms: u64,
) -> StoreResult<StylePromotion> {
    let (agree, total) =
        store.bump_learn_candidate("style", category, rule, value, agrees, now_ms)?;
    if total >= STYLE_MIN_EVIDENCE && agree as f64 / total as f64 >= STYLE_MIN_AGREEMENT {
        store.upsert_style_rule(&StyleRuleRow {
            app_category: category.to_string(),
            rule: rule.to_string(),
            value: value.to_string(),
            evidence: agree,
            updated: now_ms,
        })?;
        Ok(StylePromotion::Applied {
            category: category.to_string(),
            rule: rule.to_string(),
            value: value.to_string(),
            evidence: agree,
            total,
        })
    } else {
        Ok(StylePromotion::Pending {
            evidence: agree,
            total,
        })
    }
}

/// Records one accepted example for a category, then prunes the category
/// back to `EXAMPLE_CAP_PER_CATEGORY` most-recent rows. Returns the new
/// example's id.
pub fn record_example(
    store: &LearningStore,
    category: &str,
    raw: &str,
    final_text: &str,
    accepted_unedited: bool,
    now_ms: u64,
) -> StoreResult<i64> {
    let id = store.save_example(&ExampleRow {
        id: 0,
        app_category: category.to_string(),
        raw: raw.to_string(),
        final_text: final_text.to_string(),
        accepted_unedited,
        ts: now_ms,
    })?;
    store.prune_examples(EXAMPLE_CAP_PER_CATEGORY)?;
    Ok(id)
}

/// Undoes a learned term: tombstones the lexicon row permanently and
/// clears every candidate row for that term. A tombstoned term is never
/// re-learned by `consider_mishearing`. For learned rows, every alias is
/// recorded as a pair refusal in the learn memory (two refusals on the same
/// pair block re-promotion permanently).
pub fn undo(store: &LearningStore, term: &str) -> StoreResult<()> {
    // Candidate rows are keyed by `(alias, term)`, so clearing "every candidate
    // row for that term" means the term's alias list, which only the lexicon row
    // knows. Reading it before tombstoning is deliberate: it is the last moment
    // the alias set is guaranteed to be the one the candidates were written under.
    let aliases = match store.lexicon_by_term(term)? {
        Some(row) => {
            let learned = row.source == "learned";
            (row.aliases, learned)
        }
        None => (Vec::new(), false),
    };
    store.tombstone(term)?;
    for alias in &aliases.0 {
        // The pair is retired: drop the promotion counters and the apply
        // flag, but keep accepted/rejected as survival history.
        store.retire_learn_candidate("mishearing", alias, term, "")?;
        if aliases.1 {
            // Pair refusal (learned rows only): two "no's" block re-promotion.
            // Propagate a persistence failure: a refusal that does not reach
            // the store means the user's "no" was not durably recorded, which
            // the caller needs to know.
            learn_memory::record_refusal(store, alias, term, now_ms())?;
        }
    }
    Ok(())
}

/// Current wall clock for learn-memory timestamps (kept as a function so
/// tests stay deterministic via the `now_ms` parameter on every public API).
fn now_ms() -> u64 {
    crate::storage::now_ms()
}

/// `term` exactly matches a live lexicon row whose source is a
/// user-imported vocabulary source (contacts or manual dictionary):
/// the user is correcting a name they already know.
fn is_known_term(store: &LearningStore, term: &str) -> StoreResult<bool> {
    Ok(store
        .lexicon_by_term(term)?
        .is_some_and(|r| !r.tombstoned && (r.source == "contacts" || r.source == "manual")))
}

fn store_error_term_missing(term: &str) -> super::store::StoreError {
    use std::io::Error;
    super::store::StoreError::Io(Error::new(
        std::io::ErrorKind::NotFound,
        format!("lexicon row for {term:?} vanished during promotion"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::store::LearningStore;

    fn tmp_store() -> (tempfile::TempDir, LearningStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open(&dir.path().join("learning.sqlite")).unwrap();
        (dir, store)
    }

    fn ctx() -> JudgeContext {
        JudgeContext::default()
    }

    #[test]
    fn proper_noun_like_table() {
        assert!(proper_noun_like("PyTorch"));
        assert!(proper_noun_like("Siobhan"));
        assert!(proper_noun_like("Kafka9"));
        assert!(proper_noun_like("A1"));
        assert!(!proper_noun_like("the"));
        assert!(!proper_noun_like("kubernetes"));
        assert!(!proper_noun_like(""));
        assert!(!proper_noun_like("a"));
    }

    #[test]
    fn contact_name_promotes_on_first_observation_when_phonetically_far() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        // "Siobhan" is in the user's contacts; "Sevan" is a phonetically
        // far mishearing (sim 0.67 < 0.75) that would otherwise need two
        // observations.
        store
            .upsert_lexicon(&LexiconRow {
                term: "Siobhan".into(),
                aliases: vec![],
                source: "contacts".into(),
                weight: 0.7,
                hits: 0,
                false_alarms: 0,
                created: 500,
                last_used: Some(500),
                tombstoned: false,
                app_scope: None,
                learned_from: None,
            })
            .unwrap();

        let p = consider_mishearing(&store, "Sevan", "Siobhan", &ctx, 1000).unwrap();
        assert_eq!(
            p,
            Promotion::Promoted {
                term: "Siobhan".into(),
                alias: "Sevan".into(),
                immediate: true
            }
        );
        let row = store.lexicon_by_term("Siobhan").unwrap().unwrap();
        assert_eq!(row.source, "contacts", "existing row keeps its source");
        assert_eq!(row.aliases, vec!["Sevan".to_string()]);
    }

    #[test]
    fn unknown_name_still_needs_two_observations_when_phonetically_far() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        // Same pair as the contacts test, but "Siobhan" is not in the
        // lexicon: unchanged behavior, two observations required.
        let p1 = consider_mishearing(&store, "Sevan", "Siobhan", &ctx, 1000).unwrap();
        assert_eq!(
            p1,
            Promotion::Pending {
                observations: 1,
                needed: 2
            }
        );
        assert!(store.lexicon_by_term("Siobhan").unwrap().is_none());

        let p2 = consider_mishearing(&store, "Sevan", "Siobhan", &ctx, 2000).unwrap();
        assert_eq!(
            p2,
            Promotion::Promoted {
                term: "Siobhan".into(),
                alias: "Sevan".into(),
                immediate: false
            }
        );
    }

    #[test]
    fn common_word_swap_never_promotes_even_if_in_lexicon() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        // "a" is in the lexicon (manual/dictionary); the swap "the" -> "a"
        // is a common-word pair the judge refuses, and the known-term path
        // must not rescue it.
        store
            .upsert_lexicon(&LexiconRow {
                term: "a".into(),
                aliases: vec![],
                source: "manual".into(),
                weight: 1.0,
                hits: 0,
                false_alarms: 0,
                created: 100,
                last_used: None,
                tombstoned: false,
                app_scope: None,
                learned_from: None,
            })
            .unwrap();

        let p = consider_mishearing(&store, "the", "a", &ctx, 1000).unwrap();
        assert_eq!(
            p,
            Promotion::Refused {
                reason: RefusalReason::JudgeDisagrees
            }
        );
        assert!(store
            .learn_candidate_get("mishearing", "the", "a", "")
            .unwrap()
            .is_none());
    }

    #[test]
    fn refused_pair_never_promotes_even_if_contact() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        // "Siobhan" is a contact, but the user refused the pair twice: the
        // refusal wins over the known-term promotion path.
        store
            .upsert_lexicon(&LexiconRow {
                term: "Siobhan".into(),
                aliases: vec![],
                source: "contacts".into(),
                weight: 0.7,
                hits: 0,
                false_alarms: 0,
                created: 100,
                last_used: Some(100),
                tombstoned: false,
                app_scope: None,
                learned_from: None,
            })
            .unwrap();
        learn_memory::record_refusal(&store, "Sevan", "Siobhan", 200).unwrap();
        learn_memory::record_refusal(&store, "Sevan", "Siobhan", 300).unwrap();

        let p = consider_mishearing(&store, "Sevan", "Siobhan", &ctx, 1000).unwrap();
        assert_eq!(
            p,
            Promotion::Refused {
                reason: RefusalReason::Tombstoned
            }
        );
        assert!(store
            .learn_candidate_get("mishearing", "Sevan", "Siobhan", "")
            .unwrap()
            .is_none());
    }

    #[test]
    fn two_observations_then_promoted() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        // "zeph" -> "Zephyrine": sim ~0.69 < 0.75 → not immediate.
        let p1 = consider_mishearing(&store, "zeph", "Zephyrine", &ctx, 1000).unwrap();
        assert_eq!(
            p1,
            Promotion::Pending {
                observations: 1,
                needed: 2
            }
        );
        assert!(store.lexicon_by_term("Zephyrine").unwrap().is_none());

        let p2 = consider_mishearing(&store, "zeph", "Zephyrine", &ctx, 2000).unwrap();
        assert_eq!(
            p2,
            Promotion::Promoted {
                term: "Zephyrine".into(),
                alias: "zeph".into(),
                immediate: false
            }
        );
        let row = store.lexicon_by_term("Zephyrine").unwrap().unwrap();
        assert_eq!(row.source, "learned");
        assert_eq!(row.aliases, vec!["zeph".to_string()]);
        // Promotion zeroes the counters but keeps the row: the settle sweep
        // writes its survival tallies to it.
        assert_eq!(
            store
                .learn_candidate_get("mishearing", "zeph", "Zephyrine", "")
                .unwrap(),
            Some((0, 0))
        );

        // Third observation: still promoted (immediacy is per-observation).
        let p3 = consider_mishearing(&store, "zeph", "Zephyrine", &ctx, 3000).unwrap();
        assert!(
            matches!(p3, Promotion::Promoted { .. }),
            "expected Promoted on re-observation, got {p3:?}"
        );
        let row = store.lexicon_by_term("Zephyrine").unwrap().unwrap();
        assert_eq!(row.aliases, vec!["zeph".to_string()]);
    }

    #[test]
    fn immediate_promotion_on_first_call() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        let p = consider_mishearing(&store, "pie torch", "PyTorch", &ctx, 1000).unwrap();
        assert_eq!(
            p,
            Promotion::Promoted {
                term: "PyTorch".into(),
                alias: "pie torch".into(),
                immediate: true
            }
        );
        let row = store.lexicon_by_term("PyTorch").unwrap().unwrap();
        assert_eq!(row.source, "learned");
        assert_eq!(row.aliases, vec!["pie torch".to_string()]);
        // No candidate row was ever created on the immediate path.
        assert!(store
            .learn_candidate_get("mishearing", "pie torch", "PyTorch", "")
            .unwrap()
            .is_none());
    }

    #[test]
    fn judge_disagrees_creates_nothing() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        // "the" -> "a" judges as Skip (everyday words) → refused, and no
        // candidate or lexicon row may exist afterwards.
        let p = consider_mishearing(&store, "the", "a", &ctx, 1000).unwrap();
        assert_eq!(
            p,
            Promotion::Refused {
                reason: RefusalReason::JudgeDisagrees
            }
        );
        assert!(store.lexicon_by_term("a").unwrap().is_none());
        assert!(store.lexicon_all().unwrap().is_empty());
        assert!(store
            .learn_candidate_get("mishearing", "the", "a", "")
            .unwrap()
            .is_none());
    }

    #[test]
    fn tombstone_is_permanent() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        // Learn it, then undo it.
        consider_mishearing(&store, "pie torch", "PyTorch", &ctx, 1000).unwrap();
        undo(&store, "PyTorch").unwrap();
        let row = store.lexicon_by_term("PyTorch").unwrap().unwrap();
        assert!(row.tombstoned);

        // Five more observations: always refused, still tombstoned.
        for i in 0..5 {
            let p = consider_mishearing(&store, "pie torch", "PyTorch", &ctx, 2000 + i).unwrap();
            assert_eq!(
                p,
                Promotion::Refused {
                    reason: RefusalReason::Tombstoned
                }
            );
        }
        let row = store.lexicon_by_term("PyTorch").unwrap().unwrap();
        assert!(row.tombstoned);
        // No candidate rows leaked either.
        assert!(store
            .learn_candidate_get("mishearing", "pie torch", "PyTorch", "")
            .unwrap()
            .is_none());
    }

    /// Candidates are keyed by alias, so an undo that only clears the alias it
    /// happened to be handed leaves the rest behind. `undo` reads the term's
    /// alias list for exactly this reason.
    #[test]
    fn undo_clears_a_candidate_written_under_a_stale_observation() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        consider_mishearing(&store, "pie torch", "PyTorch", &ctx, 1000).unwrap();
        // Promotion cleared its own candidate; a later observation of the same
        // mishearing leaves a fresh one behind.
        store
            .bump_learn_candidate("mishearing", "pie torch", "PyTorch", "", true, 1100)
            .unwrap();
        assert!(store
            .learn_candidate_get("mishearing", "pie torch", "PyTorch", "")
            .unwrap()
            .is_some());

        undo(&store, "PyTorch").unwrap();

        // Undo zeroes the counters instead of deleting the row, so the settle
        // evidence on it survives. The apply flag must be gone.
        assert_eq!(
            store
                .learn_candidate_get("mishearing", "pie torch", "PyTorch", "")
                .unwrap(),
            Some((0, 0))
        );
        let row = store
            .candidate_get_full("mishearing", "pie torch", "PyTorch", "")
            .unwrap()
            .expect("row kept for its settle tallies");
        assert!(!row.auto_apply, "an undone term must not stay auto-applied");
    }

    #[test]
    fn alias_merge_keeps_manual_source_and_weight() {
        let (_dir, store) = tmp_store();
        let ctx = ctx();
        let manual = LexiconRow {
            term: "Anthropic".into(),
            aliases: vec![],
            source: "manual".into(),
            weight: 2.5,
            hits: 4,
            false_alarms: 0,
            created: 100,
            last_used: None,
            tombstoned: false,
            app_scope: None,
            learned_from: None,
        };
        store.upsert_lexicon(&manual).unwrap();

        let p = consider_mishearing(&store, "anthropik", "Anthropic", &ctx, 5000).unwrap();
        assert!(matches!(
            p,
            Promotion::Promoted {
                immediate: true,
                ..
            }
        ));
        let row = store.lexicon_by_term("Anthropic").unwrap().unwrap();
        assert_eq!(row.source, "manual", "existing row keeps its source");
        assert_eq!(row.weight, 2.5, "existing row keeps its weight");
        assert_eq!(row.hits, 4);
        assert_eq!(row.aliases, vec!["anthropik".to_string()]);
    }

    #[test]
    fn style_apply_boundary() {
        let (_dir, store) = tmp_store();
        // 2 agrees then 1 disagree: total 3, agree 2 → 0.667 < 0.70 → Pending.
        consider_style(&store, "chat", "salutation", "hey", true, 100).unwrap();
        consider_style(&store, "chat", "salutation", "hey", true, 200).unwrap();
        let p = consider_style(&store, "chat", "salutation", "hey", false, 300).unwrap();
        assert_eq!(
            p,
            StylePromotion::Pending {
                evidence: 2,
                total: 3
            }
        );
        assert!(store.style_rule("chat", "salutation").unwrap().is_none());

        // Next agree: total 4, agree 3 → 0.75 ≥ 0.70 → Applied.
        let p = consider_style(&store, "chat", "salutation", "hey", true, 400).unwrap();
        assert_eq!(
            p,
            StylePromotion::Applied {
                category: "chat".into(),
                rule: "salutation".into(),
                value: "hey".into(),
                evidence: 3,
                total: 4
            }
        );
        let r = store.style_rule("chat", "salutation").unwrap().unwrap();
        assert_eq!(r.evidence, 3);
        assert_eq!(r.updated, 400);

        // 3/3 straight agrees also applies with evidence = 3.
        let _ = consider_style(&store, "email", "signoff", "thanks", true, 500).unwrap();
        let _ = consider_style(&store, "email", "signoff", "thanks", true, 600).unwrap();
        let p = consider_style(&store, "email", "signoff", "thanks", true, 700).unwrap();
        assert_eq!(
            p,
            StylePromotion::Applied {
                category: "email".into(),
                rule: "signoff".into(),
                value: "thanks".into(),
                evidence: 3,
                total: 3
            }
        );
    }

    #[test]
    fn observed_edit_promotes_the_mishearing_it_contains() {
        let (_dir, store) = tmp_store();
        let out = consider_observed_edit(
            &store,
            "I use pie torch daily",
            "I use PyTorch daily",
            &JudgeContext::default(),
            1000,
        )
        .unwrap();
        assert_eq!(out.promotions.len(), 1, "{:?}", out.promotions);
        assert!(
            matches!(
                &out.promotions[0],
                Promotion::Promoted {
                    immediate: true,
                    ..
                }
            ),
            "{:?}",
            out.promotions
        );
        assert_eq!(out.created_terms, vec!["PyTorch".to_string()]);
        let row = store.lexicon_by_term("PyTorch").unwrap().unwrap();
        assert_eq!(row.source, "learned");
        assert_eq!(row.aliases, vec!["pie torch".to_string()]);
    }

    #[test]
    fn observed_edit_learns_nothing_from_a_synonym_rewording() {
        let (_dir, store) = tmp_store();
        let out = consider_observed_edit(
            &store,
            "We value our clients.",
            "We value our customers.",
            &JudgeContext::default(),
            1000,
        )
        .unwrap();
        assert!(out.promotions.is_empty(), "{:?}", out.promotions);
        assert!(out.created_terms.is_empty());
        assert!(store.lexicon_all().unwrap().is_empty());
    }

    #[test]
    fn observed_edit_does_not_claim_a_preexisting_term_for_undo() {
        let (_dir, store) = tmp_store();
        let manual = LexiconRow {
            term: "PyTorch".into(),
            aliases: vec![],
            source: "manual".into(),
            weight: 2.0,
            hits: 0,
            false_alarms: 0,
            created: 1,
            last_used: None,
            tombstoned: false,
            app_scope: None,
            learned_from: None,
        };
        store.upsert_lexicon(&manual).unwrap();

        let out = consider_observed_edit(
            &store,
            "I use pie torch daily",
            "I use PyTorch daily",
            &JudgeContext::default(),
            1000,
        )
        .unwrap();
        assert!(matches!(
            out.promotions.as_slice(),
            [Promotion::Promoted { .. }]
        ));
        assert!(
            out.created_terms.is_empty(),
            "a term the user already had must never be tombstoned by undo"
        );
        let row = store.lexicon_by_term("PyTorch").unwrap().unwrap();
        assert_eq!(row.source, "manual", "existing row keeps its source");
        assert_eq!(row.aliases, vec!["pie torch".to_string()]);
    }

    #[test]
    fn observed_edit_with_no_change_learns_nothing() {
        let (_dir, store) = tmp_store();
        let out = consider_observed_edit(
            &store,
            "the same text",
            "the same text",
            &JudgeContext::default(),
            1000,
        )
        .unwrap();
        assert!(out.promotions.is_empty());
        assert!(out.created_terms.is_empty());
        assert!(store.lexicon_all().unwrap().is_empty());
    }

    #[test]
    fn record_example_prunes_to_cap() {
        let (_dir, store) = tmp_store();
        for i in 0..EXAMPLE_CAP_PER_CATEGORY + 5 {
            record_example(&store, "chat", "r", "f", true, 1000 + i as u64).unwrap();
        }
        assert_eq!(
            store.examples_for("chat", 1000).unwrap().len() as i64,
            EXAMPLE_CAP_PER_CATEGORY
        );
        // Other categories keep their own rows.
        record_example(&store, "email", "r", "f", false, 1).unwrap();
        assert_eq!(store.examples_for("email", 10).unwrap().len(), 1);
    }

    proptest::proptest! {
        #[test]
        fn prop_skip_judgements_create_nothing(
            a in proptest::string::string_regex("[a-zA-Z0-9]{0,12}").unwrap(),
            b in proptest::string::string_regex("[a-zA-Z0-9]{0,12}").unwrap(),
        ) {
            let (dir, store) = tmp_store();
            let ctx = JudgeContext::default();
            let j = judge_replace(&a, &b, &ctx);
            let p = consider_mishearing(&store, &a, &b, &ctx, 1).unwrap();
            if matches!(j, Judgement::Skip { .. }) {
                assert!(
                    matches!(p, Promotion::Refused { reason: RefusalReason::JudgeDisagrees }),
                    "skip {a:?} -> {b:?} must be refused, got {p:?}"
                );
                assert!(
                    store.lexicon_by_term(&b).unwrap().is_none(),
                    "skip {a:?} -> {b:?} must not create a lexicon row"
                );
                assert!(
                    store.learn_candidate_get("mishearing", &a, &b, "").unwrap().is_none(),
                    "skip {a:?} -> {b:?} must not create a candidate row"
                );
            }
            let _ = dir;
        }
    }
}
