//! Protecting AutoText values around AI transforms.
//!
//! `protect` replaces each `/trigger` occurrence with a `{{AUTOTEXT_n}}`
//! placeholder; `restore` puts the exact configured values back. Placeholders
//! survive the model round-trip because the prompt tells the model to keep
//! them verbatim — and even if the model mangles one, the value the user
//! configured is authoritative.

use std::collections::HashMap;

use super::{match_snippet_at, placeholders, AutoTextEntry, AutoTextStore};
use crate::context::ApplicationContext;

/// The protected form of a text containing AutoText triggers.
#[derive(Debug, Clone, Default)]
pub struct ProtectedText {
    /// The text with triggers replaced by placeholders.
    pub text: String,
    /// Placeholder index → exact replacement value.
    pub values: HashMap<usize, String>,
}

impl ProtectedText {
    pub fn has_placeholders(&self) -> bool {
        !self.values.is_empty()
    }
}

/// Scans `text` for triggers from `store` that apply in `app`, and replaces
/// them with placeholders.
///
/// Triggers are matched as whole tokens: the character before the `/` must be
/// start-of-text or whitespace, and the character after the trigger must be
/// whitespace or end-of-text. This avoids eating `/` inside URLs or words.
pub fn protect(text: &str, store: &AutoTextStore, app: &ApplicationContext) -> ProtectedText {
    // Longest trigger first so `/my-email` wins over `/my`.
    let mut applicable: Vec<&AutoTextEntry> =
        store.entries.iter().filter(|e| e.applies_to(app)).collect();
    applicable.sort_by_key(|e| std::cmp::Reverse(e.trigger.len()));

    let mut out = String::with_capacity(text.len());
    let mut values = HashMap::new();
    let mut rest = text;
    while let Some(pos) = rest.find('/') {
        // Preceding char must be start-of-text or whitespace.
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
            // Match the trigger body (without leading `/`) against after_slash.
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
                    let n = values.len();
                    values.insert(n, placeholders::expand_placeholders(&entry.replacement));
                    out.push_str(&format!("{{{{AUTOTEXT_{n}}}}}"));
                    rest = after_match;
                    continue;
                }
            }
        }
        // No match at this `/`; keep it and continue after it.
        out.push_str(&rest[..pos + 1]);
        rest = &rest[pos + 1..];
    }
    out.push_str(rest);

    ProtectedText { text: out, values }
}

/// Protects every applicable **spoken snippet** phrase in `text` with a
/// placeholder, so the value survives an AI transform. The voice counterpart
/// of [`protect`]: matches the configured phrase case-insensitively on whole
/// words.
pub fn protect_snippets(
    text: &str,
    store: &AutoTextStore,
    app: &ApplicationContext,
) -> ProtectedText {
    protect_snippets_with(text, store, app, &[])
}

