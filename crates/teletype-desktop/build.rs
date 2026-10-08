fn main() {
    tauri_build::build();

    #[cfg(target_os = "macos")]
    {
        // Microphone / accessibility permission helpers.
        {
            let mut cc = cc::Build::new();
            cc.file("src/mic_permission.m")
                .flag("-fobjc-arc")
                .flag("-fobjc-weak")
                .flag("-fobjc-exceptions")
                .flag("-fexceptions");
            cc.flag("-framework").flag("Foundation");
            cc.flag("-framework").flag("AVFoundation");
            cc.compile("mic_permission");
            println!("cargo:rerun-if-changed=src/mic_permission.m");
        }

        // Native hotkey capture panel.
        {
            let mut cc = cc::Build::new();
            cc.file("src/hotkey_capture.m")
                .flag("-fobjc-arc")
                .flag("-fobjc-weak")
                .flag("-fobjc-exceptions")
                .flag("-fexceptions");
            cc.flag("-framework").flag("Cocoa");
            cc.flag("-framework").flag("Carbon");
            cc.compile("hotkey_capture");
            println!("cargo:rerun-if-changed=src/hotkey_capture.m");
        }

        // Key-down event tap for typing AutoText.
        {
            let mut cc = cc::Build::new();
            cc.file("src/typing_tap.m")
                .flag("-fobjc-arc")
                .flag("-fobjc-weak")
                .flag("-fobjc-exceptions")
                .flag("-fexceptions");
            cc.flag("-framework").flag("Cocoa");
            cc.flag("-framework").flag("CoreGraphics");
            cc.compile("typing_tap");
            println!("cargo:rerun-if-changed=src/typing_tap.m");
        }

        // Bare-Fn event tap.
        {
            let mut cc = cc::Build::new();
            cc.file("src/fn_tap.m")
                .flag("-fobjc-arc")
                .flag("-fobjc-weak")
                .flag("-fobjc-exceptions")
                .flag("-fexceptions");
            cc.flag("-framework").flag("Cocoa");
            cc.flag("-framework").flag("CoreGraphics");
            cc.compile("fn_tap");
            println!("cargo:rerun-if-changed=src/fn_tap.m");
        }

        // Bare-modifier event tap (Ctrl/Cmd/Alt/Shift), same reason as fn_tap:
        // Carbon RegisterEventHotKey never fires for a lone modifier press.
        {
            let mut cc = cc::Build::new();
            cc.file("src/mod_tap.m")
                .flag("-fobjc-arc")
                .flag("-fobjc-weak")
                .flag("-fobjc-exceptions")
                .flag("-fexceptions");
            cc.flag("-framework").flag("Cocoa");
            cc.flag("-framework").flag("CoreGraphics");
            cc.compile("mod_tap");
            println!("cargo:rerun-if-changed=src/mod_tap.m");
        }

        // Dictation start/stop cue playback (NSSound).
        {
            let mut cc = cc::Build::new();
            cc.file("src/system_sound.m")
                .flag("-fobjc-arc")
                .flag("-fobjc-weak")
                .flag("-fobjc-exceptions")
                .flag("-fexceptions");
            cc.flag("-framework").flag("AppKit");
            cc.compile("system_sound");
            println!("cargo:rerun-if-changed=src/system_sound.m");
        }

        // Unified InputEngine tap: the single native listener.
        // CGEventTap -> InputNormalizer -> ShortcutManager OR AutoTextManager.
        // Active tap (kCGEventTapOptionDefault): consumes matching shortcut
        // KeyDown/KeyUp, observes everything else for AutoText without
        // consuming. Legacy transform_tap.m / typing_tap.m below are dormant
        // (compiled for linkage, never started).
        {
            let mut cc = cc::Build::new();
            cc.file("src/input_engine_tap.m")
                .flag("-fobjc-arc")
                .flag("-fobjc-weak")
                .flag("-fobjc-exceptions")
                .flag("-fexceptions");
            cc.flag("-framework").flag("Cocoa");
            cc.flag("-framework").flag("CoreGraphics");
            cc.compile("input_engine_tap");
            println!("cargo:rerun-if-changed=src/input_engine_tap.m");
        }

        // Legacy transform_tap.m is retired (not compiled): the unified tap
        // above is the single native listener. typing_tap.m below stays
        // compiled for linkage but is never started on macOS.
    }
}
