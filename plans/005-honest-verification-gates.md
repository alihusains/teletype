# Plan 005: Make the verification gates honest and non-duplicated

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- README.md scripts/qa.sh .github/workflows/ci.yml crates/teletype-core/tests/journey_smoke.rs
> If any file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests / dx
- **Planned at**: commit 8746c1c, 2026-11-24

## Why this matters

Four separate false signals currently let a broken checkout look green:
1. README's Testing block lists only cargo test and npm run typecheck, so a contributor runs zero of the ~621 Vitest tests that CI gates.
2. scripts/qa.sh prints RESULT: PASS — shippable while never running npm test.
3. CI runs the full UI gate twice (checks job and ui job), doubling install/typecheck/vitest cost, while a stale comment claims checks deliberately does not run vitest.
4. CI's step named "Confirmed defects still reproduce" runs cargo test --test journey_smoke -- --ignored, but that file contains zero #[ignore] attributes — only comments claiming there are some. The step has been a permanently-green no-op. Separately, the ITN 2b parity suite (263 failing rows) is ignored and excluded from every gate, with no visible guard against new ignores.

This plan makes each signal honest. It does NOT fix the 263 ITN rows — that is a separate, larger piece of work.

## Current state

- README.md:214-219 — Testing block:

      ## Testing

      cargo test              # tests across core, speech, and inference
      cd ui && npm run typecheck

  ui/package.json:10 defines "test": "vitest run"; ci.yml:106 runs cd ui && npm test. docs/release-notes-v0.2.22.md:28 reports 621 vitest tests.
- scripts/qa.sh:72-82 — ui_typecheck() runs tsc only; ui_build() runs vite build only. :145-159 is the stage list: rust fmt, clippy, rust tests, ui typecheck, ui build, release build, main-thread responsive, secret scan, ew/ leak scan, static repo checks, eval gate. :170 prints RESULT: PASS — shippable. Header says Usage: scripts/qa.sh [--fast] and --fast skips the release build.
- .github/workflows/ci.yml:65-106 — checks job (name: Checks (notices, manifest, UI types)) ends with setup-node 24, npm ci, npx tsc --noEmit, npm test.
- ci.yml:976-994 — ui job repeats setup-node 24, npm ci, npx tsc --noEmit, npx vitest run. Comment: "Phase 9: UI gate — typecheck (already in checks) plus the vitest suite, which the checks job deliberately does not run." That comment is stale. ci.yml:1013-1020 — gate.needs includes both checks and ui; keep both job IDs.
- ci.yml:238-240 — step "Confirmed defects still reproduce": cargo test -p teletype-core --test journey_smoke -- --ignored, continue-on-error: true.
- crates/teletype-core/tests/journey_smoke.rs:28-37 — comment says BUG-xx tests are #[ignore]d and tells CI to run --ignored; a repo-wide search finds ZERO #[ignore] attributes in that file. All BUG-001..020 tickets are fixed (bugs/README.md and PROJECT_BRAIN.md).
- crates/teletype-core/tests/itn_parity.rs:41 — the single real #[ignore] in core integration tests, reason: 263/2416 rows failing. This exclusion is intentional for now; make it visible, do not silently unignore it.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| UI tests | cd ui && npm test | exit 0, all tests pass |
| UI typecheck | cd ui && npm run typecheck | exit 0 |
| Core tests | cargo test -p teletype-core | exit 0 |
| YAML lint | actionlint .github/workflows/ci.yml (if available; repo has actionlint.yaml) | no errors |
| QA harness (fast) | bash scripts/qa.sh --fast | reaches RESULT: PASS — shippable with the new UI test stage visible |

## Scope

In scope:
- README.md (Testing block only)
- scripts/qa.sh (one new function + one new stage line)
- .github/workflows/ci.yml (checks job UI steps, ui job comment, test-core ignored step, one new checks step)
- crates/teletype-core/tests/journey_smoke.rs (stale Confirmed defects header only)
- scripts/check-ignored-tests.sh (new, small)

Out of scope (do NOT touch):
- docs/ or any other README section — plan 007 also edits README.md Status; keep to the Testing block here.
- The 263 failing ITN parity rows and the #[ignore] on itn_parity.rs:41 — this plan makes the exclusion visible, it does not remove it.
- Any other CI job, coverage gate, or test inventory logic.
- ui/package.json and package-lock.json.

## Git workflow

- Branch: advisor/005-honest-verification
- Commit message style: fix(ci): close false-green gates and stop running the UI suite twice
- Do NOT push or open a PR unless instructed.
- Note: plan 007 also edits README.md (Status section). Keep the two edits in separate commits or coordinate with whoever runs 007.

## Steps

### Step 1: Document the real test command

In README.md's Testing block (:214-219), add a third line after npm run typecheck:

    cd ui && npm test

Add one short sentence below the code block pointing to bash scripts/qa.sh --fast as the pre-merge single-command gate (scripts/qa.sh is currently documented only in its own header). Do not change any other README section.

Verify: sed -n '214,224p' README.md shows all three commands and the qa.sh pointer.

### Step 2: Add npm test to qa.sh

