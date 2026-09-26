//! Direct-typing AutoText watcher.
//!
//! When enabled in settings, a background thread watches for `/trigger`
//! sequences typed in any application and expands them deterministically —
//! no LLM involved. On macOS a passive CGEventTap (`typing_tap.m`) reports
//! every key-down; the tap callback maps the key code to a character and
//! appends it to a buffer. The watcher thread polls the buffer every 50ms;
//! when it ends with a known trigger followed by space or enter, it
//! backspaces the trigger + delimiter and types the replacement via enigo.
//!
//! This is intentionally conservative: it only activates when
//! `settings.typing_autotext_enabled` is true. Requires Accessibility
//! permission (the app already needs it for typing).

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use tauri::AppHandle;

use teletype_core::autotext::AutoTextStore;

#[cfg(target_os = "windows")]
mod typing_windows;

static WATCHER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The AutoText store, shared with the watcher. Set once at startup from
/// `lib.rs` (the `AppState` keeps its own copy for the Tauri commands; this
/// Arc mirrors it so the watcher never touches Tauri state).
static STORE: OnceLock<Arc<Mutex<AutoTextStore>>> = OnceLock::new();

/// The channel the tap callback feeds; owned by the watcher thread.
pub static TAP_TX: OnceLock<Sender<i32>> = OnceLock::new();

/// Windows only: shared flag the hook thread polls to unhook.
#[cfg(target_os = "windows")]
static HOOK_UNHOOK: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Max characters kept in the key buffer (triggers are at most 64).
const BUFFER_CAP: usize = 128;

/// Common channel encoding for "space" and "enter": the same values the
/// macOS virtual key codes happen to use, so the watcher's drain loop is
/// platform-agnostic. On macOS the channel carries virtual key codes; on
/// Windows it carries these two sentinels plus `char` codepoints.
pub const KC_SPACE: i32 = 49;
pub const KC_ENTER: i32 = 36;

/// Decodes one channel value into a buffer character, or `None` for a key
/// that breaks a trigger sequence.
fn decode_key(kc: i32) -> Option<char> {
    if kc == KC_SPACE {
        return Some(' ');
    }
    if kc == KC_ENTER {
        return Some('\n');
    }
    #[cfg(target_os = "macos")]
    {
        key_code_to_char(kc)
    }
    #[cfg(target_os = "windows")]
    {
        char::from_u32(kc as u32)
    }
}

/// Maps a macOS virtual key code to the unshifted character it produces.
/// Returns `None` for keys that never appear in a trigger.
#[cfg(target_os = "macos")]
fn key_code_to_char(kc: i32) -> Option<char> {
    match kc {
        0 => Some('a'),
        1 => Some('s'),
        2 => Some('d'),
        3 => Some('f'),
        4 => Some('h'),
        5 => Some('g'),
        6 => Some('z'),
        7 => Some('x'),
        8 => Some('b'),
        9 => Some('v'),
        11 => Some('q'),
        12 => Some('c'),
        13 => Some('w'),
        14 => Some('e'),
        15 => Some('r'),
        16 => Some('y'),
        17 => Some('t'),
        18 => Some('1'),
        19 => Some('2'),
        20 => Some('3'),
        21 => Some('4'),
        22 => Some('6'),
        23 => Some('5'),
        25 => Some('8'),
        26 => Some('7'),
        28 => Some('9'),
        29 => Some('0'),
        31 => Some('o'),
        32 => Some('p'),
        34 => Some('i'),
        37 => Some('l'),
        38 => Some('j'),
        40 => Some('k'),
        44 => Some('/'),
        45 => Some('n'),
        46 => Some('m'),
        49 => Some(' '),
        36 => Some('\n'),
        _ => None,
    }
}

/// Registers the shared AutoText store used by the watcher to look up
/// triggers. Called once at startup; subsequent calls are ignored.
pub fn set_autotext_store(store: Arc<Mutex<AutoTextStore>>) {
    STORE.set(store).ok();
}

