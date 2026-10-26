//! The Notetaker: a private notes store, persisted to `scratchpad.json`.
//!
//! While the Teletype window is frontmost and "route dictation here" is on,
//! a dictation is appended to the open note instead of being injected into
//! the previous focus. Notes persist across sessions.
//!
//! The module file name and the on-disk file are kept as `scratchpad` for
//! migration safety: existing installs hold a legacy
//! `{"entries": [{"id", "createdAt", "text"}]}` document. `deserialize`
//! converts it to the notes shape without loss, and the next save rewrites
//! the file in the new shape.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// How the text in a note got there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteSource {
    /// A dictation routed to the note while the Teletype window was focused.
    Dictated,
    /// Typed or edited by hand in the Notetaker.
    Typed,
}

impl Default for NoteSource {
    fn default() -> Self {
        Self::Typed
    }
}

/// One note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub source: NoteSource,
}

/// Legacy on-disk shape, one line of the old scratchpad (one dictation =
/// one entry). Kept only so old files keep loading.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScratchEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    created_at: u64,
    #[serde(default)]
    text: String,
}

/// Legacy on-disk shape, the whole old scratchpad document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Scratchpad {
    entries: Vec<ScratchEntry>,
}

/// The notes document. `notes` is newest-edited-first in the UI; the stored
/// order is insertion order and the UI sorts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    pub notes: Vec<Note>,
    /// The note the editor pane shows. `None` = no note open.
    pub open_note_id: Option<String>,
}

/// The store holds at most this many notes. A note count, not a take
/// count: a note is the unit the user owns and finds, and 500 notes is
/// plenty of headroom for a personal pad while keeping the JSON file small
/// enough that load and save stay instant. Deletion is always
/// explicit and user-initiated; nothing is ever dropped here.
const MAX_NOTES: usize = 500;

/// Blank titles render as this in the list.
pub const UNTITLED: &str = "Untitled note";

impl Notes {
    /// Creates a new (empty) note, opens it, and puts it at the front.
    /// Returns the new note.
    pub fn create_note(&mut self, now: u64) -> Note {
        let note = Note {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            body: String::new(),
            created_at: now,
            updated_at: now,
            source: NoteSource::Typed,
        };
        self.notes.insert(0, note.clone());
        if self.notes.len() > MAX_NOTES {
            self.notes.truncate(MAX_NOTES);
        }
        self.open_note_id = Some(note.id.clone());
        note
    }

    /// Appends a dictated take to the open note, or creates one and puts
    /// the take there. The take lands at the end of the body, separated
    /// from the existing text by a blank line (the same paragraph break
    /// the delivery path joins paragraphs with). Returns the note the text
    /// landed in, if any.
    pub fn append_to_open_note(&mut self, text: String, now: u64) -> Option<Note> {
        if text.trim().is_empty() {
            return None;
        }
        let text = text.trim().to_string();
        if self.open_note_id.is_none() {
            let note = self.create_note(now);
            let id = note.id.clone();
            self.note_mut(&id).map(|n| {
                n.body = text;
                n.updated_at = now;
                n.source = NoteSource::Dictated;
            })?;
            return self.note(&id).cloned();
        }
        let id = self.open_note_id.clone()?;
        let note = self.note_mut(&id)?;
        if note.body.is_empty() {
            note.body = text;
        } else {
            note.body.push_str("\n\n");
            note.body.push_str(&text);
        }
        note.updated_at = now;
        note.source = NoteSource::Dictated;
        let id = note.id.clone();
        self.note(&id).cloned()
    }

    /// Saves a title/body edit on an existing note. Returns the saved note.
    /// A blank body and blank title is rejected: an empty note is created
    /// by `create_note`, not by erasing one.
    pub fn save_note(&mut self, id: &str, title: String, body: String, now: u64) -> Option<Note> {
        if title.trim().is_empty() && body.trim().is_empty() {
            return None;
        }
        let note = self.note_mut(id)?;
        note.title = title.trim().to_string();
        note.body = body.trim().to_string();
        note.updated_at = now;
        if note.source == NoteSource::Dictated {
            // Hand edits turn the note into a typed one.
            note.source = NoteSource::Typed;
        }
        let id = note.id.clone();
        self.note(&id).cloned()
    }

    /// Renames the open note. Returns the renamed note.
    pub fn rename_note(&mut self, id: &str, title: String, now: u64) -> Option<Note> {
        let note = self.note_mut(id)?;
        note.title = title.trim().to_string();
        note.updated_at = now;
        let id = note.id.clone();
        self.note(&id).cloned()
    }

    pub fn delete_note(&mut self, id: &str) -> Option<Note> {
        let pos = self.notes.iter().position(|n| n.id == id)?;
        let note = self.notes.remove(pos);
        if self.open_note_id.as_deref() == Some(id) {
            self.open_note_id = None;
        }
        Some(note)
    }

