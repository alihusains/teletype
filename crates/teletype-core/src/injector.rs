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
//! on some platforms the clipboard is served by the process that set it. Step
//! 6 runs on its own detached thread: the take returns as soon as the paste
//! keystroke is issued, and a newer dictation cancels a pending older restore
//! (process-wide generation token) so the old text is never put back over
//! the new one.
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
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use arboard::{Clipboard, ImageData};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::platform::PasteShortcut;

use tracing::{debug, error, warn};

/// How long to wait for the hotkey's modifiers to be released before the
/// simulated paste. The old fixed 250 ms settle is gone: when nothing is
/// held, [`wait_for_modifiers_released`] returns immediately; the cap only
/// fires while the user is still pressing the hotkey.
const BEFORE_PASTE_CAP: Duration = Duration::from_millis(150);
/// Poll interval for the modifier release wait.
const MODIFIER_POLL: Duration = Duration::from_millis(5);
/// How long the detached restore thread waits for the target app to read
/// the clipboard before putting the snapshot back. Off the take's critical
/// path (P1.2), so the pipeline is free for the next dictation.
const BEFORE_RESTORE: Duration = Duration::from_millis(350);

/// Generation token for the detached clipboard restore.
///
/// Every inject/paste sequence bumps it at the public entry
/// ([`TextInjector::inject`]); the detached restore thread captures the
/// value it saw and, at fire time, skips the restore and retention
/// handling entirely if the global has advanced past it. A newer
/// dictation then owns the clipboard, and putting the old snapshot back
/// would corrupt it.
static RESTORE_GENERATION: AtomicU64 = AtomicU64::new(0);

enum Saved {
    /// Round-tripped through the platform guard, so every flavour is kept.
    Snapshot(ClipboardSnapshot),
    Text(String),
    Image(ImageData<'static>),
    /// The clipboard held something we cannot read. It is left alone.
    Unreadable,
}

/// The owned form of [`Saved`], for the detached restore thread (P1.2).
/// Semantics are identical to `Saved`; the owned split exists because the
/// restore runs on its own thread and needs to move the snapshot out of
/// the injector thread.
enum OwnedSaved {
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
    /// Whether the landing was verifiable. `false` when the target app never
    /// exposed its accessibility tree (a sleeping host, no focused element):
    /// the keystroke paste still lands there, but nothing confirms it, so the
    /// clipboard is given back clean instead of retaining the dictation. Only
    /// a positive unreadable report clears this; unknown stays retained, which
    /// is today's behaviour everywhere else.
    retain_text: bool,
    paste: Option<PasteShortcut>,
    /// This take's generation for the detached clipboard restore (P1.2).
    /// Captured at the public entry; the restore thread skips its work if a
    /// newer take has bumped the global past it.
    token: u64,
}

/// The clipboard text to leave behind after a restore, if any.
///
/// Pure, so the retention rule is pinnable: retention needs both the user's
/// setting and a verifiable landing. Either one missing means clean.
fn retention_extra(keep_text: bool, retain_text: bool, text: &str) -> Option<&str> {
    (keep_text && retain_text).then_some(text)
}

/// The fire-time decision of the detached clipboard restore (P1.2).
///
/// Returns `true` if the restore should run, `false` if a newer dictation has
/// advanced the global past this take's token. A newer dictation owns the
/// clipboard now, and putting this take's snapshot back would corrupt it.
/// Extracted as a pure fn so the generation check is testable against the
/// exact production comparison and ordering, and pinnable by the mutation
/// gate (`scripts/check-mutation-kill.sh`).
pub(crate) fn restore_should_run(token: u64, latest: &AtomicU64) -> bool {
    latest.load(Ordering::Relaxed) <= token
}

/// Whether the hotkey's modifier keys are physically held, as the OS sees
/// them. A seam so the settle wait is unit-testable without real input
/// state (P1.1).
pub(crate) trait ModifierState {
    /// True while a hotkey-relevant modifier is physically held.
    fn hotkey_modifiers_held(&self) -> bool;
}

#[cfg(target_os = "macos")]
mod macos_modifier_state {
    use super::ModifierState;

