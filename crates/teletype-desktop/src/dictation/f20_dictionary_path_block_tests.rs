use super::*;
use teletype_core::dictionary::{Dictionary, LearnedChange};
use teletype_core::learning::learn_memory;
use teletype_core::learning::store::LearningStore;

/// The exact predicate `observe_pending_edit` consults before the
/// dictionary write (F-20): a blocked pair is not learned again through
/// the dictionary path.
fn dictionary_path_allows(store: Option<&LearningStore>, from: &str, to: &str) -> bool {
    match store {
        Some(store) => {
            match learn_memory::is_blocked(store, from, to, teletype_core::storage::now_ms()) {
                Ok(true) => false,
                Ok(false) => true,
                Err(_) => true,
            }
        }
        None => true,
    }
}

#[test]
fn second_no_blocks_the_dictionary_learn_path() {
    let dir = tempfile::tempdir().unwrap();
    let store = LearningStore::open(&dir.path().join("learning.sqlite")).unwrap();
    let now = teletype_core::storage::now_ms();
    // The pair is learnable today.
    assert!(dictionary_path_allows(Some(&store), "margets", "Markets"));
    // First "no" (an undo or deletion): the pair is remembered, still
    // learnable.
    let blocked = learn_memory::record_refusal(&store, "margets", "Markets", now).unwrap();
    assert!(!blocked);
    assert!(dictionary_path_allows(Some(&store), "margets", "Markets"));
    // Second "no": the pair is blocked, and the dictionary path stops
    // writing it.
    let blocked = learn_memory::record_refusal(&store, "margets", "Markets", now + 1).unwrap();
    assert!(blocked);
    assert!(!dictionary_path_allows(Some(&store), "margets", "Markets"));
    // A store that is unavailable fails open (learning continues), the
    // same as before F-20.
    assert!(dictionary_path_allows(None, "margets", "Markets"));
}

#[test]
fn dictionary_path_write_is_skipped_for_a_blocked_pair() {
    // The shape of the `observe_pending_edit` loop: the block check runs
    // at the write, before `insert_learned`, so a blocked pair never
    // touches the dictionary.
    let dir = tempfile::tempdir().unwrap();
    let store = LearningStore::open(&dir.path().join("learning.sqlite")).unwrap();
    let now = teletype_core::storage::now_ms();
    learn_memory::record_refusal(&store, "margets", "Markets", now).unwrap();
    learn_memory::record_refusal(&store, "margets", "Markets", now + 1).unwrap();

    let mut dict = Dictionary::default();
    let mut changes = Vec::new();
    let from = "margets";
    let to = "Markets";
    if commands::is_learnable_terminology(from, to)
        && dictionary_path_allows(Some(&store), from, to)
    {
        match dict.insert_learned(from, to) {
            LearnedChange::Unchanged => {}
            change => changes.push(change),
        }
    }
    assert!(changes.is_empty(), "a blocked pair must not be written");
    assert!(dict.find("Markets").is_none());
}
