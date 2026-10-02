//! The clipboard must never lose the user's data.
//!
//! `arboard` can only read text and images, and both `set_text` and
//! `set_image` replace the entire pasteboard. The injector used to answer
//! "what was on the clipboard?" with "text, or an image, or nothing", and on
//! the third answer it called `clear()`. That is how copying a file in Finder
//! and then dictating destroyed the copied file.
//!
//! These tests pin the rules the injector now follows, using a fake guard so
//! the behaviour is observable without a real pasteboard.

use teletype_core::injector::{
    pack_raw_image, unpack_raw_image, ClipboardGuard, ClipboardPart, ClipboardSnapshot,
};

/// Records what it was asked to do, and stands in for the pasteboard.
struct RecordingGuard {
    snapshot: ClipboardSnapshot,
    restored: std::sync::Mutex<Vec<(usize, Option<String>)>>,
}

impl RecordingGuard {
    fn with(items: Vec<Vec<ClipboardPart>>) -> Self {
        let mut flat = Vec::new();
        for i in &items {
            flat.extend(i.iter().cloned());
        }
        Self {
            snapshot: ClipboardSnapshot { parts: flat, items },
            restored: std::sync::Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> Vec<(usize, Option<String>)> {
        self.restored.lock().unwrap().clone()
    }
}

impl ClipboardGuard for RecordingGuard {
    fn snapshot(&self) -> Option<ClipboardSnapshot> {
        Some(self.snapshot.clone())
    }
    fn restore(&self, snapshot: &ClipboardSnapshot, extra: Option<&str>) {
        self.restored
            .lock()
            .unwrap()
            .push((snapshot.items.len(), extra.map(str::to_string)));
    }
}

fn part(uti: &str, data: &[u8]) -> ClipboardPart {
    ClipboardPart {
        uti: uti.into(),
        data: data.to_vec(),
    }
}

#[test]
fn a_copied_file_is_kept_as_a_first_class_flavour() {
    // The Finder case: `public.file-url` plus the filename as text.
    let guard = RecordingGuard::with(vec![vec![
        part("public.file-url", b"file:///Users/me/report.pdf"),
        part(ClipboardSnapshot::TEXT, b"report.pdf"),
    ]]);

    let snap = guard.snapshot().expect("snapshot");
    assert_eq!(snap.items.len(), 1, "one pasteboard item");
    assert_eq!(snap.items[0].len(), 2, "both flavours kept");
    assert!(
        snap.items[0].iter().any(|p| p.uti == "public.file-url"),
        "the file url is the flavour the old code destroyed"
    );
    assert_eq!(snap.first_text().as_deref(), Some("report.pdf"));
}

#[test]
fn a_multi_item_selection_round_trips_as_multiple_items() {
    let guard = RecordingGuard::with(vec![
        vec![part("public.file-url", b"file:///tmp/a.png")],
        vec![part("public.file-url", b"file:///tmp/b.png")],
    ]);
    let snap = guard.snapshot().expect("snapshot");
    assert_eq!(snap.items.len(), 2, "two items, not one merged blob");
}

#[test]
fn the_dictation_is_added_as_an_extra_item_not_a_replacement() {
    let guard = RecordingGuard::with(vec![vec![part("public.file-url", b"file:///x")]]);
    guard.restore(&guard.snapshot().unwrap(), Some("the dictation"));
    let calls = guard.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].1.as_deref(),
        Some("the dictation"),
        "the dictation rides along as an extra item, so it lands in the \
         clipboard history instead of replacing the user's copy"
    );
}

#[test]
fn an_empty_snapshot_never_implies_clearing() {
    // `ClipboardSnapshot::default()` is what a platform that cannot read the
    // pasteboard produces. The injector treats an empty snapshot as "leave it
    // alone", so this must be distinguishable from a failure to read.
    let empty = ClipboardSnapshot::default();
    assert!(empty.is_empty());
    assert_eq!(empty.first_text(), None);
    assert!(empty.items.is_empty());
}