/// Starts the typing watcher (no-op if already running).
pub fn start(_app: AppHandle) {
    // The watcher is started lazily on first enable to avoid holding
    // Accessibility permission when the user hasn't opted in.
    WATCHER_ACTIVE.store(false, Ordering::SeqCst);
}

/// Enables or disables the typing watcher at runtime.
pub fn set_enabled(app: &AppHandle, enabled: bool) {
    let was = WATCHER_ACTIVE.swap(enabled, Ordering::SeqCst);
    if enabled && !was {
        let app = app.clone();
        thread::Builder::new()
            .name("teletype-typing".into())
            .spawn(move || typing_loop(app))
            .ok();
    }
    #[cfg(target_os = "windows")]
    {
        // The hook thread polls this flag; disable it when the watcher goes
        // off so the low-level hook is released promptly.
        if let Some(flag) = HOOK_UNHOOK.get() {
            flag.store(!enabled, Ordering::SeqCst);
        }
    }
}

fn typing_loop(_app: AppHandle) {
    let (tx, rx) = mpsc::channel::<i32>();
    if TAP_TX.set(tx.clone()).is_err() {
        // A previous watcher is still active; don't fight it.
        return;
    }

    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn teletype_typing_tap_start(cb: extern "C" fn(i64)) -> i32;
        }
        // SAFETY: installs a passive CGEventTap on the main run loop; the
        // callback is a plain C function pointer with no Rust state.
        let ok = unsafe { teletype_typing_tap_start(on_key_down) } != 0;
        if !ok {
            tracing::warn!(
                "typing watcher: event tap could not be created \
                 (grant Teletype Accessibility access)"
            );
            return;
        }
    }
    #[cfg(target_os = "windows")]
    {
        let unhook = Arc::new(AtomicBool::new(false));
        HOOK_UNHOOK.set(unhook.clone()).ok();
        let hook_tx = tx.clone();
        let hook_ok = thread::Builder::new()
            .name("teletype-typing-hook".into())
            .spawn(move || typing_windows::run_hook(hook_tx, unhook))
            .is_ok();
        if !hook_ok {
            tracing::warn!("typing watcher (windows): could not spawn hook thread");
            return;
        }
        // Give the hook a moment to install before we start draining.
        thread::sleep(Duration::from_millis(100));
        drop(tx); // the hook thread owns the only real sender now
    }

    tracing::info!("typing watcher started");
    let mut buf = String::new();
    while WATCHER_ACTIVE.load(Ordering::SeqCst) {
        // Drain every keystroke since the last poll into the buffer.
        // A delimiter (space/enter) completes whatever was typed before it;
        // any other unrecognized key breaks the sequence.
        let mut completed: Option<String> = None;
        while let Ok(kc) = rx.try_recv() {
            match kc {
                KC_SPACE | KC_ENTER => {
                    completed = Some(std::mem::take(&mut buf));
                }
                _ => match decode_key(kc) {
                    Some(c) => buf.push(c),
                    None => buf.clear(),
                },
            }
            if buf.len() > BUFFER_CAP {
                // Keep only the tail: a trigger is at most 64 chars.
                let start = buf.len() - BUFFER_CAP;
                let tail = buf[start..].to_string();
                buf = tail;
            }
        }

        // If the text before the delimiter is a known trigger, expand it.
        if let Some(text) = completed {
            if let Some(replacement) = lookup_replacement(&text) {
                expand(&text, &replacement);
            }
        }

        thread::sleep(Duration::from_millis(50));
    }

    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn teletype_typing_tap_stop();
        }
        // SAFETY: removes the tap installed by teletype_typing_tap_start.
        unsafe { teletype_typing_tap_stop() }
    }
    let _ = TAP_TX.get().cloned();
    tracing::info!("typing watcher stopped");
}

