fn main() {
    tauri_build::build();

    // Compile the macOS microphone-permission Obj-C helper.
    #[cfg(target_os = "macos")]
    {
        let mut cc = cc::Build::new();
        cc.file("src/mic_permission.m")
            .flag("-fobjc-arc")
            .flag("-fobjc-weak")
            .flag("-fobjc-exceptions")
            .flag("-fexceptions");
        // Link the frameworks the helper uses. The cc crate emits these as
        // -framework flags; we must NOT also emit cargo:rustc-link-lib (that
        // produces -lAVFoundation, which the linker can't find).
        cc.flag("-framework").flag("Foundation");
        cc.flag("-framework").flag("AVFoundation");
        cc.compile("mic_permission");
        println!("cargo:rerun-if-changed=src/mic_permission.m");
    }
}
