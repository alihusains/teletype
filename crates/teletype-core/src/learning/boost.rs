//! Per-take boost list (Phase 7.5).
//!
//! Every take sends the engine a **boost list** — the user's vocabulary most
//! likely to help *this* take. The list is rebuilt per take as the top N rows
//! by score, with a cheap one-entry cache keyed by
//! `(lexicon_version, app_category, screen_terms_hash)` so a rebuild stays
//! far under 1 ms when nothing relevant changed.
//!
//! Score (plan §7.5 formula), then multiplied by the row's own `weight` so
//! replay's weight-halving and the 0.7 contacts weight apply:
//!
//! ```text
//! score = 2.0·[manual] + 1.5·[learned] + 1.0·[screen] + 0.7·[contacts]
//!       + 0.5·[pack]
//!       + 0.8·log1p(hits) − 2.0·false_alarm_rate
//!       + 0.5·[app_scope matches] + recency_decay(last_used, 30d)
//! ```
//!
//! `false_alarm_rate = false_alarms / max(1, hits + false_alarms)`.
//!
//! `recency_decay = 1.0 / (1.0 + age_days / 30.0)`: a term used today scores
//! 1.0, one used 30 days ago 0.5, 60 days ago 1/3. A half-life-ish decay on
//! a 30-day horizon keeps recently-used vocabulary ahead of stale entries
//! without ever reaching zero, so an old-but-relevant term is never
//! fully excluded by recency alone. `last_used = None` (never used) decays
//! to 0.0 — the term has no evidence of being spoken, so it contributes
//! nothing from recency.
//!
//! Screen terms (words observed on the current screen) enter the list at the
//! `screen` indicator with the base replaced by 0.5: score = 0.5 + recency
//! (1.0, since they are being used right now), no hits/false-alarm terms.
//! They are deduped against lexicon terms case-insensitively; the lexicon
//! row wins.
//!
//! Deterministic order: score descending, then term ascending. The result
//! must not depend on input row order.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use super::store::LexiconRow;

/// One custom-vocabulary term for native boosting.
///
/// This is [`crate::rescoring::RescoreTerm`] — the crate's single boost-term
/// shape, so the rescoring pass consumes boost lists directly.
/// `teletype-speech` is not a dependency of `teletype-core`; the
/// engine-facing wiring unit converts to `teletype_speech::BoostTerm`
/// (a zero-cost struct copy). Engines without native boosting ignore the
/// list; post-ASR rescoring covers them.
pub use crate::rescoring::RescoreTerm as BoostTerm;

/// Per-engine list sizes a native booster would use (plan §7.5 starting
/// values). Not applied today: `boost_capacity` in the desktop wiring returns
/// zero for every engine until the bake-off shows boosting helps, so these are
/// the sizes to restore, not the shipped defaults.
pub const N_FLUID: usize = 100;
pub const N_SHERPA: usize = 50;
pub const N_WHISPER: usize = 30;
pub const N_WHISTLE: usize = 50;

/// The take-local context a boost list is built for.
pub struct BoostContext<'a> {
    /// The focused app's category, if known (e.g. `"chat"`, `"email"`).
    pub app_category: Option<&'a str>,
    /// Terms observed on the current screen (OCR / accessibility text).
    pub screen_terms: &'a [String],
    /// Current time in epoch milliseconds.
    pub now_ms: i64,
}

// ── Scoring ──────────────────────────────────────────────────────────────────

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// `1.0 / (1.0 + age_days / 30.0)`; 0.0 when `last_used` is `None`.
fn recency_decay(last_used: Option<u64>, now_ms: i64) -> f64 {
    let Some(used) = last_used else {
        return 0.0;
    };
    let age_ms = now_ms.saturating_sub(used as i64);
    let age_days = age_ms as f64 / DAY_MS as f64;
    1.0 / (1.0 + age_days / 30.0)
}

/// `false_alarms / max(1, hits + false_alarms)`.
fn false_alarm_rate(hits: i64, false_alarms: i64) -> f64 {
    let total = (hits + false_alarms).max(1);
    false_alarms as f64 / total as f64
}

