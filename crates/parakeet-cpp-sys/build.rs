//! Builds the `parakeet` static library from mudler/parakeet.cpp (pinned
//! commit) and links it into the final binary.
//!
//! parakeet.cpp is a C++/ggml Parakeet runtime whose GGUF loader keeps
//! per-tensor quant types, so mixed-quant GGUFs (e.g. Fluid Speech Mini's
//! Q2_K/Q4_K encoder) load natively. We vendor the source at a pinned commit,
//! build the static `parakeet` target (which pulls in its own ggml), and emit
//! the include/link flags for the `teletype-speech` bindings.
//!
//! The pinned commit is recorded here and in THIRD-PARTY-NOTICES.txt.

use std::path::{Path, PathBuf};

const PARAKEET_COMMIT: &str = "5c9b0fbbd8e611d88e47d7e1ebae9bca9b447e8f";
const PARAKEET_URL: &str = "https://api.github.com/repos/mudler/parakeet.cpp/zipball/5c9b0fbbd8e611d88e47d7e1ebae9bca9b447e8f";
// The vendored ggml submodule commit (third_party/ggml at PARAKEET_COMMIT).
// GitHub zipballs do not include submodule contents, so it is fetched
// separately — see main().
const GGML_COMMIT: &str = "e705c5fed490514458bdd2eaddc43bd098fcce9b";
const GGML_URL: &str =
    "https://api.github.com/repos/ggml-org/ggml/zipball/e705c5fed490514458bdd2eaddc43bd098fcce9b";

fn main() {
    let cargo_target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let src = out.join("parakeet.cpp");

    if !src.join("include/parakeet_capi.h").exists() {
        fetch_and_extract(&out, &src);
    }
    // GitHub zipballs do not include submodule contents. The CMake build
    // needs third_party/ggml (ced.cpp and voice-detect.cpp are disabled in
    // the build flags, so only ggml is fetched).
    let ggml = src.join("third_party").join("ggml");
    if !ggml.join("CMakeLists.txt").exists() {
        fetch_zip(GGML_URL, &ggml);
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
        .define("PARAKEET_SHARED", "OFF")
        .define("PARAKEET_BUILD_CLI", "OFF")
        .define("PARAKEET_BUILD_SERVER", "OFF")
        .define("PARAKEET_BUILD_TESTS", "OFF")
        .define("PARAKEET_WITH_CED", "OFF")
        .define("PARAKEET_WITH_VOICEDETECT", "OFF")
        .define("GGML_NATIVE", "OFF");
    // Metal only exists on macOS.
    if cargo_target_os == "macos" {
        config.define("PARAKEET_GGML_METAL", "ON");
        // ggml's dynamic-backend loader uses <filesystem>, which requires a
        // macOS 10.15+ deployment target. Tauri injects a 10.13 floor into the
        // rustc invocation, so we must set it explicitly for CMake or the C++
        // build fails with "'path' is unavailable: introduced in macOS 10.15".
        config.define("CMAKE_OSX_DEPLOYMENT_TARGET", "11.0");
    } else {
        config.define("PARAKEET_GGML_METAL", "OFF");
    }
    config.build_target("parakeet").build();

    println!("cargo:include={}", src.join("include").display());
    // Expose the OUT_DIR root to downstream build scripts
    // (DEP_PARAKEET_CPP_SYS_ROOT) so they can locate the compiled static
    // libraries and re-emit link flags.
    println!("cargo:root={}", out.display());

    // Merge every produced archive into a single shared library and hand the
    // path to teletype-speech (DEP_PARAKEET_CPP_SYS_LIB).
    //
    // The final binary links BOTH parakeet engines, and the two ship two
    // different ggml versions under identical archive names (libggml*.a). A
    // static link would bind every ggml symbol from whichever archive is
    // first on the search path, so parakeet.cpp's objects (compiled against
    // ggml 0.13) would run on the whisper.cpp ggml 0.9 runtime and crash.
    // A separate shared library keeps the two ggml runtimes disjoint: the
    // provider dlopens this library at load time and resolves the C API
    // through it.
    if cargo_target_os == "macos" {
        let build_dir = out.join("build");
        let mut archives: Vec<PathBuf> = Vec::new();
        for name in [
            "libparakeet.a",
            "libggml.a",
            "libggml-base.a",
            "libggml-cpu.a",
            "libggml-metal.a",
            "libggml-blas.a",
        ] {
            let mut stack = vec![build_dir.clone()];
            while let Some(dir) = stack.pop() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            stack.push(p);
                        } else if p.file_name().map(|f| f == name).unwrap_or(false) {
                            archives.push(p.clone());
                        }
                    }
                }
            }
        }
        if archives.is_empty() {
            panic!(
                "parakeet-cpp-sys: no archives produced under {}",
                build_dir.display()
            );
        }
        // Merge the archives into one (libtool -static keeps every object,
        // including same-named ones from different archives), then link that
        // single archive into a shared library.
        let merged = out.join("libparakeet_cpp_merged.a");
        let _ = std::fs::remove_file(&merged);
        let merged_s = merged.to_string_lossy().into_owned();
        let arch_args: Vec<String> = archives
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let status = std::process::Command::new("libtool")
            .args(["-static", "-o", &merged_s])
            .args(&arch_args)
            .output()
            .expect("run `libtool` to merge parakeet.cpp archives");
        println!(
            "cargo:warning=libtool exit={:?} merged={merged_s}",
            status.status
        );
        for a in &arch_args {
            println!(
                "cargo:warning=  archive: {a} exists={} size={}",
                std::path::Path::new(a).exists(),
                std::fs::metadata(a).map(|m| m.len()).unwrap_or(0)
            );
        }
        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            panic!(
                "parakeet-cpp-sys: `libtool -static` failed with {}: {stderr}",
                status.status
            );
        }
        let merged_size = std::fs::metadata(&merged).map(|m| m.len()).unwrap_or(0);
        if merged_size < 1_000_000 {
            panic!(
                "parakeet-cpp-sys: merged archive suspiciously small ({merged_size} bytes); archives: {:?}",
                arch_args
            );
        }
        let dylib = out.join("libteletype_parakeet_cpp.dylib");
        let _ = std::fs::remove_file(&dylib);
        let dylib_s = dylib.to_string_lossy().into_owned();
        let merged_s = merged.to_string_lossy().into_owned();
        let status = std::process::Command::new("clang++")
            .args(["-shared", "-o", &dylib_s, "-Wl,-all_load", &merged_s])
            .args([
                "-lc++",
                "-framework",
                "Accelerate",
                "-framework",
                "Foundation",
                "-framework",
                "Metal",
                "-framework",
                "MetalKit",
                "-framework",
                "QuartzCore",
                "-framework",
                "CoreGraphics",
            ])
            .status()
            .expect("run `clang++` to link the parakeet.cpp shared library");
        if !status.success() {
            panic!("parakeet-cpp-sys: shared-library link failed with {status}");
        }
        // Verify the dylib actually exports the C API (guards against a
        // silently-empty shared library).
        let nm = std::process::Command::new("nm")
            .arg("-gU")
            .arg(&dylib)
            .output()
            .expect("run `nm` on the parakeet.cpp shared library");
        let nm_out = String::from_utf8_lossy(&nm.stdout);
        if !nm_out.contains("parakeet_capi_load") {
            panic!(
                "parakeet-cpp-sys: shared library missing parakeet_capi symbols ({} bytes)",
                std::fs::metadata(&dylib).map(|m| m.len()).unwrap_or(0)
            );
        }
        println!("cargo:rustc-env=PARAKEET_CPP_SYS_LIB={}", dylib.display());
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
}

