//! Direct expansion: turn `/trigger` occurrences into their values.
//!
//! Used for typed input (no AI involved) and as the post-transform step when a
//! transform ran. Unlike [`super::protect`], this replaces the trigger with
//! the value directly.

use super::{match_snippet_at, AutoTextEntry, AutoTextStore};
use crate::context::ApplicationContext;

/// Expands every applicable trigger in `text` to its value.
///
/// Matching rules are the same as protection: whole tokens only (start or
/// whitespace before, whitespace or end after).
pub fn expand(text: &str, store: &AutoTextStore, app: &ApplicationContext) -> String {
    let mut applicable: Vec<&AutoTextEntry> =
        store.entries.iter().filter(|e| e.applies_to(app)).collect();
    applicable.sort_by_key(|e| std::cmp::Reverse(e.trigger.len()));

    if applicable.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(pos) = rest.find('/') {
        let prev_ok = if pos == 0 {
            true
        } else {
            rest[..pos]
                .chars()
                .next_back()
                .is_some_and(|p| p.is_whitespace())
        };

        if prev_ok {
            let after_slash = &rest[pos + 1..];
            let mut matched: Option<(&AutoTextEntry, usize)> = None;
            for entry in &applicable {
                let body = &entry.trigger[1..];
                if after_slash.starts_with(body) {
                    matched = Some((entry, body.len()));
                    break;
                }
            }
            if let Some((entry, body_len)) = matched {
                let after_match = &after_slash[body_len..];
                let after_ok = after_match
                    .chars()
                    .next()
                    .is_none_or(|n| !n.is_alphanumeric() && n != '_');
                if after_ok {
                    out.push_str(&rest[..pos]);
                    out.push_str(&entry.replacement);
                    rest = after_match;
                    continue;
                }
            }
        }
        out.push_str(&rest[..pos + 1]);
        rest = &rest[pos + 1..];
    }
    out.push_str(rest);
    out
}

/// Expands every applicable **spoken snippet** phrase in `text` to its value.
///
/// This is the voice counterpart of [`expand`]: it matches the configured
/// phrase (e.g. "my email") case-insensitively on whole words and replaces it
/// with the replacement. Multiple snippets in one sentence all expand.
pub fn expand_snippets(text: &str, store: &AutoTextStore, app: &ApplicationContext) -> String {
    let snippets = store.snippets_for(app);
    if snippets.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        // Find the earliest snippet occurrence in `rest`.
        let mut best: Option<(usize, &AutoTextEntry, usize)> = None; // (start, entry, end)
        let rest_chars: Vec<char> = rest.chars().collect();
        for entry in &snippets {
            let phrase = entry.snippet_phrase();
            let first = phrase.to_lowercase().chars().next().unwrap();
            let mut search_from = 0usize;
            while let Some(pos) = rest_chars[search_from..]
                .iter()
                .position(|&c| c.to_ascii_lowercase() == first)
            {
                let start = search_from + pos;
                if let Some(end) = match_snippet_at(rest, start, phrase) {
                    let cand = (start, *entry, end);
                    match best {
                        Some((bs, _, _)) if bs <= start => break,
                        _ => best = Some(cand),
                    }
                    break;
                }
                search_from = start + 1;
            }
        }
        match best {
            Some((start, entry, end)) => {
                out.push_str(&rest[..start]);
                out.push_str(&entry.replacement);
                rest = &rest[end..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_triggers() {
        let mut store = AutoTextStore::default();
        store
            .insert(AutoTextEntry::new("/email", "user@example.com"))
            .unwrap();
        store
            .insert(AutoTextEntry::new("/name", "Ali Husain"))
            .unwrap();
        let app = ApplicationContext::unknown();
        let out = expand("hi /name, send to /email", &store, &app);
        assert_eq!(out, "hi Ali Husain, send to user@example.com");
    }

    #[test]
    fn leaves_unknown_triggers_alone() {
        let mut store = AutoTextStore::default();
        store
            .insert(AutoTextEntry::new("/email", "user@example.com"))
            .unwrap();
        let app = ApplicationContext::unknown();
        assert_eq!(expand("keep /unknown", &store, &app), "keep /unknown");
    }

    #[test]
    fn no_entries_is_identity() {
        let store = AutoTextStore::default();
        let app = ApplicationContext::unknown();
        assert_eq!(expand("/email hello", &store, &app), "/email hello");
    }

    fn snippet_store() -> AutoTextStore {
        let mut store = AutoTextStore::default();
        let mut e1 = AutoTextEntry::new("/email", "abcd@gmail.com");
        e1.snippet = "my email".into();
        store.insert(e1).unwrap();
        let mut e2 = AutoTextEntry::new("/linkedin", "https://www.linkedin.com/in/john-doe/");
        e2.snippet = "my linkedin".into();
        store.insert(e2).unwrap();
        store
    }

    #[test]
    fn expands_snippet_phrase() {
        let store = snippet_store();
        let app = ApplicationContext::unknown();
        let out = expand_snippets("my email please", &store, &app);
        assert_eq!(out, "abcd@gmail.com please");
    }

    #[test]
    fn expands_snippet_case_insensitive_and_mid_sentence() {
        let store = snippet_store();
        let app = ApplicationContext::unknown();
        let out = expand_snippets("Send it to MY EMAIL and my LinkedIn thanks", &store, &app);
        assert_eq!(
            out,
            "Send it to abcd@gmail.com and https://www.linkedin.com/in/john-doe/ thanks"
        );
    }

    #[test]
    fn multiple_snippets_in_one_sentence() {
        let store = snippet_store();
        let app = ApplicationContext::unknown();
        let out = expand_snippets("my email, my linkedin", &store, &app);
        assert_eq!(
            out,
            "abcd@gmail.com, https://www.linkedin.com/in/john-doe/"
        );
    }

    #[test]
    fn snippet_not_matched_as_substring() {
        let store = snippet_store();
        let app = ApplicationContext::unknown();
        // "my emailish" should not expand "my email" (emailish != email).
        assert_eq!(expand_snippets("my emailish", &store, &app), "my emailish");
        // A complete phrase followed by another word still matches.
        assert_eq!(
            expand_snippets("my email address", &store, &app),
            "abcd@gmail.com address"
        );
    }

    #[test]
    fn typed_only_entries_not_expanded_as_snippets() {
        let mut store = AutoTextStore::default();
        store.insert(AutoTextEntry::new("/only", "typed")).unwrap(); // no snippet
        let app = ApplicationContext::unknown();
        assert_eq!(expand_snippets("my only words", &store, &app), "my only words");
    }
}
