//! Text injection into the focused application.
//!
//! Strategy (safest, most reliable on both macOS and Windows):
//! 1. Save the current clipboard (text or image).
//! 2. Put our text on the clipboard.
//! 3. Wait a short settle time so the hotkey's modifiers are released.
//! 4. Simulate the paste shortcut (⌘V / Ctrl+V).
//! 5. Wait for the target app to read the clipboard.
//! 6. Restore the saved clipboard.
//!
//! One dedicated thread owns the clipboard for the process lifetime, because
//! on some platforms the clipboard is served by the process that set it.

use std::{
    sync::mpsc::{self, Sender},
    thread,
    time::Duration,
};

use arboard::{Clipboard, ImageData};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::platform::PasteShortcut;

/// Let go of the hotkey's modifiers before the simulated paste.
const BEFORE_PASTE: Duration = Duration::from_millis(250);
/// Let the target app read the clipboard before restoring it.
const BEFORE_RESTORE: Duration = Duration::from_millis(350);

enum Saved {
    Text(String),
    Image(ImageData<'static>),
    Empty,
}

struct Job {
    text: String,
    restore_clipboard: bool,
    paste: Option<PasteShortcut>,
}

/// Handle to the injector thread. Cloning is cheap.
#[derive(Clone)]
pub struct TextInjector {
    tx: Sender<Job>,
}

impl TextInjector {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = thread::Builder::new()
            .name("teletype-inject".into())
            .spawn(move || {
                let mut clipboard: Option<Clipboard> = None;
                let mut enigo: Option<Enigo> = None;
                for job in rx {
                    if clipboard.is_none() {
                        clipboard = Clipboard::new()
                            .inspect_err(|e| eprintln!("[inject] clipboard unavailable: {e}"))
                            .ok();
                    }
                    let Some(clipboard) = clipboard.as_mut() else {
                        continue;
                    };
                    if enigo.is_none() {
                        enigo = Enigo::new(&Settings::default())
                            .inspect_err(|e| {
                                eprintln!("[inject] keyboard simulation unavailable: {e}")
                            })
                            .ok();
                    }
                    if let Err(e) = inject(clipboard, enigo.as_mut(), &job) {
                        eprintln!("[inject] {e}");
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("[inject] couldn't start injector thread: {e}");
        }
        Self { tx }
    }

    /// Queues `text` for insertion. Returns immediately.
    pub fn inject(&self, text: String, restore_clipboard: bool, paste: Option<PasteShortcut>) {
        let _ = self.tx.send(Job {
            text,
            restore_clipboard,
            paste,
        });
    }
}

impl Default for TextInjector {
    fn default() -> Self {
        Self::spawn()
    }
}

fn inject(clipboard: &mut Clipboard, enigo: Option<&mut Enigo>, job: &Job) -> Result<(), String> {
    // 1. Save clipboard.
    let saved = if job.restore_clipboard {
        match clipboard.get_text() {
            Ok(text) if !text.is_empty() => Saved::Text(text),
            Ok(_) => Saved::Empty,
            Err(_) => match clipboard.get_image() {
                Ok(img) => Saved::Image(img),
                Err(_) => Saved::Empty,
            },
        }
    } else {
        Saved::Empty
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
            Some(PasteShortcut::ControlV) => (&[Key::Control][..], Key::Unicode('v')),
            _ => paste_keys(),
        };
        press_combo(enigo, mods, key).map_err(|e| format!("Paste shortcut failed: {e}"))?;
    } else {
        // No keyboard simulation: leave the text on the clipboard and tell
        // the user (the caller can surface this).
        eprintln!("[inject] no keyboard simulation; text left on clipboard");
    }

    // 5. Let the target read it.
    thread::sleep(BEFORE_RESTORE);

    // 6. Restore.
    if job.restore_clipboard {
        match saved {
            Saved::Text(t) => {
                let _ = clipboard.set_text(&t);
            }
            Saved::Image(img) => {
                let _ = clipboard.set_image(img);
            }
            Saved::Empty => {
                let _ = clipboard.clear();
            }
        }
    }
    Ok(())
}

/// The default paste keys for the current platform.
fn paste_keys() -> (&'static [Key], Key) {
    if cfg!(target_os = "macos") {
        (&[Key::Meta][..], Key::Unicode('v'))
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
        assert_eq!(key, Key::Unicode('v'));
        if cfg!(target_os = "macos") {
            assert_eq!(mods, &[Key::Meta]);
        } else {
            assert_eq!(mods, &[Key::Control]);
        }
    }
}
