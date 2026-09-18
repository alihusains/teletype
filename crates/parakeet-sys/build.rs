//! Builds the `parakeet` static library from whisper.cpp (v1.9.4) and links it
//! into the final binary.
//!
//! whisper.cpp ≥ 1.9.2 ships NVIDIA's Parakeet TDT as a first-class engine
//! (`include/parakeet.h`, `src/parakeet.cpp`) with its own CMake target. We
//! vendor the release source, build just the ggml + parakeet targets, and
//! emit the include/link flags for the `teletype-speech` bindings.

use std::path::{Path, PathBuf};

const WHISPER_TAG: &str = "v1.9.4";
const WHISPER_URL: &str = "https://api.github.com/repos/ggml-org/whisper.cpp/zipball/v1.9.4";

fn main() {
    let cargo_target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let src = out.join("whisper.cpp");

    if !src.join("include/parakeet.h").exists() {
        fetch_and_extract(&out, &src);
    }

    // A CMakeCache.txt left over from a *different* environment (e.g. a prior
    // cross-compile, or a different deployment target) bakes in stale flags and
    // breaks the native build. Remove it so CMake reconfigures cleanly every
    // time; the C++ compile is the cost of correctness.
    let cmake_cache = out.join("build").join("CMakeCache.txt");
    if cmake_cache.exists() {
        let _ = std::fs::remove_file(&cmake_cache);
        let _ = std::fs::remove_dir_all(out.join("build").join("CMakeFiles"));
    }

    let mut config = cmake::Config::new(src.clone());
    config
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("GGML_STATIC", "ON")
        .define("GGML_F16", "ON")
        .define("GGML_KLEIDIAI", "OFF")
        .define("GGML_NATIVE", "OFF");
    // Metal only exists on macOS.
    if cargo_target_os == "macos" {
        config.define("GGML_METAL", "ON");
        // ggml's dynamic-backend loader uses <filesystem>, which requires a
        // macOS 10.15+ deployment target. Tauri injects a 10.13 floor into the
        // rustc invocation, so we must set it explicitly for CMake or the C++
        // build fails with "'path' is unavailable: introduced in macOS 10.15".
        config.define("CMAKE_OSX_DEPLOYMENT_TARGET", "11.0");
    } else {
        config.define("GGML_METAL", "OFF");
    }
    config.build_target("parakeet").build();

    println!("cargo:include={}", src.join("include").display());
    // Expose the OUT_DIR root to downstream build scripts (DEP_PARAKEET_SYS_ROOT)
    // so they can locate the compiled static libraries and re-emit link flags.
    println!("cargo:root={}", out.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
}

fn fetch_and_extract(_out: &Path, src: &Path) {
    println!("cargo:warning=Downloading whisper.cpp {WHISPER_TAG}…");
    let mut data = Vec::new();
    let mut reader = ureq::get(WHISPER_URL)
        .set("User-Agent", "teletype/0.1")
        .call()
        .expect("download of whisper.cpp failed")
        .into_reader();
    std::io::Read::read_to_end(&mut reader, &mut data).expect("reading whisper.cpp archive failed");

    // GitHub zipballs nest everything under a single top-level directory.
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data)).expect("invalid zip");
    let _ = std::fs::remove_dir_all(src);
    std::fs::create_dir_all(src).expect("create source dir");
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("zip entry read failed");
        let raw_name = entry.name();
        // Strip the single top-level prefix directory.
        let name = raw_name.split_once('/').map(|x| x.1).unwrap_or(raw_name);
        if name.is_empty() {
            continue;
        }
        let dest = src.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).expect("create dir");
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            let mut f = std::fs::File::create(&dest).expect("create file");
            std::io::copy(&mut entry, &mut f).expect("extract file");
        }
    }
}