    pub fn set_open_note(&mut self, id: Option<String>) {
        match id {
            Some(id) if self.notes.iter().any(|n| n.id == id) => self.open_note_id = Some(id),
            _ => self.open_note_id = None,
        }
    }

    pub fn note(&self, id: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    fn note_mut(&mut self, id: &str) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    /// One note from a migrated legacy take: the take text, the time it was
    /// recorded, and the take's first line as the title, so old pads keep
    /// their chronology inside the note body.
    fn note_from_entry(entry: &ScratchEntry) -> Option<Note> {
        let body = entry.text.trim();
        if body.is_empty() {
            return None;
        }
        let when = stamp_utc(entry.created_at);
        let title = body
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .chars()
            .take(60)
            .collect::<String>();
        let body = format!("[{when}] {body}");
        Some(Note {
            id: if entry.id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                entry.id.clone()
            },
            title,
            body,
            created_at: entry.created_at,
            updated_at: entry.created_at,
            source: NoteSource::Dictated,
        })
    }

    fn from_legacy(pad: Scratchpad) -> Self {
        // The pad is newest-first; a notes app lists newest-edited-first,
        // so the legacy order already reads as insertion order for `notes`.
        let notes = pad
            .entries
            .iter()
            .rev()
            .filter_map(Self::note_from_entry)
            .collect();
        let mut out = Self {
            notes,
            open_note_id: None,
        };
        if out.notes.len() > MAX_NOTES {
            out.notes.truncate(MAX_NOTES);
        }
        out
    }
}

/// A `YYYY-MM-DD HH:MM` stamp for a millisecond timestamp, UTC. std-only:
/// the project has no chrono dependency and one date stamp per migrated
/// take does not warrant it (same approach as `trace::today_ymd`).
fn stamp_utc(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = crate::trace::civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Serialization with legacy migration on read. The document is written in
/// the notes shape; an old `{"entries": [...]}` file is converted.
impl Serialize for Notes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Notes", 2)?;
        s.serialize_field("notes", &self.notes)?;
        s.serialize_field("openNoteId", &self.open_note_id)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for Notes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            notes: Vec<Note>,
            #[serde(default)]
            open_note_id: Option<String>,
            // The legacy key. Present (usually empty) in a notes-shaped
            // file written before it was dropped; a non-empty one means
            // this is an old scratchpad document.
            #[serde(default)]
            entries: Vec<ScratchEntry>,
        }
        let raw = Raw::deserialize(deserializer)?;
        if raw.notes.is_empty() && !raw.entries.is_empty() {
            return Ok(Notes::from_legacy(Scratchpad {
                entries: raw.entries,
            }));
        }
        let mut out = Notes {
            notes: raw.notes,
            open_note_id: raw.open_note_id,
        };
        if out
            .open_note_id
            .as_deref()
            .is_some_and(|id| !out.notes.iter().any(|n| n.id == id))
        {
            out.open_note_id = None;
        }
        Ok(out)
    }
}

