# L003: Status documents go stale; git log is ground truth for what is already done

**ID:** L003
**Date:** 2026-09-25
**Agent:** OpenCode (team lead) + omp (teammate)
**Project area:** project brain / findings.md / planning process
**Status:** candidate
**Evidence:** findings.md P0-10 and PROJECT_BRAIN.md "uncommitted P0 batch" entries contradicted by git history; omp's baseline verification (commits `3fa2793`, `0e8b94e..1658caf`, `9163631`, `56dc123`, `1c9f3d9`, 222 tests green)

## Problem

A 3-week build plan was created from `findings.md` and the project brain. Two of its
first tasks (commit the "uncommitted P0 batch", "fix the 12 verified P0 bugs") turned
out to be already done: the P0 batch had been committed on 2026-09-23 (`3fa2793`), the
12 P0 fixes as `0e8b94e..1658caf`, and the "uncommitted WIP" turned out to be a
different feature batch (S1 control line, scaled timeouts, pill styles).

## Symptoms

- Task briefs cited `findings.md` entries that no longer matched the working tree.
- A teammate assigned "commit the WIP" found the tree clean except for unrelated WIP.
- Risk of duplicate or conflicting work if two teammates act on the same stale item.

## Initial hypotheses

1. The fixes were never made (wrong; they were committed after findings.md was written).
2. findings.md tracks current state (wrong; it is a dated snapshot, 2026-09-23).

## Investigation

- `git log --oneline` and `git status` compared against the claims in findings.md
  and PROJECT_BRAIN.md before any task was dispatched further.

## Evidence

- findings.md warned a clean checkout does not compile (P0-10); omp verified a clean
  checkout builds and passes 222 tests.
- Brain entries D002, D004, Q001 still said "implementation in uncommitted P0 batch".

## Root cause

Status documents are point-in-time snapshots. Nobody re-verified them against git
history before turning them into work items.

## Fix

- Updated PROJECT_BRAIN.md (Current status, D002, D004, Q001) with commit hashes.
- Re-scoped board tasks: "fix 12 P0 bugs" gated on a verification pass, S1 control
  line task changed from build to verify/gap-fill.

## Verification

- Brain edits applied; board task descriptions updated; baseline re-verified by omp
  (build clean, `cargo test --workspace` 222/0, `tsc --noEmit` clean).

## Reusable lesson

Before turning any status document (findings.md, checkpoint.md, brain entries,
roadmap checklists) into tasks, re-verify each claim against `git log` and
`git status`. Treat dated documents as hypotheses about current state, not facts.
Prefer verification tasks ("confirm and gap-fill") over build tasks when the doc may
be stale.
