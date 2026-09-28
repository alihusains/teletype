//! Text injection into the focused application.
//!
//! Strategy (safest, most reliable on both macOS and Windows):
//! 1. Snapshot the current clipboard.
//! 2. Put our text on the clipboard.
//! 3. Wait a short settle time so the hotkey's modifiers are released.
//! 4. Simulate the paste shortcut (⌘V / Ctrl+V).
//! 5. Wait for the target app to read the clipboard.
//! 6. Put the snapshot back, and add the dictated text as an extra item.
//!
//! One dedicated thread owns the clipboard for the process lifetime, because
//! on some platforms the clipboard is served by the process that set it.
//!
//! # Why step 1 is a snapshot and not "text or image"
//!
//! `arboard` can only read text and images, and both `set_text` and
//! `set_image` *replace the entire pasteboard*. The previous code therefore
//! destroyed anything it could not round-trip: copying a file in Finder and
//! then dictating wiped the copied file, because `get_text` and `get_image`
//! both failed, the save became `Empty`, and the restore called `clear()`.
//! The same silent loss hit a copy that carried both a text and an image
//! flavour.
//!
//! [`ClipboardGuard`] exists so a platform can snapshot every representation
//! the OS knows about, not just the two `arboard` exposes. Where no guard is
//! supplied the code still never calls `clear()`: an unsaveable clipboard is
//! left exactly as it was found, which costs the user nothing.

use std::{
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread,
    time::Duration,
};

use arboard::{Clipboard, ImageData};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::platform::PasteShortcut;

use tracing::{error, warn};

/// Let go of the hotkey's modifiers before the simulated paste.
const BEFORE_PASTE: Duration = Duration::from_millis(250);
/// Let the target app read the clipboard before restoring it.
const BEFORE_RESTORE: Duration = Duration::from_millis(350);

enum Saved {
    /// Round-tripped through the platform guard, so every flavour is kept.
    Snapshot(ClipboardSnapshot),
    Text(String),
    Image(ImageData<'static>),
    /// The clipboard held something we cannot read. It is left alone.
    Unreadable,
}

struct Job {
    text: String,
    restore_clipboard: bool,
    /// Also leave the dictated text on the clipboard, as an extra item, so it
    /// shows up in the system's clipboard history.
    keep_text: bool,
    paste: Option<PasteShortcut>,
}

/// One clipboard flavour: a UTI (or the platform's equivalent type name) and
/// its bytes. Opaque to the core, which only moves it back and forth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardPart {
    pub uti: String,
    pub data: Vec<u8>,
}

/// Everything that was on the clipboard when we started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub parts: Vec<ClipboardPart>,
    /// More than one pasteboard item, so a multi-item selection round-trips.
    pub items: Vec<Vec<ClipboardPart>>,
}

impl ClipboardSnapshot {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// UTI for the plain-text flavour, on either platform's naming.
    pub const TEXT: &'static str = "public.utf8-plain-text";
    /// UTI for a raw RGBA image as `arboard` hands it over. Not a real UTI:
    /// `arboard` gives raw pixels with no container, so there is nothing to
    /// name, and inventing a public one would be a lie to the OS.
    pub const RAW_IMAGE: &'static str = "teletype.arboard.raw-rgba";
    /// Marks a pasteboard item as one Teletype added for a dictation.
    ///
    /// The pasteboard is a *stack*, not a slot. Every restore writes back
    /// everything it snapshotted plus the dictation, and the next dictation
    /// snapshots that, so without a marker each dictation leaves its own text
    /// behind forever: the pasteboard grows by one item per dictation, the
    /// oldest text stays at the front where Cmd+V reads it, and every restore
    /// rewrites a bigger pile. This UTI is how a snapshot tells our own
    /// leftovers apart from what the user copied.
    pub const DICTATION: &'static str = "org.teletype.dictation";
    /// Ceiling on restored items, so the pasteboard cannot grow without bound
    /// even if something else is adding items too. macOS's own clipboard
    /// managers keep a similar handful.
    pub const MAX_ITEMS: usize = 16;

    /// The plain-text flavour, if the snapshot captured one.
    pub fn first_text(&self) -> Option<String> {
        self.items
            .iter()
            .flatten()
            .find(|p| p.uti == Self::TEXT || p.uti == "text/plain")
            .and_then(|p| String::from_utf8(p.data.clone()).ok())
    }

    /// True when this item is a dictation a previous restore left behind.
    pub fn is_dictation_item(item: &[ClipboardPart]) -> bool {
        item.iter().any(|p| p.uti == Self::DICTATION)
    }

