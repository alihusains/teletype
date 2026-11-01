//! Build script for `teletype-apple`.
//!
//! On macOS Apple Silicon this builds the `TeletypeApple` Swift package
//! (`swift/Package.swift`, system frameworks only, no dependencies) and links
//! the resulting static library into the crate through `swift-rs`'s
//! `SwiftLinker`. Everywhere else the crate compiles the stub provider in
//! `src/unsupported.rs` and no Swift toolchain is needed.
//!
//! Escape hatch: set `TELETYPE_APPLE_DISABLE_SWIFT=1` to skip the Swift build
//! on macOS (the crate then compiles the stub; used by fast CI checks).

#[cfg(target_os = "macos")]
fn link_swift() {
    swift_rs::SwiftLinker::new("14.0")
        .with_package("TeletypeApple", "swift")
        .link();

    // SwiftPM on this toolchain (macOS 27 CLT) writes static products to
    // `<build-path>/out/Products/<Debug|Release>/`. swift-rs 1.0.9 only knows
    // the legacy `<build-path>/<arch>-apple-macosx/<config>/` layout and the
    // Xcode-27 `Products` layout, the latter only when `xcodebuild` is
    // detectable, which it is not on a CLT-only machine. Point the linker at
    // the real directory explicitly (same fix as teletype-fluid's build.rs).
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let products = std::path::Path::new(&out_dir)
        .join("swift-rs")
        .join("TeletypeApple")
        .join("out")
        .join("Products")
        .join(if std::env::var("DEBUG").as_deref() == Ok("true") {
            "Debug"
        } else {
            "Release"
        });
    if products.is_dir() {
        println!("cargo:rustc-link-search=native={}", products.display());
    } else {
        // The legacy search path swift-rs emitted points nowhere; surface the
        // mismatch instead of a bare "library not found" at link time.
        println!(
            "cargo:warning=teletype-apple: SwiftPM products not found at {}",
            products.display()
        );
    }

    // The Swift runtime dylibs (libswift_Concurrency, libswiftCore, ...) are
    // referenced as `@rpath/...` by Swift's autolinking but live only in the
    // OS dyld cache under /usr/lib/swift, which is not a default loader path.
    // `cargo:rustc-link-arg` covers this package's own binaries and tests;
    // downstream binaries (the desktop app) must add the same rpath
    // themselves (teletype-fluid's build.rs already emits it for the app).
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

#[cfg(not(target_os = "macos"))]
fn link_swift() {}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=swift/Package.swift");
    println!("cargo:rerun-if-changed=swift/Sources/TeletypeApple/Bridge.swift");
    println!("cargo:rustc-check-cfg=cfg(teletype_apple_swift)");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let swift_enabled = target_os == "macos"
        && target_arch == "aarch64"
        && std::env::var_os("TELETYPE_APPLE_DISABLE_SWIFT").is_none();

    if swift_enabled {
        println!("cargo:rustc-cfg=teletype_apple_swift");
        link_swift();
    }
}
