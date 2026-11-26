# Plan 006: Make the brain freshness checker detect a stale HEAD

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- scripts/check-brain-freshness.sh .agents/brain/PROJECT_BRAIN.md
> If either file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit 8746c1c, 2026-11-24

## Why this matters

AGENTS.md says freshness is mandatory and requires the brain-freshness marker to be updated when HEAD changes. The marker in PROJECT_BRAIN.md:3 says head=c81bf44 while the checkout is at 8746c1c (six commits ahead). scripts/check-brain-freshness.sh already extracts mk_head at :32 and the live head at :25, but never compares them — so it prints "brain is fresh" and exits 0 despite the drift. The mandated gate gives a false PASS. This plan makes the check honest (WARN only, preserving the script's always-exit-0 diagnostic contract) and refreshes the marker.

## Current state

- .agents/brain/PROJECT_BRAIN.md:3 — marker:
      <!-- brain-freshness: version=v0.2.22 head=c81bf44 updated=2026-11-24 -->
  Actual git rev-parse --short HEAD is 8746c1c.
- scripts/check-brain-freshness.sh — read-only diagnostic, always exits 0, set -uo pipefail. Helpers at :20-21: pass() and warn_msg() (the latter increments warn).
  - :25 — head=$(git -C "$root" rev-parse --short HEAD ...)
  - :28-38 — marker parsed into mk_version, mk_head, mk_updated. The version check is at :34-38 (PASS if marker version == latest tag, else WARN).
  - :39-44 — topology-year check.
  - :46-50 — duplicate decision ID check; :52-55 — untracked brain file check.
  - :57-61 — final line: if warn -eq 0 print "brain is fresh" else print N warnings. Exit 0 either way.
- AGENTS.md:66-67 — Freshness is mandatory; update the marker and the status/topology sections when the release line, HEAD, or verified facts change.
- Observed output of bash scripts/check-brain-freshness.sh at baseline: PASS version, PASS topology, PASS decisions, PASS untracked, then "brain is fresh" — with no HEAD comparison at all.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Freshness check | bash scripts/check-brain-freshness.sh | exits 0; includes PASS: marker head matches checkout HEAD |
| HEAD | git rev-parse --short HEAD | prints the short SHA named in the marker |
| Shell syntax | bash -n scripts/check-brain-freshness.sh | exit 0 |

## Scope

In scope:
- scripts/check-brain-freshness.sh
- .agents/brain/PROJECT_BRAIN.md (the single marker line only)

Out of scope (do NOT touch):
- The body of PROJECT_BRAIN.md — no prose, topology, or decision edits.
- AGENTS.md — its instruction is correct; the script is what failed.
- Any other script or CI file.
- No automatic marker rewrite inside the checker — it must remain a read-only diagnostic.

## Git workflow

- Branch: advisor/006-brain-freshness-head
- Commit message style: fix(brain): warn when the freshness marker head differs from HEAD
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add the HEAD comparison

In scripts/check-brain-freshness.sh, immediately after the marker version check (:34-38) and before the topology-year check, add:

    if [[ -n "$mk_head" && -n "$head" && "$mk_head" == "$head" ]]; then
      pass 'marker head matches checkout HEAD'
    else
      warn_msg "marker head '$mk_head' != checkout HEAD '$head'"
    fi

Match the existing quoting, indentation and helper functions exactly. Do not change the final exit status (still exit 0 always).

Verify: bash -n scripts/check-brain-freshness.sh -> exit 0; bash scripts/check-brain-freshness.sh -> exits 0 and prints WARN: marker head 'c81bf44' != checkout HEAD '8746c1c' (before the marker is refreshed in step 2).

### Step 2: Refresh the marker

Edit only .agents/brain/PROJECT_BRAIN.md:3 to:

    <!-- brain-freshness: version=v0.2.22 head=8746c1c updated=2026-11-24 -->

Do not edit any other brain line.

Verify: bash scripts/check-brain-freshness.sh -> exits 0, prints PASS: marker head matches checkout HEAD, and the final line is "brain is fresh".

## Test plan

- No unit test framework applies to this shell diagnostic. Verification is the two runs in steps 1 and 2, which together prove both the WARN branch and the PASS branch execute.
- Optional negative check after step 2: with a scratch copy of the brain file outside the repo, point the script at it (if the script supports an override) and confirm WARN; if there is no override, rely on the step-1 output as the negative case and do not modify the real brain file.
- Verification: bash -n and both diagnostic runs.

## Done criteria

- [ ] grep -n 'marker head' scripts/check-brain-freshness.sh shows both the PASS and WARN branches.
- [ ] bash scripts/check-brain-freshness.sh exits 0 and prints PASS: marker head matches checkout HEAD.
- [ ] The only PROJECT_BRAIN.md change is line 3 (git diff -- .agents/brain/PROJECT_BRAIN.md shows one line).
- [ ] The script still exits 0 even when the marker is stale (verified in step 1).
- [ ] Only the two in-scope files plus plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live script at :25-38 does not match these excerpts.
- The script already has a HEAD comparison (then only refresh the marker and report that step 1 was a no-op).
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- Because this plan's own commit advances HEAD, a freshness run immediately after committing will legitimately WARN again (the marker names the pre-commit SHA). That is correct behavior — update the marker in the next brain-maintenance commit, or accept the one-commit lag explicitly. Do not weaken the check to avoid this.
- The checker is advisory (always exit 0) by design; do not turn the WARN into a non-zero exit without an explicit owner decision.
- Every future commit that changes verified project facts should update the marker, now with a real signal that catches omissions.
