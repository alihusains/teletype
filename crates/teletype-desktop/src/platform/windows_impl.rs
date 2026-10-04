//! Windows platform implementation.
//!
//! Uses Win32 (via the `windows` crate) for foreground-app detection. Text
//! injection, hotkeys, and the tray are handled cross-platform by
//! `teletype-core` (enigo/arboard) and Tauri, so this file only implements the
//! `Platform` trait.

use teletype_core::{
    context::{self, ApplicationContext},
    platform::{PasteShortcut, Permission, PermissionKind, PermissionState, Platform},
};

pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn active_application(&self) -> Option<ApplicationContext> {
        foreground_app()
    }

    fn permissions(&self) -> Vec<Permission> {
        vec![
            // Windows prompts for mic access on first capture; there is no
            // pre-check that reliably reflects the per-app grant, so report
            // granted=true (the OS will prompt if needed).
            Permission {
                kind: PermissionKind::Microphone,
                state: PermissionState::Granted,
            },
            // Windows has no Accessibility-style permission gate for synthetic
            // keystrokes, so there is nothing to report as ungranted.
            Permission {
                kind: PermissionKind::Accessibility,
                state: PermissionState::Granted,
            },
        ]
    }

    fn request_permission(&self, kind: PermissionKind) {
        match kind {
            // Trigger the OS mic prompt by probing the default input device.
            PermissionKind::Microphone => {
                use cpal::traits::HostTrait;
                let _ = cpal::default_host().default_input_device();
            }
            // No programmatic prompt on Windows.
            PermissionKind::Accessibility => {}
        }
    }

    fn permission_settings_url(&self, kind: PermissionKind) -> Option<String> {
        Some(match kind {
            PermissionKind::Microphone => "ms-settings:privacy-microphone".into(),
            PermissionKind::Accessibility => "ms-settings:privacy-inkkeyboardhandwriting".into(),
        })
    }

    fn default_hotkey(&self) -> String {
        "Ctrl+Shift+Space".into()
    }

    fn paste_shortcut(&self) -> Option<PasteShortcut> {
        Some(PasteShortcut::ControlV)
    }

    fn setup_notes(&self) -> Vec<String> {
        vec![
            "Grant Microphone access in Settings → Privacy & security → Microphone."
                .into(),
            "Windows blocks simulated input from reaching apps that run as administrator — run Teletype with matching elevation if dictation doesn't type into elevated apps."
                .into(),
        ]
    }
}

/// Returns the foreground application's process name (normalized) via
/// `GetForegroundWindow` → `GetWindowThreadProcessId` →
/// `QueryFullProcessImageNameW`.
fn foreground_app() -> Option<ApplicationContext> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }

    let mut pid: u32 = 0;
    // SAFETY: pid is a valid writable u32; hwnd is a valid foreground window.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }

    // SAFETY: PROCESS_QUERY_LIMITED_INFORMATION is the minimal right needed to
    // query the image name; pid is a valid process id.
    let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(h) => h,
        Err(_) => return None,
    };

    let name = query_image_name(&handle)?;
    // SAFETY: handle is valid and was opened above.
    unsafe {
        let _ = CloseHandle(handle);
    }

    Some(context::normalize(&name, &name))
}

/// Resolves a process handle to its executable file name (without path or
/// extension), e.g. `C:\...\chrome.exe` → `chrome`.
fn query_image_name(handle: &windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{QueryFullProcessImageNameW, PROCESS_NAME_WIN32};

    const MAX_PATH: u32 = 260;
    let mut buf = [0u16; MAX_PATH as usize];
    let mut size = MAX_PATH as u32;

    // SAFETY: buf is a valid writable wide buffer of `size` elements; handle is
    // valid with PROCESS_QUERY_LIMITED_INFORMATION.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            *handle,
            PROCESS_NAME_WIN32,
            PWSTR::from_raw(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
    };
    if !ok || size == 0 {
        return None;
    }

    let path = String::from_utf16_lossy(&buf[..size as usize]);
    let file_name = path.rsplit('\\').next()?.to_string();
    let stem = file_name.strip_suffix(".exe").unwrap_or(&file_name);
    Some(stem.to_string())
}
