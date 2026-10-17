//! Phase 5.6: audio ducking — lower the default output device's volume while
//! a take is recording, so playback audio stops bleeding into the
//! microphone (plan §5.6, "Echo and other audio playing").
//!
//! ## Shape
//!
//! [`AudioDucker`] is the seam the dictation controller holds (behind a
//! `Box<dyn AudioDucker>`): `duck(level)` on a successful take start,
//! `restore()` on every stop path. The production macOS implementation
//! ([`CoreAudioDucker`]) writes `kAudioDevicePropertyVolumeScalar` on the
//! default output device; every other platform (and every macOS failure)
//! gets [`UnsupportedDucker`], a documented no-op.
//!
//! ## Crash restore
//!
//! A successful `duck` writes `<recovery dir>/audio_duck.state.json` with
//! the device id, the saved volume, and the applied level. If the app
//! crashes before `restore` runs, the next launch finds the file via
//! [`apply_pending_restore`], puts the volume back, and deletes the file.
//! The state file lives in the escape-recovery spool directory (the same
//! one `recovery.rs` owns) so there is exactly one crash-artifact location.
//!
//! ## Pause media (opt-in)
//!
//! [`toggle_media_play_pause`] sends the system play/pause media key
//! (F8) through `CGEventPost`. There is no public API to check whether
//! anything is playing, so the setting it backs is off by default and the
//! call site must gate on it. macOS only; a no-op elsewhere.

use std::path::{Path, PathBuf};

/// An error setting or restoring the output volume. Displayable: the call
/// sites log it and move on (ducking must never break a dictation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuckerError {
    /// No default output device is available right now.
    NoDefaultDevice,
    /// The device does not expose a scalar volume control.
    NoVolumeControl,
    /// CoreAudio refused the property read/write.
    CoreAudio { status: i32 },
}

impl std::fmt::Display for DuckerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDefaultDevice => write!(f, "no default output device"),
            Self::NoVolumeControl => write!(f, "output device has no volume control"),
            Self::CoreAudio { status } => write!(f, "CoreAudio status {status}"),
        }
    }
}

impl std::error::Error for DuckerError {}

/// The seam the dictation controller holds. `duck` clamps its level to
/// 0.0-1.0 and is a no-op (Ok) when already active; `restore` is a warning
/// no-op when not active.
pub trait AudioDucker {
    /// Lowers the output volume to `level` and remembers the previous one.
    fn duck(&mut self, level: f32) -> Result<(), DuckerError>;
    /// Restores the pre-take volume. No-op (with a warning) if not active.
    fn restore(&mut self) -> Result<(), DuckerError>;
    /// True while a duck is in effect and a restore is pending.
    fn active(&self) -> bool;
}

/// The on-disk crash artifact, one per pending duck.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuckState {
    /// The CoreAudio id of the device whose volume was lowered.
    pub device_id: u32,
    /// The exact scalar volume to put back.
    pub saved_volume: f32,
    /// The level that was applied (for logging/verification).
    pub duck_level: f32,
    /// Unix milliseconds the duck was applied.
    pub ts: u64,
}

/// The state file name, relative to the recovery spool directory.
const STATE_FILE: &str = "audio_duck.state.json";

/// The state file path for a recovery spool directory.
pub fn state_file_path(recovery_dir: &Path) -> PathBuf {
    recovery_dir.join(STATE_FILE)
}

