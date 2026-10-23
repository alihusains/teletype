# L050 — Three ways this pipeline reports green while it is broken

- **ID:** L050
- **Date:** 2026-10-23
- **Agent:** Qoder (post-v0.2.9 CI hardening)
- **Area:** `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `ui` toolchain, shell pipelines in steps
- **Status:** verified

## Problem

Five CI jobs went red on run 39893783804 (`release/v0.2.7`): Tests (inference),
Benches, macOS release link, Coverage gate, Checks. Two of those failure classes
had previously been invisible: the UI test suite was masked, and the local
reproduction of a build failure was hidden by my own log filter. The general
shape to remember is: a green check is only evidence about the command that
actually ran.

## 1. `continue-on-error` turned a real breakage into a pass

`cd ui && npx vitest run` carried `continue-on-error: true` from when the suite
landed. It then stopped running at all and the job still reported success.

Root cause was a toolchain floor, not a test bug: CI pinned `node-version: 20`
(runner image gave 20.20.2) while the installed packages require newer Node:

- vitest 5.0.5: `^22.12.0 || ^24.0.0 || >=26.0.0`
- jsdom 30: `^22.22.2 || ^24.15.0 || >=26.0.0`
- undici 8: `>=22.19.0`

Every fork worker died with `Cannot read properties of undefined (reading
'bind')` and vitest reported "no tests". A green job that runs zero tests is the
most expensive kind of green.

Fix: all six `node-version` pins in `ci.yml` + `release.yml` went to 24, and the
vitest step lost its `continue-on-error`. The `checks` job's vitest was the only
*blocking* UI gate, so a fix that only un-masked `ui` would have left the same
hole open.

Rule: `continue-on-error` is allowed on a step only where the comment says what
is deliberately not gated. The three that remain in `ci.yml` do
(`tests-core`'s "confirmed defects still reproduce" probe, the x86_64 compile
check, the bench and catalog-URL jobs). Anything that can "pass" by not
executing is a gate: prefer a step that fails when its test count is zero.

## 2. Shell pipelines report the last command, not the failing one

`cargo build --release ... 2>&1 | tail -8` returned 0 while cargo was failing on
LTO (see L049), because the pipeline status is `tail`'s. I reported a passing
build that had not happened.

Rule for any build/test step whose exit status matters: write full output to a
file and echo the status, or set `set -o pipefail`.

```bash
cargo build --release -p X > /tmp/build.log 2>&1; echo "EXIT=$?"
```

Related trap from L036 and still true: this class of failure prints a bare
`error:` line, not `error[E0xxx]`, so `grep "error\["` finds nothing in a dead
build.

## 3. A release-only failure needs a branch-only job that links the real binary

`cargo test -p teletype-desktop` cannot catch a shipping-link break: the test
harnesses never construct the speech provider, so ld64 never pulls the object
referencing the nemo symbols. Only the `teletype` executable link does. That is
why `link-macos` (job 5b) exists as a blocking `cargo build --release -p
teletype-desktop --bin teletype`. It is what caught L049 on a push instead of on
a tag.

Corollary for Windows: `windows-compile` is `cargo check --workspace`, which
catches the `libc::sigaction`-style compile errors on push but not a Windows
*link* error. The Windows test link is additionally gated off because a prebuilt
`needle-sys` `needle.cpp.obj` fails with `LNK1143` (corrupt upstream artifact,
recorded in the workflow comment). That is a known open hole, not a solved one.

## Verification

- Node floor evidence: read from the installed packages' `package.json`
  `engines` fields, not from memory.
- Un-masked vitest and the Node 24 pins: committed in `64da707`; awaiting the
  first CI run on them.
- `link-macos` turned red on 39893783804 with the L049 error, proving the gate
  fires before a tag.
