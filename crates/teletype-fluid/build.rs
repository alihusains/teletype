//! Build script for `teletype-fluid`.
//!
//! On macOS Apple Silicon this builds the `TeletypeFluid` Swift package
//! (`swift/Package.swift`, which pulls FluidAudio pinned `exact: 0.17.7`) and
//! links the resulting static library into the crate through `swift-rs`'s
//! `SwiftLinker`. Everywhere else the crate compiles the stub provider in
//! `src/unsupported.rs` and no Swift toolchain is needed.
//!
//! Escape hatch: set `TELETYPE_FLUID_DISABLE_SWIFT=1` to skip the Swift build
//! on macOS (the crate then compiles the stub; used by fast CI checks).

#[cfg(target_os = "macos")]
fn link_swift() {
    swift_rs::SwiftLinker::new("14.0")
        .with_package("TeletypeFluid", "swift")
        .link();

    // SwiftPM on this toolchain (macOS 27 CLT) writes static products to
    // `<build-path>/out/Products/<Debug|Release>/`. swift-rs 1.0.9 only knows
    // the legacy `<build-path>/<arch>-apple-macosx/<config>/` layout and the
    // Xcode-27 `Products` layout — the latter only when `xcodebuild` is
    // detectable, which it is not on a CLT-only machine. Point the linker at
    // the real directory explicitly.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let products = std::path::Path::new(&out_dir)
        .join("swift-rs")
        .join("TeletypeFluid")
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
            "cargo:warning=teletype-fluid: SwiftPM products not found at {}",
            products.display()
        );
    }

    // FluidAudio's Nemo text normalizer ships as a SwiftPM *binary artifact*:
    // `libtext_processing_rs.a`, which defines the `_nemo_tn_fst` /
    // `_nemo_free_string` symbols `EnglishTextNormalizer` calls. Swift's
    // autolink emits a bare `-ltext_processing_rs`, but that only resolves if
    // the archive's own directory is on the search path — otherwise the final
    // app link dies with "Undefined symbols: _nemo_tn_fst".
    //
    // The artifact is an .xcframework whose slices share one basename
    // (`macos-arm64_x86_64`, `ios-arm64`, `ios-...-simulator`, `...-maccatalyst`
    // all contain a `libtext_processing_rs.a`). `ld64` binds `-lNAME` to the
    // FIRST `-L` directory holding that name and does not fall through to a
    // later one, so pointing at every slice is an ordering trap: if an iOS
    // slice wins, ld64 skips its members as "built for iOS" and the symbol
    // stays undefined — the very failure we are fixing. Therefore emit exactly
    // ONE search path: the macOS slice whose arch matches the target.
    let scratch = std::path::Path::new(&out_dir)
        .join("swift-rs")
        .join("TeletypeFluid");
    // Rust spells the Apple-Silicon arch `aarch64`; the xcframework slice
    // directory spells it `arm64` (`macos-arm64_x86_64`). Translate so the
    // slice filter actually matches, or the macOS slice is silently skipped.
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let arch = match target_arch.as_str() {
        "aarch64" => "arm64",
        other => other,
    };
    if let Some(dir) = nemo_slice_dir(&scratch, arch) {
        println!("cargo:rustc-link-search=native={}", dir.display());
        // Explicit `-l`, not just the autolink: guarantees the archive is on
        // the link line even if the toolchain's autolink directive is absent
        // (the CI Xcode runner did not emit it). `static` because the artifact
        // is a `.a`; this propagates to every downstream binary (the desktop
        // app), which `cargo:rustc-link-arg` would not.
        println!("cargo:rustc-link-lib=static=text_processing_rs");
    } else {
        println!(
            "cargo:warning=teletype-fluid: Nemo text_processing_rs macOS slice not found under {}",
            scratch.display()
        );
    }

    // The Swift runtime dylibs (libswift_Concurrency, libswiftCore, ...) are
    // referenced as `@rpath/...` by Swift's autolinking but live only in the
    // OS dyld cache under /usr/lib/swift, which is not a default loader path.
    // `cargo:rustc-link-arg` covers this package's own binaries and tests;
    // downstream binaries (the desktop app) must add the same rpath
    // themselves — noted in PROGRESS.md and the P3-A learning.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

#[cfg(not(target_os = "macos"))]
fn link_swift() {}

/// The directory holding the macOS `libtext_processing_rs.a` slice, matched to
/// `arch` (`aarch64` -> a `macos-*arm64*` slice; `x86_64` -> `macos-*x86_64*`).
/// Returns `None` if no archive for this arch exists, so the caller can warn
/// instead of emitting a `-l` the linker cannot satisfy. Search order is
/// deterministic: it looks only under `NemoTextProcessing.xcframework`'s
/// `macos-*` slices, never iOS/catalyst, so a same-basename iOS archive can
/// never shadow the macOS one.
#[cfg(target_os = "macos")]
fn nemo_slice_dir(root: &std::path::Path, arch: &str) -> Option<std::path::PathBuf> {
    let needle = "libtext_processing_rs.a";
    let mut stack = vec![root.to_path_buf()];
    let mut matches: Vec<std::path::PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Only descend into macOS xcframework slices for the archive;
                // every other directory (checkouts, ios-* slices) is scanned
                // for structure but never used as a link path.
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == needle) {
                let slice = path.parent()?.file_name()?.to_string_lossy().to_string();
                if slice.starts_with("macos-") && slice.contains(arch) {
                    matches.push(path.parent()?.to_path_buf());
                }
            }
        }
    }
    matches.sort();
    matches.into_iter().next()
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=swift/Package.swift");
    println!("cargo:rerun-if-changed=swift/Sources/TeletypeFluid/Bridge.swift");
    println!("cargo:rustc-check-cfg=cfg(teletype_fluid_swift)");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let swift_enabled = target_os == "macos"
        && target_arch == "aarch64"
        && std::env::var_os("TELETYPE_FLUID_DISABLE_SWIFT").is_none();

    if swift_enabled {
        println!("cargo:rustc-cfg=teletype_fluid_swift");
        link_swift();
    }
}
