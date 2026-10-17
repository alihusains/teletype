// The `teletype-fluid` engine dependency drags Swift runtime dylibs
// (referenced `@rpath/...` by Swift autolinking) into this crate's binaries
// and test binaries. teletype-fluid's own build script applies the rpath to
// its own artifacts only (see `crates/teletype-fluid/build.rs`), so
// downstream binaries must add it themselves. This keeps
// `cargo test -p teletype-eval` runnable without `DYLD_LIBRARY_PATH`.
#[cfg(target_os = "macos")]
fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
