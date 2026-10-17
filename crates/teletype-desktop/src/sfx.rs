//! P8.5: start/stop dictation cues on Windows via `PlaySoundW`.
//!
//! The macOS equivalent is the Obj-C `teletype_play_system_sound` bridge
//! (NSSound, named system sounds like "Tink"). Windows has no named system
//! sound API in the Win32 surface we use; `PlaySoundW` with
//! `SND_SYSTEM` plays the *system* event sounds, and the two that map onto
//! a record start/stop are the "Asterisk" and "Beep" event aliases
//! (always present on every Windows install, no file to ship).
//!
//! The cue is best-effort throughout, exactly like the macOS path: a
//! failure to play must never interrupt a dictation, so [`play_start`] and
//! [`play_stop`] log and drop.

use windows::{
    core::PCWSTR,
    Win32::System::Diagnostics::Debug::PlaySoundW,
};

/// PlaySoundW flags: system sound + async (return immediately; the sound
/// plays on the system thread).
const FLAGS: u32 = windows::Win32::System::Diagnostics::Debug::SND_SYSTEM
    | windows::Win32::System::Diagnostics::Debug::SND_ASYNC;

/// Plays a system sound alias (e.g. "Asterisk"). Returns `Ok(())` when the
/// OS accepted the request; the sound may still fail to be audible (no
/// audio device, muted session) and that is not distinguishable here, so
/// callers treat this as best-effort.
fn play_system(alias: &str) -> Result<(), String> {
    let wide: Vec<u16> = alias.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the
    // call; PlaySoundW copies what it needs.
    let ok = unsafe { PlaySoundW(PCWSTR::from_raw(wide.as_ptr()), std::ptr::null_mut(), FLAGS) };
    if ok.as_bool() {
        Ok(())
    } else {
        Err(format!("PlaySoundW({alias}) failed"))
    }
}

/// The take-start cue: the "Asterisk" system sound (the Windows
/// equivalent of the macOS "Tink" default).
pub fn play_start() {
    if let Err(e) = play_system("Asterisk") {
        tracing::info!("recording start cue unavailable: {e}");
    }
}

/// The take-stop cue: the "Beep" system sound.
pub fn play_stop() {
    if let Err(e) = play_system("Beep") {
        tracing::info!("recording stop cue unavailable: {e}");
    }
}