    // `kCGEventSourceStateHIDSystemState`.
    const HID_SYSTEM_STATE: i32 = 0;
    // `CGEventFlags` is a `CGSOptionFlags` (u64) on both arm64 and x86_64.
    // The five masks below are their documented values.
    const FLAG_MASK_SHIFT: u64 = 0x200000;
    const FLAG_MASK_CONTROL: u64 = 0x400000;
    const FLAG_MASK_ALTERNATE: u64 = 0x800000;
    const FLAG_MASK_COMMAND: u64 = 0x1000000;
    const FLAG_MASK_SECONDARY_FN: u64 = 0x80000000;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceFlagsState(stateid: i32) -> u64;
    }

    pub(super) struct CgModifierState;

    impl ModifierState for CgModifierState {
        fn hotkey_modifiers_held(&self) -> bool {
            // SAFETY: `CGEventSourceFlagsState` takes only a state id and
            // returns a flag word; no pointers, no allocation, callable
            // from any thread.
            let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
            let hotkey =
                FLAG_MASK_COMMAND | FLAG_MASK_ALTERNATE | FLAG_MASK_CONTROL | FLAG_MASK_SHIFT;
            let secondary = FLAG_MASK_SECONDARY_FN;
            (flags & hotkey) != 0 || (flags & secondary) != 0
        }
    }
}

#[cfg(windows)]
mod windows_modifier_state {
    use super::ModifierState;

    const VK_CONTROL: i32 = 0x11;
    const VK_MENU: i32 = 0x12;
    const VK_SHIFT: i32 = 0x10;
    const VK_LWIN: i32 = 0x5B;
    const VK_RWIN: i32 = 0x5C;

    extern "system" {
        fn GetAsyncKeyState(vk_code: i32) -> i16;
    }

    pub(super) struct WinModifierState;