    /// Drop the items a previous restore added, so a snapshot is only ever what
    /// the *user* put there.
    pub fn without_our_own_items(&self) -> ClipboardSnapshot {
        ClipboardSnapshot {
            parts: self
                .items
                .iter()
                .filter(|i| !Self::is_dictation_item(i))
                .flatten()
                .cloned()
                .collect(),
            items: self
                .items
                .iter()
                .filter(|i| !Self::is_dictation_item(i))
                .cloned()
                .collect(),
        }
    }

    /// The items to write back, in the order to write them.
    ///
    /// Order is the whole point. `NSPasteboard` pastes **item 0**, so:
    ///
    /// - `extra` (the dictation) goes **first**. It is the most recent thing in
    ///   the world, so Cmd+V right after dictating has to give the dictation.
    ///   Putting it last left the user's old copy at the front, which is how
    ///   "it keeps pasting the previous clipboard text" happened: the dictation
    ///   was on the pasteboard but unreachable without Paste Special, and the
    ///   old text was what every paste produced.
    /// - Then the user's own items, in their original order, so a copied file,
    ///   image or multi-selection is still whole and still pasteable.
    ///
    /// A dictation can never contain a NUL, so the two parts cannot collide.
    pub fn restored_with(&self, extra: Option<&str>) -> Vec<Vec<ClipboardPart>> {
        let mut out: Vec<Vec<ClipboardPart>> = Vec::with_capacity(self.items.len() + 1);
        if let Some(text) = extra {
            out.push(vec![
                ClipboardPart {
                    uti: Self::TEXT.to_string(),
                    data: text.as_bytes().to_vec(),
                },
                ClipboardPart {
                    uti: Self::DICTATION.to_string(),
                    // The marker value must not be empty: an empty NSData
                    // representation is dropped by the pasteboard, taking the
                    // marker with it and re-opening the growth bug.
                    data: b"1".to_vec(),
                },
            ]);
        }
        for item in &self.items {
            if Self::is_dictation_item(item) {
                continue;
            }
            out.push(item.clone());
            if out.len() >= Self::MAX_ITEMS {
                break;
            }
        }
        out
    }
}

/// Pack `arboard`'s raw RGBA image into one part: 8 bytes of dimensions
/// followed by the pixels.
pub fn pack_raw_image(img: &ImageData<'_>) -> ClipboardPart {
    let mut data = Vec::with_capacity(16 + img.bytes.len());
    data.extend_from_slice(&(img.width as u64).to_le_bytes());
    data.extend_from_slice(&(img.height as u64).to_le_bytes());
    data.extend_from_slice(&img.bytes);
    ClipboardPart {
        uti: ClipboardSnapshot::RAW_IMAGE.to_string(),
        data,
    }
}

/// Unpack what [`pack_raw_image`] wrote. `None` if the part is malformed.
pub fn unpack_raw_image(part: &ClipboardPart) -> Option<ImageData<'static>> {
    if part.data.len() < 16 {
        return None;
    }
    let w = u64::from_le_bytes(part.data[0..8].try_into().ok()?) as usize;
    let h = u64::from_le_bytes(part.data[8..16].try_into().ok()?) as usize;
    if w == 0 || h == 0 {
        return None;
    }
    Some(ImageData {
        width: w,
        height: h,
        bytes: std::borrow::Cow::Owned(part.data[16..].to_vec()),
    })
}

/// A platform hook for snapshotting and restoring the clipboard faithfully.
///
/// `teletype-core` stays platform-free (see `docs/architecture.md`), so the
/// trait lives here and the macOS implementation lives in
/// `teletype-desktop`. Without one, the injector degrades to `arboard` and
/// still never destroys anything it could not read.
pub trait ClipboardGuard: Send + Sync {
    /// Capture every representation currently on the clipboard.
    fn snapshot(&self) -> Option<ClipboardSnapshot>;

    /// Put `snapshot` back, and additionally publish `extra` as its own item
    /// so the dictated text is added to the system clipboard history rather
    /// than replacing what the user had.
    fn restore(&self, snapshot: &ClipboardSnapshot, extra: Option<&str>);
}

/// Handle to the injector thread. Cloning is cheap.
#[derive(Clone)]
pub struct TextInjector {
    tx: Sender<Job>,
}

impl TextInjector {
    pub fn spawn() -> Self {
        Self::spawn_with_guard(None)
    }

