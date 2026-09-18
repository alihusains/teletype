//! Emits the native link flags for the Parakeet engine built by `parakeet-sys`.
//!
//! `parakeet-sys` (a normal dependency) compiles the `parakeet` + `ggml`
//! static libraries from whisper.cpp v1.9.4 via CMake in its own build
//! script. We re-emit the same link directives from our build script so they
//! are guaranteed to reach the final binary's link line.
//!
//! We locate the static libraries by scanning the shared build directory for
//! the `parakeet-sys-*/out/build/src/libparakeet.a` artifact, which is robust
//! to cargo's per-unit OUT_DIR hashing.

use std::path::PathBuf;

fn main() {
    let cargo_target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // OUT_DIR is <target>/build/teletype-speech-<hash>/out, so the sibling
    // directories are <target>/build/parakeet-sys-<hash>/out.
    let build_root = out
        .parent()
        .and_then(|p| p.parent())
        .expect("cannot locate cargo build root from OUT_DIR");

    let mut found = None;
    if let Ok(entries) = std::fs::read_dir(build_root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("parakeet-sys-") {
                let candidate = entry
                    .path()
                    .join("out")
                    .join("build")
                    .join("src")
                    .join("libparakeet.a");
                if candidate.exists() {
                    found = Some(entry.path().join("out").join("build"));
                    break;
                }
            }
        }
    }

    let build_dir = match found {
        Some(d) => d,
        None => {
            // Fall back to a best-guess; the link step will surface a clear
            // error if the libraries are genuinely missing.
            out.join("build")
        }
    };

    println!(
        "cargo:rustc-link-search=native={}",
        build_dir.join("src").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        build_dir.join("ggml").join("src").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        build_dir
            .join("ggml")
            .join("src")
            .join("ggml-cpu")
            .display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        build_dir
            .join("ggml")
            .join("src")
            .join("ggml-blas")
            .display()
    );
    if cargo_target_os == "macos" {
        println!(
            "cargo:rustc-link-search=native={}",
            build_dir
                .join("ggml")
                .join("src")
                .join("ggml-metal")
                .display()
        );
    }
    // Static libraries don't record their own dependencies: link the full
    // transitive set of ggml sub-libraries.
    println!("cargo:rustc-link-lib=static=parakeet");
    println!("cargo:rustc-link-lib=static=ggml");
    println!("cargo:rustc-link-lib=static=ggml-base");
    println!("cargo:rustc-link-lib=static=ggml-cpu");
    if cargo_target_os == "macos" {
        println!("cargo:rustc-link-lib=static=ggml-metal");
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }
    println!("cargo:rustc-link-lib=static=ggml-blas");
    if cargo_target_os != "macos" {
        println!("cargo:rustc-link-lib=dylib=pthread");
    }
}
