---
id: 2026-10-15-swift-rs-swiftpm-layout-and-rpath
title: swift-rs 1.0.9 on a CLT-only macOS 27 fails to link Swift packages twice over — SwiftPM product layout moved, and the Swift runtime needs an rpath
status: verified
type: integration / build-system
area: crates/teletype-fluid (Swift bridge) / build.rs
agent: opencode
tool: opencode (lead session, inline implementation)
date: 2026-10-15
---

# swift-rs 1.0.9 + Swift 6.4 CLI tools: two hard build constraints

Both were hit building `crates/teletype-fluid` (P3-A, FluidAudio bridge) and
both cost a build cycle each. Remember them the next time any crate links
Swift on this machine class (CLT-only, no Xcode).

## 1. SwiftPM product layout: `out/Products/<Debug|Release>`, not the legacy arch dir

`swift build --build-path X` on this toolchain writes the static archive to

    X/out/Products/Debug/libTeletypeFluid.a

`swift-rs` 1.0.9 computes the linker search path as

    X/<arch>-apple-macosx/<configuration>        // legacy layout

and has an Xcode-27-aware fallback (`X[/out]/Products/<capitalized config>/`)
that only runs when `xcrun xcodebuild -version` succeeds — which it never does
on a CLT-only machine (`xcodebuild` requires Xcode). Result: build script
"success", then `cargo test` fails at link with a missing
`libTeletypeFluid.a`.

Fix (in `build.rs`, after `SwiftLinker::link()`): emit the real path yourself.

    println!("cargo:rustc-link-search=native={}", products.display());

where `products = $OUT_DIR/swift-rs/<Package>/out/Products/<Debug|Release>`
(`Debug` for `DEBUG=true`, else `Release`). The bogus legacy `-L` swift-rs
emits is harmless; the linker finds the archive in the extra directory.

Verification: `swift build --show-bin-path` prints the real product dir;
`nm .../libTeletypeFluid.a | grep _ttf_` shows the `@_cdecl` symbols as
global `T` on this toolchain (no Xcode-27 symbol-internalization fix-up
needed).

## 2. The Swift runtime dylibs need `-rpath /usr/lib/swift`

The archive's autolinking references `@rpath/libswift_Concurrency.dylib`
(and friends). Those dylibs are not real files under
`/Library/Developer/CommandLineTools/usr/lib/swift/macosx` — they live in the
OS dyld cache under `/usr/lib/swift` (compare a plain `swiftc` binary: it
records absolute `/usr/lib/swift/libswiftCore.dylib` and has no rpath at all).
Rust binaries get no such rpath, so the test binary aborted at launch with
`Library not loaded: @rpath/libswift_Concurrency.dylib`.

Fix (in `build.rs`):

    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

`cargo:rustc-link-arg` covers the package's own tests/bins only. **Downstream
packages (the Tauri app) do not inherit it** — whoever links a Swift static
library into the app binary must add the same rpath itself. Verifying
`otool -l <binary> | grep -A2 LC_RPATH` is the cheap check.

## Reusable lesson

- Treat swift-rs's artifact discovery as advisory: after it runs, assert the
  product directory and add it explicitly. A `cargo:warning` in the else
  branch turns a future layout change into a clear message instead of a
  mystery link failure.
- "Runs in SwiftPM" and "runs as a Rust binary" are different launch
  environments: SwiftPM's own binaries carry toolchain-provided loader paths;
  Rust binaries do not. Exercise the Rust binary (`cargo test`) as part of the
  bridge's own verification, not just `swift build`.
- `xcrun xcodebuild -version` failing is the normal state here; anything that
  branches on Xcode detection (swift-rs, build tooling) will silently take the
  CLT path.