    impl ModifierState for WinModifierState {
        fn hotkey_modifiers_held(&self) -> bool {
            // SAFETY: `GetAsyncKeyState` takes a virtual-key code and
            // returns a state word; no pointers, callable from any thread.
            // The high bit is set while the key is down.
            [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
                .iter()
                .any(|vk| (unsafe { GetAsyncKeyState(*vk) as u16 } & 0x8000) != 0)
        }
    }
}

/// The platform's modifier-state source.
#[cfg(target_os = "macos")]
type PlatformModifierState = macos_modifier_state::CgModifierState;
#[cfg(target_os = "macos")]
const PLATFORM_MODIFIER_STATE: PlatformModifierState = macos_modifier_state::CgModifierState;
#[cfg(windows)]
type PlatformModifierState = windows_modifier_state::WinModifierState;
#[cfg(windows)]
const PLATFORM_MODIFIER_STATE: PlatformModifierState = windows_modifier_state::WinModifierState;
/// Fallback for platforms with no implementation: nothing is ever reported
/// as held, so the wait returns immediately.
#[cfg(not(any(target_os = "macos", windows)))]
struct NoModifierState;
#[cfg(not(any(target_os = "macos", windows)))]
impl ModifierState for NoModifierState {
    fn hotkey_modifiers_held(&self) -> bool {
        false
    }
}
#[cfg(not(any(target_os = "macos", windows)))]
type PlatformModifierState = NoModifierState;
#[cfg(not(any(target_os = "macos", windows)))]
const PLATFORM_MODIFIER_STATE: PlatformModifierState = NoModifierState;

/// Wait for the hotkey's modifiers to be physically released before the
/// simulated paste (P1.1). Checks once before sleeping, so the common case
/// (nothing held) returns with zero sleeps; otherwise polls every `poll`
/// until `max`. Returns `true` if released before the cap, `false` on
/// timeout.
fn wait_for_modifiers_released(source: &dyn ModifierState, max: Duration, poll: Duration) -> bool {
    let deadline = Instant::now() + max;
    loop {
        if !source.hotkey_modifiers_held() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(poll);
    }
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
    /// Order is the whole point. A macOS pasteboard is a **stack**: a normal
    /// Cmd+V reads the **last** item, and older items are reachable only via
    /// Paste Special. So:
    ///
    /// - The user's own items come **first**, in their original order, so a
    ///   copied file, image or multi-selection is still whole and still
    ///   pasteable.
    /// - `extra` (the dictation) goes **last**. It is the most recent thing in
    ///   the world, so Cmd+V right after dictating has to give the dictation.
    ///   Putting it first left the user's old copy at the back of the stack,
    ///   which is the "current" entry for a normal paste — that is how "it
    ///   keeps pasting the previous clipboard text" happened: the dictation
    ///   was on the pasteboard but unreachable without Paste Special.
    ///
    /// A dictation can never contain a NUL, so the two parts cannot collide.
    pub fn restored_with(&self, extra: Option<&str>) -> Vec<Vec<ClipboardPart>> {
        let mut out: Vec<Vec<ClipboardPart>> = Vec::with_capacity(self.items.len() + 1);
        for item in &self.items {
            if Self::is_dictation_item(item) {
                continue;
            }
            out.push(item.clone());
            if out.len() >= Self::MAX_ITEMS {
                break;
            }
        }
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

/// Test-only: a handle that never spawns the injector thread. The concurrency
/// tests build an `AppState` that is dropped immediately, and a live injector
/// thread would outlive the process exit and hang the test harness.
#[cfg(any(test, feature = "test-support"))]
impl TextInjector {
    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::channel::<Job>();
        Self { tx }
    }
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
                    if let Err(e) = inject(clipboard, enigo.as_mut(), &job, guard.as_ref()) {
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
    /// `retain_text` is the landing-verifiable bit: pass `false` when the
    /// target never exposed its accessibility tree, and the clipboard is
    /// restored clean even with `keep_text` on.
    pub fn inject(
        &self,
        text: String,
        restore_clipboard: bool,
        keep_text: bool,
        retain_text: bool,
        paste: Option<PasteShortcut>,
    ) {
        // P1.2: bump the generation before the job is queued, so a newer
        // dictation's restore cancels this one's pending restore. The token
        // is this take's last value: the restore thread skips its work when
        // the global has advanced past it.
        let token = RESTORE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send(Job {
            text,
            restore_clipboard,
            keep_text,
            retain_text,
            paste,
            token,
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
    guard: Option<&Arc<dyn ClipboardGuard>>,
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

    // 3. Settle: wait for the hotkey's modifiers to be physically released
    // (P1.1). Replaces the old fixed 250 ms sleep: nothing held means an
    // immediate return, and the 150 ms cap only fires while the user is
    // still pressing the hotkey.
    let _modifiers_released =
        wait_for_modifiers_released(&PLATFORM_MODIFIER_STATE, BEFORE_PASTE_CAP, MODIFIER_POLL);

    // 4. Paste. A failed paste must still restore: the clipboard is already
    // holding our text and the user's original is sitting in `saved`, so an
    // early return here would strand it. The restore after a failure is clean
    // (no extra item): a failed landing is unverifiable by definition, so
    // retention is off regardless of the setting.
    let paste_failed = if let Some(enigo) = enigo {
        let (mods, key) = match job.paste {
            // macOS cannot use Key::Unicode('v') — see paste_keys() below.
            Some(PasteShortcut::ControlV) if cfg!(target_os = "macos") => {
                (&[Key::Control][..], Key::Other(9))
            }
            Some(PasteShortcut::ControlV) => (&[Key::Control][..], Key::Unicode('v')),
            _ => paste_keys(),
        };
        press_combo(enigo, mods, key)
            .inspect_err(|e| warn!("[inject] paste shortcut failed: {e}"))
            .is_err()
    } else {
        // No keyboard simulation: leave the text on the clipboard and tell
        // the user (the caller can surface this).
        warn!("[inject] no keyboard simulation; text left on clipboard");
        false
    };

    // 5. + 6. Restore, and add the dictated text as an extra clipboard item
    // when retention applies (the setting plus a verifiable landing; see
    // `retention_extra`). Failures restore clean for the reason above.
    //
    // P1.2: this runs on a detached thread so the take returns as soon as
    // the paste keystroke is issued. The `BEFORE_RESTORE` wait now happens
    // inside the restore thread. A newer dictation (a higher generation
    // token) cancels this restore: the clipboard now belongs to the newer
    // take, and putting this snapshot back would corrupt it.
    if job.restore_clipboard {
        let extra = if paste_failed {
            None
        } else {
            retention_extra(job.keep_text, job.retain_text, job.text.as_str())
        };
        let extra = extra.map(|s| s.to_string());
        let saved = match saved {
            Saved::Snapshot(snap) => OwnedSaved::Snapshot(snap),
            Saved::Text(t) => OwnedSaved::Text(t),
            Saved::Image(img) => OwnedSaved::Image(img),
            Saved::Unreadable => OwnedSaved::Unreadable,
        };
        // The restore runs on a detached thread, so it needs ownership of
        // the guard. `guard` is a borrow of the `Arc<dyn ClipboardGuard>`
        // held by the injector thread's spawn closure; `Arc::clone` gives
        // the restore thread a real reference of its own (one atomic
        // increment, undone when the thread exits), so the referent stays
        // alive for the full 350 ms window no matter when the injector
        // thread itself shuts down.
        let guard_arc: Option<Arc<dyn ClipboardGuard>> = guard.cloned();
        let token = job.token;
        let _ = thread::Builder::new()
            .name("teletype-restore".into())
            .spawn(move || {
                // Let the target app read the clipboard before we touch it.
                thread::sleep(BEFORE_RESTORE);
                // A newer dictation owns the clipboard now; skip the restore
                // and retention handling entirely. Log the token values only,
                // never the text.
                if !restore_should_run(token, &RESTORE_GENERATION) {
                    debug!(
                        "[inject] restore skipped: newer dictation (token {token}, global now {})",
                        RESTORE_GENERATION.load(Ordering::Relaxed)
                    );
                    return;
                }
                match &saved {
                    OwnedSaved::Snapshot(_) => {
                        if let Some(snap) = snapshot_for_restore(&saved) {
                            if let Some(g) = &guard_arc {
                                g.restore(&snap, extra.as_deref());
                            } else {
                                match Clipboard::new() {
                                    Ok(mut cb) => {
                                        restore_portable(&mut cb, &snap, extra.as_deref())
                                    }
                                    Err(e) => warn!("[inject] restore: clipboard unavailable: {e}"),
                                }
                            }
                        }
                    }
                    OwnedSaved::Text(t) => {
                        let mut cb = match Clipboard::new() {
                            Ok(cb) => cb,
                            Err(e) => {
                                warn!("[inject] restore: clipboard unavailable: {e}");
                                return;
                            }
                        };
                        restore_portable(
                            &mut cb,
                            &ClipboardSnapshot {
                                parts: vec![],
                                items: vec![vec![ClipboardPart {
                                    uti: "public.utf8-plain-text".into(),
                                    data: t.clone().into_bytes(),
                                }]],
                            },
                            extra.as_deref(),
                        );
                    }
                    OwnedSaved::Image(img) => {
                        let mut cb = match Clipboard::new() {
                            Ok(cb) => cb,
                            Err(e) => {
                                warn!("[inject] restore: clipboard unavailable: {e}");
                                return;
                            }
                        };
                        restore_portable(
                            &mut cb,
                            &ClipboardSnapshot {
                                parts: vec![],
                                items: vec![vec![pack_raw_image(img)]],
                            },
                            extra.as_deref(),
                        );
                    }
                    OwnedSaved::Unreadable => {
                        warn!("[inject] clipboard was not readable before dictation; leaving it untouched");
                    }
                }
            })
            .inspect_err(|e| warn!("[inject] couldn't start restore thread: {e}"));
    } else if job.keep_text {
        // No restore wanted, but the user asked for the dictated text to end
        // up in the clipboard history. It is already there from step 2.
    }
    if paste_failed {
        return Err("paste shortcut failed; clipboard restored clean".to_string());
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

/// The snapshot a guard restore hands back to the platform.
///
/// `Some` only for a full snapshot save; text and image saves go through the
/// portable path, and an unreadable clipboard is left alone. The value is
/// always *what was saved* — a replacement (or empty) snapshot here is the
/// historical data-loss bug, and the mutation gate
/// (`scripts/check-mutation-kill.sh`, M9) pins it.
fn snapshot_for_restore(saved: &OwnedSaved) -> Option<ClipboardSnapshot> {
    match saved {
        OwnedSaved::Snapshot(s) => Some(s.clone()),
        _ => None,
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

    #[test]
    fn retention_needs_the_setting_and_a_verifiable_landing() {
        // The reference rule: an unverifiable landing never retains, even
        // with the setting on. A sleeping host gets the clipboard back clean.
        assert_eq!(retention_extra(true, true, "hi"), Some("hi"));
        assert_eq!(retention_extra(true, false, "hi"), None);
        assert_eq!(retention_extra(false, true, "hi"), None);
        assert_eq!(retention_extra(false, false, "hi"), None);
    }

    // --- P1.1: modifier-release wait ---

    /// A fake modifier source that is always released (nothing held) and
    /// counts how many times it is polled and how many poll-sleeps the wait
    /// takes.
    struct ReleasedSource {
        polls: std::sync::atomic::AtomicUsize,
    }

    impl ModifierState for ReleasedSource {
        fn hotkey_modifiers_held(&self) -> bool {
            self.polls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            false
        }
    }

    #[test]
    fn wait_for_modifiers_released_returns_immediately_when_none_held() {
        let source = ReleasedSource {
            polls: std::sync::atomic::AtomicUsize::new(0),
        };
        let start = Instant::now();
        let released = wait_for_modifiers_released(&source, BEFORE_PASTE_CAP, MODIFIER_POLL);
        let elapsed = start.elapsed();
        assert!(released, "nothing held should return true");
        assert!(
            elapsed < Duration::from_millis(20),
            "immediate return should take < 20 ms, took {elapsed:?}"
        );
        assert_eq!(
            source.polls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "should check once before sleeping"
        );
    }

    /// A fake modifier source that always reports a modifier held.
    struct HeldForeverSource;

    impl ModifierState for HeldForeverSource {
        fn hotkey_modifiers_held(&self) -> bool {
            true
        }
    }

    #[test]
    fn wait_for_modifiers_released_times_out_when_held_forever() {
        let source = HeldForeverSource;
        let start = Instant::now();
        let released = wait_for_modifiers_released(
            &source,
            Duration::from_millis(30),
            Duration::from_millis(5),
        );
        let elapsed = start.elapsed();
        assert!(!released, "held forever should time out");
        // 30 ms floor: the timeout must actually elapse. 150 ms ceiling:
        // a real regression (no timeout) blocks far longer; the ceiling is
        // 5x the 30 ms budget for CI-runner scheduling jitter (a 60 ms
        // ceiling flaked on a loaded runner, 2026-10-18).
        assert!(
            elapsed >= Duration::from_millis(30) && elapsed < Duration::from_millis(150),
            "should time out in ~30 ms, took {elapsed:?}"
        );
    }

    // --- P1.2: restore generation token ---

    #[test]
    fn restore_skipped_when_a_newer_dictation_started() {
        let global = AtomicU64::new(0);
        // Simulate: take 1 bumps to token 1, take 2 bumps to token 2.
        global.fetch_add(1, Ordering::Relaxed);
        let token1 = global.load(Ordering::Relaxed);
        global.fetch_add(1, Ordering::Relaxed);
        let token2 = global.load(Ordering::Relaxed);
        // Take 1's restore fires: global (2) > token1 (1) -> skip.
        assert!(
            !restore_should_run(token1, &global),
            "take 1's restore must be skipped when take 2 has started"
        );
        // Take 2's restore fires: global (2) == token2 (2) → run.
        assert!(
            restore_should_run(token2, &global),
            "take 2's restore must run when it is the latest"
        );
    }

    #[test]
    fn restore_fires_and_retains_when_no_newer() {
        let global = AtomicU64::new(0);
        // Take 1 bumps to token 1.
        global.fetch_add(1, Ordering::Relaxed);
        let token = global.load(Ordering::Relaxed);
        // No newer take: global (1) == token (1) -> run.
        assert!(
            restore_should_run(token, &global),
            "restore must run when no newer dictation has started"
        );
        // Retention: the extra item is applied when both the setting and a
        // verifiable landing are present.
        assert_eq!(retention_extra(true, true, "kept"), Some("kept"));
        // A failed paste (unverifiable) restores clean.
        assert_eq!(retention_extra(true, false, "kept"), None);
    }

    // --- restore content: the guard gets what was saved ---

    #[test]
    fn the_guard_restore_gets_the_saved_snapshot_not_a_replacement() {
        let snap = ClipboardSnapshot {
            parts: vec![],
            items: vec![vec![ClipboardPart {
                uti: ClipboardSnapshot::TEXT.into(),
                data: b"the user's copy".to_vec(),
            }]],
        };
        let owned = OwnedSaved::Snapshot(snap.clone());
        let for_guard = snapshot_for_restore(&owned)
            .expect("a snapshot save must hand a snapshot to the guard");
        assert_eq!(for_guard, snap, "the guard must get exactly what was saved");
        assert!(
            !for_guard.is_empty(),
            "an empty replacement is the data-loss bug"
        );
        assert!(
            snapshot_for_restore(&OwnedSaved::Unreadable).is_none(),
            "an unreadable clipboard does not use the snapshot path"
        );
    }
}
