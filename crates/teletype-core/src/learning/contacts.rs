//! Contacts import (Phase 7.6, core half).
//!
//! macOS Contacts (`CNContactStore`) is read by the desktop bridge; this
//! module is the pure core: the contact record shape plus the upsert that
//! turns contact records into booster terms with `source = "contacts"` and
//! weight 0.7 (plan §7.6). The desktop unit serializes `ContactRecord`s
//! over IPC into `import_contacts_apply`.
//!
//! Per contact, the terms are, in priority order:
//!
//! 1. **Full name** (`given family`, trimmed) — the primary term.
//! 2. **Nickname** — an alias of the full name.
//! 3. **Organization** — its own term (a company is distinct from the
//!    person who works there).
//!
//! A nickname that exactly matches the full name (case-insensitive) is
//! dropped: it carries no disambiguation. A nickname that matches one
//! *word* of the full name (e.g. "Kate" for "Kate Miller") is kept: the
//! ASR often hears only the nickname, and the alias is what the rescoring
//! pass matches on.
//!
//! Upsert policy (tombstone-respecting):
//!
//! - **New term** → `source = "contacts"`, `weight = 0.7`, `created` and
//!   `last_used` = now.
//! - **Existing non-tombstoned row, any source** → the row is kept as-is
//!   (`source`/`weight`/`hits`/`false_alarms`/`created` untouched); only
//!   the contact's aliases are merged in (deduped case-insensitively,
//!   stable order). A manual or learned term is never reclassified as a
//!   contact.
//! - **Existing tombstoned row** → skipped entirely, never un-tombstoned.
//!   Undo (tombstone) is permanent, same as plan §7.4.
//!
//! Idempotent: re-importing the same contacts merges no new aliases and
//! changes no rows.

use super::store::{LexiconRow, LearningStore, StoreResult};

/// The plan §7.5 contacts weight: the row's own `weight` multiplier on top
/// of the 0.7 `contacts` indicator in [`super::boost::row_score`].
pub const CONTACTS_WEIGHT: f64 = 0.7;

/// One contact as the macOS bridge hands it over.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactRecord {
    #[serde(default)]
    pub given_name: String,
    #[serde(default)]
    pub family_name: String,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
}

impl ContactRecord {
    /// The full name (`given family`, trimmed); `None` when both are blank.
    pub fn full_name(&self) -> Option<String> {
        let full = format!("{} {}", self.given_name, self.family_name)
            .trim()
            .to_string();
        if full.is_empty() {
            None
        } else {
            Some(full)
        }
    }

    /// The terms this contact contributes: the full name (with the
    /// nickname as its alias, when distinct) plus the organization as its
    /// own term.
    pub fn terms(&self) -> Vec<ContactTerm> {
        let mut out = Vec::new();
        if let Some(full) = self.full_name() {
            let alias = self
                .nickname
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter(|n| !n.eq_ignore_ascii_case(&full))
                .map(str::to_string);
            out.push(ContactTerm {
                term: full,
                alias,
            });
        }
        if let Some(org) = self
            .organization
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            out.push(ContactTerm {
                term: org.to_string(),
                alias: None,
            });
        }
        out
    }
}

/// One term a contact contributes, with an optional alias (the nickname).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactTerm {
    pub term: String,
    pub alias: Option<String>,
}

/// Outcome of one contacts import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContactsImportCounts {
    /// Terms inserted as new `source = "contacts"` rows.
    pub added: usize,
    /// Existing non-tombstoned rows that gained at least one new alias.
    pub aliases_added: usize,
    /// Terms already present (no alias change) — re-import idempotency.
    pub unchanged: usize,
    /// Terms skipped because their row is tombstoned.
    pub tombstoned_skipped: usize,
}