/// C callback from the CGEventTap (main run loop): one key-down per call.
extern "C" fn on_key_down(kc: i64) {
    if let Some(tx) = TAP_TX.get() {
        // Non-blocking: the watcher drains the channel every 50ms.
        let _ = tx.send(kc as i32);
    }
}

/// Looks up `text` (the buffered trigger, e.g. `/email`) in the shared
/// store. Returns the replacement when the entry is enabled and applies
/// everywhere.
fn lookup_replacement(trigger: &str) -> Option<String> {
    let store = STORE.get()?;
    let store = store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = store.find_by_trigger(trigger)?;
    if !entry.enabled {
        return None;
    }
    // V1: only expand entries that apply everywhere. Application-scoped
    // entries need frontmost-app detection, which is out of scope here.
    if !matches!(entry.scope, teletype_core::autotext::AutoTextScope::Everywhere) {
        return None;
    }
    // Placeholder expansion ({{date}}/{{time}}/{{clipboard}}) happens at
    // expansion time, same as the voice pipeline.
    Some(teletype_core::autotext::placeholders::expand_placeholders(
        &entry.replacement,
    ))
}

/// Backspaces `trigger` + the delimiter, then types `replacement`.
/// Returns true on success.
fn expand(trigger: &str, replacement: &str) -> bool {
    let mut enigo = match Enigo::new(&EnigoSettings::default()) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("typing watcher: enigo init failed: {e}");
            return false;
        }
    };
    let mut ok = true;
    // Backspace the trigger and the delimiter (space or enter).
    let presses = trigger.chars().count() + 1;
    for _ in 0..presses {
        if enigo.key(Key::Backspace, Direction::Click).is_err() {
            ok = false;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    if ok {
        // Let the target app settle before the new text arrives.
        thread::sleep(Duration::from_millis(30));
        if enigo.text(replacement).is_err() {
            tracing::warn!("typing watcher: enigo.text failed");
            ok = false;
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two lookup tests mutate the process-global STORE, so they must
    /// not run in parallel with each other.
    static STORE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn maps_letters_slash_space_enter() {
        assert_eq!(key_code_to_char(0), Some('a'));
        assert_eq!(key_code_to_char(11), Some('q'));
        assert_eq!(key_code_to_char(6), Some('z'));
        assert_eq!(key_code_to_char(44), Some('/'));
        assert_eq!(key_code_to_char(49), Some(' '));
        assert_eq!(key_code_to_char(36), Some('\n'));
        assert_eq!(key_code_to_char(53), None); // escape
    }

    #[test]
    fn maps_digits() {
        assert_eq!(key_code_to_char(29), Some('0'));
        assert_eq!(key_code_to_char(18), Some('1'));
        assert_eq!(key_code_to_char(28), Some('9'));
    }

    #[test]
    fn lookup_finds_enabled_everywhere_trigger() {
        let _guard = STORE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut store = AutoTextStore::default();
        store
            .insert(teletype_core::autotext::AutoTextEntry::new("/email", "a@b.c"))
            .unwrap();
        STORE.set(Arc::new(Mutex::new(store))).ok();

        assert_eq!(lookup_replacement("/email"), Some("a@b.c".into()));
        assert_eq!(lookup_replacement("/EMAIL"), Some("a@b.c".into()));
        assert_eq!(lookup_replacement("/unknown"), None);
    }

    #[test]
    fn lookup_skips_disabled_and_scoped_entries() {
        let _guard = STORE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut store = AutoTextStore::default();
        let mut disabled = teletype_core::autotext::AutoTextEntry::new("/off", "x");
        disabled.enabled = false;
        store.insert(disabled).unwrap();
        let mut scoped = teletype_core::autotext::AutoTextEntry::new("/g", "y");
        scoped.scope =
            teletype_core::autotext::AutoTextScope::Application("com.google.gmail".into());
        store.insert(scoped).unwrap();
        STORE.set(Arc::new(Mutex::new(store))).ok();

        assert_eq!(lookup_replacement("/off"), None);
        assert_eq!(lookup_replacement("/g"), None);
    }
}
