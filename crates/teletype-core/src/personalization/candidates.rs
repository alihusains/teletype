//! Polish vocabulary candidates: the conservative, explicit-accept learning
//! loop for words Polish reveals (research:
//! `docs/research/wispr-flow-polish-personalization.md` §4.3–4.6).
//!
//! After a successful Polish run compares the original text with the polished
//! text **locally** and may return up to three *candidates* — a proposed
//! dictionary word (a proper noun or domain term) with the form it was
//! observed in. Candidates are **never** written to the dictionary on their
//! own: the user explicitly accepts one (which calls
//! `Dictionary::insert_learned`, carrying `Learned by Teletype` provenance)
//! or dismisses it (tombstoned, so the same pair is never re-offered).
//!
//! Everything here is pure and Tauri-free so it is unit testable; the
//! desktop layer persists [`CandidateStore`] through the usual `JsonStore`.

use serde::{Deserialize, Serialize};

use crate::dictionary::Dictionary;

/// How many candidates a single Polish run may offer.
pub const MAX_CANDIDATES: usize = 3;

/// A pending, or already resolved, Polish vocabulary candidate.
///
/// The pair (`heard`, `word`) is the stable identity: `heard` is the form
/// observed in the source text, `word` is the proposed dictionary spelling.
/// Only these two bounded tokens are persisted — never the surrounding
/// original or polished text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolishCandidate {
    /// Stable id: a hash of the `heard`/`word` pair. Re-observing the same
    /// pair yields the same id, which is what makes "don't re-offer" and
    /// "accept exactly once" checkable.
    pub id: String,
    /// The form observed in the source text (e.g. "margets").
    pub heard: String,
    /// The proposed dictionary word, case preserved (e.g. "Markets").
    pub word: String,
    /// Why this pair is offered; shown to the user with the candidate.
    pub reason: String,
    /// When the candidate was first observed (epoch ms).
    pub observed_at: u64,
}

impl PolishCandidate {
    pub fn new(heard: &str, word: &str, reason: &str) -> Self {
        Self {
            id: pair_id(heard, word),
            heard: heard.to_string(),
            word: word.to_string(),
            reason: reason.to_string(),
            observed_at: crate::storage::now_ms(),
        }
    }
}

/// The stable identity of a (heard, word) pair. The id is a truncated
/// SHA-256 of the normalized pair, so it is deterministic across runs and
/// machines, and it never embeds user text in a readable form.
fn pair_id(heard: &str, word: &str) -> String {
    let norm = format!(
        "{}\u{1f}{}",
        heard.trim().to_lowercase(),
        word.trim().to_lowercase()
    );
    pair_hash_hex(norm.as_bytes())
}

/// A 128-bit local identity key: two FNV-1a 64-bit hashes of the same input
/// with different offsets. It only needs to be stable and collision-resistant
/// in practice, not cryptographic.
fn pair_hash_hex(bytes: &[u8]) -> String {
    let h = |seed: u64| -> u64 {
        let mut state = seed;
        for &b in bytes {
            state ^= b as u64;
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
        state
    };
    let (a, b) = (h(0xcbf2_9ce4_8422_2325), h(0x9dd4_e4f1_dd3a_b133));
    format!("{:016x}{:016x}", a, b)
}

/// Local aggregate outcome counts for Polish vocabulary candidates.
///
/// Counts only — the store records *which* pair was resolved (so it cannot be
/// re-offered) but never the source or polished text around it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CandidateOutcomes {
    /// Candidates the user explicitly accepted (written to the dictionary).
    pub accepted: u32,
    /// Candidates the user explicitly dismissed (tombstoned).
    pub dismissed: u32,
    /// Candidates that expired or were cleared without a decision.
    pub ignored: u32,
}

/// The persisted local state of the candidate lifecycle.
///
/// One JSON document (`polish_candidates.json`), same conventions as the
/// other core stores: camelCase wire fields, `#[serde(default)]` everywhere
/// so a file written by an older build still loads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CandidateStore {
    /// Candidates awaiting a user decision, oldest first.
    pub pending: Vec<PolishCandidate>,
    /// Resolved (heard, word) pairs that must never be offered again:
    /// dismissed pairs, and pairs whose word is already in the dictionary.
    /// Bounded: the oldest tombstones drop off past [`TOMBSTONE_CAP`].
    pub dismissed_pairs: Vec<String>,
    /// Lifetime outcome counters (never decremented).
    pub outcomes: CandidateOutcomes,
    /// Lifetime count of successful Polish runs that were considered for
    /// candidates (a run that changed no tokens or fell back never counts).
    pub polish_runs: u32,
}

