//! Contacts import IPC commands (Phase 7.6, desktop half).
//!
//! Two Tauri commands:
//! - `list_contacts`: fetches all contacts from macOS Contacts via the
//!   `contacts_import.m` bridge and returns them as `Vec<ContactRecord>`.
//! - `import_contacts_apply`: takes `Vec<ContactRecord>` over IPC and calls
//!   `teletype_core::learning::contacts::import_contacts` to upsert booster
//!   terms with `source = "contacts"`, weight 0.7.

use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::AppState;

type CommandResult<T> = Result<T, String>;

/// The path to the learning store, relative to the app data directory.
fn learning_store_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("learning").join("learning.sqlite")
}

// ---- list_contacts ----

/// Parses a JSON array of contact objects into `Vec<ContactRecord>`.
///
/// This is the pure parse layer extracted from the FFI path so it can be
/// unit-tested without triggering a TCC prompt.
fn parse_contacts_json(
    json: &str,
) -> Result<Vec<teletype_core::learning::contacts::ContactRecord>, String> {
    serde_json::from_str(json).map_err(|e| format!("Failed to parse contacts JSON: {e}"))
}

/// Fetches all contacts from the macOS Contacts database.
///
/// On macOS: calls the `contacts_import.m` bridge (which requests TCC access
/// and returns a malloc'd JSON string). On other platforms: returns an error.
#[tauri::command]
pub fn list_contacts() -> CommandResult<Vec<teletype_core::learning::contacts::ContactRecord>> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn teletype_contacts_fetch() -> *mut std::os::raw::c_char;
            fn teletype_contacts_free(ptr: *mut std::os::raw::c_char);
        }

        // SAFETY: The C function returns a malloc'd UTF-8 string or NULL.
        // We take ownership and free it after conversion.
        let ptr = unsafe { teletype_contacts_fetch() };
        if ptr.is_null() {
            return Err(
                "Contacts access denied — enable it in System Settings → Privacy & Security → Contacts"
                    .into(),
            );
        }

        // SAFETY: `ptr` is non-null (checked above) and points to a valid C
        // string owned by us until we free it below; no aliasing of the
        // buffer in the meantime.
        let c_str = unsafe { std::ffi::CStr::from_ptr(ptr) };
        let json_str = c_str
            .to_str()
            .map_err(|e| format!("Contacts JSON is not valid UTF-8: {e}"))?;

        // Free the C buffer before parsing (the parse is on the Rust string).
        // SAFETY: `ptr` was allocated by `teletype_contacts_fetch` and is
        // freed here exactly once, immediately after its last use.
        unsafe { teletype_contacts_free(ptr) };

        parse_contacts_json(json_str)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Contacts import is only supported on macOS".into())
    }
}

// ---- import_contacts_apply ----

/// The import counts returned to the UI after applying contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactsImportResult {
    pub added: usize,
    pub aliases_added: usize,
    pub unchanged: usize,
    pub tombstoned_skipped: usize,
}