    /// Spawn the injector thread with a platform clipboard guard. Supplying
    /// one lets a dictation restore every flavour the clipboard held, not just
    /// the text and image `arboard` can see.
    pub fn spawn_with_guard(guard: Option<Arc<dyn ClipboardGuard>>) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = thread::Builder::new()
            .name("teletype-inject".into())
            .spawn(move || {
                let mut clipboard: Option<Clipboard> = None;
                let mut enigo: Option<Enigo> = None;
                for job in rx {
                    if clipboard.is_none() {
                        clipboard = Clipboard::new()
                            .inspect_err(|e| warn!("[inject] clipboard unavailable: {e}"))
                            .ok();
                    }
                    let Some(clipboard) = clipboard.as_mut() else {
                        continue;
                    };
                    if enigo.is_none() {
                        enigo = Enigo::new(&Settings::default())
                            .inspect_err(|e| warn!("[inject] keyboard simulation unavailable: {e}"))
                            .ok();
                    }
                    if let Err(e) = inject(clipboard, enigo.as_mut(), &job, guard.as_deref()) {
                        warn!("[inject] {e}");
                    }
                }
            });
        if let Err(e) = spawned {
            error!("[inject] couldn't start injector thread: {e}");
        }
        Self { tx }
    }

    /// Queues `text` for insertion. Returns immediately.
    ///
    /// `keep_text` leaves the dictated text on the clipboard as an extra item
    /// after restoring, so it lands in the system clipboard history.
    pub fn inject(
        &self,
        text: String,
        restore_clipboard: bool,
        keep_text: bool,
        paste: Option<PasteShortcut>,
    ) {
        let _ = self.tx.send(Job {
            text,
            restore_clipboard,
            keep_text,
            paste,
        });
    }
}

impl Default for TextInjector {
    fn default() -> Self {
        Self::spawn()
    }
}

fn inject(
    clipboard: &mut Clipboard,
    enigo: Option<&mut Enigo>,
    job: &Job,
    guard: Option<&dyn ClipboardGuard>,
) -> Result<(), String> {
    // 1. Snapshot the clipboard.
    //
    // Preference order: the platform guard (every flavour), then arboard for
    // text, then arboard for an image. If none of them can read what is
    // there, remember that and leave it alone -- never clear it.
    let saved = if job.restore_clipboard {
        if let Some(g) = guard {
            match g.snapshot() {
                Some(snap) if !snap.is_empty() => Saved::Snapshot(snap),
                // A guard that cannot read anything must not be treated as
                // "the clipboard was empty", or the restore wipes it.
                _ => unreadable(clipboard),
            }
        } else {
            unreadable(clipboard)
        }
    } else {
        Saved::Unreadable
    };

    // 2. Set our text.
    clipboard
        .set_text(&job.text)
        .map_err(|e| format!("Couldn't set clipboard: {e}"))?;

    // 3. Settle.
    thread::sleep(BEFORE_PASTE);

    // 4. Paste.
    if let Some(enigo) = enigo {
        let (mods, key) = match job.paste {
            // macOS cannot use Key::Unicode('v') — see paste_keys() below.
            Some(PasteShortcut::ControlV) if cfg!(target_os = "macos") => {
                (&[Key::Control][..], Key::Other(9))
            }
            Some(PasteShortcut::ControlV) => (&[Key::Control][..], Key::Unicode('v')),
            _ => paste_keys(),
        };
        press_combo(enigo, mods, key).map_err(|e| format!("Paste shortcut failed: {e}"))?;
    } else {
        // No keyboard simulation: leave the text on the clipboard and tell
        // the user (the caller can surface this).
        warn!("[inject] no keyboard simulation; text left on clipboard");
    }

    // 5. Let the target read it.
    thread::sleep(BEFORE_RESTORE);

    // 6. Restore, and add the dictated text as an extra clipboard item.
    if job.restore_clipboard {
        let extra = job.keep_text.then_some(job.text.as_str());
        match &saved {
            Saved::Snapshot(snap) => {
                if let Some(g) = guard {
                    g.restore(snap, extra);
                } else {
                    restore_portable(clipboard, snap, extra);
                }
            }
            Saved::Text(t) => restore_portable(
                clipboard,
                &ClipboardSnapshot {
                    parts: vec![],
                    items: vec![vec![ClipboardPart {
                        uti: "public.utf8-plain-text".into(),
                        data: t.clone().into_bytes(),
                    }]],
                },
                extra,
            ),
            Saved::Image(img) => {
                restore_portable(
                    clipboard,
                    &ClipboardSnapshot {
                        parts: vec![],
                        items: vec![vec![pack_raw_image(img)]],
                    },
                    extra,
                );
            }
            // We never worked out what was there, so we do not touch it. This
            // is the case that used to wipe a copied file out of the clipboard.
            Saved::Unreadable => {
                warn!("[inject] clipboard was not readable before dictation; leaving it untouched");
            }
        }
    } else if job.keep_text {
        // No restore wanted, but the user asked for the dictated text to end
        // up in the clipboard history. It is already there from step 2.
    }
    Ok(())
}