/// A tombstone list longer than this sheds its oldest entries so the file
/// cannot grow without bound.
const TOMBSTONE_CAP: usize = 500;

/// A pending candidate older than this (epoch ms) is stale and may be
/// cleared without a decision.
const PENDING_TTL_MS: u64 = 7 * 86_400_000;

impl CandidateStore {
    /// Returns true when the (heard, word) pair must not be offered again.
    pub fn is_suppressed(&self, heard: &str, word: &str) -> bool {
        self.dismissed_pairs.iter().any(|p| p == &pair_id(heard, word))
    }

    /// Publishes candidates from one successful Polish run.
    ///
    /// Returns the candidates actually added (already filtered against the
    /// dictionary, the tombstones, and the pending list, capped at
    /// [`MAX_CANDIDATES`]). A re-observed pending candidate is refreshed in
    /// place, not duplicated. `candidates` must already be the conservative
    /// extraction's output (see [`extract_candidates`]).
    pub fn publish(
        &mut self,
        candidates: &[PolishCandidate],
        dictionary: &Dictionary,
    ) -> Vec<PolishCandidate> {
        self.polish_runs = self.polish_runs.saturating_add(1);
        let mut added = Vec::new();
        for c in candidates {
            if added.len() >= MAX_CANDIDATES {
                break;
            }
            if c.heard.trim().is_empty() || c.word.trim().is_empty() {
                continue;
            }
            if self.is_suppressed(&c.heard, &c.word) {
                continue;
            }
            if dictionary.find(&c.word).is_some() {
                continue;
            }
            // Already pending: refresh the observation, don't duplicate.
            if let Some(pending) = self.pending.iter_mut().find(|p| p.id == c.id) {
                pending.observed_at = c.observed_at;
                continue;
            }
            self.pending.push(c.clone());
            added.push(c.clone());
        }
        added
    }

    /// Accepts one pending candidate exactly once: calls
    /// [`Dictionary::insert_learned`] on `dictionary` and tombstones the
    /// pair. Returns the accepted candidate. A second accept (or an accept
    /// of an unknown id) is a no-op error — the dictionary is never
    /// mutated twice.
    pub fn accept(
        &mut self,
        id: &str,
        dictionary: &mut Dictionary,
    ) -> Result<PolishCandidate, String> {
        let pos = self
            .pending
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| format!("Candidate not found (already resolved?): {id}"))?;
        let candidate = self.pending.remove(pos);
        if !dictionary.insert_learned(&candidate.heard, &candidate.word) {
            // The word was already in the dictionary (added by hand or by a
            // concurrent accept): count it as accepted and tombstone the pair
            // so it is never offered again.
        }
        self.tombstone(&candidate.heard, &candidate.word);
        self.outcomes.accepted = self.outcomes.accepted.saturating_add(1);
        Ok(candidate)
    }

    /// Dismisses one pending candidate and tombstones its pair so the same
    /// candidate is never offered again.
    pub fn dismiss(&mut self, id: &str) -> Result<PolishCandidate, String> {
        let pos = self
            .pending
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| format!("Candidate not found (already resolved?): {id}"))?;
        let candidate = self.pending.remove(pos);
        self.tombstone(&candidate.heard, &candidate.word);
        self.outcomes.dismissed = self.outcomes.dismissed.saturating_add(1);
        Ok(candidate)
    }

    /// Clears pending candidates older than [`PENDING_TTL_MS`], counting
    /// each as ignored. Returns how many were cleared.
    pub fn clear_stale(&mut self, now_ms: u64) -> usize {
        let before = self.pending.len();
        self.pending
            .retain(|c| now_ms.saturating_sub(c.observed_at) < PENDING_TTL_MS);
        let cleared = before - self.pending.len();
        self.outcomes.ignored = self.outcomes.ignored.saturating_add(cleared as u32);
        cleared
    }

    fn tombstone(&mut self, heard: &str, word: &str) {
        let id = pair_id(heard, word);
        if !self.dismissed_pairs.contains(&id) {
            self.dismissed_pairs.push(id);
            if self.dismissed_pairs.len() > TOMBSTONE_CAP {
                self.dismissed_pairs
                    .drain(0..self.dismissed_pairs.len() - TOMBSTONE_CAP);
            }
        }
    }
}