#[test]
fn a_raw_image_survives_the_round_trip() {
    let img = arboard::ImageData {
        width: 3,
        height: 2,
        bytes: std::borrow::Cow::Borrowed(&[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        ]),
    };
    let packed = pack_raw_image(&img);
    let back = unpack_raw_image(&packed).expect("round trip");
    assert_eq!(back.width, 3);
    assert_eq!(back.height, 2);
    assert_eq!(&*back.bytes, &*img.bytes);
}

#[test]
fn a_malformed_image_part_is_rejected_rather_than_panicking() {
    let truncated = ClipboardPart {
        uti: ClipboardSnapshot::RAW_IMAGE.to_string(),
        data: vec![1, 2, 3],
    };
    assert!(unpack_raw_image(&truncated).is_none());
    let zero_sized = ClipboardPart {
        uti: ClipboardSnapshot::RAW_IMAGE.to_string(),
        data: vec![0; 16],
    };
    assert!(unpack_raw_image(&zero_sized).is_none());
}

// ===========================================================================
// What order the pasteboard is written back in
// ===========================================================================
//
// A macOS pasteboard is a **stack**: a normal Cmd+V reads the **last** item,
// and older items are reachable only via Paste Special. That single fact is
// the whole difference between "pastes the dictation" and "pastes whatever was
// there before", and it is not visible in any return value, so it has to be
// pinned here.
//
// The bug these cover: the dictation was written as the *first* item (item 0),
// so the user's previous copy stayed at the back of the stack — the "current"
// entry for a normal paste. Every Cmd+V after dictating pasted the old text,
// which read as "it keeps pasting the previous clipboard text". The dictation
// was on the pasteboard the whole time, unreachable without Paste Special.

/// The last item is what Cmd+V pastes.
fn pasted_item(snapshot: &ClipboardSnapshot, extra: Option<&str>) -> Option<String> {
    snapshot
        .restored_with(extra)
        .into_iter()
        .last()
        .and_then(|item| {
            item.iter()
                .find(|p| p.uti == ClipboardSnapshot::TEXT)
                .and_then(|p| String::from_utf8(p.data.clone()).ok())
        })
}

#[test]
fn the_dictation_is_what_pastes_after_a_dictation() {
    let snap = ClipboardSnapshot {
        parts: vec![],
        items: vec![vec![part(ClipboardSnapshot::TEXT, b"the old clipboard")]],
    };
    assert_eq!(
        pasted_item(&snap, Some("the dictation")).as_deref(),
        Some("the dictation"),
        "Cmd+V after dictating must give the dictation, not the old clipboard"
    );
}

#[test]
fn the_dictation_is_pasted_even_when_the_user_had_copied_a_file() {
    // A Finder copy: no plain text at all, just a file URL.
    let snap = ClipboardSnapshot {
        parts: vec![],
        items: vec![vec![part(
            "public.file-url",
            b"file:///Users/someone/Desktop/report.pdf",
        )]],
    };
    assert_eq!(
        pasted_item(&snap, Some("the dictation")).as_deref(),
        Some("the dictation")
    );
    // ...and the file is still there to paste, one item along.
    let written = snap.restored_with(Some("the dictation"));
    assert_eq!(written.len(), 2, "the file copy must survive: {written:?}");
    assert!(
        written[0].iter().any(|p| p.uti == "public.file-url"),
        "the user's file URL must still be on the pasteboard: {written:?}"
    );
}

#[test]
fn every_flavour_of_the_users_copy_survives_in_order() {
    // A rich-text copy is one *item* with several representations. All of them
    // have to come back, and the item order has to be preserved, because a
    // multi-item selection pastes in the order it was copied.
    let snap = ClipboardSnapshot {
        parts: vec![],
        items: vec![
            vec![
                part("public.utf8-plain-text", b"bold"),
                part("public.rtf", b"{\\rtf1 bold}"),
            ],
            vec![part("public.file-url", b"file:///tmp/b.txt")],
            vec![part("public.file-url", b"file:///tmp/c.txt")],
        ],
    };
    let written = snap.restored_with(Some("dictated"));
    assert_eq!(written.len(), 4, "one dictation + three user items");
    // The user's items keep their original order; the dictation is last.
    assert_eq!(written[0].len(), 2, "both flavours of the first user item survive");
    assert_eq!(written[1][0].data, b"file:///tmp/b.txt");
    assert_eq!(written[2][0].data, b"file:///tmp/c.txt");
    assert_eq!(written[3][0].uti, ClipboardSnapshot::TEXT);
    assert!(
        written[3].iter().any(|p| p.uti == ClipboardSnapshot::DICTATION),
        "the last item is the marked dictation"
    );
}