/// Persists the duck state so a crash leaves a restorable artifact.
/// Atomic (tmp + rename) like every other settings write in the app.
pub fn save_state(path: &Path, state: &DuckState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(state)?;
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    use std::io::Write as _;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Reads the pending duck state, if a file exists and parses.
pub fn load_state(path: &Path) -> Option<DuckState> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Deletes the state file (no-op when absent).
pub fn clear_state(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Startup crash-restore: if a duck survived a crash, put the volume back
/// and delete the artifact. Returns a description of what was restored
/// (for the Developer-tab log) or `None` when there was nothing pending.
///
/// A corrupt file is deleted and warned about, never fatal: the worst case
/// is a volume that stays at the duck level until the user or the next
/// take's restore fixes it, and that case is logged.
pub fn apply_pending_restore(
    recovery_dir: &Path,
    restore: &dyn Fn(u32, f32) -> Result<(), DuckerError>,
) -> Result<Option<String>, String> {
    let path = state_file_path(recovery_dir);
    let Some(state) = load_state(&path) else {
        // No file, or a file that cannot be parsed: nothing restorable. A
        // corrupt file is deleted (and warned about) because it is useless;
        // the worst case is a volume stuck at the duck level, which the
        // next take's restore fixes and which is logged here.
        if path.exists() {
            crate::log_entry(
                crate::LogLevel::Warn,
                format!(
                    "audio duck: pending restore state at {} is corrupt; deleting it",
                    path.display()
                ),
            );
            clear_state(&path);
        }
        return Ok(None);
    };
    match restore(state.device_id, state.saved_volume) {
        Ok(()) => {
            clear_state(&path);
            Ok(Some(format!(
                "device {} back to {:.0}%",
                state.device_id,
                state.saved_volume * 100.0
            )))
        }
        Err(e) => {
            // Keep the file: the next launch retries, and a manual restore
            // is still possible without losing the saved value.
            Err(format!(
                "could not restore output volume after crash ({e}); keeping {} for retry",
                path.display()
            ))
        }
    }
}

/// macOS: send the system play/pause media key (F8) through
/// `CGEventPost`. Raw `CoreGraphics` FFI (the same pattern as
/// `teletype_core::injector`'s modifier check) so no new dependency is
/// added. Returns `true` when the event was posted; the caller treats
/// pause-media as best-effort.
#[cfg(target_os = "macos")]
pub fn toggle_media_play_pause() -> bool {
    use std::ffi::c_void;

    // `kCGEventSourceStateHIDSystemState`.
    const HID_SYSTEM_STATE: i32 = 0;
    // `kCGSessionEventTap`.
    const SESSION_EVENT_TAP: i32 = 0;
    // `kVK_F8`: the play/pause media key (kVK_F1=16 ... kVK_F8=23).
    const KVK_F8: u16 = 23;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(stateid: i32) -> *mut c_void;
        fn CGEventCreateKeyboardEvent(
            source: *mut c_void,
            virtual_key: u16,
            key_down: bool,
        ) -> *mut c_void;
        fn CGEventPost(tap: i32, event: *mut c_void);
        fn CFRelease(cf: *const c_void);
    }

    // SAFETY: `CGEventSourceCreate` returns a CF object we release; the
    // event is built from it and both are released after posting.
    unsafe {
        let source = CGEventSourceCreate(HID_SYSTEM_STATE);
        if source.is_null() {
            crate::log_entry(
                crate::LogLevel::Warn,
                "pause media: could not create CGEventSource",
            );
            return false;
        }
        let down = CGEventCreateKeyboardEvent(source, KVK_F8, true);
        let up = CGEventCreateKeyboardEvent(source, KVK_F8, false);
        CFRelease(source);
        if down.is_null() || up.is_null() {
            for ev in [down, up] {
                if !ev.is_null() {
                    CFRelease(ev);
                }
            }
            crate::log_entry(
                crate::LogLevel::Warn,
                "pause media: could not create the play/pause key event",
            );
            return false;
        }
        CGEventPost(SESSION_EVENT_TAP, down);
        CGEventPost(SESSION_EVENT_TAP, up);
        CFRelease(down);
        CFRelease(up);
        true
    }
}

/// Non-macOS: pause-media is not implemented.
#[cfg(not(target_os = "macos"))]
pub fn toggle_media_play_pause() -> bool {
    false
}

#[cfg(target_os = "macos")]
mod coreaudio {
    use super::*;

    pub struct CoreAudioDucker {
        active: bool,
        device_id: Option<u32>,
        saved_volume: f32,
    }

    impl Default for CoreAudioDucker {
        fn default() -> Self {
            Self::new()
        }
    }

    impl CoreAudioDucker {
        pub fn new() -> Self {
            Self {
                active: false,
                device_id: None,
                saved_volume: 1.0,
            }
        }

        /// The default output device id, or an error when there is none.
        fn default_output_device() -> Result<u32, DuckerError> {
            use objc2_core_audio::{
                kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
                kAudioObjectPropertyScopeGlobal, AudioObjectGetPropertyData,
                AudioObjectPropertyAddress,
            };
            use std::ptr::NonNull;

            const K_AUDIO_OBJECT_SYSTEM_OBJECT: u32 = 1;
            let address = AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDefaultOutputDevice,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };
            let mut size: u32 = 4;
            let mut device: u32 = 0;
            let size_ptr = NonNull::from(&mut size);
            let device_ptr = NonNull::from(&mut device).cast::<std::ffi::c_void>();
            // SAFETY: `address` is a live AudioObjectPropertyAddress; the
            // system object id 1 is the documented CoreAudio singleton;
            // `device` is a valid 4-byte buffer for the property.
            let status = unsafe {
                AudioObjectGetPropertyData(
                    K_AUDIO_OBJECT_SYSTEM_OBJECT,
                    NonNull::from(&address),
                    0,
                    std::ptr::null(),
                    size_ptr,
                    device_ptr,
                )
            };
            if status != 0 {
                return Err(DuckerError::CoreAudio { status });
            }
            Ok(device)
        }

        /// Reads the current master (element 0) scalar volume of a device.
        fn get_volume(device: u32) -> Result<f32, DuckerError> {
            use objc2_core_audio::{
                kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyElementMain,
                kAudioObjectPropertyScopeOutput, AudioObjectGetPropertyData,
                AudioObjectPropertyAddress,
            };
            use std::ptr::NonNull;

            let address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyVolumeScalar,
                mScope: kAudioObjectPropertyScopeOutput,
                mElement: kAudioObjectPropertyElementMain,
            };
            let mut size: u32 = 4;
            let mut volume: f32 = 1.0;
            let size_ptr = NonNull::from(&mut size);
            let volume_ptr = NonNull::from(&mut volume).cast::<std::ffi::c_void>();
            // SAFETY: a live address, a valid 4-byte f32 buffer, and a
            // device id we just read from CoreAudio.
            let status = unsafe {
                AudioObjectGetPropertyData(
                    device,
                    NonNull::from(&address),
                    0,
                    std::ptr::null(),
                    size_ptr,
                    volume_ptr,
                )
            };
            match status {
                0 => Ok(volume),
                // 2796 = kAudioDevicePropertyNotPresent: the device has no
                // volume control (common for HDMI/USB outputs).
                2796 => Err(DuckerError::NoVolumeControl),
                other => Err(DuckerError::CoreAudio { status: other }),
            }
        }

        /// Writes the master (element 0) scalar volume of a device.
        pub fn set_volume(device: u32, volume: f32) -> Result<(), DuckerError> {
            use objc2_core_audio::{
                kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyElementMain,
                kAudioObjectPropertyScopeOutput, AudioObjectPropertyAddress,
                AudioObjectSetPropertyData,
            };
            use std::ptr::NonNull;

            let address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyVolumeScalar,
                mScope: kAudioObjectPropertyScopeOutput,
                mElement: kAudioObjectPropertyElementMain,
            };
            let size: u32 = 4;
            let volume = volume.clamp(0.0, 1.0);
            // SAFETY: a live address, a by-value 4-byte f32, and a device
            // id CoreAudio accepted for reads.
            let status = unsafe {
                AudioObjectSetPropertyData(
                    device,
                    NonNull::from(&address),
                    0,
                    std::ptr::null(),
                    size,
                    NonNull::from(&volume).cast::<std::ffi::c_void>(),
                )
            };
            match status {
                0 => Ok(()),
                2796 => Err(DuckerError::NoVolumeControl),
                other => Err(DuckerError::CoreAudio { status: other }),
            }
        }
    }

    impl AudioDucker for CoreAudioDucker {
        fn duck(&mut self, level: f32) -> Result<(), DuckerError> {
            let level = level.clamp(0.0, 1.0);
            if self.active {
                // Already ducked (e.g. a second take started before the
                // first stopped): keep the first take's saved volume.
                return Ok(());
            }
            let device = Self::default_output_device()?;
            let saved = Self::get_volume(device)?;
            Self::set_volume(device, level)?;
            let ts = teletype_core::storage::now_ms();
            let state = DuckState {
                device_id: device,
                saved_volume: saved,
                duck_level: level,
                ts,
            };
            self.active = true;
            self.device_id = Some(device);
            self.saved_volume = saved;
            // Crash artifact: best-effort, but log when it fails so a
            // crashed duck is at least visible.
            if let Some(dir) = duck_state_dir() {
                if let Err(e) = save_state(&state_file_path(&dir), &state) {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("audio duck: could not save restore state: {e}"),
                    );
                }
            }
            Ok(())
        }

        fn restore(&mut self) -> Result<(), DuckerError> {
            if !self.active {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    "audio duck: restore called while not active; no-op",
                );
                return Ok(());
            }
            let device = self.device_id.expect("active implies a device");
            let saved = self.saved_volume;
            let result = Self::set_volume(device, saved);
            self.active = false;
            self.device_id = None;
            if let Some(dir) = duck_state_dir() {
                clear_state(&state_file_path(&dir));
            }
            result
        }

        fn active(&self) -> bool {
            self.active
        }
    }

    /// The recovery spool directory. The duck state file lives beside the
    /// escape-recovery PCM spool, so a crash leaves both artifacts in one
    /// place. `None` when the data dir cannot be resolved (tests).
    fn duck_state_dir() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("recovery"))
    }
}

