//! Emits the native link flags for the Parakeet engine built by `parakeet-sys`.
//!
//! The Parakeet engine is a **macOS-only** dependency: `parakeet-sys` is wired
//! into the workspace under `target.'cfg(target_os = "macos")'`, so on every
//! other target (notably the native Windows release) there is no engine to
//! compile and nothing to link. This build script therefore emits no link
//! flags at all on non-macOS targets.
//!
//! On macOS, `parakeet-sys` compiles the `parakeet` + `ggml` static libraries
//! from whisper.cpp v1.9.4 via CMake in its own build script. We re-emit the
//! same link directives here so they are guaranteed to reach the final
//! binary's link line.
//!
//! We locate the static libraries by scanning the `parakeet-sys-*/out/build`
//! tree for the compiled `.a` files and emitting a `-L` search path for every
//! directory that contains one. This is robust to cargo's per-unit OUT_DIR
//! hashing, and to any CMake output-layout change.

use std::path::{Path, PathBuf};

fn main() {
    let cargo_target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // No Parakeet engine is built off macOS, so there is nothing to link.
    // Emitting the static=parakeet/ggml* flags here would fail the final link
    // with "could not find native static library `parakeet`".
    if cargo_target_os != "macos" {
        return;
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // OUT_DIR is <target>/build/teletype-speech-<hash>/out, so the sibling
    // directories are <target>/build/parakeet-sys-<hash>/out.
    let build_root = out
        .parent()
        .and_then(|p| p.parent())
        .expect("cannot locate cargo build root from OUT_DIR");

    // Find the parakeet-sys build tree, then collect every directory that
    // holds a compiled static library. We key off the parakeet library itself
    // (the artifact that proves the build completed) to pick the right tree.
    let mut search_dirs: Vec<PathBuf> = Vec::new();
    let mut found_parakeet = false;

    if let Ok(entries) = std::fs::read_dir(build_root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("parakeet-sys-") {
                continue;
            }
            let build_dir = entry.path().join("out").join("build");
            if !build_dir.is_dir() {
                continue;
            }
            // Recursively walk the build tree for static-library files.
            let mut dirs: Vec<PathBuf> = Vec::new();
            collect_lib_dirs(&build_dir, &mut dirs);
            let has_parakeet = dirs.iter().any(|d| d.join("libparakeet.a").is_file());
            if !has_parakeet {
                continue;
            }
            found_parakeet = true;
            search_dirs = dirs;
            break;
        }
    }

    if !found_parakeet {
        // Fall back to a best-guess; the link step will surface a clear error
        // if the libraries are genuinely missing.
        let guess = out.join("build");
        let mut dirs = Vec::new();
        collect_lib_dirs(&guess, &mut dirs);
        if dirs.is_empty() {
            dirs.push(guess.join("src"));
        }
        search_dirs = dirs;
    }

    // Emit one -L per directory that actually contains a static library, so
    // the linker finds parakeet, ggml, and every ggml sub-library.
    for dir in &search_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }

    // Static libraries don't record their own dependencies: link the full
    // transitive set of ggml sub-libraries.
    println!("cargo:rustc-link-lib=static=parakeet");
    println!("cargo:rustc-link-lib=static=ggml");
    println!("cargo:rustc-link-lib=static=ggml-base");
    println!("cargo:rustc-link-lib=static=ggml-cpu");
    println!("cargo:rustc-link-lib=static=ggml-metal");
    println!("cargo:rustc-link-lib=framework=Accelerate");
    println!("cargo:rustc-link-lib=static=ggml-blas");
    println!("cargo:rustc-link-lib=dylib=pthread");
}

/// Recursively collects every directory under `root` that contains at least
/// one compiled static library (`.a` on Unix, `.lib` on MSVC). Skips CMake's
/// bookkeeping directories to keep the walk small.
fn collect_lib_dirs(root: &Path, out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let is_lib_dir = dir_name.ends_with(".a") || has_static_lib(&dir);
        if is_lib_dir {
            out.push(dir.clone());
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let n = entry.file_name().to_string_lossy().into_owned();
                // Skip CMake's internal and source directories.
                if n == "CMakeFiles" || n == "CMakeSrc" || n == "CMakeScripts" {
                    continue;
                }
                stack.push(entry.path());
            }
        }
    }
}

/// True if `dir` directly contains a compiled static library file.
fn has_static_lib(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".a")
            || (name.ends_with(".lib") && !name.starts_with("lib"))
        {
            return true;
        }
    }
    false
}