// ===========================================================================
// The pasteboard must not grow without bound
// ===========================================================================
//
// Every restore writes back the whole snapshot plus the dictation, and the next
// dictation snapshots that. So each dictation used to add one item, forever:
// after 50 dictations the pasteboard had 50 items and every restore rewrote all
// of them, and the oldest text was still sitting at the front where every paste
// read it. That is the "again and again" half of the report.

/// What a dictation leaves on the pasteboard, fed back in as the next snapshot.
fn one_dictation_round(previous: &ClipboardSnapshot, text: &str) -> ClipboardSnapshot {
    let items = previous.restored_with(Some(text));
    // Reading a snapshot back is what the guard does: every part, every item.
    let parts = items.iter().flatten().cloned().collect();
    ClipboardSnapshot { parts, items }
}

#[test]
fn the_pasteboard_does_not_grow_without_bound() {
    let mut snap = ClipboardSnapshot {
        parts: vec![],
        items: vec![vec![part(ClipboardSnapshot::TEXT, b"the user's copy")]],
    };
    for n in 0..200 {
        snap = one_dictation_round(&snap, &format!("dictation {n}"));
    }
    assert!(
        snap.items.len() <= ClipboardSnapshot::MAX_ITEMS,
        "after 200 dictations the pasteboard held {} items, cap is {}",
        snap.items.len(),
        ClipboardSnapshot::MAX_ITEMS
    );
    // Two items, not 201: the user's copy, and the latest dictation.
    assert_eq!(
        snap.items.len(),
        2,
        "expected the user's copy plus one dictation, got {:?}",
        snap.items
            .iter()
            .map(|i| i
                .iter()
                .find(|p| p.uti == ClipboardSnapshot::TEXT)
                .map(|p| String::from_utf8_lossy(&p.data).into_owned())
                .unwrap_or_default())
            .collect::<Vec<_>>()
    );
    // And the text is still the newest thing, not the oldest.
    assert_eq!(
        pasted_item(&snap, Some("dictation 199")).as_deref(),
        Some("dictation 199")
    );
}

#[test]
fn an_old_dictation_is_never_carried_forward() {
    let first = ClipboardSnapshot {
        parts: vec![],
        items: vec![vec![part(ClipboardSnapshot::TEXT, b"user copy")]],
    };
    let after_first = one_dictation_round(&first, "dictation one");
    // The dictation item is recognisable, so a later snapshot can drop it.
    let dictation_items: Vec<_> = after_first
        .items
        .iter()
        .filter(|i| ClipboardSnapshot::is_dictation_item(i))
        .collect();
    assert_eq!(
        dictation_items.len(),
        1,
        "exactly one item should be marked as ours"
    );
    let cleaned = after_first.without_our_own_items();
    assert_eq!(cleaned.items.len(), 1, "only the user's copy is left");
    assert_eq!(cleaned.first_text().as_deref(), Some("user copy"));
}

#[test]
fn the_dictation_item_carries_a_non_empty_marker() {
    // An empty NSData representation is dropped by the pasteboard, which would
    // drop the marker with it and quietly re-open the growth bug.
    let snap = ClipboardSnapshot::default();
    let written = snap.restored_with(Some("hello"));
    // With no user items, the dictation is the only item (and therefore the
    // last one, which is what a normal paste reads).
    let dictation = written
        .last()
        .expect("the dictation is the only item written");
    let marker = dictation
        .iter()
        .find(|p| p.uti == ClipboardSnapshot::DICTATION)
        .expect("the dictation item is marked");
    assert!(!marker.data.is_empty(), "marker data must not be empty");
    assert!(ClipboardSnapshot::is_dictation_item(dictation));
}

#[test]
fn restoring_nothing_leaves_the_decision_to_the_caller() {
    // An empty snapshot and no dictation writes nothing, which is what stops
    // `clearContents` from running and wiping an unreadable clipboard.
    assert!(ClipboardSnapshot::default().restored_with(None).is_empty());
}
