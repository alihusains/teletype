use super::*;

/// The pure predicate `clear_undo_offer_if` applies to the live offer
/// (F-84): an offer id names the live offer only while it is the one in
/// the slot.
fn still_live(offer: Option<&commands::LearnedOffer>, offer_id: u64) -> bool {
    offer.is_some_and(|o| o.matches_offer_id(offer_id))
}

fn offer(id: u64, pref_ids: Vec<String>) -> commands::LearnedOffer {
    commands::LearnedOffer {
        id,
        preference_ids: pref_ids,
        dictionary_changes: Vec::new(),
        lexicon_terms: Vec::new(),
    }
}

#[test]
fn expiry_clears_only_its_own_offer() {
    // Offer 1 is live.
    let live = offer(1, vec!["p1".into()]);
    assert!(still_live(Some(&live), 1));
    // A newer learn overwrites the slot (the real path replaces the offer).
    let live2 = offer(2, vec!["p2".into()]);
    // Offer 1's expiry timer fires: it no longer names the live offer,
    // so it must leave offer 2 alone.
    assert!(!still_live(Some(&live2), 1));
    assert!(still_live(Some(&live2), 2));
    // No live offer at all: an expired id clears nothing.
    assert!(!still_live(None, 2));
}

#[test]
fn stale_undo_is_distinguished_from_applied() {
    // The `undo_offer_from_state` contract the `undo_learned` command
    // maps onto `UndoOutcomeKind`: a live offer applies, an offer that is
    // gone (undone or expired) is stale, and never-offered is none
    // (F-84). The command's mapping is `Some => Applied`, `None => Stale`.
    let offer = commands::LearnedOffer {
        id: 7,
        preference_ids: vec!["p1".into()],
        dictionary_changes: Vec::new(),
        lexicon_terms: Vec::new(),
    };
    // The ids name the live offer: the undo applies.
    assert!(offer.matches(&["p1".into()]));
    // The ids name an older offer: nothing is touched, and the command
    // reports stale instead of closing as if it worked.
    assert!(!offer.matches(&["p0".into()]));
    // An empty ids list (nothing offered) is not a match either:
    // the tray path maps that to `None`, the toast path to `Stale`.
    assert!(!offer.matches(&[]));
}
