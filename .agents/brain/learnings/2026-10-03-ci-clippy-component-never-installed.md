---
id: 2026-10-03-ci-clippy-component-never-installed
title: CI's clippy step never ran - the toolchain install omitted the clippy component and 2>/dev/null hid it
status: verified
type: debugging
area: ci / lint / release plumbing
agent: kilo
tool: kilo-auto/free
date: 2026-10-03
---

# CI's clippy step never ran: the toolchain install omitted the clippy component and `2>/dev/null` hid it

## Problem

The `Test (Rust + UI)` job in `.github/workflows/ci.yml` failed on every push
to `main`, continuously, for weeks. The log's last two lines before the red X
contradicted each other:

    clippy: clean (no lint warnings)
    ##[error]Process completed with exit code 1.

A linter reporting success immediately before the step dies reads like a flaky
or misconfigured gate. It was neither: the linter had never executed once.

## Symptoms

- Every `CI` run on `main` failed in `Test (Rust + UI)`, at the final `Clippy`
  step. Every other step, including four `cargo test` invocations, passed.
- The `Clippy` step took **0.3 seconds** while the `cargo test` steps in the
  same job took 10-11 minutes each.
- The step printed `clippy: clean (no lint warnings)` and then exited 1.
- The repo was, in fact, clippy-clean: a local run on the same pinned
  toolchain exited 0.

## Initial hypotheses

1. Real lint warnings, being fixed one commit at a time. Several commits in the
   log are literally titled `fix(lint): clear the 5 clippy warnings failing CI`
   and `fix(qa): make qa.sh fmt and clippy stages match CI`.
2. `scripts/check-clippy-clean.py` miscounting, or the clang `-framework ...
   'linker' input unused` build-script noise being treated as fatal.
3. A flake in the shell pipeline (`set -o pipefail` misbehaving).
4. **The clippy component was never installed** (confirmed).

## Investigation

1. `gh run list` -> every recent `CI` run on `main` failed; Windows compile,
   macOS Intel and Dictation eval jobs all passed. Only `Test (Rust + UI)`
   failed, and only at the last step.
2. `gh run view --log-failed` -> `clippy: clean` on stdout, exit code 1, and
   **zero** `Checking`/`Compiling` lines in the step's 0.3s window.
3. `gh run view --log` -> the toolchain action's expanded command line:

       rustup toolchain install 1.90 --component rustfmt --profile minimal

   `--profile minimal` is rustc + rust-std + cargo. **No clippy.** The workflow
   asked for `components: "rustfmt"` and nothing else.
4. Local `cargo +1.90 clippy --version` -> `clippy 0.1.90`, matching CI's
   1.90.0. So the versions were never the problem; only the component list was.
5. Reproduced the exact failure by installing a rustfmt-only minimal toolchain
   and running the CI command against it:

       error: 'cargo-clippy' is not installed for the toolchain '1.91-...'.
       help: run `rustup component add --toolchain 1.91-... clippy` to install it

   Piping that through `scripts/check-clippy-clean.py` reproduced the signature
   exactly: `clippy: clean (no lint warnings)` on stdout, pipeline exit 1.

## Evidence

- CI logs for runs 37130633009, 37139950208, 37145653715: the install command
  line contains `--component rustfmt` and no clippy; the Clippy step is 0.3s
  with no compiler output; the last two lines are `clippy: clean` + exit 1.
- A toolchain installed with `--profile minimal --component rustfmt` only:
  `cargo clippy` exits 1 with the message above and writes **nothing** to
  stdout, because `--message-format=json` output only exists once a real
  compilation runs.
- `scripts/check-clippy-clean.py` (before the fix) treated an empty stdin as
  "no warnings found" and printed `clippy: clean`, returning 0.
- `set -o pipefail` then propagated clippy's exit 1, so the step failed while
  the checker claimed success.

## Root cause

Three independent facts compounded into one opaque failure:

1. **`dtolnay/rust-toolchain` installs `--profile minimal`**, which contains
   rustc, rust-std and cargo but *not* clippy. Requesting
   `components: "rustfmt"` therefore leaves `cargo clippy` non-functional.