/// Upserts `contacts` into the lexicon as `source = "contacts"` rows at
/// [`CONTACTS_WEIGHT`]. See the module docs for the per-term policy.
///
/// `now_ms` is injected so the function is deterministic and testable.
pub fn import_contacts(
    store: &LearningStore,
    contacts: &[ContactRecord],
    now_ms: u64,
) -> StoreResult<ContactsImportCounts> {
    let mut counts = ContactsImportCounts::default();
    for contact in contacts {
        for ct in contact.terms() {
            let existing = store.lexicon_by_term(&ct.term)?;
            let Some(mut row) = existing else {
                let mut aliases = Vec::new();
                if let Some(alias) = &ct.alias {
                    aliases.push(alias.clone());
                }
                let row = LexiconRow {
                    term: ct.term.clone(),
                    aliases,
                    source: "contacts".into(),
                    weight: CONTACTS_WEIGHT,
                    hits: 0,
                    false_alarms: 0,
                    created: now_ms,
                    last_used: Some(now_ms),
                    tombstoned: false,
                    app_scope: None,
                };
                store.upsert_lexicon(&row)?;
                counts.added += 1;
                continue;
            };
            if row.tombstoned {
                // Undo is permanent: a contacts re-import never
                // resurrects a tombstoned term.
                counts.tombstoned_skipped += 1;
                continue;
            }
            match ct.alias.as_deref() {
                Some(alias) if !row
                    .aliases
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(alias)) =>
                {
                    row.aliases.push(alias.to_string());
                    store.upsert_lexicon(&row)?;
                    counts.aliases_added += 1;
                }
                _ => {
                    counts.unchanged += 1;
                }
            }
        }
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> (tempfile::TempDir, LearningStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open(&dir.path().join("learning.sqlite")).unwrap();
        (dir, store)
    }

    fn contact(
        given: &str,
        family: &str,
        nickname: Option<&str>,
        org: Option<&str>,
    ) -> ContactRecord {
        ContactRecord {
            given_name: given.into(),
            family_name: family.into(),
            nickname: nickname.map(str::to_string),
            organization: org.map(str::to_string),
        }
    }

    fn term_row(term: &str, source: &str, weight: f64) -> LexiconRow {
        LexiconRow {
            term: term.into(),
            aliases: vec![],
            source: source.into(),
            weight,
            hits: 3,
            false_alarms: 0,
            created: 100,
            last_used: Some(200),
            tombstoned: false,
            app_scope: None,
        }
    }

    #[test]
    fn terms_emits_full_name_nickname_alias_and_org() {
        let c = contact("Siobhan", "O'Brien", Some("Sio"), Some("Acme Corp"));
        let terms = c.terms();
        assert_eq!(
            terms,
            vec![
                ContactTerm {
                    term: "Siobhan O'Brien".into(),
                    // Distinct nickname → alias.
                    alias: Some("Sio".into()),
                },
                ContactTerm {
                    term: "Acme Corp".into(),
                    alias: None,
                },
            ]
        );
    }

    #[test]
    fn terms_nickname_dropped_when_it_equals_the_full_name() {
        let c = contact("Siobhan", "O'Brien", Some("siobhan o'brien"), Some("Acme Corp"));
        let terms = c.terms();
        assert_eq!(
            terms,
            vec![
                ContactTerm {
                    term: "Siobhan O'Brien".into(),
                    alias: None,
                },
                ContactTerm {
                    term: "Acme Corp".into(),
                    alias: None,
                },
            ]
        );
    }

    #[test]
    fn terms_nickname_kept_when_one_word_of_the_full_name_case_insensitive() {
        let c = contact("Kate", "Miller", Some("kate"), None);
        let terms = c.terms();
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].term, "Kate Miller");
        assert_eq!(terms[0].alias.as_deref(), Some("kate"));
    }

    #[test]
    fn terms_blank_fields_yield_nothing() {
        assert!(contact("  ", "", None, Some(" ")).terms().is_empty());
        assert!(contact("", "", None, None).terms().is_empty());
    }

    #[test]
    fn imports_new_contacts_as_contacts_source_weight_0_7() {
        let (_dir, store) = tmp_store();
        let counts = import_contacts(
            &store,
            &[contact("Siobhan", "O'Brien", Some("Sio"), Some("Acme Corp"))],
            5_000,
        )
        .unwrap();
        assert_eq!(
            counts,
            ContactsImportCounts {
                added: 2,
                aliases_added: 0,
                unchanged: 0,
                tombstoned_skipped: 0,
            }
        );
        let row = store.lexicon_by_term("Siobhan O'Brien").unwrap().unwrap();
        assert_eq!(row.source, "contacts");
        assert!((row.weight - 0.7).abs() < f64::EPSILON, "weight is 0.7");
        assert_eq!(row.aliases, vec!["Sio".to_string()]);
        assert_eq!(row.created, 5_000);
        assert_eq!(row.last_used, Some(5_000));
        assert!(!row.tombstoned);
        // Organization is its own term.
        let org = store.lexicon_by_term("Acme Corp").unwrap().unwrap();
        assert_eq!(org.source, "contacts");
        assert!(org.aliases.is_empty());
    }

    #[test]
    fn reimport_is_idempotent() {
        let (_dir, store) = tmp_store();
        let contacts = [contact("Kate", "Miller", Some("K"), None)];
        let first = import_contacts(&store, &contacts, 1_000).unwrap();
        assert_eq!(first.added, 1);
        let second = import_contacts(&store, &contacts, 2_000).unwrap();
        assert_eq!(
            second,
            ContactsImportCounts {
                added: 0,
                aliases_added: 0,
                unchanged: 1,
                tombstoned_skipped: 0,
            }
        );
        assert_eq!(store.lexicon_all().unwrap().len(), 1);
        // Alias kept, not duplicated.
        let row = store.lexicon_by_term("Kate Miller").unwrap().unwrap();
        assert_eq!(row.aliases, vec!["K".to_string()]);
    }

    #[test]
    fn existing_manual_row_keeps_source_and_weight_gains_alias() {
        let (_dir, store) = tmp_store();
        let mut manual = term_row("Kate Miller", "manual", 2.0);
        manual.aliases = vec!["kate-m".into()];
        store.upsert_lexicon(&manual).unwrap();

        let counts =
            import_contacts(&store, &[contact("Kate", "Miller", Some("KT"), None)], 9_000)
                .unwrap();
        assert_eq!(
            counts,
            ContactsImportCounts {
                added: 0,
                aliases_added: 1,
                unchanged: 0,
                tombstoned_skipped: 0,
            }
        );
        let row = store.lexicon_by_term("Kate Miller").unwrap().unwrap();
        assert_eq!(row.source, "manual", "existing row keeps its source");
        assert!((row.weight - 2.0).abs() < f64::EPSILON, "existing row keeps its weight");
        assert_eq!(row.created, 100, "existing row keeps its created");
        assert_eq!(row.hits, 3);
        // Old alias preserved, new alias appended.
        assert_eq!(row.aliases, vec!["kate-m".to_string(), "KT".to_string()]);
    }

    #[test]
    fn tombstoned_row_is_skipped_and_never_untombstoned() {
        let (_dir, store) = tmp_store();
        let mut row = term_row("Kate Miller", "contacts", CONTACTS_WEIGHT);
        row.tombstoned = true;
        store.upsert_lexicon(&row).unwrap();

        let counts =
            import_contacts(&store, &[contact("Kate", "Miller", Some("K"), None)], 9_000)
                .unwrap();
        assert_eq!(
            counts,
            ContactsImportCounts {
                added: 0,
                aliases_added: 0,
                unchanged: 0,
                tombstoned_skipped: 1,
            }
        );
        let row = store.lexicon_by_term("Kate Miller").unwrap().unwrap();
        assert!(row.tombstoned, "tombstone is permanent");
        assert!(row.aliases.is_empty(), "no alias merged into a tombstoned row");
    }

    #[test]
    fn duplicate_terms_within_contacts_deduped() {
        let (_dir, store) = tmp_store();
        // Contact #1: full name "Kate Miller", no org.
        // Contact #2: full name "K Miller" (nickname "K" + family "Miller"),
        // org "Miller LLC".
        // Expected: "Kate Miller" added, "K Miller" added, "Miller LLC"
        // added → 3 terms, no aliases merged.
        let contacts = [
            contact("Kate", "Miller", None, None),
            contact("", "Miller", Some("K"), Some("Miller LLC")),
        ];
        let counts = import_contacts(&store, &contacts, 1_000).unwrap();
        assert_eq!(counts.added, 3);
        assert_eq!(counts.aliases_added, 0);
        assert_eq!(store.lexicon_all().unwrap().len(), 3);
        // Re-import: all three terms unchanged.
        let counts2 = import_contacts(&store, &contacts, 2_000).unwrap();
        assert_eq!(counts2.added, 0);
        assert_eq!(counts2.unchanged, 3);
        assert_eq!(counts2.aliases_added, 0);
    }

    #[test]
    fn alias_merge_is_case_insensitive() {
        let (_dir, store) = tmp_store();
        import_contacts(&store, &[contact("Kate", "Miller", Some("kate"), None)], 1_000)
            .unwrap();
        // Same alias, different case: not re-added.
        let counts =
            import_contacts(&store, &[contact("Kate", "Miller", Some("KATE"), None)], 2_000)
                .unwrap();
        assert_eq!(counts.unchanged, 1);
        assert_eq!(counts.aliases_added, 0);
        let row = store.lexicon_by_term("Kate Miller").unwrap().unwrap();
        assert_eq!(row.aliases, vec!["kate".to_string()]);
    }
}
