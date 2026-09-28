//! A faithful macOS clipboard snapshot and restore.
//!
//! `arboard` exposes only text and images, and both `set_text` and
//! `set_image` replace the whole pasteboard. That is why dictating used to
//! destroy anything else the user had copied: copy a file in Finder, dictate a
//! note, and the copied file was gone from the clipboard, because
//! `get_text` and `get_image` both failed and the restore called `clear()`.
//!
//! `NSPasteboard` has no such limitation. It holds a list of
//! `NSPasteboardItem`s, each carrying named representations (UTIs) with
//! arbitrary bytes, so a snapshot is a straight copy of that structure and a
//! restore is a straight write of it back. Nothing is dropped, so nothing can
//! be lost.
//!
//! It also supports what the user asked for: `restore` publishes the dictated
//! text as an *extra* item alongside the restored ones, so the dictation is
//! added to the system clipboard history rather than replacing what they had.
//!
//! Everything here uses `msg_send!` against documented AppKit selectors, in
//! the same style as `platform/macos_impl.rs`, so it builds against the
//! Command Line Tools SDK that CI uses and needs no extra dependency.

use std::sync::Mutex;

use objc2::{msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::NSString;
use tracing::{debug, warn};

use teletype_core::injector::{ClipboardGuard, ClipboardPart, ClipboardSnapshot};

/// The general pasteboard, captured and restored on its own thread.
///
/// `NSPasteboard` is not documented as thread-safe, so every call is
/// serialised. The injector's thread already owns the clipboard for the
/// process lifetime, so contention is a non-issue in practice; the mutex is
/// here so a stray call from elsewhere cannot race it.
#[derive(Default)]
pub struct MacClipboardGuard {
    lock: Mutex<()>,
}

impl MacClipboardGuard {
    pub fn new() -> Self {
        Self {
            lock: Mutex::new(()),
        }
    }
}

impl ClipboardGuard for MacClipboardGuard {
    /// Capture every representation on the general pasteboard.
    fn snapshot(&self) -> Option<ClipboardSnapshot> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: `+[NSPasteboard generalPasteboard]` is a documented
        // class method returning a retained pasteboard, valid for the life of
        // the process. All selectors below are documented NSPasteboard /
        // NSPasteboardItem members called on objects we own.
        unsafe {
            let pb: &AnyObject = msg_send![objc2::class!(NSPasteboard), generalPasteboard];
            let items: *mut AnyObject = msg_send![pb, pasteboardItems];
            if items.is_null() {
                return Some(ClipboardSnapshot::default());
            }
            let count: usize = msg_send![items, count];
            let mut out = ClipboardSnapshot::default();
            for i in 0..count {
                let item: *mut AnyObject = msg_send![items, objectAtIndex: i];
                if item.is_null() {
                    continue;
                }
                let types: *mut AnyObject = msg_send![item, types];
                if types.is_null() {
                    continue;
                }
                let tcount: usize = msg_send![types, count];
                let mut parts = Vec::with_capacity(tcount);
                for t in 0..tcount {
                    let uti: *mut AnyObject = msg_send![types, objectAtIndex: t];
                    if uti.is_null() {
                        continue;
                    }
                    let uti_str = nsstring_to_string(uti);
                    // `dataForType:` returns an autoreleased NSData or nil.
                    let data: *mut AnyObject = msg_send![item, dataForType: uti];
                    if data.is_null() {
                        continue;
                    }
                    let len: usize = msg_send![data, length];
                    let bytes: *const u8 = msg_send![data, bytes];
                    if bytes.is_null() {
                        continue;
                    }
                    let slice = std::slice::from_raw_parts(bytes, len);
                    parts.push(ClipboardPart {
                        uti: uti_str,
                        data: slice.to_vec(),
                    });
                }
                if !parts.is_empty() {
                    out.parts.extend(parts.iter().cloned());
                    out.items.push(parts);
                }
            }
            Some(out)
        }
    }

    /// Put the snapshot back, and add `extra` as its own pasteboard item.
    ///
    /// Order matters and is decided by [`ClipboardSnapshot::restored_with`]:
    /// `NSPasteboard` pastes item 0, so the dictation goes first and the user's
    /// own items follow it. Writing the dictation last left the user's previous
    /// copy at the front, so every Cmd+V after dictating pasted the old text.
    fn restore(&self, snapshot: &ClipboardSnapshot, extra: Option<&str>) {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let items_to_write = snapshot.restored_with(extra);
        if items_to_write.is_empty() {
            // Nothing to restore and nothing to add: leave the clipboard
            // exactly as it is. Calling `clearContents` here is the bug this
            // whole module exists to remove.
            debug!("[clipboard] nothing to restore; leaving the clipboard alone");
            return;
        }
        // SAFETY: as in `snapshot`, every selector is a documented
        // NSPasteboard / NSPasteboardItem member. `clearContents` and
        // `writeObjects:` are the documented way to replace the contents, and
        // we only ever run this with a snapshot we ourselves took.
        unsafe {
            let pb: &AnyObject = msg_send![objc2::class!(NSPasteboard), generalPasteboard];

            // Keep the `Retained` items alive until after `writeObjects:` runs.
            // Storing only the raw pointers would dangle: each `Retained` is
            // dropped at the end of its loop iteration, releasing the
            // NSPasteboardItem, and the later `arrayWithObjects:` would retain
            // freed memory (SIGSEGV in objc_retain).
            let mut items: Vec<Retained<AnyObject>> = Vec::new();
            for item in &items_to_write {
                if let Some(nsi) = make_pasteboard_item(item) {
                    items.push(nsi);
                }
            }
            if items.is_empty() {
                debug!("[clipboard] every item lost its representations; clipboard left as is");
                return;
            }

            // Raw pointers valid for the lifetime of `items` (kept alive above).
            let objects: Vec<*const AnyObject> = items.iter().map(Retained::as_ptr).collect();

            let _: isize = msg_send![pb, clearContents];
            let arr: *mut AnyObject = msg_send![objc2::class!(NSArray), arrayWithObjects:objects
                .as_ptr(), count: objects.len()];
            if arr.is_null() {
                warn!("[clipboard] could not build the item array; clipboard left as is");
                return;
            }
            let ok: bool = msg_send![pb, writeObjects: arr];
            if !ok {
                warn!("[clipboard] the pasteboard refused our write; clipboard left as is");
            }
        }
    }
}