Add a ui_tests() function beside ui_typecheck() (:72-82), matching its double-run pattern (one piped/tailed run for output, one silent run for the exit code):

    ui_tests() {
      (cd ui && npm test 2>&1) | tail -20
      (cd ui && npm test) >/dev/null 2>&1
    }

Add stage "ui tests (vitest)" ui_tests immediately after the ui typecheck stage at :152. It must run in both full and --fast modes.

Verify: bash scripts/qa.sh --fast -> output includes stage ui tests (vitest) and a PASS/FAIL line for it; final RESULT line unchanged in format.

### Step 3: Run the UI gate in exactly one CI job

Keep the ui job as the single owner. Remove the setup-node, Install UI deps, UI typecheck and UI tests steps from the checks job (:96-106), and rename that job to "Checks (notices, manifest, static gates)" since it no longer touches UI. Update the stale comment above the ui job (:976-977) to say the UI gate lives there exclusively. Do NOT remove the ui job or its entry in gate.needs (:1015).

Verify: grep -n 'npm test|vitest|npx tsc|setup-node' .github/workflows/ci.yml -> setup-node/npm/tsc/vitest appear in the ui job and nowhere in checks; grep -n '^  ui:' and gate.needs still list ui. If actionlint is installed, run actionlint .github/workflows/ci.yml -> no errors.

### Step 4: Replace the no-op ignored-tests step with a fail-closed assertion

At ci.yml:238-240, delete the step "Confirmed defects still reproduce" and replace it with:

    - name: No ignored journey-smoke defects
      run: |
        if grep -Rq '#[ignore' crates/teletype-core/tests/journey_smoke.rs; then
          echo "::error::journey_smoke.rs contains ignored tests but CI no longer runs --ignored; fix the header or restore a gate"
          exit 1
        fi

In crates/teletype-core/tests/journey_smoke.rs, rewrite the stale # Confirmed defects block (:26-37) to state that all BUG-xx tests are fixed and active, that the suite runs in normal cargo test, and that a new #[ignore] requires a CI gate — delete the cargo test -- --ignored instruction.

Verify: cargo test -p teletype-core --test journey_smoke -> exit 0; sed -n '232,242p' .github/workflows/ci.yml shows the new step; grep -c '#[ignore' crates/teletype-core/tests/journey_smoke.rs -> 0.

### Step 5: Make the remaining ignore explicit and guarded

Create scripts/check-ignored-tests.sh (bash, set -uo pipefail, read-only, always exits 0 unless drift is found). It must:
- scan crates/teletype-core/tests/*.rs for the literal #[ignore;
- allow exactly one committed exception: crates/teletype-core/tests/itn_parity.rs (the 263-row phase-2b suite);
- fail with a clear message if any OTHER core integration test file gains an ignore, or if itn_parity.rs loses its ignore (that change requires the separate ITN plan and a new CI gate);
- print the allowed exception and its reason so the excluded suite is visible in CI logs.

Add a CI step in the checks job calling bash scripts/check-ignored-tests.sh, and add the same script as a qa.sh stage named "ignored-test contract" next to the static repo checks stage.

Verify: bash scripts/check-ignored-tests.sh -> exit 0 printing the itn_parity exception; temporarily appending an #[ignore] to a scratch copy under crates/teletype-core/tests/ and re-running causes a non-zero exit (remove the scratch file immediately after — do not leave it behind).

## Test plan

- New artifact: scripts/check-ignored-tests.sh with one positive run and one negative (scratch-file) run.
- Existing tests to re-run: cd ui && npm test; cargo test -p teletype-core; bash scripts/qa.sh --fast.
- No production code is changed, so no new Rust/TS unit tests are required.
- Verification: all commands in the table pass; the QA summary lists the new stage.

## Done criteria

- [ ] grep -n 'npm test' README.md finds the documented command.
- [ ] grep -n 'ui tests (vitest)' scripts/qa.sh finds the stage; bash scripts/qa.sh --fast shows it.
- [ ] grep -n 'npm test|vitest' .github/workflows/ci.yml finds the ui job only, not checks.
- [ ] grep -c '#[ignore' crates/teletype-core/tests/journey_smoke.rs -> 0.
- [ ] bash scripts/check-ignored-tests.sh exits 0 and names the single allowed exception.
- [ ] cargo test -p teletype-core exits 0.
- [ ] Only the five in-scope paths plus plans/README.md are modified.

## STOP conditions

Stop and report if:
- The gate aggregator (ci.yml:1013-1020) needs structural changes beyond keeping ui in needs.
- journey_smoke.rs actually contains ignored tests in the live tree (then the header is right and the CI step must be redesigned, not deleted).
- A CI YAML edit fails actionlint twice.
- Plan 007 has already edited README.md and the Testing block context no longer matches.

## Maintenance notes

- scripts/check-ignored-tests.sh is a contract, not a coverage gate: it prevents silent growth of ignored tests. Removing the itn_parity exception must be paired with a real gate for the 263 rows (separate M-effort work).
- If a future CI job is added that also runs Vitest, update the ui-job-owns-UI comment immediately — the duplicate arose from exactly that kind of drift.
- qa.sh --fast intentionally still runs all fast checks; the new UI test and ignored-test stages must never be skipped by --fast.
