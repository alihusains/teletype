# Plan 013: Guard the Windows llama.cpp release tag in the pin check

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- scripts/check-artifact-pins.sh scripts/build-llama-server.sh .github/workflows/release.yml THIRD-PARTY-NOTICES.txt
> If any file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dependencies / dx
- **Planned at**: commit 8746c1c, 2026-11-25

## Why this matters

scripts/check-artifact-pins.sh compares the 40-hex llama.cpp commit across three sources, but the Windows prebuilt is also identified by a release tag (b12200) that appears independently in the release workflow's zip name and download URL, in the build script, and in THIRD-PARTY-NOTICES.txt. Bumping only the tag passes the current check, so macOS and Windows can ship different llama-server builds while all checks are green — exactly the drift this script was written to prevent.

## Current state

- scripts/check-artifact-pins.sh:1-27 — header says the pin lives in THREE independent places and names them; it documents an EXTENSION POINT (add one extraction function, one source label, one value, include in comparison). Env overrides at :34-37: LLAMA_SCRIPT, RELEASE_WORKFLOW, NOTICES_FILE, SPEECH_CATALOG.
- :55-87 — extracts script_pin (COMMIT=<40hex>), wf_pin (llama.cpp pin: <40hex>), notices_pin (commit <40hex>), then compares the three and fails on mismatch. :89 onward separately checks the speech model smoke URL.
- scripts/build-llama-server.sh:26-30 — comment block mentions b12200, b11960 and b11488; COMMIT="c19043f67e52f1f1e00d797246e19643bb2f168e" at :30. The multiple b-numbers in prose mean a regex over the whole file is unsafe — add an explicit marker instead.
- .github/workflows/release.yml:824-834 — comment "# llama.cpp pin: c19043f... (release b12200)"; then $zip = 'llama-b12200-bin-win-vulkan-x64.zip' and $url = ".../releases/download/b12200/$zip".
- THIRD-PARTY-NOTICES.txt:30 — "b12200 Vulkan prebuilt, which is the same commit."
- Current pinned commit is consistent across sources; the tag is currently b12200 everywhere.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Pin check | bash scripts/check-artifact-pins.sh | exit 0, prints OK for commit, tag and model URL |
| Shell syntax | bash -n scripts/check-artifact-pins.sh | exit 0 |
| Negative test | temp copies with one tag changed, run with env overrides | exit 1 naming the deviating source |

## Scope

In scope:
- scripts/check-artifact-pins.sh
- scripts/build-llama-server.sh (one added marker line)
- .github/workflows/release.yml (one added marker comment line)
- THIRD-PARTY-NOTICES.txt (one added marker line in the llama.cpp section)

Out of scope (do NOT touch):
- The actual pinned values (commit c19043f..., tag b12200) — this plan adds a guard, it does not bump anything.
- The Windows zip name, URL, or any release step logic.
- The speech-model smoke URL check at the bottom of the script.
- Any other notices entry or script.

## Git workflow

- Branch: advisor/013-release-tag-pin
- Commit message style: fix(ci): check the Windows llama.cpp release tag too
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add explicit tag markers to the three sources

Add one machine-readable marker line to each source, adjacent to the existing pin text:
- scripts/build-llama-server.sh, just above COMMIT=: LLAMA_RELEASE_TAG="b12200" with a short comment that it must match the Windows prebuilt in release.yml.
- .github/workflows/release.yml, inside the existing pin comment block near :829: # llama.cpp release tag: b12200
- THIRD-PARTY-NOTICES.txt, in the llama.cpp attribution block near line 30: llama.cpp release tag b12200

Do not change any existing text or value. The markers are additive and make extraction unambiguous in files that contain other b-numbers.

Verify: grep -rn 'LLAMA_RELEASE_TAG\|llama.cpp release tag' scripts/build-llama-server.sh .github/workflows/release.yml THIRD-PARTY-NOTICES.txt -> one match per file.

### Step 2: Extract and compare the tag in the check script

In scripts/check-artifact-pins.sh:
- Update the header comment from THREE to FOUR independent places and add the release tag to the list.
- Following the documented EXTENSION POINT, add three extractions using grep -oE with the exact marker text from step 1, e.g. LLAMA_RELEASE_TAG="b[0-9]+" from the script, "llama.cpp release tag: b[0-9]+" from the workflow, "llama.cpp release tag b[0-9]+" from the notices. Fail if any extraction is empty (a missing pin is a failure, not a skip).
- Compare all three tag values against the first one and fail with a message naming each source and its value on mismatch. Keep the existing 40-hex commit comparison unchanged and independent — a tag mismatch and a commit mismatch must both be reported.
- Print an OK line for the tag on success, matching the existing output style.

Verify: bash -n scripts/check-artifact-pins.sh -> exit 0; bash scripts/check-artifact-pins.sh -> exit 0 with OK lines for commit, tag and the smoke URL.

### Step 3: Prove the new guard fails on drift

Using the existing env overrides and temp copies (never editing the real sources):
1. tmp=$(mktemp -d); copy the four source files into it.
2. sed the tag in the temp copy of release.yml from b12200 to b99999.
3. Run RELEASE_WORKFLOW="$tmp/release.yml" LLAMA_SCRIPT=... NOTICES_FILE=... SPEECH_CATALOG=... bash scripts/check-artifact-pins.sh and assert exit 1 with a message naming release.yml.
4. Repeat with the notice file's tag changed, asserting exit 1 naming THIRD-PARTY-NOTICES.txt.
5. Remove the temp directory.

Verify: both negative runs exit 1; the unmodified run still exits 0.

## Test plan

- No unit-test framework applies to this shell check. Verification is one positive run and two env-override negative runs (step 3).
- The negative runs are the meaningful tests: a check that cannot fail on tag drift is worse than no check.
- Verification: bash -n, positive run, two negative runs.

## Done criteria

- [ ] grep -n 'release tag' scripts/check-artifact-pins.sh shows extraction, comparison and the OK line.
- [ ] bash scripts/check-artifact-pins.sh exits 0 with three OK lines (commit, tag, model URL).
- [ ] Both negative env-override runs exit 1 naming the deviating source.
- [ ] No pinned value changed: grep -n 'COMMIT=\|b12200' scripts/build-llama-server.sh .github/workflows/release.yml THIRD-PARTY-NOTICES.txt still shows the original values.
- [ ] Only the four in-scope files plus plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live excerpts at build-llama-server.sh:26-30, release.yml:824-834 or notices:30 do not match.
- The marker strings already exist in a different form (adapt the extraction, but report the deviation).
- A negative test unexpectedly exits 0 — report it; that means the guard is not wired into the comparison.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- Every future llama.cpp bump must move the commit AND the tag in all three sources; this script will now catch a half-done bump.
- The markers are intentionally simple strings rather than prose regexes: the build script's comment contains historical b-numbers (b11960, b11488) that a broad pattern would pick up.
- If a fourth distribution channel adds its own prebuilt, add another marker and another comparison row rather than widening an existing regex.
