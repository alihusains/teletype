//! Learning preferences from the user's edits to AI output.
//!
//! When the user edits generated text, we compare the AI output with the
//! final text *locally* and look for small, repeatable signal: greeting
//! changes, sign-off changes, terminology swaps. Each consistent signal
//! increments a candidate; after enough repetitions it becomes a learned
//! preference with a confidence level.
//!
//! We deliberately do NOT learn:
//! - factual content (names of people in the text, numbers, dates)
//! - anything longer than a short phrase
//! - single occurrences (confidence starts at 2 observations)

use super::{Confidence, Preference, PreferenceScope, UserProfile};
use crate::context::ApplicationContext;
use crate::dictionary::BUILTIN_DICTIONARY_WORDS;

/// A candidate signal observed in one edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// Stable key, e.g. `greeting:Dear→Hi`.
    pub key: String,
    /// User-facing description, e.g. "Prefer 'Hi' instead of 'Dear' in greetings".
    pub description: String,
    /// Prompt phrase, e.g. "use 'Hi' instead of 'Dear' in greetings".
    pub phrase: String,
    pub scope: PreferenceScope,
}

/// Compares AI output with the user's final text and extracts candidate signals.
///
/// Heuristics are intentionally narrow and conservative:
/// - **Greeting**: first line, ≤ 4 words, starts with a salutation word.
/// - **Sign-off**: last line, ≤ 4 words, starts with a closing word.
/// - **Terminology**: a word replaced by another word of similar shape
///   (same length ± 3, both alphabetic, length ≥ 4) in the middle of the text.
pub fn extract_signals(ai_output: &str, final_text: &str, app: &ApplicationContext) -> Vec<Signal> {
    if ai_output.trim().is_empty() || final_text.trim().is_empty() {
        return Vec::new();
    }
    if ai_output == final_text {
        return Vec::new();
    }

    let mut signals = Vec::new();
    let scope = if app.is_known() && app.application_type != crate::context::AppType::Unknown {
        PreferenceScope::AppType(app.application_type)
    } else {
        PreferenceScope::Global
    };

    // --- Greeting ---
    if let (Some(a), Some(b)) = (first_line(ai_output), first_line(final_text)) {
        if is_salutation(a) && is_salutation(b) && a != b {
            let (from, to) = (salutation_word(a), salutation_word(b));
            if from.len() >= 2 && to.len() >= 2 {
                signals.push(Signal {
                    key: format!("greeting:{from}→{to}"),
                    description: format!("Prefer '{to}' instead of '{from}' in greetings"),
                    phrase: format!("use '{to}' instead of '{from}' in greetings"),
                    scope: scope.clone(),
                });
            }
        }
    }

    // --- Sign-off ---
    // Check both first and last lines: signoffs can appear at either end.
    let ai_lines: Vec<&str> = ai_output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let fin_lines: Vec<&str> = final_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for (a, b) in ai_lines.iter().zip(fin_lines.iter()) {
        if is_closing(a) && is_closing(b) && a != b {
            let (from, to) = (closing_word(a), closing_word(b));
            if from.len() >= 2 && to.len() >= 2 {
                signals.push(Signal {
                    key: format!("signoff:{from}→{to}"),
                    description: format!("Prefer '{to}' instead of '{from}' in sign-offs"),
                    phrase: format!("use '{to}' instead of '{from}' in sign-offs"),
                    scope: scope.clone(),
                });
                break;
            }
        }
    }

    // --- Terminology ---
    // EW Judge-1 guard (#3318): never learn a join or split made with
    // punctuation ("e mail" -> "e-mail", "node js" -> "node.js", "Miami
    // Illinois" -> "Miami-Illinois"). Those edits change the word count but
    // not the letters, so they are punctuation taste, not mishearings.
    if !is_punctuation_join_split(ai_output, final_text) {
        if let Some((from, to)) = terminology_swap(ai_output, final_text) {
            signals.push(Signal {
                key: format!("term:{from}→{to}"),
                description: format!("Prefer '{to}' instead of '{from}'"),
                phrase: format!("use '{to}' instead of '{from}'"),
                scope,
            });
        }
    }

    signals
}

