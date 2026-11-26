# Plan 014: Add the three vendored C/C++ engine pins to THIRD-PARTY-NOTICES.txt and the pin-consistency check

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- THIRD-PARTY-NOTICES.txt scripts/check-artifact-pins.sh crates/parakeet-sys/build.rs crates/parakeet-cpp-sys/build.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

The app ships three substantial C/C++ codebases built from pinned commits:
whisper.cpp v1.9.4 (via `parakeet-sys`), parakeet.cpp at commit `5c9b0fbb`
(via `parakeet-cpp-sys`), and the ggml submodule at commit `e705c5fe` (fetched
separately because GitHub zipballs exclude submodules). None of the three
appear in `THIRD-PARTY-NOTICES.txt`, and none are checked by
`scripts/check-artifact-pins.sh`. A license-audit or compliance review reading
only the notices file would conclude these codebases are absent from the
shipped binary — a legal risk for a distributed app. The pin-consistency
check exists precisely to catch drift (a 2026-10 incident is documented in
the script header), but it only covers llama.cpp.

## Current state

- `crates/parakeet-sys/build.rs:11` — `const WHISPER_TAG: &str = "v1.9.4";`
- `crates/parakeet-sys/build.rs:12` — `const WHISPER_URL: &str = "https://api.github.com/repos/ggml-org/whisper.cpp/zipball/v1.9.4";`
- `crates/parakeet-cpp-sys/build.rs:14` — `const PARAKEET_COMMIT: &str = "5c9b0fbbd8e611d88e47d7e1ebae9bca9b447e8f";`
- `crates/parakeet-cpp-sys/build.rs:20` — `const GGML_COMMIT: &str = "e705c5fed490514458bdd2eaddc43bd098fcce9b";`
- `crates/parakeet-cpp-sys/build.rs:10` — comment claims "The pinned commit is recorded here and in THIRD-PARTY-NOTICES.txt" — **false** for the notices file.
- `THIRD-PARTY-NOTICES.txt` — entries start with `=== <name> <version> ===`. The llama.cpp entry is at line 22. No entry for whisper.cpp, parakeet.cpp, or ggml.
- `scripts/check-artifact-pins.sh` — header at lines 28-30 documents the EXTENSION POINT: "add another row to the extraction table below — one extraction function, one source label, one value variable — and include it in the comparison."

**Licensing facts** (verify before writing notices entries):
- whisper.cpp: MIT (see `LICENSE` in the whisper.cpp repo)
- parakeet.cpp: check `LICENSE` in `mudler/parakeet.cpp` at the pinned commit — likely MIT or GPL; **if GPL, STOP and report** (the app is MIT and cannot ship GPL code)
- ggml: MIT (see `LICENSE` in the ggml repo)

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Pin check | `bash scripts/check-artifact-pins.sh` | exit 0, prints "OK" for every pin |
| Notices check | `bash scripts/check-notices.sh` | exit 0 |
| Brain freshness | `bash scripts/check-brain-freshness.sh` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `THIRD-PARTY-NOTICES.txt`
- `scripts/check-artifact-pins.sh`
- `crates/parakeet-cpp-sys/build.rs` (fix the false comment at line 10)

**Out of scope** (do NOT touch):
- `crates/parakeet-sys/build.rs` — the pin is already there; only the notices and check script need updating
- Any Cargo.toml, Cargo.lock, or build script logic
- Any Rust or TypeScript source file

## Git workflow

- Branch: `advisor/014-engine-pins`
- Commit message style: conventional commits, e.g. `fix(notices): add whisper.cpp, parakeet.cpp, ggml pins to THIRD-PARTY-NOTICES.txt`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Verify the parakeet.cpp license

Open `https://github.com/mudler/parakeet.cpp/blob/5c9b0fbbd8e611d88e47d7e1ebae9bca9b447e8f/LICENSE` (or fetch the zipball and read the LICENSE file). Confirm the license is MIT, BSD, or Apache-2.0. **If it is GPL or any copyleft license, STOP and report** — the app cannot ship it.

**Verify**: license is permissive (MIT/BSD/Apache-2.0). Record the exact license name in your notes.

### Step 2: Add three entries to THIRD-PARTY-NOTICES.txt

Add three entries following the existing format (`=== <name> <version> ===` header, then a short description and license line). Place them after the llama.cpp entry (line 22 block) and before the first crate entry (line 61).

Format to match:

