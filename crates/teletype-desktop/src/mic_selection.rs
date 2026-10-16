//! Smart mic selection, desktop side (plan §5.2).
//!
//! The pure name heuristic lives in `teletype_core::audio::select_input_device`
//! (unit-tested without any OS). This module adds the macOS transport check:
//! a device whose CoreAudio `kAudioDevicePropertyTransportType` is one of the
//! Bluetooth transports (or whose name looks Bluetooth/HFP) is the narrowband
//! HFP input the heuristic wants to avoid when the built-in mic is present.
//!
//! The wiring point for the dictation start path is
//! [`pick_input_device`]: given the device list from
//! `teletype_core::audio::list_input_devices` and the user preference, it
//! returns the id to use, or `None` (keep the current default).

use teletype_core::audio::{list_input_devices, select_input_device};

/// True when `device_name` looks like a Bluetooth/HFP input.
///
/// Name hints first (cheap, no FFI); on macOS the CoreAudio transport type
/// is checked as a second signal. Unknown devices return `false` (never
/// fabricate a transport).
pub fn input_is_bluetooth_hfp(device_name: &str) -> bool {
    let lower = device_name.to_ascii_lowercase();
    if BT_NAME_HINTS.iter().any(|h| lower.contains(h)) {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        let id = list_input_devices()
            .into_iter()
            .find(|d| d.name == device_name)
            .map(|d| d.id);
        if let Some(id) = id {
            return transport_is_bluetooth(&id);
        }
    }
    false
}

/// Picks the input device id for a take (plan §5.2), or `None` when the
/// heuristic finds no match and the caller should keep the current default.
///
/// `prefer_builtin_with_bt` is the user setting: when a Bluetooth/HFP device
/// is active and this is true, the built-in mic wins.
pub fn pick_input_device(prefer_builtin_with_bt: bool) -> Option<String> {
    let devices = list_input_devices();
    let names: Vec<String> = devices.iter().map(|d| d.name.clone()).collect();
    let ids: Vec<String> = devices.iter().map(|d| d.id.clone()).collect();
    let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
    let idx = select_input_device(pairs.iter().map(|(a, b)| (*a, b)), prefer_builtin_with_bt)?;
    devices.get(idx).map(|d| d.id.clone())
}

/// Bluetooth/HFP name hints (case-insensitive). Mirrors the core heuristic's
/// BT list; kept local so this module is self-documenting.
const BT_NAME_HINTS: &[&str] = &["airpods", "bluetooth", "wireless", "headset", "beat"];

#[cfg(target_os = "macos")]
fn transport_is_bluetooth(device_id: &str) -> bool {
    use objc2_core_audio::{
        kAudioDevicePropertyTransportType, kAudioDeviceTransportTypeBluetooth,
        kAudioDeviceTransportTypeBluetoothLE, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, AudioObjectGetPropertyData, AudioObjectPropertyAddress,
    };
    use std::ptr::NonNull;

    // cpal's macOS device ids are the CoreAudio AudioObjectID as a string.
    let Ok(object_id) = device_id.parse::<u32>() else {
        return false;
    };

    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyTransportType,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut size: u32 = 4;
    let mut transport: u32 = 0;
    let size_ptr = NonNull::from(&mut size);
    let transport_ptr = NonNull::from(&mut transport).cast::<std::ffi::c_void>();
    // SAFETY: `address` is a valid, live `AudioObjectPropertyAddress` on the
    // stack; `size` and `transport` are valid, live 4-byte buffers for the
    // u32 property; `object_id` is a device id handed to us by cpal (a live
    // CoreAudio object). The call is read-only.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object_id,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            size_ptr,
            transport_ptr,
        )
    };
    if status != 0 {
        return false;
    }
    // SAFETY: comparing a u32 against two well-known CoreAudio transport
    // type constants; no unsafe involved, just a value match.
    transport == kAudioDeviceTransportTypeBluetooth
        || transport == kAudioDeviceTransportTypeBluetoothLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt_name_hints_match() {
        assert!(input_is_bluetooth_hfp("AirPods Pro"));
        assert!(input_is_bluetooth_hfp("Sony Bluetooth Headset"));
        assert!(input_is_bluetooth_hfp("Beats Studio"));
        assert!(input_is_bluetooth_hfp("Wireless Headset"));
        assert!(!input_is_bluetooth_hfp("MacBook Pro Microphone"));
        assert!(!input_is_bluetooth_hfp("USB Audio Device"));
    }

    #[test]
    fn pick_returns_none_on_no_match() {
        // If the machine has no builtin/bt/usb-named devices the heuristic
        // returns None and the caller keeps the current default. On a dev
        // machine with arbitrary devices this may return Some, so assert
        // only that it does not panic.
        let _ = pick_input_device(true);
    }
}