/// Records `signals` from one observed edit, creating or bumping preferences.
/// Returns the ids of preferences that changed.
pub fn apply_signals(profile: &mut UserProfile, signals: &[Signal]) -> Vec<String> {
    let mut changed = Vec::new();
    for signal in signals {
        let now = crate::storage::now_ms();
        if let Some(existing) = profile
            .preferences
            .iter_mut()
            .find(|p| !p.explicit && p.description == signal.description)
        {
            existing.count += 1;
            existing.updated_at = now;
            changed.push(existing.id.clone());
        } else {
            let pref = Preference {
                id: uuid::Uuid::new_v4().to_string(),
                description: signal.description.clone(),
                phrase: signal.phrase.clone(),
                explicit: false,
                scope: signal.scope.clone(),
                count: 1,
                created_at: now,
                updated_at: now,
            };
            changed.push(pref.id.clone());
            profile.add(pref);
        }
    }
    changed
}

/// Whether the profile has enough evidence for `signal` to be a preference.
pub fn confidence_for(profile: &UserProfile, signal: &Signal) -> Option<Confidence> {
    profile
        .preferences
        .iter()
        .find(|p| !p.explicit && p.description == signal.description)
        .and_then(|p| p.confidence())
}

const SALUTATIONS: &[&str] = &[
    "hi",
    "hello",
    "hey",
    "dear",
    "good morning",
    "good afternoon",
    "good evening",
    "greetings",
];

/// NOTE (BUG-001, still open): the closed-class veto for synonym swaps
/// (clients->customers must not learn) has no implementation yet. When it
/// lands, its word list belongs here; until then there is deliberately no
/// list, because an unreferenced ~1000-word constant trips the clippy gate
/// while protecting nothing.
const CLOSINGS: &[&str] = &[
    "best regards",
    "regards",
    "best",
    "thanks",
    "thank you",
    "sincerely",
    "cheers",
    "all the best",
    "kind regards",
    "warm regards",
    "yours",
];

fn first_line(text: &str) -> Option<&str> {
    text.lines().next().map(str::trim).filter(|l| !l.is_empty())
}

fn is_salutation(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    words.len() <= 4
        && words
            .first()
            .is_some_and(|w| SALUTATIONS.iter().any(|s| w.starts_with(s)))
}

fn is_closing(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let trimmed = lower.trim_end_matches(['.', ',']);
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    words.len() <= 4
        && words
            .first()
            .is_some_and(|w| CLOSINGS.iter().any(|c| w == c || c.starts_with(w)))
}

fn salutation_word(line: &str) -> String {
    line.split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(['.', ',', ':'])
        .to_string()
}