/// Read whatever `arboard` can see, and say so honestly when it cannot see
/// enough to restore it faithfully.
fn unreadable(clipboard: &mut Clipboard) -> Saved {
    match clipboard.get_text() {
        Ok(text) if !text.is_empty() => Saved::Text(text),
        _ => match clipboard.get_image() {
            Ok(img) => Saved::Image(img),
            // Nothing readable: a file, a PDF, a folder, or a flavour arboard
            // does not expose. Leave it alone.
            Err(_) => Saved::Unreadable,
        },
    }
}

/// Restore path for when there is no platform guard, and for the text and
/// image flavours a snapshot carries.
///
/// Note what is *not* here any more: `clipboard.clear()`. It used to run for
/// every flavour we could not round-trip, which is how a copied file ended up
/// destroyed. Now the dictated text is simply left in place, which is also
/// what the user asked for: their clipboard gains the dictation rather than
/// losing whatever was on it.
fn restore_portable(clipboard: &mut Clipboard, snap: &ClipboardSnapshot, extra: Option<&str>) {
    for item in &snap.items {
        for part in item {
            if part.uti == ClipboardSnapshot::TEXT || part.uti == "text/plain" {
                if let Ok(text) = String::from_utf8(part.data.clone()) {
                    if let Err(e) = clipboard.set_text(text) {
                        warn!("[inject] clipboard restore (text) failed: {e}");
                        return;
                    }
                }
            } else if part.uti == ClipboardSnapshot::RAW_IMAGE {
                if let Some(img) = unpack_raw_image(part) {
                    if let Err(e) = clipboard.set_image(img) {
                        warn!("[inject] clipboard restore (image) failed: {e}");
                        return;
                    }
                }
            }
            // Any other flavour is one only the platform guard can restore, so
            // the portable path leaves it alone rather than replacing the
            // clipboard with something less than what was there.
        }
    }
    // `extra` is the dictation the user asked to keep. `arboard` can hold one
    // text flavour, so this replaces the restored text; the full-fidelity
    // path (the macOS guard) keeps both as separate items.
    if let Some(text) = extra {
        let _ = clipboard.set_text(text);
    }
}

/// The default paste keys for the current platform.
fn paste_keys() -> (&'static [Key], Key) {
    if cfg!(target_os = "macos") {
        // Use the fixed ANSI 'V' keycode (9) rather than Key::Unicode('v').
        // Key::Unicode triggers enigo's layout-dependent keycode lookup, which
        // calls TSMGetInputSourceProperty — a TextServices API that asserts
        // (crashes) when invoked off the main thread. Key::Other(9) maps
        // straight to the ANSI_V keycode with no TSM call.
        (&[Key::Meta][..], Key::Other(9))
    } else {
        (&[Key::Control][..], Key::Unicode('v'))
    }
}

/// Holds `modifiers`, taps `key`, then releases the modifiers even on error.
fn press_combo(enigo: &mut Enigo, modifiers: &[Key], key: Key) -> Result<(), String> {
    let mut result = Ok(());
    let mut held = Vec::new();
    for modifier in modifiers {
        match enigo.key(*modifier, Direction::Press) {
            Ok(()) => held.push(*modifier),
            Err(e) => {
                result = Err(e.to_string());
                break;
            }
        }
    }
    if result.is_ok() {
        result = enigo.key(key, Direction::Click).map_err(|e| e.to_string());
    }
    for modifier in held.iter().rev() {
        let _ = enigo.key(*modifier, Direction::Release);
    }
    result
}

/// A no-op injector for tests and headless runs.
#[derive(Debug, Clone, Default)]
pub struct MockInjector;

impl MockInjector {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_keys_per_platform() {
        let (mods, key) = paste_keys();
        if cfg!(target_os = "macos") {
            // Fixed ANSI 'V' keycode, not a Unicode key (see paste_keys).
            assert_eq!(key, Key::Other(9));
            assert_eq!(mods, &[Key::Meta]);
        } else {
            assert_eq!(key, Key::Unicode('v'));
            assert_eq!(mods, &[Key::Control]);
        }
    }
}