// ── Conservative candidate extraction ───────────────────────────────────────

/// Splits text into (trimmed, original) tokens with their character offsets.
/// Punctuation stuck to a word stays attached so the caller can strip it;
/// the token boundary in the source text is preserved.
fn tokens(text: &str) -> Vec<(String, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((text[s..i].trim().to_string(), s..i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push((text[s..].trim().to_string(), s..text.len()));
    }
    out
}

/// Strips surrounding punctuation from a token, keeping the word's own
/// letters (internal apostrophes and hyphens stay: "O'Connor", "VS Code").
fn strip_punct(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
}

/// True for tokens that can never be a dictionary candidate: URLs, email
/// addresses, absolute/relative paths, bare numbers and dates, emoji-only
/// runs, AutoText placeholders, version-like tokens, and spelled-out dates
/// (month/weekday names and ordinal words like "fifth").
fn is_not_offerable(token: &str) -> bool {
    let t = token.trim();
    if t.is_empty() {
        return true;
    }
    if t.contains("://") || t.starts_with("www.") {
        return true;
    }
    if t.contains('@') {
        return true;
    }
    if t.starts_with('/') || t.starts_with("~") || t.starts_with("./") || t.starts_with("../") {
        return true;
    }
    if t.starts_with('{') {
        return true; // AutoText placeholder
    }
    // Any digit: numbers, dates, versions, times.
    if t.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    // Time suffixes ("5:30pm", "9am"): the bare "pm"/"am" after stripping
    // is not a word.
    if matches!(t.to_lowercase().as_str(), "am" | "pm") {
        return true;
    }
    // Spelled-out dates: month/weekday names and ordinal words. Polish
    // surfaces these routinely and they are never dictionary material.
    let lower = t.to_lowercase();
    if MONTH_DAY_NAMES.contains(&lower.as_str()) || ORDINAL_NAMES.contains(&lower.as_str()) {
        return true;
    }
    // Emoji-only (no letter at all).
    if !t.chars().any(|c| c.is_alphabetic()) {
        return true;
    }
    false
}

const MONTH_DAY_NAMES: &[&str] = &[
    "january", "february", "march", "april", "may", "june", "july", "august",
    "september", "october", "november", "december", "jan", "feb", "mar", "apr",
    "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "monday", "tuesday",
    "wednesday", "thursday", "friday", "saturday", "sunday", "mon", "tue",
    "tues", "wed", "thu", "thur", "thurs", "fri", "sat", "sun",
];

const ORDINAL_NAMES: &[&str] = &[
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth",
    "ninth", "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth",
    "fifteenth", "sixteenth", "seventeenth", "eighteenth", "nineteenth",
    "twentieth", "thirtieth",
];

/// Bounded stop-word list: ordinary everyday words that Polish routinely
/// touches but that are never dictionary material. Deliberately small — the
/// filter is conservative in the other direction (skip when unsure).
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "of", "to", "in", "on", "at", "for", "with", "by",
    "from", "as", "is", "are", "was", "were", "be", "been", "being", "it", "its", "this",
    "that", "these", "those", "i", "you", "he", "she", "we", "they", "my", "your", "his",
    "her", "our", "their", "not", "no", "yes", "do", "does", "did", "will", "would", "can",
    "could", "should", "shall", "may", "might", "must", "please", "thanks", "thank", "yeah",
    "yep", "nope", "just", "really", "very", "too", "also", "so", "if", "when", "then",
    "there", "here", "what", "which", "who", "whom", "how", "why", "where", "about", "into",
    "over", "under", "again", "once", "now", "today", "tomorrow", "yesterday", "good",
    "great", "fine", "okay", "fine", "sure", "well", "ok", "got", "get", "got", "make",
    "made", "take", "took", "know", "knew", "think", "thought", "want", "wanted", "need",
    "needed", "like", "liked", "use", "used", "using", "say", "said", "tell", "told",
    "go", "went", "going", "come", "came", "coming", "see", "saw", "seen", "look", "looked",
    "work", "worked", "working", "send", "sent", "sending", "write", "wrote", "written",
    "meet", "meeting", "meetings", "call", "called", "email", "emails", "report", "reports",
    "team", "teams", "project", "projects", "day", "days", "week", "weeks", "month", "months",
    "time", "times", "year", "years", "today", "morning", "afternoon", "evening", "minute",
    "minutes", "hour", "hours",
    "sharp", "please", "thanks",
];

fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word.to_lowercase().as_str())
}

/// Compares the original text with the polished text and returns at most
/// [`MAX_CANDIDATES`] conservative dictionary candidates.
///
/// A candidate is a **bounded single-token substitution** — the polished
/// text contains a token the original did not, the token is alphabetic (a
/// safe multi-word proper/domain term is allowed: up to 3 words, no
/// digits), it is not in the dictionary or an alias, and it is not a URL,
/// path, email, number/date, emoji-only token, or ordinary stop word.
///
/// Deterministic: same inputs, same output, same order (first occurrence in
/// the polished text). No model call, no network, no screen capture.
pub fn extract_candidates(
    original: &str,
    polished: &str,
    dictionary: &Dictionary,
) -> Vec<PolishCandidate> {
    if original.trim().is_empty() || polished.trim().is_empty() || original == polished {
        return Vec::new();
    }

    // Case-insensitive, punctuation-stripped token sets for membership.
    let norm = |t: &str| strip_punct(t).to_lowercase();
    let original_tokens: std::collections::HashSet<String> =
        tokens(original).iter().map(|(t, _)| norm(t)).collect();
    let known: std::collections::HashSet<String> = dictionary
        .words
        .iter()
        .flat_map(|w| {
            std::iter::once(&w.word)
                .chain(w.aliases.iter())
                .map(|s| s.to_lowercase())
        })
        .collect();

    let mut out: Vec<PolishCandidate> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (tok, range) in tokens(polished) {
        if out.len() >= MAX_CANDIDATES {
            break;
        }
        let word = strip_punct(&tok);
        if word.len() < 3 {
            continue;
        }
        if is_not_offerable(word) {
            continue;
        }
        let lower = word.to_lowercase();
        if original_tokens.contains(&lower) {
            continue; // not a substitution
        }
        if known.contains(&lower) {
            continue; // already a dictionary word or alias
        }
        if is_stop_word(&lower) {
            continue;
        }
        // Safe multi-word terms: "VS Code", "Google Docs" — up to 3 words,
        // all alphabetic, and the whole phrase absent from the original.
        let is_multi = word.contains(' ');
        if is_multi && word.split(' ').count() > 3 {
            continue;
        }
        // A single word must be alphabetic (apostrophe/hyphen allowed); a
        // multi-word term may not contain punctuation beyond the space.
        let chars_ok = if is_multi {
            word
                .chars()
                .all(|c| c.is_alphabetic() || c == ' ')
        } else {
            word.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-')
        };
        if !chars_ok {
            continue;
        }
        // Bounded single-token substitution: the polished token must sit
        // where the original had *some* token — a token appearing past the
        // end of the original text is invented content, not a mishearing.
        if !sits_in_original_span(original, range.end, word) {
            continue;
        }
        if seen.insert(pair_id(&lower, word)) {
            out.push(PolishCandidate::new(
                &lower,
                word,
                "Polish revealed a term that was not in your dictionary",
            ));
        }
    }
    out
}