#[cfg(target_os = "macos")]
pub use coreaudio::CoreAudioDucker;

/// The no-op ducker: Windows (the real implementation needs
/// `IAudioSessionManager2`, a follow-up) and macOS failure modes. Logs once
/// so the user can see why ducking is not happening, then inert.
pub struct UnsupportedDucker {
    warned: bool,
}

impl Default for UnsupportedDucker {
    fn default() -> Self {
        Self::new()
    }
}

impl UnsupportedDucker {
    pub fn new() -> Self {
        Self { warned: false }
    }
}

impl AudioDucker for UnsupportedDucker {
    fn duck(&mut self, _level: f32) -> Result<(), DuckerError> {
        if !self.warned {
            self.warned = true;
            crate::log_entry(
                crate::LogLevel::Warn,
                "audio ducking is not available on this platform; output volume will not be lowered during dictation",
            );
        }
        Ok(())
    }

    fn restore(&mut self) -> Result<(), DuckerError> {
        Ok(())
    }

    fn active(&self) -> bool {
        false
    }
}

/// The ducker the app actually runs: CoreAudio on macOS, the documented
/// no-op elsewhere.
pub fn default_ducker() -> Box<dyn AudioDucker + Send> {
    #[cfg(target_os = "macos")]
    {
        Box::new(CoreAudioDucker::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(UnsupportedDucker::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scriptable ducker for the state-file and hook tests.
    #[derive(Default)]
    pub struct MockDucker {
        pub active_now: bool,
        pub duck_calls: Vec<f32>,
        pub restore_calls: u32,
        pub saved: f32,
    }

    impl MockDucker {
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl AudioDucker for MockDucker {
        fn duck(&mut self, level: f32) -> Result<(), DuckerError> {
            if self.active_now {
                return Ok(());
            }
            let level = level.clamp(0.0, 1.0);
            self.duck_calls.push(level);
            self.active_now = true;
            self.saved = 0.8;
            Ok(())
        }

        fn restore(&mut self) -> Result<(), DuckerError> {
            if !self.active_now {
                return Ok(());
            }
            self.restore_calls += 1;
            self.active_now = false;
            Ok(())
        }

        fn active(&self) -> bool {
            self.active_now
        }
    }

    #[test]
    fn mock_duck_then_restore_round_trip() {
        let mut d = MockDucker::new();
        assert!(!d.active());
        d.duck(0.2).unwrap();
        assert!(d.active());
        assert_eq!(d.duck_calls, vec![0.2]);
        d.restore().unwrap();
        assert!(!d.active());
        assert_eq!(d.restore_calls, 1);
    }

    #[test]
    fn restore_when_not_active_is_a_noop() {
        let mut d = MockDucker::new();
        assert!(d.restore().is_ok());
        assert_eq!(d.restore_calls, 0);
    }

    #[test]
    fn duck_clamps_out_of_range_levels() {
        let mut d = MockDucker::new();
        d.duck(-1.0).unwrap();
        assert_eq!(d.duck_calls, vec![0.0]);
        d.restore().unwrap();
        d.duck(2.0).unwrap();
        assert_eq!(d.duck_calls, vec![0.0, 1.0]);
    }

    #[test]
    fn second_duck_while_active_is_a_noop() {
        let mut d = MockDucker::new();
        d.duck(0.2).unwrap();
        d.duck(0.2).unwrap();
        assert_eq!(
            d.duck_calls.len(),
            1,
            "already-active duck must not re-duck"
        );
    }

    #[test]
    fn state_file_round_trip_and_clear() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = state_file_path(dir.path());
        let state = DuckState {
            device_id: 29,
            saved_volume: 0.75,
            duck_level: 0.2,
            ts: 1_700_000_000,
        };
        save_state(&path, &state).expect("save");
        let back = load_state(&path).expect("load");
        assert_eq!(back.device_id, 29);
        assert!((back.saved_volume - 0.75).abs() < f32::EPSILON);
        assert!((back.duck_level - 0.2).abs() < f32::EPSILON);
        assert_eq!(back.ts, 1_700_000_000);
        clear_state(&path);
        assert!(load_state(&path).is_none());
        // Clearing twice is a no-op.
        clear_state(&path);
        assert!(!path.exists());
    }

    #[test]
    fn apply_pending_restore_empty_dir_is_none() {
        let dir = tempfile::tempdir().expect("temp dir");
        let out = apply_pending_restore(dir.path(), &|_, _| Ok(()));
        assert!(out.is_ok());
        assert!(out.unwrap().is_none());
    }

    #[test]
    fn apply_pending_restore_restores_and_deletes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = state_file_path(dir.path());
        save_state(
            &path,
            &DuckState {
                device_id: 41,
                saved_volume: 0.8,
                duck_level: 0.2,
                ts: 123,
            },
        )
        .unwrap();
        let restored = std::sync::Arc::new(std::sync::Mutex::new(None::<(u32, f32)>));
        let out = apply_pending_restore(dir.path(), &|dev, vol| {
            *restored.lock().unwrap() = Some((dev, vol));
            Ok(())
        })
        .expect("restore");
        assert!(out.is_some());
        assert_eq!(*restored.lock().unwrap(), Some((41, 0.8)));
        assert!(
            !path.exists(),
            "the state file must be deleted after a restore"
        );
    }

    #[test]
    fn apply_pending_restore_corrupt_file_deletes_and_warns() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = state_file_path(dir.path());
        std::fs::write(&path, b"{ not json").unwrap();
        let out = apply_pending_restore(dir.path(), &|_, _| Ok(()));
        assert!(out.is_ok(), "corrupt JSON must not surface as an error");
        assert!(
            out.unwrap().is_none(),
            "no restorable state means no restore message"
        );
        assert!(!path.exists(), "a corrupt file is useless; delete it");
    }

    #[test]
    fn apply_pending_restore_keeps_file_when_restore_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = state_file_path(dir.path());
        save_state(
            &path,
            &DuckState {
                device_id: 7,
                saved_volume: 0.5,
                duck_level: 0.2,
                ts: 1,
            },
        )
        .unwrap();
        let out = apply_pending_restore(dir.path(), &|_, _| Err(DuckerError::NoVolumeControl));
        assert!(out.is_err(), "a failed restore must be reported");
        assert!(path.exists(), "keep the file so the next launch can retry");
    }

    #[test]
    fn unsupported_ducker_is_a_documented_noop() {
        let mut d = UnsupportedDucker::new();
        assert!(!d.active());
        assert!(d.duck(0.2).is_ok());
        assert!(!d.active(), "the no-op ducker never becomes active");
        assert!(d.restore().is_ok());
    }
}
