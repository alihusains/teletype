//! Windows key-event tap for the typing AutoText watcher.
//!
//! A `WH_KEYBOARD_LL` low-level hook runs on its own thread with its own
//! message loop (the hook only fires while that thread pumps messages).
//! Every key-down is mapped to either the space/enter sentinel or the
//! unshifted character's codepoint, and sent over the channel the watcher
//! thread in `typing.rs` drains.
//!
//! The hook has a strict time budget (Windows kills low-level hooks that
//! take too long), so the callback only does a map lookup and a
//! non-blocking channel send.

use std::sync::mpsc::Sender;
use std::sync::{Arc, atomic::AtomicBool};

use windows::{
    Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG,
            WH_KEYBOARD_LL,
        },
    },
};

use super::{KC_ENTER, KC_SPACE, TAP_TX};

const VK_SPACE: u32 = 0x20;
const VK_RETURN: u32 = 0x0D;
const WM_KEYDOWN: u32 = 0x0100;

type KbdHookProc = unsafe extern "system" fn(isize, WPARAM, LPARAM) -> LRESULT;

/// Layout of the structure Windows passes in `l_param` for keyboard hooks.
#[repr(C)]
struct KBDLLHOOKSTRUCT {
    vkCode: u32,
    scanCode: u32,
    flags: u32,
    time: u32,
    dwExtraInfo: usize,
}

/// Installs the hook and pumps messages until `unhook` is set. Runs on the
/// caller's thread; `typing.rs` spawns it on a dedicated thread.
pub fn run_hook(tx: Sender<i32>, unhook: Arc<AtomicBool>) {
    // SAFETY: GetModuleHandleW(NULL) returns the current module handle and
    // does not fail.
    let module = unsafe { GetModuleHandleW(None) };

    let hook = match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(kbd_hook), module, 0) } {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("typing watcher (windows): SetWindowsHookExW failed: {e}");
            return;
        }
    };

    let mut msg = MSG::default();
    while !unhook.load(std::sync::atomic::Ordering::SeqCst) {
        // GetMessageW blocks until a message arrives (hook callbacks are
        // delivered as messages to this thread) or the thread is killed.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        // The hook callback runs synchronously inside GetMessageW; the
        // message itself needs no further processing.
        let _ = msg;
    }

    unsafe {
        UnhookWindowsHookEx(hook);
    }
    tracing::info!("typing watcher (windows): hook removed");
}

/// The hook callback: fires on the hook thread for every key event.
unsafe extern "system" fn kbd_hook(n_code: isize, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 && w_param.0 as u32 == WM_KEYDOWN {
        // SAFETY: Windows guarantees l_param points to a valid
        // KBDLLHOOKSTRUCT for keyboard hook messages.
        let kb: &KBDLLHOOKSTRUCT = unsafe { &*(l_param.0 as *const KBDLLHOOKSTRUCT) };
        if let Some(kc) = vk_to_channel_code(kb.vkCode) {
            if let Some(tx) = TAP_TX.get() {
                let _ = tx.send(kc);
            }
        }
    }
    // SAFETY: always forward to keep the hook chain alive.
    unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
}

/// Maps a Windows virtual key code to the channel encoding: the
/// space/enter sentinels, or the unshifted character's codepoint.
fn vk_to_channel_code(vk: u32) -> Option<i32> {
    match vk {
        VK_SPACE => Some(KC_SPACE),
        VK_RETURN => Some(KC_ENTER),
        0x41..=0x5A => Some((b'a' + (vk - 0x41)) as i32),
        0x30..=0x39 => Some((b'0' + (vk - 0x30)) as i32),
        0x2F => Some(b'/' as i32),
        _ => None,
    }
}
