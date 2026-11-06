//! Downloads the prebuilt `libcactus.a` static library and `cactus.h` header
//! for the current platform from the HuggingFace `Cactus-Compute/cactus`
//! repo (`releases/4.0.0`), and emits the link flags for the `needle-sys`
//! bindings.
//!
//! The Cactus runtime 4.0.0 is the renamed Needle 3 engine: the engine, the
//! C symbols, and the artifacts were renamed from `needle_*`/`libneedle.a`
//! to `cactus_*`/`libcactus.a`. The C signatures are unchanged.
//!
//! On Windows the upstream prebuilt is built with LLVM-MinGW + libc++ (the
//! Itanium C++ ABI). MSVC `link.exe` rejects its COMDATs with `LNK1143`, and
//! switching to `lld-link` still leaves the libc++/libc++abi runtime symbols
//! undefined. Windows therefore compiles the pure-Rust stub in `src/lib.rs`
//! (`cfg(needle_stub)`) and never downloads or links the archive.
//!
//! Each platform folder under `releases/4.0.0` on HF ships:
//!   - `libcactus.a`  (static library, ~1.5–2.2 MB)
//!   - `cactus.h`     (C header, identical across platforms)
//!   - `cactus`       (CLI runner, not needed for the FFI crate)
//!
//! The 4.0.0 release has no per-platform zips; the individual files above
//! are downloaded directly.
//!
//! The `.cact` model file (29–35 MB) is NOT downloaded here — it is managed
//! by `teletype-inference`'s download pipeline, same as GGUF models.

use std::path::{Path, PathBuf};

const HF_BASE: &str = "https://huggingface.co/Cactus-Compute/cactus/resolve/main/releases/4.0.0";

/// Map the Rust target triple to the Cactus 4.0.0 platform folder name.
fn platform_folder() -> &'static str {
    let target = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    match (os.as_str(), target.as_str(), env.as_str()) {
        ("macos", "aarch64", _) => "macos-arm64",
        ("macos", "x86_64", _) => "macos-x86_64",
        ("linux", "x86_64", _) => "linux-x86_64",
        ("linux", "aarch64", _) => "linux-arm64",
        ("windows", "x86_64", _) => "windows-x86_64",
        ("windows", "aarch64", _) => "windows-arm64",
        (other_os, other_arch, _) => {
            panic!(
                "needle-sys: unsupported target {other_os}/{other_arch}. \
                 Supported: macos-arm64, linux-x86_64, linux-arm64, \
                 windows-x86_64, windows-arm64"
            )
        }
    }
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(needle_stub)");
    // Windows: the upstream archive is a MinGW/libc++ object, not an MSVC
    // library. Compile the pure-Rust stub instead of downloading it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-cfg=needle_stub");
        println!(
            "cargo:warning=needle-sys: Windows uses the pure-Rust stub; \
             the upstream libcactus.a is a MinGW/libc++ archive"
        );
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let folder = platform_folder();
    // Upstream (HF Cactus-Compute/cactus, releases/4.0.0) ships NO
    // macos-x86_64 prebuilt: the supported folders are macos-arm64,
    // ios/tvos/watchos-arm64, linux/windows x86_64 and arm64. `cargo check`
    // on that target never links the FFI symbols, so skip the download and
    // the link flags instead of panicking; a real x86_64 BUILD is
    // unsupported until upstream publishes the folder.
    if folder == "macos-x86_64" {
        println!(
            "cargo:warning=needle-sys: no upstream macos-x86_64 prebuilt in \
             Cactus-Compute/cactus 4.0.0; emitting no link flags \
             (cargo-check-only target)"
        );
        return;
    }
    // The 4.0.0 release ships no musl prebuilts (no linux-*-musl folders).
    // Do not silently map musl to the glibc archive. `cargo check` on a musl
    // target never links the FFI symbols, so skip the download and the link
    // flags, same as the macos-x86_64 path above.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("musl") {
        println!(
            "cargo:warning=needle-sys: no upstream musl prebuilt in \
             Cactus-Compute/cactus 4.0.0; emitting no link flags \
             (cargo-check-only target)"
        );
        return;
    }

    let lib_name = "libcactus.a";
    let header_name = "cactus.h";

    let lib_path = out.join(folder).join(lib_name);
    let header_path = out.join(folder).join(header_name);

    // Reuse a previously downloaded library if present (avoids re-download
    // on every incremental build).
    if !lib_path.exists() || !header_path.exists() {
        download_platform_folder(folder, &out, lib_name, header_name);
    }

    // Emit link flags.
    let lib_dir = out.join(folder);
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=cactus");

    // On macOS, the C++ engine needs the C++ standard library and CoreFoundation.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=c++");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
    // On Linux, the engine may need pthread and dl.
    if target_os == "linux" {
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=dl");
    }

    // Expose the OUT_DIR root to downstream build scripts.
    println!("cargo:root={}", out.display());

    // Re-run when the target changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ENV");
}

fn download_platform_folder(folder: &str, out: &Path, lib_name: &str, header_name: &str) {
    let lib_url = format!("{HF_BASE}/{folder}/{lib_name}");
    let header_url = format!("{HF_BASE}/{folder}/{header_name}");

    let dest_dir = out.join(folder);
    std::fs::create_dir_all(&dest_dir)
        .unwrap_or_else(|e| panic!("needle-sys: cannot create {dest_dir:?}: {e}"));

    if let Err(e) = download_file(&lib_url, &dest_dir.join(lib_name)) {
        panic!("needle-sys: failed to download {lib_url}: {e}");
    }
    if let Err(e) = download_file(&header_url, &dest_dir.join(header_name)) {
        panic!("needle-sys: failed to download {header_url}: {e}");
    }

    // Verify the library landed.
    if !dest_dir.join(lib_name).exists() {
        panic!("needle-sys: {lib_name} not found after download for {folder}");
    }
}

fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    println!("needle-sys: downloading {url}");
    let mut data = Vec::new();
    ureq::get(url)
        .set("User-Agent", "teletype/0.2")
        .call()
        .map_err(|e| e.to_string())?
        .into_reader()
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    std::fs::write(dest, &data).map_err(|e| e.to_string())?;
    Ok(())
}