fn fetch_and_extract(_out: &Path, src: &Path) {
    println!("cargo:warning=Downloading parakeet.cpp {PARAKEET_COMMIT}…");
    let mut data = Vec::new();
    let mut reader = ureq::get(PARAKEET_URL)
        .set("User-Agent", "teletype/0.1")
        .call()
        .expect("download of parakeet.cpp failed")
        .into_reader();
    std::io::Read::read_to_end(&mut reader, &mut data)
        .expect("reading parakeet.cpp archive failed");

    // GitHub zipballs nest everything under a single top-level directory.
    let archive = zip::ZipArchive::new(std::io::Cursor::new(data)).expect("invalid zip");
    let _ = std::fs::remove_dir_all(src);
    std::fs::create_dir_all(src).expect("create source dir");
    extract_zip(archive, src);
}

/// Downloads a GitHub zipball and extracts it into `dest`, stripping the
/// single top-level prefix directory.
fn fetch_zip(url: &str, dest: &Path) {
    println!("cargo:warning=Downloading {url}…");
    let mut data = Vec::new();
    let mut reader = ureq::get(url)
        .set("User-Agent", "teletype/0.1")
        .call()
        .expect("download failed")
        .into_reader();
    std::io::Read::read_to_end(&mut reader, &mut data).expect("reading archive failed");

    let archive = zip::ZipArchive::new(std::io::Cursor::new(data)).expect("invalid zip");
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest).expect("create source dir");
    extract_zip(archive, dest);
}

fn extract_zip(mut archive: zip::ZipArchive<std::io::Cursor<Vec<u8>>>, src: &Path) {
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