/// Applies a batch of contact records to the learning store as booster terms.
///
/// Opens the learning store, calls `import_contacts`, and returns the counts.
/// The store is opened and closed per call (same pattern as `learning_undo_term`).
#[tauri::command]
pub fn import_contacts_apply(
    state: State<'_, AppState>,
    contacts: Vec<teletype_core::learning::contacts::ContactRecord>,
) -> CommandResult<ContactsImportResult> {
    let store =
        teletype_core::learning::store::LearningStore::open(&learning_store_path(&state.data_dir))
            .map_err(|e| e.to_string())?;

    let now_ms = teletype_core::storage::now_ms();
    let counts = teletype_core::learning::contacts::import_contacts(&store, &contacts, now_ms)
        .map_err(|e| e.to_string())?;

    Ok(ContactsImportResult {
        added: counts.added,
        aliases_added: counts.aliases_added,
        unchanged: counts.unchanged,
        tombstoned_skipped: counts.tombstoned_skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use teletype_core::learning::contacts::{ContactRecord, CONTACTS_WEIGHT};
    use teletype_core::learning::store::LearningStore;

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

    // ---- parse_contacts_json tests ----

    #[test]
    fn parse_contacts_json_valid() {
        let json = r#"[
            {"givenName":"John","familyName":"Doe","nickname":null,"organization":"Acme"},
            {"givenName":"Jane","familyName":"Smith","nickname":"Jay","organization":null}
        ]"#;
        let records = parse_contacts_json(json).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].given_name, "John");
        assert_eq!(records[0].family_name, "Doe");
        assert_eq!(records[0].nickname, None);
        assert_eq!(records[0].organization.as_deref(), Some("Acme"));
        assert_eq!(records[1].nickname.as_deref(), Some("Jay"));
        assert_eq!(records[1].organization, None);
    }

    #[test]
    fn parse_contacts_json_empty_array() {
        let records = parse_contacts_json("[]").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn parse_contacts_json_invalid() {
        assert!(parse_contacts_json("not json").is_err());
        assert!(parse_contacts_json("{}").is_err());
    }

    // ---- import_contacts_apply tests (via the core function directly) ----

    #[test]
    fn import_empty_input_returns_zero_counts() {
        let (_dir, store) = tmp_store();
        let counts =
            teletype_core::learning::contacts::import_contacts(&store, &[], 1_000).unwrap();
        assert_eq!(counts.added, 0);
        assert_eq!(counts.aliases_added, 0);
        assert_eq!(counts.unchanged, 0);
        assert_eq!(counts.tombstoned_skipped, 0);
    }

    #[test]
    fn import_two_contacts_creates_rows_with_correct_source_and_weight() {
        let (_dir, store) = tmp_store();
        let contacts = vec![
            contact("Alice", "Wong", Some("Al"), Some("Globex")),
            contact("Bob", "Chen", None, None),
        ];
        let counts =
            teletype_core::learning::contacts::import_contacts(&store, &contacts, 5_000).unwrap();
        // "Alice Wong" + "Globex" + "Bob Chen" = 3 terms added.
        assert_eq!(counts.added, 3);

        let row = store.lexicon_by_term("Alice Wong").unwrap().unwrap();
        assert_eq!(row.source, "contacts");
        assert!((row.weight - CONTACTS_WEIGHT).abs() < f64::EPSILON);
        assert_eq!(row.aliases, vec!["Al".to_string()]);

        let org = store.lexicon_by_term("Globex").unwrap().unwrap();
        assert_eq!(org.source, "contacts");
        assert!((org.weight - CONTACTS_WEIGHT).abs() < f64::EPSILON);

        let bob = store.lexicon_by_term("Bob Chen").unwrap().unwrap();
        assert_eq!(bob.source, "contacts");
        assert!(bob.aliases.is_empty());
    }

    #[test]
    fn import_idempotent_rerun_no_changes() {
        let (_dir, store) = tmp_store();
        let contacts = vec![contact("Kate", "Miller", Some("K"), None)];

        let first =
            teletype_core::learning::contacts::import_contacts(&store, &contacts, 1_000).unwrap();
        assert_eq!(first.added, 1);

        let second =
            teletype_core::learning::contacts::import_contacts(&store, &contacts, 2_000).unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.aliases_added, 0);
        assert_eq!(second.unchanged, 1);
        assert_eq!(second.tombstoned_skipped, 0);
    }

    #[test]
    fn import_tombstoned_term_stays_tombstoned() {
        let (_dir, store) = tmp_store();
        // Pre-seed a tombstoned row.
        let row = teletype_core::learning::store::LexiconRow {
            term: "Kate Miller".into(),
            aliases: vec![],
            source: "contacts".into(),
            weight: CONTACTS_WEIGHT,
            hits: 0,
            false_alarms: 0,
            created: 100,
            last_used: Some(200),
            tombstoned: true,
            app_scope: None,
            learned_from: None,
        };
        store.upsert_lexicon(&row).unwrap();

        let counts = teletype_core::learning::contacts::import_contacts(
            &store,
            &[contact("Kate", "Miller", Some("K"), None)],
            9_000,
        )
        .unwrap();
        assert_eq!(counts.tombstoned_skipped, 1);
        assert_eq!(counts.added, 0);

        let row = store.lexicon_by_term("Kate Miller").unwrap().unwrap();
        assert!(row.tombstoned, "tombstone is permanent");
    }
}
