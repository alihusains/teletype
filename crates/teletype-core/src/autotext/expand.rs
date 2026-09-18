//! Direct expansion: turn `/trigger` occurrences into their values.
//!
//! Used for typed input (no AI involved) and as the post-transform step when a
//! transform ran. Unlike [`super::protect`], this replaces the trigger with
//! the value directly.

use super::{AutoTextEntry, AutoTextStore};
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
}
