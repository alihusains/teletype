//! Direct expansion: turn `/trigger` occurrences into their values.
//!
//! Used for typed input (no AI involved) and as the post-transform step when a
//! transform ran. Unlike [`super::protect`], this replaces the trigger with
//! the value directly.

use super::{match_snippet_at, system, AutoTextEntry, AutoTextStore};
use crate::context::ApplicationContext;

/// Combines custom snippets (from `store`) with System snippets, with custom
/// entries overriding System entries that share the same phrase. Longest
/// phrase first so multi-word triggers win over their prefixes.
pub(crate) fn combined_snippets<'a>(
    store: &'a AutoTextStore,
    app: &ApplicationContext,
    system: &'a [AutoTextEntry],
) -> Vec<&'a AutoTextEntry> {
    let custom = store.snippets_for(app);
    // Phrases already covered by a custom entry (custom wins).
    let custom_phrases: std::collections::HashSet<String> = custom
        .iter()
        .map(|e| e.snippet_phrase().to_lowercase())
        .collect();
    let mut v: Vec<&AutoTextEntry> = custom;
    for e in system {
        if !e.applies_to(app) {
            continue;
        }
        let phrase = e.snippet_phrase();
        if phrase.is_empty() || custom_phrases.contains(&phrase.to_lowercase()) {
            continue;
        }
        v.push(e);
    }
    v.sort_by_key(|e| std::cmp::Reverse(e.snippet_phrase().len()));
    v
}

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
    expand_snippets_with(text, store, app, &[])
}

/// Like [`expand_snippets`] but also applies the built-in System entries
/// (`system`), with custom entries overriding System ones.
pub fn expand_snippets_with(
    text: &str,
    store: &AutoTextStore,
    app: &ApplicationContext,
    system: &[AutoTextEntry],
) -> String {
    let snippets = combined_snippets(store, app, system);
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
                // Determine spacing: System entries carry an explicit rule; a
                // replacement containing a newline trims both sides (a clean
                // line break); anything else keeps normal spacing.
                let mut trim_left = false;
                let mut trim_right = false;
                if entry.system {
                    match system::spacing_for(entry.snippet_phrase()) {
                        system::Spacing::AttachLeft | system::Spacing::AttachBoth => trim_left = true,
                        system::Spacing::AttachRight | system::Spacing::AttachBoth => trim_right = true,
                        system::Spacing::Normal => {}
                    }
                }
                if entry.replacement.contains('\n') {
                    trim_left = true;
                    trim_right = true;
                }
                let before = &rest[..start];
                let after = &rest[end..];
                let before_out = if trim_left { before.trim_end() } else { before };
                let after_rest = if trim_right { after.trim_start() } else { after };
                out.push_str(before_out);
                out.push_str(&entry.replacement);
                rest = after_rest;
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

    // ---- System AutoText ----

    fn sys() -> &'static [AutoTextEntry] {
        super::super::system::entries()
    }
    fn empty() -> AutoTextStore {
        AutoTextStore::default()
    }
    fn app() -> ApplicationContext {
        ApplicationContext::unknown()
    }

    #[test]
    fn system_punctuation_comma() {
        assert_eq!(
            expand_snippets_with("hello comma world", &empty(), &app(), sys()),
            "hello, world"
        );
    }

    #[test]
    fn system_punctuation_question() {
        assert_eq!(
            expand_snippets_with("are you coming question mark", &empty(), &app(), sys()),
            "are you coming?"
        );
    }

    #[test]
    fn system_punctuation_exclamation() {
        assert_eq!(
            expand_snippets_with("this is great exclamation mark", &empty(), &app(), sys()),
            "this is great!"
        );
    }

    #[test]
    fn system_new_line() {
        assert_eq!(
            expand_snippets_with("hello new line world", &empty(), &app(), sys()),
            "hello\nworld"
        );
    }

    #[test]
    fn system_new_paragraph() {
        assert_eq!(
            expand_snippets_with("one new paragraph two", &empty(), &app(), sys()),
            "one\n\ntwo"
        );
    }

    #[test]
    fn system_colon_and_semicolon() {
        assert_eq!(
            expand_snippets_with("hello colon world", &empty(), &app(), sys()),
            "hello: world"
        );
        assert_eq!(
            expand_snippets_with("hello semicolon world", &empty(), &app(), sys()),
            "hello; world"
        );
    }

    #[test]
    fn system_multi_word_triggers() {
        let a = app();
        // Opening/closing marks attach to their content: "(hi)" not "( hi )".
        assert_eq!(
            expand_snippets_with("open parenthesis hi close parenthesis", &empty(), &a, sys()),
            "(hi)"
        );
        assert_eq!(
            expand_snippets_with("open square bracket x close square bracket", &empty(), &a, sys()),
            "[x]"
        );
        // "exclamation point" must match as one trigger, not "exclamation" alone.
        assert_eq!(
            expand_snippets_with("wow exclamation point", &empty(), &a, sys()),
            "wow!"
        );
        // "full stop" is one trigger.
        assert_eq!(
            expand_snippets_with("end full stop", &empty(), &a, sys()),
            "end."
        );
    }

    #[test]
    fn system_symbols_and_math() {
        let a = app();
        // "at sign" is Normal spacing (you don't glue an email by saying it).
        assert_eq!(
            expand_snippets_with("email at sign example dot com", &empty(), &a, sys()),
            "email @ example dot com"
        );
        assert_eq!(
            expand_snippets_with("two plus two equals four", &empty(), &a, sys()),
            "two + two = four"
        );
        assert_eq!(
            expand_snippets_with("a and and b or or c", &empty(), &a, sys()),
            "a && b || c"
        );
    }

    #[test]
    fn system_no_space_before_terminal_punct() {
        // "hello question mark" -> "hello?" (no space before ?).
        assert_eq!(
            expand_snippets_with("hello question mark", &empty(), &app(), sys()),
            "hello?"
        );
        // Multiple in a row.
        assert_eq!(
            expand_snippets_with("are you sure question mark are you sure exclamation mark", &empty(), &app(), sys()),
            "are you sure? are you sure!"
        );
    }

    #[test]
    fn custom_overrides_system_same_phrase() {
        // System has "comma" -> ",". A custom entry with the same phrase wins.
        let mut store = AutoTextStore::default();
        let mut e = AutoTextEntry::new("/comma", "CUSTOM");
        e.snippet = "comma".into();
        store.insert(e).unwrap();
        let a = app();
        assert_eq!(
            expand_snippets_with("hello comma world", &store, &a, sys()),
            "hello CUSTOM world"
        );
    }

    #[test]
    fn custom_and_system_coexist() {
        // Custom "my email" + System "comma" both expand in one pass.
        let store = snippet_store(); // has "my email"
        let a = app();
        assert_eq!(
            expand_snippets_with("my email comma my linkedin", &store, &a, sys()),
            "abcd@gmail.com, https://www.linkedin.com/in/john-doe/"
        );
    }
}