/// Like [`protect_snippets`] but also protects the built-in System entries
/// (`system`), with custom entries overriding System ones.
pub fn protect_snippets_with(
    text: &str,
    store: &AutoTextStore,
    app: &ApplicationContext,
    system: &[AutoTextEntry],
) -> ProtectedText {
    let snippets = super::expand::combined_snippets(store, app, system);
    if snippets.is_empty() {
        return ProtectedText::default();
    }

    let mut out = String::with_capacity(text.len());
    let mut values = HashMap::new();
    let mut rest = text;
    loop {
        let mut best: Option<(usize, &AutoTextEntry, usize)> = None;
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
                let mut trim_left = false;
                let mut trim_right = false;
                if entry.system {
                    match super::system::spacing_for(entry.snippet_phrase()) {
                        super::system::Spacing::AttachLeft | super::system::Spacing::AttachBoth => {
                            trim_left = true
                        }
                        super::system::Spacing::AttachRight
                        | super::system::Spacing::AttachBoth => trim_right = true,
                        super::system::Spacing::Normal => {}
                    }
                }
                if entry.replacement.contains('\n') {
                    trim_left = true;
                    trim_right = true;
                }
                let before = &rest[..start];
                let after = &rest[end..];
                let before_out = if trim_left { before.trim_end() } else { before };
                let after_rest = if trim_right {
                    after.trim_start()
                } else {
                    after
                };
                out.push_str(before_out);
                let n = values.len();
                values.insert(n, placeholders::expand_placeholders(&entry.replacement));
                out.push_str(&format!("{{{{AUTOTEXT_{n}}}}}"));
                rest = after_rest;
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    ProtectedText { text: out, values }
}

/// Restores placeholders in `text` with the values in `protected`.
///
/// A placeholder the model dropped or mangled simply stays absent — the
/// surrounding text is returned as-is. The original trigger is not re-inserted
/// (the expansion already happened conceptually), which matches
/// "if expansion fails, preserve the original text gracefully".
pub fn restore(text: &str, protected: &ProtectedText) -> String {
    if protected.values.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("{{AUTOTEXT_") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos + "{{AUTOTEXT_".len()..];
        let digits_end = tail
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(tail.len());
        let digits = &tail[..digits_end];
        if let (Ok(n), Some(c)) = (digits.parse::<usize>(), tail.get(digits_end..)) {
            // Placeholder format: {{AUTOTEXT_n}} — two closing braces.
            if let Some(after_braces) = c.strip_prefix("}}") {
                if let Some(value) = protected.values.get(&n) {
                    out.push_str(value);
                    rest = after_braces;
                    continue;
                }
            }
        }
        // Not a valid placeholder; keep it literally.
        out.push_str(&rest[pos..pos + "{{AUTOTEXT_".len()]);
        rest = &rest[pos + "{{AUTOTEXT_".len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(email: &str) -> AutoTextStore {
        let mut store = AutoTextStore::default();
        store.insert(AutoTextEntry::new("/email", email)).unwrap();
        store
    }

    #[test]
    fn replaces_whole_token_triggers() {
        let store = store("user@example.com");
        let app = ApplicationContext::unknown();
        let p = protect("send this to /email and cc /email2", &store, &app);
        assert_eq!(p.text, "send this to {{AUTOTEXT_0}} and cc /email2");
        assert_eq!(p.values.get(&0), Some(&"user@example.com".to_string()));
    }

    #[test]
    fn does_not_eat_slashes_in_urls_or_words() {
        let store = store("user@example.com");
        let app = ApplicationContext::unknown();
        let p = protect("see https://example.com/email or /emailish", &store, &app);
        assert_eq!(p.text, "see https://example.com/email or /emailish");
        assert!(p.values.is_empty());
    }

    #[test]
    fn multiple_occurrences_get_distinct_placeholders() {
        let store = store("user@example.com");
        let app = ApplicationContext::unknown();
        let p = protect("/email and /email", &store, &app);
        assert_eq!(p.text, "{{AUTOTEXT_0}} and {{AUTOTEXT_1}}");
        assert_eq!(p.values.len(), 2);
    }

    #[test]
    fn restore_puts_exact_values_back() {
        let store = store("user@example.com");
        let app = ApplicationContext::unknown();
        let p = protect("send it to /email", &store, &app);
        // Simulate the model returning the text with the placeholder intact.
        let restored = restore(&p.text, &p);
        assert_eq!(restored, "send it to user@example.com");
    }

    #[test]
    fn restore_survives_mangled_placeholder() {
        let store = store("user@example.com");
        let app = ApplicationContext::unknown();
        let p = protect("send it to /email", &store, &app);
        // Model dropped the placeholder entirely — no crash, text preserved.
        let restored = restore("send it to ", &p);
        assert_eq!(restored, "send it to ");
    }

    #[test]
    fn protects_snippet_phrases() {
        let mut store = AutoTextStore::default();
        let mut e = AutoTextEntry::new("/email", "abcd@gmail.com");
        e.snippet = "my email".into();
        store.insert(e).unwrap();
        let app = ApplicationContext::unknown();
        let p = protect_snippets("send to my email please", &store, &app);
        assert_eq!(p.text, "send to {{AUTOTEXT_0}} please");
        assert_eq!(p.values.get(&0), Some(&"abcd@gmail.com".to_string()));
    }

    #[test]
    fn protect_and_restore_roundtrip_snippet() {
        let mut store = AutoTextStore::default();
        let mut e = AutoTextEntry::new("/email", "abcd@gmail.com");
        e.snippet = "my email".into();
        store.insert(e).unwrap();
        let app = ApplicationContext::unknown();
        let p = protect_snippets("my email", &store, &app);
        assert_eq!(restore(&p.text, &p), "abcd@gmail.com");
    }

    #[test]
    fn scoped_entry_not_protected_in_other_app() {
        let mut store = AutoTextStore::default();
        let mut entry = AutoTextEntry::new("/sig", "Best regards, Ali");
        entry.scope = crate::autotext::AutoTextScope::Application("com.google.gmail".into());
        store.insert(entry).unwrap();

        let gmail = ApplicationContext {
            application_id: "com.google.gmail".into(),
            application_name: "Gmail".into(),
            ..Default::default()
        };
        let other = ApplicationContext {
            application_id: "Slack".into(),
            application_name: "Slack".into(),
            ..Default::default()
        };
        assert!(protect("bye /sig", &store, &gmail).has_placeholders());
        assert!(!protect("bye /sig", &store, &other).has_placeholders());
    }
}
