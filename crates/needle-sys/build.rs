//! Downloads the prebuilt `libneedle.a` static library and `needle.h` header
//! for the current platform from the HuggingFace `Cactus-Compute/needle3`
//! repo, and emits the link flags for the `needle-sys` bindings.
//!
//! The Needle 3 engine is a self-contained C++ binary with no external
//! dependencies. Each platform folder on HF ships:
//!   - `libneedle.a`  (static library, ~1.5–2.2 MB)
//!   - `needle.h`     (C header, identical across platforms)
//!   - `needle`       (CLI runner, not needed for the FFI crate)
//!
//! The `.cact` model file (29–35 MB) is NOT downloaded here — it is managed
//! by `teletype-inference`'s download pipeline, same as GGUF models.

use std::path::{Path, PathBuf};

const HF_BASE: &str = "https://huggingface.co/Cactus-Compute/needle3/resolve/main";

/// Map the Rust target triple to the Needle 3 platform folder name.
fn platform_folder() -> &'static str {
    let target = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    match (os.as_str(), target.as_str(), env.as_str()) {
        ("macos", "aarch64", _) => "macos-arm64",
        ("macos", "x86_64", _) => "macos-x86_64",
        ("linux", "x86_64", "musl") => "linux-x86_64-musl",
        ("linux", "x86_64", _) => "linux-x86_64",
        ("linux", "aarch64", "musl") => "linux-arm64-musl",
        ("linux", "aarch64", _) => "linux-arm64",
        ("windows", "x86_64", _) => "windows-x86_64",
        ("windows", "aarch64", _) => "windows-arm64",
        (other_os, other_arch, _) => {
            panic!(
                "needle-sys: unsupported target {other_os}/{other_arch}. \
                 Supported: macos-arm64, macos-x86_64, linux-x86_64, linux-arm64, \
                 windows-x86_64, windows-arm64"
            )
        }
    }
}

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let folder = platform_folder();
    // Both delivery folders currently ship the same static-library name;
    // revisit if a Windows build starts shipping a different artifact.
    let lib_name = "libneedle.a";
    let header_name = "needle.h";

    let lib_path = out.join(folder).join(lib_name);
    let header_path = out.join(folder).join(header_name);

    // Reuse a previously downloaded library if present (avoids re-download
    // on every incremental build).
    if !lib_path.exists() || !header_path.exists() {
        download_platform_folder(folder, &out);
    }

    // Emit link flags.
    let lib_dir = out.join(folder);
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=needle");

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

fn download_platform_folder(folder: &str, out: &Path) {
    let url = format!("{HF_BASE}/{folder}.zip");
    // Fallback: try individual file downloads if the zip isn't available.
    let lib_url = format!("{HF_BASE}/{folder}/libneedle.a");
    let header_url = format!("{HF_BASE}/{folder}/needle.h");

    let dest_dir = out.join(folder);
    std::fs::create_dir_all(&dest_dir)
        .unwrap_or_else(|e| panic!("needle-sys: cannot create {dest_dir:?}: {e}"));

    // Try the zip first (single download), fall back to individual files.
    let zip_path = out.join(format!("{folder}.zip"));
    let zip_ok = download_file(&url, &zip_path).is_ok();

    if zip_ok {
        extract_zip(&zip_path, &dest_dir);
        let _ = std::fs::remove_file(&zip_path);
    } else {
        // Individual file downloads.
        if let Err(e) = download_file(&lib_url, &dest_dir.join("libneedle.a")) {
            panic!("needle-sys: failed to download {lib_url}: {e}");
        }
        if let Err(e) = download_file(&header_url, &dest_dir.join("needle.h")) {
            panic!("needle-sys: failed to download {header_url}: {e}");
        }
    }

    // Verify the library landed.
    if !dest_dir.join("libneedle.a").exists() {
        panic!("needle-sys: libneedle.a not found after download for {folder}");
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

fn extract_zip(zip_path: &Path, dest: &Path) {
    let file = std::fs::File::open(zip_path).expect("open zip");
    let mut archive = zip::ZipArchive::new(file).expect("invalid zip");
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("zip entry");
        let raw_name = entry.name().to_string();
        // Strip the top-level folder prefix if present.
        let name = raw_name.split_once('/').map(|x| x.1).unwrap_or(&raw_name);
        if name.is_empty() {
            continue;
        }
        let out_path = dest.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).ok();
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut f = std::fs::File::create(&out_path).expect("create file");
            std::io::copy(&mut entry, &mut f).expect("extract file");
        }
    }
}