/// The plan §7.5 indicator sum for one lexicon row.
fn row_score(row: &LexiconRow, ctx: &BoostContext) -> f64 {
    let mut score = 0.0;
    match row.source.as_str() {
        "manual" => score += 2.0,
        "learned" => score += 1.5,
        "contacts" => score += 0.7,
        "pack" => score += 0.5,
        _ => {}
    }
    score += 0.8 * (row.hits.max(0) as f64 + 1.0).ln();
    score -= 2.0 * false_alarm_rate(row.hits, row.false_alarms);
    if let (Some(scope), Some(cat)) = (row.app_scope.as_deref(), ctx.app_category) {
        if scope == cat {
            score += 0.5;
        }
    }
    score += recency_decay(row.last_used, ctx.now_ms);
    score * row.weight
}

/// A screen term enters with the `screen` indicator replaced by a 0.5 base:
/// no hits/false-alarm terms, and recency is 1.0 (it is being used right now).
fn screen_term_score() -> f64 {
    0.5 + 1.0
}

/// Builds the top-`n` boost list for `rows` under `ctx`.
///
/// Excludes tombstoned rows. Screen terms are included (deduped
/// case-insensitively against lexicon terms; the lexicon row wins) and
/// ordered deterministically: score descending, then term ascending.
pub fn boost_list(rows: &[LexiconRow], ctx: &BoostContext, n: usize) -> Vec<BoostTerm> {
    if n == 0 {
        return Vec::new();
    }
    let mut scored: Vec<(f64, String, Vec<String>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for row in rows {
        if row.tombstoned {
            continue;
        }
        let key = row.term.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let score = row_score(row, ctx);
        scored.push((score, row.term.clone(), row.aliases.clone()));
    }

    for term in ctx.screen_terms {
        let key = term.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        scored.push((screen_term_score(), term.clone(), Vec::new()));
    }

    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });
    scored.truncate(n);

    scored
        .into_iter()
        .map(|(score, text, aliases)| BoostTerm {
            text,
            aliases,
            weight: score as f32,
        })
        .collect()
}

// ── Fingerprint / hashing ────────────────────────────────────────────────────

/// A stable, cheap hash over the lexicon rows that matters for boost
/// scoring. Callers use it as the cache's `lexicon_version` without store
/// changes: any change to (term, source, weight, hits, false_alarms,
/// tombstoned, last_used, app_scope) changes the fingerprint.
pub fn lexicon_fingerprint(rows: &[LexiconRow]) -> u64 {
    // Sort by term first so the hash is independent of input order.
    let mut sorted: Vec<&LexiconRow> = rows.iter().collect();
    sorted.sort_by(|a, b| a.term.cmp(&b.term));

    let mut hasher = DefaultHasher::new();
    for row in sorted {
        row.term.hash(&mut hasher);
        row.source.hash(&mut hasher);
        row.weight.to_bits().hash(&mut hasher);
        row.hits.hash(&mut hasher);
        row.false_alarms.hash(&mut hasher);
        row.tombstoned.hash(&mut hasher);
        row.last_used.hash(&mut hasher);
        row.app_scope.hash(&mut hasher);
    }
    hasher.finish()
}

/// Order-insensitive hash of a set of screen terms (sorts a copy first).
pub fn screen_terms_hash(terms: &[String]) -> u64 {
    let mut sorted: Vec<&String> = terms.iter().collect();
    sorted.sort();

    let mut hasher = DefaultHasher::new();
    for term in sorted {
        term.hash(&mut hasher);
    }
    hasher.finish()
}

// ── Cache ────────────────────────────────────────────────────────────────────

/// Cache key for a built boost list. Key correctness is the invalidation:
/// no time-based expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoostCacheKey {
    pub lexicon_version: u64,
    pub app_category: Option<String>,
    pub screen_terms_hash: u64,
}

/// One-entry boost list cache. The single entry is keyed by
/// `BoostCacheKey`; a key change evicts and rebuilds.
#[derive(Debug, Default)]
pub struct BoostCache {
    entry: Mutex<Option<(BoostCacheKey, Vec<BoostTerm>)>>,
}