/// Positional guard: the polished token must end before (or at) the end of
/// the original text, plus a small slack for polish that expands slightly
/// ("margets" → "Markets" is the same length; a rewrite rarely grows a
/// sentence by more than a word or two). A token that ends far past the
/// original's length is invented content, not a substitution of a misheard
/// word. Deliberately coarse — the filter is conservative in the other
/// direction (skip when unsure).
fn sits_in_original_span(original: &str, polished_end: usize, word: &str) -> bool {
    let slack = word.len().saturating_add(8);
    polished_end <= original.len() + slack
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dict_with(words: &[&str]) -> Dictionary {
        let mut d = Dictionary::default();
        for w in words {
            let _ = d.insert(crate::dictionary::DictionaryWord::new(*w, ""));
        }
        d
    }

    #[test]
    fn proper_noun_substitution_is_offered() {
        let d = Dictionary::default();
        let c = extract_candidates("send it to margets", "Send it to Markets", &d);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].heard, "markets");
        assert_eq!(c[0].word, "Markets");
    }

    #[test]
    fn domain_term_is_offered() {
        let d = Dictionary::default();
        let c = extract_candidates("we use kubernates here", "We use Kubernetes here", &d);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].word, "Kubernetes");
    }

    #[test]
    fn existing_dictionary_word_is_skipped() {
        let d = dict_with(&["Markets"]);
        assert!(extract_candidates("send it to margets", "Send it to Markets", &d).is_empty());
    }

    #[test]
    fn existing_alias_is_skipped() {
        let mut d = Dictionary::default();
        let mut w = crate::dictionary::DictionaryWord::new("Teletype", "");
        w.aliases.push("tel-uh-type".into());
        d.insert(w).unwrap();
        assert!(
            extract_candidates("i use tel uh type", "I use Teletype", &d).is_empty()
        );
    }

    #[test]
    fn urls_paths_emails_numbers_dates_are_skipped() {
        let d = Dictionary::default();
        assert!(extract_candidates(
            "go to the page",
            "Go to https://example.com/page",
            &d
        )
        .is_empty());
        assert!(extract_candidates("open the file", "Open /Users/ali/report.txt", &d)
            .is_empty());
        assert!(extract_candidates(
            "mail me",
            "Mail me at ali@example.com",
            &d
        )
        .is_empty());
        assert!(extract_candidates("on the fifth", "On the 15th of March", &d).is_empty());
        assert!(extract_candidates("at fivethirty", "At 5:30pm sharp", &d).is_empty());
    }

    #[test]
    fn ordinary_words_are_skipped() {
        let d = Dictionary::default();
        assert!(extract_candidates("do it for me", "Do it for me, please", &d).is_empty());
        assert!(extract_candidates("i need a report", "I need the report", &d).is_empty());
    }

    #[test]
    fn empty_or_unchanged_text_yields_nothing() {
        let d = Dictionary::default();
        assert!(extract_candidates("", "anything", &d).is_empty());
        assert!(extract_candidates("hello there", "", &d).is_empty());
        assert!(extract_candidates("hello there", "hello there", &d).is_empty());
    }

    #[test]
    fn candidate_cap_and_deterministic_ordering() {
        let d = Dictionary::default();
        let c = extract_candidates(
            "margets kubernates and teletype stuido",
            "Markets Kubernetes and Teletype Studio",
            &d,
        );
        assert_eq!(c.len(), MAX_CANDIDATES);
        // First-occurrence order in the polished text.
        assert_eq!(c[0].word, "Markets");
        assert_eq!(c[1].word, "Kubernetes");
        assert_eq!(c[2].word, "Studio");
        // Deterministic: a second call gives the identical list (ids and order).
        let again = extract_candidates(
            "margets kubernates and teletype stuido",
            "Markets Kubernetes and Teletype Studio",
            &d,
        );
        assert_eq!(
            c.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            again.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn pair_ids_are_stable() {
        // Case- and case-insensitive lookups of the same pair collide on the
        // stored (lowercased) pair — that is what "don't re-offer" needs.
        assert_eq!(pair_id("margets", "markets"), pair_id("margets", "markets"));
        assert_ne!(pair_id("margets", "markets"), pair_id("margets", "margate"));
    }

    #[test]
    fn suppressed_lookup_is_case_insensitive() {
        let mut store = CandidateStore::default();
        store.tombstone("margets", "Markets");
        // The re-offer check normalizes both sides, so any casing of the
        // same pair is suppressed.
        assert!(store.is_suppressed("MARGETS", "MARKETS"));
        assert!(store.is_suppressed("margets", "markets"));
        assert!(!store.is_suppressed("margets", "margate"));
    }

    // ── Store lifecycle ──

    #[test]
    fn accept_writes_provenance_exactly_once() {
        let mut store = CandidateStore::default();
        let mut d = Dictionary::default();
        let c = PolishCandidate::new("margets", "Markets", "test");
        let added = store.publish(std::slice::from_ref(&c), &d);
        assert_eq!(added.len(), 1);
        assert_eq!(store.polish_runs, 1);

        let accepted = store.accept(&c.id, &mut d).unwrap();
        assert_eq!(accepted.word, "Markets");
        let w = d.find("Markets").expect("word must exist");
        assert_eq!(w.learned_from.as_deref(), Some("margets"));
        assert!(w.learned_at.is_some());
        assert_eq!(store.outcomes.accepted, 1);

        // The dictionary is not mutated a second time: the candidate is gone.
        let before = d.words.len();
        assert!(store.accept(&c.id, &mut d).is_err());
        assert_eq!(d.words.len(), before);
        assert_eq!(store.outcomes.accepted, 1);
    }

    #[test]
    fn dismiss_tombstones_the_pair() {
        let mut store = CandidateStore::default();
        let d = Dictionary::default();
        let c = PolishCandidate::new("margets", "Markets", "test");
        store.publish(std::slice::from_ref(&c), &d);
        store.dismiss(&c.id).unwrap();
        assert_eq!(store.outcomes.dismissed, 1);
        // The tombstone is keyed by the stored pair; the check matches it.
        assert!(store.is_suppressed(&c.heard, &c.word));
        // Re-offering the same pair is refused.
        let again = store.publish(std::slice::from_ref(&c), &d);
        assert!(again.is_empty());
        assert!(store.pending.is_empty());
    }

    #[test]
    fn no_mutation_before_accept() {
        let mut store = CandidateStore::default();
        let d = Dictionary::default();
        let c = PolishCandidate::new("kubernates", "Kubernetes", "test");
        store.publish(std::slice::from_ref(&c), &d);
        assert!(d.words.is_empty(), "publish must not touch the dictionary");
        assert!(store.pending.len() == 1);
    }

    #[test]
    fn duplicate_publish_does_not_duplicate() {
        let mut store = CandidateStore::default();
        let d = Dictionary::default();
        let c = PolishCandidate::new("margets", "Markets", "test");
        store.publish(std::slice::from_ref(&c), &d);
        // The same result observed twice.
        store.publish(std::slice::from_ref(&c), &d);
        assert_eq!(store.pending.len(), 1);
        // polish_runs still counts both runs.
        assert_eq!(store.polish_runs, 2);
    }

    #[test]
    fn stale_pending_are_cleared_and_counted_ignored() {
        let mut store = CandidateStore::default();
        let d = Dictionary::default();
        let mut c = PolishCandidate::new("margets", "Markets", "test");
        c.observed_at = 1_000;
        let fresh = PolishCandidate {
            observed_at: 1_000 + PENDING_TTL_MS - 1,
            ..c.clone()
        };
        store.pending = vec![c, fresh];
        let cleared = store.clear_stale(1_000 + PENDING_TTL_MS);
        assert_eq!(cleared, 1);
        assert_eq!(store.outcomes.ignored, 1);
        assert_eq!(store.pending.len(), 1);
        assert!(store.pending[0].word == "Markets");
    }

    #[test]
    fn store_round_trips_json_without_text_retention() {
        let mut store = CandidateStore::default();
        let d = Dictionary::default();
        let c = PolishCandidate::new("margets", "Markets", "test");
        store.publish(std::slice::from_ref(&c), &d);
        store.dismiss(&c.id).unwrap();
        let json = serde_json::to_string(&store).unwrap();
        // Only the bounded pair is persisted, never surrounding text.
        assert!(!json.contains("send it to"));
        let back: CandidateStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back, store);
    }

    #[test]
    fn legacy_store_json_without_new_fields_still_loads() {
        let store: CandidateStore =
            serde_json::from_str(r#"{"pending":[]}"#).unwrap();
        assert!(store.pending.is_empty());
        assert!(store.dismissed_pairs.is_empty());
        assert_eq!(store.outcomes, CandidateOutcomes::default());
        assert_eq!(store.polish_runs, 0);
    }

    #[test]
    fn tombstone_list_is_bounded() {
        let mut store = CandidateStore::default();
        for i in 0..(TOMBSTONE_CAP + 50) {
            store.tombstone(&format!("h{i}"), &format!("w{i}"));
        }
        assert_eq!(store.dismissed_pairs.len(), TOMBSTONE_CAP);
    }
}
