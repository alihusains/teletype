---
id: L022
title: A reset --soft chain on a shared branch silently drops a sibling's unpushed commit; recover from reflog
status: verified
type: debugging
area: git / multi-agent coordination
agent: OpenCode (lead) with Pi2
tool: git
date: 2026-09-25
---

# A reset --soft chain on a shared branch silently drops a sibling's unpushed commit

## Problem

While reorganizing commits, one teammate ran a chain of `git reset --soft` moves
on `main`. The chain succeeded, but a sibling's commit (`91c838c`, roadmap work)
was no longer reachable from `main`. The work was silently lost from the branch
history with no error and no warning.

## Symptoms

- `git log` after the reset chain no longer showed `91c838c`.
- The tree looked clean, tests passed, nothing appeared wrong.
- The lost commit was only noticed because the lead was tracking expected
  commits against the log.

## Initial hypotheses

1. The sibling never actually committed (task reported but not committed).
2. The commit was made on a worktree branch, not on main.
3. The reset chain rewound past it.

## Investigation

1. Compared the teammate's task report against `git log --oneline`.
2. Checked worktree branches: commit not there either.
3. Ran `git reflog` and found `91c838c` in the reset entries.
4. Confirmed content with `git show 91c838c` and diffed it against the current
   tree to see exactly which pieces were missing.

## Evidence

- `git reflog` entry showing the commit existed at the tip before the reset
  chain moved HEAD backwards past it.
- `git show 91c838c` displayed the full lost content.
- Note: `reset --soft` keeps changes staged, but it does not keep *commits*
  that HEAD rewinds past; they become unreachable except via reflog.

## Root cause

`git reset --soft` moves HEAD backwards. Any commit that was at the tip and is
not re-referenced becomes unreachable from the branch. In a shared working tree
where one agent resets while another's commit sits at the tip, that sibling's
commit is dropped from the branch. Unreferenced objects survive in the reflog
for roughly 30 days, which makes recovery possible.

## Fix

1. Recovered both lost pieces from reflog: `git show <sha>` for the dropped
   commit and the old file blobs for a partially overwritten doc edit.
2. Re-applied the content and committed it as `7a399b3` (roadmap worktree-audit
   verdict, phase-2b line, pipeline.rs test comments).
3. New team rule: no `git reset`, `git commit --amend`, or history rewrite on
   a shared branch without explicit lead approval.

## Verification

- `7a399b3` on main contains the restored roadmap content (verified by diff
  against `git show 91c838c`).
- Subsequent `git log` shows an unbroken chain with no missing reported
  commits.

## Reusable lesson

In a multi-agent shared working tree, treat `reset`, `rebase`, and `amend` as
destructive-to-others operations: they can silently drop commits teammates
believe are safe. Two rules: (1) never rewrite a shared branch without lead
approval, (2) if a reported commit is missing, check `git reflog` before
assuming it was never made, objects usually survive and are recoverable.

## Evidence / sources

- Commit: 7a399b3 (restored content), lost original 91c838c (reflog only)
- Commit: f826112 (L004, same session's documentation wave)
- User confirmation: lead verified the restoration by diff before committing

## Promotion

Verified: recovery was performed and the restored content diffed equal to the
lost commit's content.

---