impl BoostCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached list for `key`, or builds (and caches) one.
    pub fn get_or_build(
        &self,
        key: &BoostCacheKey,
        build: impl FnOnce() -> Vec<BoostTerm>,
    ) -> Vec<BoostTerm> {
        let mut guard = self.entry.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some((cached_key, terms)) if cached_key == key => terms.clone(),
            _ => {
                let terms = build();
                *guard = Some((key.clone(), terms.clone()));
                terms
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000; // fixed "now" for deterministic tests

    fn row(term: &str, source: &str) -> LexiconRow {
        LexiconRow {
            term: term.into(),
            aliases: vec![],
            source: source.into(),
            weight: 1.0,
            hits: 0,
            false_alarms: 0,
            created: NOW as u64,
            last_used: None,
            tombstoned: false,
            app_scope: None,
        }
    }

    fn ctx() -> BoostContext<'static> {
        BoostContext {
            app_category: None,
            screen_terms: &[],
            now_ms: NOW,
        }
    }

    #[test]
    fn source_indicator_ordering() {
        // manual > learned > screen > contacts > pack, all else equal.
        let rows = vec![
            row("z-pack", "pack"),
            row("z-contacts", "contacts"),
            row("z-learned", "learned"),
            row("z-manual", "manual"),
        ];
        let ctx = BoostContext {
            app_category: None,
            screen_terms: &["z-screen".to_string()],
            now_ms: NOW,
        };
        let list = boost_list(&rows, &ctx, 10);
        let order: Vec<&str> = list.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            order,
            vec!["z-manual", "z-learned", "z-screen", "z-contacts", "z-pack"]
        );
    }

    #[test]
    fn app_scope_match_adds_half() {
        let mut scoped = row("scoped", "learned");
        scoped.app_scope = Some("chat".into());
        let mut unscoped = row("scoped", "learned");
        unscoped.app_scope = Some("email".into());

        let ctx = BoostContext {
            app_category: Some("chat"),
            screen_terms: &[],
            now_ms: NOW,
        };
        let with_match = boost_list(&[scoped.clone()], &ctx, 1);
        let with_mismatch = boost_list(&[unscoped], &ctx, 1);
        // Mismatch does not remove the term; match scores higher.
        assert!(with_mismatch[0].weight > 0.0, "mismatch keeps the term");
        assert!(
            with_match[0].weight > with_mismatch[0].weight,
            "match adds 0.5: {} vs {}",
            with_match[0].weight,
            with_mismatch[0].weight
        );
    }

    #[test]
    fn false_alarm_penalty_drops_term_below_clean_one() {
        let mut dirty = row("dirty", "learned");
        dirty.hits = 1;
        dirty.false_alarms = 1; // rate = 0.5 → −1.0
        let clean = row("clean", "learned");

        let ctx = ctx();
        let list = boost_list(&[dirty.clone(), clean.clone()], &ctx, 2);
        assert_eq!(list[0].text, "clean");
        assert_eq!(list[1].text, "dirty");
    }

    #[test]
    fn fresh_term_beats_60_day_old_term() {
        let mut fresh = row("fresh", "learned");
        fresh.last_used = Some(NOW as u64); // recency 1.0
        let mut stale = row("stale", "learned");
        stale.last_used = Some((NOW - 60 * DAY_MS) as u64); // recency 1/3

        let list = boost_list(&[stale, fresh], &ctx(), 2);
        assert_eq!(list[0].text, "fresh");
        assert_eq!(list[1].text, "stale");
    }

    #[test]
    fn weight_multiplier_halves_score() {
        let a = row("term", "learned");
        let mut b = row("term2", "learned");
        b.weight = 0.5;
        let full = boost_list(&[a], &ctx(), 1);
        let half = boost_list(&[b], &ctx(), 1);
        let expected = (full[0].weight as f64) * 0.5;
        assert!((half[0].weight as f64 - expected).abs() < 1e-4);
    }

    #[test]
    fn tombstoned_rows_never_appear() {
        let live = row("live", "manual");
        let mut dead = row("dead", "manual");
        dead.tombstoned = true;
        let list = boost_list(&[live, dead], &ctx(), 10);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].text, "live");
    }

    #[test]
    fn screen_terms_included_at_half_and_deduped_case_insensitive() {
        let row = row("Term", "learned"); // lexicon row wins over screen
        let ctx = BoostContext {
            app_category: None,
            screen_terms: &[
                "term".to_string(), // duplicate of lexicon row, different case
                "freshscreen".to_string(),
            ],
            now_ms: NOW,
        };
        let list = boost_list(&[row], &ctx, 10);
        let texts: Vec<&str> = list.iter().map(|t| t.text.as_str()).collect();
        // "Term" appears once (lexicon row, not the screen entry).
        assert_eq!(
            texts
                .iter()
                .filter(|t| t.eq_ignore_ascii_case("term"))
                .count(),
            1
        );
        assert!(texts.contains(&"freshscreen"));
        // The screen term's weight is exactly 1.5 (0.5 base + 1.0 recency).
        let screen = list.iter().find(|t| t.text == "freshscreen").unwrap();
        assert!((screen.weight - 1.5).abs() < 1e-6);
    }

    #[test]
    fn top_n_truncation_and_order_independence() {
        let rows: Vec<LexiconRow> = (0..10)
            .map(|i| {
                let mut r = row(&format!("term{i:02}"), "learned");
                r.hits = i as i64;
                r
            })
            .collect();
        let reversed: Vec<LexiconRow> = rows.iter().rev().cloned().collect();
        let ctx = ctx();
        let a = boost_list(&rows, &ctx, 3);
        let b = boost_list(&reversed, &ctx, 3);
        assert_eq!(a.len(), 3);
        assert_eq!(a, b, "output must not depend on input row order");
        // Highest hits first.
        assert_eq!(a[0].text, "term09");
        assert_eq!(a[1].text, "term08");
        assert_eq!(a[2].text, "term07");
    }

    #[test]
    fn tie_break_is_term_ascending() {
        // Two rows with identical scores: term asc decides.
        let a = row("bbb", "learned");
        let b = row("aaa", "learned");
        let list = boost_list(&[a, b], &ctx(), 2);
        assert_eq!(list[0].text, "aaa");
        assert_eq!(list[1].text, "bbb");
    }

    #[test]
    fn cache_builds_once_per_key_and_rebuilds_on_version_change() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = BoostCache::new();
        let rows = vec![row("alpha", "manual")];
        let ctx = ctx();

        let key1 = BoostCacheKey {
            lexicon_version: 1,
            app_category: None,
            screen_terms_hash: 0,
        };
        let builds = AtomicUsize::new(0);
        let first = cache.get_or_build(&key1, || {
            builds.fetch_add(1, Ordering::SeqCst);
            boost_list(&rows, &ctx, 5)
        });
        let second = cache.get_or_build(&key1, || {
            builds.fetch_add(1, Ordering::SeqCst);
            boost_list(&rows, &ctx, 5)
        });
        assert_eq!(
            builds.load(Ordering::SeqCst),
            1,
            "second call must hit cache"
        );
        assert_eq!(first, second);

        // Version change → rebuild.
        let key2 = BoostCacheKey {
            lexicon_version: 2,
            app_category: None,
            screen_terms_hash: 0,
        };
        let _ = cache.get_or_build(&key2, || {
            builds.fetch_add(1, Ordering::SeqCst);
            boost_list(&rows, &ctx, 5)
        });
        assert_eq!(
            builds.load(Ordering::SeqCst),
            2,
            "version change must rebuild"
        );
    }

    #[test]
    fn fingerprints_are_stable_and_sensitive() {
        let rows = vec![row("a", "manual"), row("b", "learned")];
        let reversed: Vec<LexiconRow> = rows.iter().rev().cloned().collect();
        assert_eq!(
            lexicon_fingerprint(&rows),
            lexicon_fingerprint(&reversed),
            "fingerprint must not depend on row order"
        );

        let mut changed = rows.clone();
        changed[0].hits += 1;
        assert_ne!(lexicon_fingerprint(&rows), lexicon_fingerprint(&changed));

        let mut t = rows.clone();
        t[1].tombstoned = true;
        assert_ne!(lexicon_fingerprint(&rows), lexicon_fingerprint(&t));
    }

    #[test]
    fn screen_terms_hash_is_order_insensitive() {
        let a = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let b = vec!["z".to_string(), "x".to_string(), "y".to_string()];
        assert_eq!(screen_terms_hash(&a), screen_terms_hash(&b));
        let c = vec!["x".to_string(), "y".to_string(), "w".to_string()];
        assert_ne!(screen_terms_hash(&a), screen_terms_hash(&c));
    }

    proptest::proptest! {
        #[test]
        fn boost_list_never_panics_and_is_deterministic(
            terms in proptest::collection::vec(proptest::string::string_regex("[a-zA-Z]{1,12}").unwrap(), 0..12),
            n in 0usize..15,
        ) {
            let sources = ["manual", "learned", "contacts", "pack", "import"];
            let rows: Vec<LexiconRow> = terms.iter().enumerate().map(|(i, t)| {
                let mut r = row(t, sources[i % sources.len()]);
                r.hits = i as i64;
                r
            }).collect();
            let ctx = ctx();
            let a = boost_list(&rows, &ctx, n);
            let b = boost_list(&rows, &ctx, n);
            assert_eq!(a, b, "same inputs must give the same output");
            assert!(a.len() <= n);
        }
    }
}