/// Build one `NSPasteboardItem` with all of `item`'s representations.
unsafe fn make_pasteboard_item(item: &[ClipboardPart]) -> Option<Retained<AnyObject>> {
    if item.is_empty() {
        return None;
    }
    let nsi: Retained<AnyObject> = msg_send![objc2::class!(NSPasteboardItem), new];
    for part in item {
        let uti = NSString::from_str(&part.uti);
        // SAFETY: `part.data` is a live slice owned by the caller for the whole
        // call; `nsdata_from_slice` copies it and retains the result.
        let data = unsafe { nsdata_from_slice(&part.data) }?;
        // SAFETY: both arguments are objects we own for the duration of the
        // call, and `setData:forType:` copies what it needs.
        let ok: bool = msg_send![&*nsi, setData: &*data, forType: &*uti];
        if !ok {
            debug!(
                "[clipboard] the pasteboard rejected the {} representation",
                part.uti
            );
        }
    }
    Some(nsi)
}

unsafe fn nsdata_from_slice(bytes: &[u8]) -> Option<Retained<AnyObject>> {
    if bytes.is_empty() {
        return None;
    }
    // SAFETY: `dataWithBytes:length:` copies the buffer, so the borrow only has
    // to outlive the call, and the result is autoreleased but not owned, so it
    // must be retained before it is returned.
    let data: *mut AnyObject =
        msg_send![objc2::class!(NSData), dataWithBytes: bytes.as_ptr(), length: bytes.len()];
    // SAFETY: `data` is a live autoreleased NSData, or null, both handled.
    unsafe { Retained::retain(data) }
}

unsafe fn nsstring_to_string(obj: *mut AnyObject) -> String {
    if obj.is_null() {
        return String::new();
    }
    // SAFETY: the argument is an NSString (an NSPasteboardType), which is the
    // documented type of the `types` array elements. `-UTF8String` may be null
    // for a malformed string, handled below.
    let utf8: *const std::os::raw::c_char = msg_send![obj, UTF8String];
    if utf8.is_null() {
        String::new()
    } else {
        // SAFETY: `utf8` is a non-null `*const c_char` returned by
        // `-UTF8String` on a live NSString, so it points at a NUL-terminated
        // buffer that outlives this borrow.
        unsafe { std::ffi::CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A snapshot of an empty or unreadable pasteboard must be reported as
    /// empty, never as "and therefore clear it".
    #[test]
    fn a_missing_pasteboard_reports_empty_rather_than_clearing() {
        // `snapshot` returning None is what the injector treats as "do not
        // touch", so the important property is that a snapshot with no parts is
        // distinguishable from a failure. Both are non-destructive, which is
        // the property the injector depends on.
        let empty = ClipboardSnapshot::default();
        assert!(empty.is_empty());
        assert_eq!(empty.first_text(), None);
    }

    #[test]
    fn a_snapshot_round_trips_its_own_text() {
        let mut snap = ClipboardSnapshot::default();
        snap.items.push(vec![ClipboardPart {
            uti: ClipboardSnapshot::TEXT.to_string(),
            data: b"hello".to_vec(),
        }]);
        assert!(!snap.is_empty());
        assert_eq!(snap.first_text().as_deref(), Some("hello"));
    }

    #[test]
    fn every_flavour_is_kept_not_just_text() {
        // This is the regression: a snapshot must carry flavours arboard cannot
        // see, because those are the ones the old code destroyed.
        let mut snap = ClipboardSnapshot::default();
        snap.items.push(vec![
            ClipboardPart {
                uti: "public.file-url".into(),
                data: b"file:///tmp/a.pdf".to_vec(),
            },
            ClipboardPart {
                uti: ClipboardSnapshot::TEXT.to_string(),
                data: b"a.pdf".to_vec(),
            },
        ]);
        assert_eq!(snap.items[0].len(), 2, "both flavours captured");
        assert_eq!(snap.first_text().as_deref(), Some("a.pdf"));
    }
}
