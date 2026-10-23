---
id: 2026-09-27-ax-headers-need-pregenerated-bindings
title: Accessibility code must not be written in Objective-C on this toolchain
date: 2026-09-27
agent: OpenCode (Space Bunny Free)
area: build / macOS / injection
status: verified
evidence: crates/teletype-desktop/src/ax_text.rs; crates/teletype-desktop/Cargo.toml
---

# Accessibility code must not be written in Objective-C on this toolchain

## Problem

Adding the direct AX injection tier (write at the caret through the
Accessibility API) started as an Objective-C bridge, `ax_write.m`, matching the
repo's existing `.m` pattern.

## Symptoms

Compiling the `.m` file failed with undeclared identifiers for
`kAXSearchFieldRole`, `kAXTextURLFieldRole`, `kAXSecureTextFieldRole`,
`kAXSystemWideElement`, `AXValueGetRangeValue`, and `CFTypeGetID`.

## Investigation

1. The repo's other `.m` bridges build fine, so the toolchain is not simply
   missing Objective-C support. Those files use Cocoa, Foundation, AVFoundation,
   Carbon, and CoreGraphics, all of which resolve.
2. `xcrun --show-sdk-path` points at
   `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`. There is no
   `/Applications/Xcode.app` on this machine.
3. `ApplicationServices.framework/Headers/` in that SDK contains exactly three
   files (`ApplicationServices.h`, `.r`, `.apinotes`) and zero AX headers. The
   Accessibility API headers ship only in the full Xcode SDK.
4. `grep` for the AX symbols in `ApplicationServices.tbd` found
   `AXUIElementCopyAttributeValue`, `AXUIElementSetAttributeValue`,
   `AXUIElementCreateSystemWide`, `AXValueCreate`, `AXValueGetTypeID`, and
   `AXValueGetValue`. So the *symbols* are present and linkable; only the headers
   are missing.

## Root cause

Two distinct gaps. The headers are absent from the Command Line Tools SDK. The
`kAX*` attribute and role constants are `CFSTR` macros in the C headers, not
symbols, so even the pre-generated bindings do not cover them.

## Fix

Deleted `ax_write.m` and wrote the tier in Rust against
`objc2-application-services`, which ships pre-generated AX bindings
(`AXUIElement`, `AXValue`, `AXError`, `AXValueType`) and therefore compiles
without the Xcode SDK headers. The `kAX*` names are constructed as plain
`&'static str` constants (`"AXValue"`, `"AXFocusedUIElement"`, `"AXTextField"`,
and so on) and the string values are pinned by a test, because a typo in a
constant silently breaks every AX call with no compile error.

Also required: the umbrella `HIServices` feature, not just the leaf `AX*` ones,
since the leaf modules are not re-exported at the crate root without it.

## Verification

- `cargo build -p teletype-desktop` clean.
- `cargo test -p teletype-desktop --lib`: 48 passed, including the constant-pinning
  test and the writable-role allowlist.
- CI uses `macos-latest` with full Xcode, and the Command Line Tools path is the
  stricter of the two, so this is the configuration most likely to break.

## Reusable lesson

When a framework's headers are missing from the active SDK, check whether the
linker symbols are present before assuming the approach is blocked. Missing
headers and missing symbols are different problems with different solutions:
pre-generated bindings fix the first, and neither fixes the second.

Do not assume a repo's established pattern (Objective-C bridges) is right for a
new dependency. It was right for run loops and ObjC blocks, which are genuinely
awkward from Rust. AX calls are plain C over CF types, and Rust plus pre-generated
bindings is both simpler and portable across toolchains.
