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
    }
}
