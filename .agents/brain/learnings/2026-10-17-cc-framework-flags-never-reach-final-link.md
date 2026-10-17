# L035 — cc::Build `-framework` flags never reach the final link; lib tests hide the break

- **ID:** L035
- **Title:** P7.6's Contacts bridge compiled with `-framework Contacts` but the app binary failed to link (`CNContact*Key` undefined); `--lib` tests could not see it
- **Date:** 2026-10-17
- **Agent/tool:** OpenCode lead session (worktree `ahsorathiya7-fuzzy-happiness`)
- **Project area:** `crates/teletype-desktop/build.rs` (P7.6 contacts import bridge)
- **Status:** verified
- **Evidence:** `cargo test -p teletype-desktop --lib` 189/189 green while `cargo test --workspace` (bin `teletype` link) failed: `Undefined symbols ... _CNContactGivenNameKey, referenced from _teletype_contacts_fetch in contacts_import.o`. Fix: `println!("cargo:rustc-link-lib=framework=Contacts");` in the P7.6 build.rs block; bin then linked and the full workspace gate went green.

## Problem

A new ObjC bridge (`contacts_import.m`) compiled fine and its crate's lib
tests passed, but the app binary failed to link with undefined
`CNContact*Key` symbols only in the final `teletype` bin.

## Root cause

`cc::Build`'s `.flag("-framework").flag("Contacts")` applies to the **clang
compile** step of the `.m` file (where it is ignored/harmless). What links
into the final binary is driven by `cargo:rustc-link-lib=` build-script
output, which the block never emitted. The break stayed invisible in lib
tests because rlibs are archives: `contacts_import.o` is only extracted into
a test binary when some reachable symbol needs `teletype_contacts_fetch`.
The lib test binary never pulls it in, so its undefined references are
never resolved; the bin (where `main.rs` -> `commands` references it) does.

ObjC **classes** (e.g. `CNContactStore`) need no link-time symbols - the
ObjC runtime resolves them - so existing bridges that only use classes and
protocols link fine without `rustc-link-lib`. **C-level constants and
functions** (string keys, `extern` constants) do need the framework linked.

## Rule

For native (`.m`/`.c`) sources that reference C-level symbols from a
framework, emit `cargo:rustc-link-lib=framework=<Framework>` in build.rs;
`cc.flag("-framework")` alone does not reach the final link. Gate any new
native bridge with `cargo test --workspace` (forces the bin link), not just
the crate's `--lib` tests.

## Verification

After the one-line fix: `cargo test --workspace` 0 failures (bin linked),
`cargo clippy --workspace -- -D warnings` clean (2026-10-17).