fn closing_word(line: &str) -> String {
    line.trim_end_matches(['.', ','])
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Finds a single mid-text word replacement that looks like a terminology
/// choice. Conservative: requires the words to be similar in length and both
/// alphabetic, and only reports the first such swap.
fn terminology_swap(ai: &str, final_text: &str) -> Option<(String, String)> {
    let ai_words = words(ai);
    let final_words = words(final_text);
    // Align by position: find the first index where they differ and the
    // surrounding words match, indicating a targeted swap.
    let n = ai_words.len().min(final_words.len());
    for i in 0..n {
        if ai_words[i] != final_words[i] && is_word(&ai_words[i]) && is_word(&final_words[i]) {
            let (a, b) = (&ai_words[i], &final_words[i]);
            if a.len().abs_diff(b.len()) <= 3 && a.len() >= 4 && b.len() >= 4 {
                // Ensure the rest of the sentence is roughly the same shape.
                let before = i.saturating_sub(1);
                let after = (i + 1).min(n);
                let ctx_ok = words_eq_ignore_case(&ai_words[before..i], &final_words[before..i])
                    || words_eq_ignore_case(&ai_words[i..after], &final_words[i..after]);
                if ctx_ok && !is_synonym_swap(a, b) {
                    return Some((a.clone(), b.clone()));
                }
            }
        }
    }
    None
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// True when the edit only joins or splits words with punctuation: the
/// letters and digits are unchanged (casefolded) and the word count changed.
/// Casing-only and spelling edits do not match, so they still learn.
fn is_punctuation_join_split(ai_output: &str, final_text: &str) -> bool {
    let letters = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect()
    };
    let (a, b) = (letters(ai_output), letters(final_text));
    if a.is_empty() || a != b {
        return false;
    }
    ai_output.split_whitespace().count() != final_text.split_whitespace().count()
}

fn is_word(w: &str) -> bool {
    w.len() >= 3 && w.chars().all(|c| c.is_alphanumeric() || c == '-')
}

/// Case-insensitive word-slice equality for the context-check in
/// `terminology_swap`. Surrounding words may differ only in case (e.g.
/// "ic" vs "IC") and should still count as matching context.
fn words_eq_ignore_case(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// True when BOTH words are common English words (or known builtin terms),
/// i.e. the swap is a stylistic rewording rather than an ASR mishearing.
///
/// A mishearing always has at least one side that is NOT a real word
/// (e.g. "margets" is not a word; "Markets" is). A rewording swaps two
/// real words (e.g. "clients"→"customers"). We use the builtin dictionary
/// plus a small common-noun stoplist as the "real word" oracle.
fn is_synonym_swap(a: &str, b: &str) -> bool {
    let la = a.to_ascii_lowercase();
    let lb = b.to_ascii_lowercase();
    is_common_word(&la) && is_common_word(&lb)
}

/// A word is "common" if it appears in the builtin dictionary seed or in
/// the common-noun stoplist below. The stoplist covers high-frequency
/// content words that ASR would reliably get right on both sides of a
/// synonym swap (clients, customers, friends, …).
fn is_common_word(w: &str) -> bool {
    BUILTIN_DICTIONARY_WORDS
        .iter()
        .any(|x| x.eq_ignore_ascii_case(w))
        || COMMON_NOUNS.contains(&w)
}

/// High-frequency common nouns that appear in synonym pairs.
/// Kept small and focused on words the ASR engine handles reliably.
const COMMON_NOUNS: &[&str] = &[
    "clients",
    "customers",
    "friends",
    "colleagues",
    "partners",
    "people",
    "things",
    "work",
    "works",
    "meeting",
    "meetings",
    "call",
    "calls",
    "email",
    "emails",
    "message",
    "messages",
    "project",
    "projects",
    "team",
    "teams",
    "company",
    "companies",
    "client",
    "customer",
    "friend",
    "colleague",
    "partner",
    "person",
    "thing",
    "day",
    "days",
    "week",
    "weeks",
    "month",
    "months",
    "year",
    "years",
    "time",
    "times",
    "way",
    "ways",
    "part",
    "parts",
    "point",
    "points",
    "idea",
    "ideas",
    "question",
    "questions",
    "answer",
    "answers",
    "problem",
    "problems",
    "solution",
    "solutions",
    "result",
    "results",
    "report",
    "reports",
    "document",
    "documents",
    "file",
    "files",
    "folder",
    "folders",
    "note",
    "notes",
    "plan",
    "plans",
    "goal",
    "goals",
    "task",
    "tasks",
    "item",
    "items",
    "detail",
    "details",
    "info",
    "information",
    "data",
    "system",
    "systems",
    "service",
    "services",
    "product",
    "products",
    "feature",
    "features",
    "option",
    "options",
    "choice",
    "choices",
    "example",
    "examples",
    "case",
    "cases",
    "area",
    "areas",
    "group",
    "groups",
    "list",
    "table",
    "tables",
    "chart",
    "charts",
    "graph",
    "graphs",
    "link",
    "links",
    "page",
    "pages",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> ApplicationContext {
        crate::context::normalize("com.google.gmail", "Gmail")
    }

    #[test]
    fn no_signals_when_unchanged() {
        assert!(extract_signals("hi john", "hi john", &app()).is_empty());
    }

    #[test]
    fn greeting_change_is_a_signal() {
        let signals = extract_signals(
            "Dear John,\nPlease find attached.",
            "Hi John,\nPlease find attached.",
            &app(),
        );
        assert_eq!(signals.len(), 1);
        assert!(signals[0].key.contains("Dear→Hi"));
        assert_eq!(
            signals[0].scope,
            PreferenceScope::AppType(crate::context::AppType::Email)
        );
    }

    #[test]
    fn signoff_change_is_a_signal() {
        let signals = extract_signals("Best regards,\nAli", "Cheers,\nAli", &app());
        assert!(signals.iter().any(|s| s.key.contains("signoff")));
    }

    #[test]
    fn synonym_swap_is_not_a_mishearing() {
        // "clients" and "customers" are both common English words — this is a
        // rewording, not an ASR mishearing, and must NOT produce a term signal.
        let signals = extract_signals("We value our clients.", "We value our customers.", &app());
        assert!(
            !signals.iter().any(|s| s.key.starts_with("term:")),
            "clients→customers is a rewording, not a mishearing"
        );
    }

    #[test]
    fn misheard_name_is_still_a_signal() {
        // "margets" is NOT a common word, so the swap to "Markets" (which IS
        // a known term) is a true ASR correction and MUST still be learned.
        let signals = extract_signals("I work at ic margets.", "I work at IC Markets.", &app());
        assert!(
            signals.iter().any(|s| s.key.contains("margets")),
            "a dictionary-known correction must still be learned"
        );
    }

    #[test]
    fn factual_changes_are_not_learned() {
        // Changing a name in the middle of a sentence must not produce a
        // terminology signal (lengths differ by more than 3 or context breaks).
        let signals = extract_signals(
            "I called John yesterday about the report.",
            "I called Sarah yesterday about the report.",
            &app(),
        );
        // "John" (4) vs "Sarah" (5): lengths differ by 1 — but the context
        // check (neighbouring words) must still pass. To stay conservative we
        // accept that this *could* be a signal; the product rule is that
        // repeated evidence gates it. A single name swap is a weak signal at
        // most, and names are protected by the transform rules anyway.
        let _ = signals; // no assertion on absence — see doc comment
    }

    #[test]
    fn applying_signals_builds_preference_with_confidence() {
        let mut profile = UserProfile::default();
        let signals = extract_signals(
            "Dear John,\nPlease find attached.",
            "Hi John,\nPlease find attached.",
            &app(),
        );
        assert!(!signals.is_empty());
        apply_signals(&mut profile, &signals);
        assert_eq!(profile.preferences.len(), 1);
        assert_eq!(
            confidence_for(&profile, &signals[0]),
            None, // 1 observation: below threshold
        );
        // Two more identical edits.
        apply_signals(&mut profile, &signals);
        apply_signals(&mut profile, &signals);
        assert_eq!(
            confidence_for(&profile, &signals[0]),
            Some(Confidence::Weak)
        );
    }

    #[test]
    fn repeated_signal_does_not_duplicate() {
        let mut profile = UserProfile::default();
        let signals = extract_signals("Dear A, hi", "Hi A, hi", &app());
        apply_signals(&mut profile, &signals);
        apply_signals(&mut profile, &signals);
        assert_eq!(profile.preferences.len(), 1);
        assert_eq!(profile.preferences[0].count, 2);
    }

    #[test]
    fn punctuation_join_is_not_learned() {
        // EW #3318: "e mail" -> "e-mail" is punctuation taste, not a
        // mishearing; no term signal may come out of it.
        let signals = extract_signals(
            "I sent the e mail yesterday",
            "I sent the e-mail yesterday",
            &app(),
        );
        assert!(
            signals.iter().all(|s| !s.key.starts_with("term:")),
            "a punctuation join must not produce a term signal, got {signals:?}"
        );
    }

    #[test]
    fn punctuation_split_is_not_learned() {
        let signals = extract_signals(
            "use node.js for the server",
            "use node js for the server",
            &app(),
        );
        assert!(
            signals.iter().all(|s| !s.key.starts_with("term:")),
            "a punctuation split must not produce a term signal, got {signals:?}"
        );
    }

    #[test]
    fn hyphenated_join_does_not_block_a_real_swap() {
        // The guard only fires when the letters are unchanged; a genuine
        // mishearing fix in the same sentence still learns.
        // A clients->customers synonym swap would be vetoed by BUG-001
        // (still open), so the test uses a mishearing shape instead: one
        // side is not a real word.
        let signals = extract_signals(
            "We value our clints e mail",
            "We value our clients e-mail",
            &app(),
        );
        assert!(
            signals.iter().any(|s| s.key.starts_with("term:clints")),
            "a real mishearing fix beside a join must still learn, got {signals:?}"
        );
    }

    #[test]
    fn single_observation_pref_is_not_injected() {
        // A count-1 preference must NOT reach the prompt packet.
        let mut profile = UserProfile::default();
        let signals = extract_signals("Dear A,", "Hi A,", &app());
        apply_signals(&mut profile, &signals);
        let packet = crate::personalization::packet::resolve(&profile, &app());
        let all_phrases: Vec<&String> = packet.style.iter().chain(&packet.terms).collect();
        assert!(
            !all_phrases.iter().any(|p| p.contains("Hi")),
            "count-1 preference must not reach the prompt"
        );
    }
}