2. **`2>/dev/null` on the clippy command discarded cargo's stderr**, which is
   the only place the `cargo-clippy is not installed` error appears.
3. **The JSON filter read an empty stream as "clean"**, printing
   `clippy: clean (no lint warnings)` and exiting 0, because "no warnings"
   and "no lint ran" are indistinguishable when you only count what arrived.

`set -o pipefail` then surfaced the failure with no explanation attached.

Net effect: **CI has never run clippy once.** Every commit titled "clear the
clippy warnings failing CI" was written against a linter CI could not have
reported from.

## Fix

- `ci.yml`: `components: "rustfmt, clippy"`.
- `ci.yml` + `scripts/qa.sh`: dropped `2>/dev/null` so cargo's own diagnostics
  reach the log. `check-clippy-clean.py` only reads stdout, so keeping stderr
  costs nothing and is the difference between a legible error and a bare exit
  code.
- `scripts/check-clippy-clean.py`:
  - fails on `compiler-message` entries with `level == "error"` (previously
    ignored entirely, so a compile error read as "no warnings");
  - fails unless the run demonstrably happened, counted via
    **`compiler-artifact`** messages plus `build-finished.success`.

The choice of `compiler-artifact` as the liveness signal is load-bearing and
was itself a mistake on the first attempt: a clean workspace emits **zero**
`compiler-message` lines (only `compiler-artifact` and `build-finished`), so
counting those to decide "did clippy run" fails every healthy build. The
local clean run produced 604 artifacts, 71 `build-script-executed`, 1
`build-finished`, 0 `compiler-message`.

## Verification

On the same toolchain CI pins (1.90.0 / clippy 0.1.90):

- the exact CI pipeline (`set -o pipefail; cargo clippy --workspace
  --all-targets --message-format=json | python3 scripts/check-clippy-clean.py`)
  exits 0 with `clippy: clean (604 crate(s), 0 lint warnings)`.
- checker exits 1 on: an empty stream, `build-finished.success=false`, a real
  lint with a `code`, and a `level: "error"` compiler message.
- checker exits 0 on build-script clang `-framework ... 'linker' input unused`
  noise (still non-fatal, as intended).

## Reusable lesson

- **`--profile minimal` does not include clippy.** Any CI job that runs
  `cargo clippy` must list `components: "rustfmt, clippy"` (or equivalent) on
  `dtolnay/rust-toolchain`. Verify by grepping the *expanded* command line in
  the run log (`rustup toolchain install ... --component ...`), not by reading
  the workflow, because the omission is invisible in the YAML's brevity.
- **`2>/dev/null` on a `cargo --message-format=json` pipeline is a trap.** It
  removes the only channel carrying the reason cargo failed. Keep stderr.
- **A checker that counts findings cannot distinguish "no findings" from "did
  not run."** Any pass/fail filter fed by a subprocess must independently
  assert that the subprocess did its work - here, `compiler-artifact` > 0 and
  `build-finished.success == true`.
- **Compare the duration of a suspect step against its neighbours.** 0.3s next
  to 10-minute cargo steps is the tell that nothing ran; no error message was
  needed to find this one.
- **"Passes locally, fails in CI" with an opaque CI failure is a CI-config
  bug until proven otherwise.** Check that CI runs the same tools you run, at
  the same versions, with the same components installed.
- **Beware the "just fix the warnings" trap.** When a gate has been red for a
  long time and commits keep claiming to fix it, verify the gate is capable of
  observing the thing it claims to check before believing any of them.

## Related

- `2026-10-03-headless-objc-exception-aborts-test-binary` verified its fix with
  "clippy (via `scripts/check-clippy-clean.py`) clean". That local claim was
  true; the same check in CI was a no-op. Two different runners, two different
  guarantees - worth separating when reading any "CI is green" claim about a
  gate added in the same change.

## Sources

- CI runs: 37130633009, 37139950208, 37145653715 (job "Test (Rust + UI)")
- Fix commits: `53d651f` (clippy component + checker), on `main`.