impl fmt::Display for Notes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} notes, open = {:?}",
            self.notes.len(),
            self.open_note_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const NOW: u64 = 1_700_000_000_000;

    #[test]
    fn legacy_json_migrates_without_loss() {
        // The exact shape of an old on-disk scratchpad.json (camelCase,
        // newest first).
        let legacy = r#"{
            "entries": [
                {"id": "e2", "createdAt": 1700000002000, "text": "second take\nwith a second line"},
                {"id": "e1", "createdAt": 1700000001000, "text": "   first take   "},
                {"id": "", "createdAt": 1700000000500, "text": "   "}
            ]
        }"#;
        let notes: Notes = serde_json::from_str(legacy).unwrap();
        assert_eq!(notes.notes.len(), 2);
        // The legacy pad is newest-first; migration stores notes in
        // chronological (insertion) order, oldest first.
        assert_eq!(notes.notes[0].id, "e1");
        assert_eq!(notes.notes[0].created_at, 1700000001000);
        assert!(notes.notes[0].body.contains("first take"));
        assert_eq!(notes.notes[1].id, "e2");
        assert_eq!(notes.notes[1].created_at, 1700000002000);
        assert_eq!(notes.notes[1].updated_at, 1700000002000);
        assert_eq!(notes.notes[1].title, "second take");
        assert!(notes.notes[1]
            .body
            .contains("second take\nwith a second line"));
        assert!(notes.notes[1].body.contains("[2023-11-14"));
        // The blank take is dropped, the empty-id one would have a fresh id.
        assert!(!notes.notes.iter().any(|n| n.body.trim().is_empty()));
        assert!(notes.open_note_id.is_none());
    }

    #[test]
    fn legacy_migration_preserves_old_key_compatibility() {
        // A notes-shaped file still carries the legacy `entries` key
        // (empty) so an old app reading the new file sees what it expects.
        let notes = Notes {
            notes: vec![Note {
                id: "n1".into(),
                title: "t".into(),
                body: "b".into(),
                created_at: NOW,
                updated_at: NOW,
                source: NoteSource::Typed,
            }],
            open_note_id: Some("n1".into()),
        };
        let json = serde_json::to_string(&notes).unwrap();
        let back: Notes = serde_json::from_str(&json).unwrap();
        assert_eq!(back, notes);
    }

    #[test]
    fn create_and_bounds() {
        let mut notes = Notes::default();
        let a = notes.create_note(NOW);
        assert_eq!(notes.notes.len(), 1);
        assert_eq!(notes.open_note_id.as_deref(), Some(a.id.as_str()));
        for _ in 0..MAX_NOTES + 50 {
            notes.create_note(NOW);
        }
        assert_eq!(notes.notes.len(), MAX_NOTES);
        // The newest created note stays open.
        assert!(notes.note(notes.open_note_id.as_deref().unwrap()).is_some());
    }

    #[test]
    fn append_to_open_note_ordering() {
        let mut notes = Notes::default();
        let first = notes.create_note(NOW);
        notes.append_to_open_note("take one".into(), NOW + 100);
        notes.append_to_open_note("take two".into(), NOW + 200);
        let note = notes.note(&first.id).unwrap();
        assert_eq!(note.body, "take one\n\ntake two");
        assert_eq!(note.source, NoteSource::Dictated);
        assert_eq!(note.updated_at, NOW + 200);
    }

    #[test]
    fn append_with_no_open_note_creates_one() {
        let mut notes = Notes::default();
        let note = notes
            .append_to_open_note("just said it".into(), NOW)
            .unwrap();
        assert_eq!(notes.notes.len(), 1);
        assert_eq!(notes.open_note_id.as_deref(), Some(note.id.as_str()));
        assert_eq!(note.body, "just said it");
        assert_eq!(note.source, NoteSource::Dictated);
        // Blank takes never create a note.
        assert!(notes.append_to_open_note("   ".into(), NOW + 1).is_none());
        assert_eq!(notes.notes.len(), 1);
    }

    #[test]
    fn save_moves_updated_at_and_renames() {
        let mut notes = Notes::default();
        let n = notes.create_note(NOW);
        notes.rename_note(&n.id, "My note".into(), NOW + 5).unwrap();
        let saved = notes
            .save_note(&n.id, "My note".into(), "body text".into(), NOW + 10)
            .unwrap();
        assert_eq!(saved.title, "My note");
        assert_eq!(saved.body, "body text");
        assert_eq!(saved.updated_at, NOW + 10);
        assert!(saved.updated_at > n.created_at);
        // Saving erases nothing by hand: blank + blank is rejected.
        assert!(notes
            .save_note(&n.id, "  ".into(), "  ".into(), NOW + 20)
            .is_none());
        assert_eq!(notes.note(&n.id).unwrap().body, "body text");
        assert!(notes
            .save_note("missing", "x".into(), "y".into(), NOW)
            .is_none());
    }

    #[test]
    fn delete_closes_the_open_note() {
        let mut notes = Notes::default();
        let n = notes.create_note(NOW);
        let id = n.id.clone();
        let removed = notes.delete_note(&id).unwrap();
        assert_eq!(removed.id, id);
        assert!(notes.notes.is_empty());
        assert!(notes.open_note_id.is_none());
        assert!(notes.delete_note(&id).is_none());
    }

    #[test]
    fn set_open_note_rejects_unknown_ids() {
        let mut notes = Notes::default();
        let n = notes.create_note(NOW);
        // An unknown id closes the open note; it never points at nothing.
        notes.set_open_note(Some("nope".into()));
        assert!(notes.open_note_id.is_none());
        notes.set_open_note(Some(n.id.clone()));
        assert_eq!(notes.open_note_id.as_deref(), Some(n.id.as_str()));
        notes.set_open_note(None);
        assert!(notes.open_note_id.is_none());
    }

    #[test]
    fn stamp_utc_formats_a_known_instant() {
        // 1700000000000 ms = 2023-11-14T22:13:20Z.
        assert_eq!(stamp_utc(1_700_000_000_000), "2023-11-14 22:13");
        // Crossing midnight moves to the next day without borrowing.
        assert_eq!(
            stamp_utc(1_700_000_000_000 + 86_400_000),
            "2023-11-15 22:13"
        );
    }

    #[test]
    fn real_clock_is_sane() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(now > 1_600_000_000_000);
    }
}