```
=== whisper.cpp v1.9.4 (ggml-org) ===
Vendored and compiled into the binary via parakeet-sys/build.rs (CMake).
Provides the Parakeet TDT ASR engine and the ggml backend.
License: MIT (https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/LICENSE)
Pinned at: crates/parakeet-sys/build.rs (WHISPER_TAG)

=== parakeet.cpp 5c9b0fbb (mudler) ===
Vendored and compiled into the binary via parakeet-cpp-sys/build.rs (CMake).
C++/ggml Parakeet runtime with per-tensor quant-type GGUF loader.
License: <LICENSE FROM STEP 1> (<URL>)
Pinned at: crates/parakeet-cpp-sys/build.rs (PARAKEET_COMMIT)

=== ggml e705c5fe (ggml-org, submodule of parakeet.cpp) ===
Fetched separately (GitHub zipballs exclude submodules) and compiled via
parakeet-cpp-sys/build.rs. ggml backend for the parakeet.cpp runtime.
License: MIT (https://github.com/ggml-org/ggml/blob/e705c5fed490514458bdd2eaddc43bd098fcce9b/LICENSE)
Pinned at: crates/parakeet-cpp-sys/build.rs (GGML_COMMIT)
```

**Verify**: `grep -c "whisper.cpp\|parakeet.cpp\|ggml" THIRD-PARTY-NOTICES.txt` returns ≥ 6 (each name appears in header + pin line).

### Step 3: Extend scripts/check-artifact-pins.sh

Follow the EXTENSION POINT documented in the script header (lines 28-30). Add three extraction rows to the pin-consistency check, mirroring the llama.cpp pattern:

1. **whisper.cpp tag**: extract `WHISPER_TAG` from `crates/parakeet-sys/build.rs`, compare against the value in `THIRD-PARTY-NOTICES.txt` (the `v1.9.4` in the `=== whisper.cpp v1.9.4 ===` header).
2. **parakeet.cpp commit**: extract `PARAKEET_COMMIT` from `crates/parakeet-cpp-sys/build.rs`, compare against the 8-char prefix in the notices header.
3. **ggml commit**: extract `GGML_COMMIT` from `crates/parakeet-cpp-sys/build.rs`, compare against the 8-char prefix in the notices header.

The script already has the pattern: one extraction function, one source label, one value variable, included in the comparison. Read the existing llama.cpp extraction (lines 45-100) and mirror it.

Add env overrides for the two build scripts (mirroring the existing `LLAMA_SCRIPT` pattern):
- `PARAKEET_SYS_BUILD=...` (default `crates/parakeet-sys/build.rs`)
- `PARAKEET_CPP_BUILD=...` (default `crates/parakeet-cpp-sys/build.rs`)

**Verify**: `bash scripts/check-artifact-pins.sh` → exit 0, prints OK for all four pins (llama.cpp + three new ones).

### Step 4: Negative test — introduce drift and confirm the check catches it

Temporarily change `WHISPER_TAG` in `crates/parakeet-sys/build.rs` from `v1.9.4` to `v1.9.5`. Run `bash scripts/check-artifact-pins.sh` → expect **exit 1** with a drift message. Revert the change. Run again → exit 0.

**Verify**: drift detected (exit 1), revert restores (exit 0).

### Step 5: Fix the false comment in parakeet-cpp-sys/build.rs

Line 10 currently says: "The pinned commit is recorded here and in THIRD-PARTY-NOTICES.txt."

After Step 2, this is now true. No change needed — but verify the comment is accurate. If the notices entries use a different format than the comment implies, adjust the comment to match reality.

**Verify**: `sed -n '10p' crates/parakeet-cpp-sys/build.rs` — comment is accurate.

## Test plan

- The pin-consistency script IS the test. Step 4 (negative test) proves it catches drift.
- No new Rust or TypeScript tests needed — this is a notices + shell-script change.

## Done criteria

- [ ] `bash scripts/check-artifact-pins.sh` exits 0 and reports all four pins consistent
- [ ] `bash scripts/check-artifact-pins.sh` exits 1 when a pin is drifted (negative test)
- [ ] `grep "whisper.cpp" THIRD-PARTY-NOTICES.txt` returns the new entry
- [ ] `grep "parakeet.cpp" THIRD-PARTY-NOTICES.txt` returns the new entry
- [ ] `grep "ggml" THIRD-PARTY-NOTICES.txt` returns the new entry
- [ ] No files outside the in-scope list are modified (`git diff --name-only`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- parakeet.cpp's license is GPL or any copyleft license (Step 1).
- The code at the locations in "Current state" doesn't match the excerpts.
- A step's verification fails twice after a reasonable fix attempt.
- The fix appears to require touching an out-of-scope file.

## Maintenance notes

- If `parakeet-sys` bumps `WHISPER_TAG`, the pin check will catch the notices drift. Update both in the same commit.
- If `parakeet-cpp-sys` bumps `PARAKEET_COMMIT` or `GGML_COMMIT`, same rule.
- The notices entries use 8-char commit prefixes for readability; the full 40-char hash is in the build script. The check script compares the prefix.
