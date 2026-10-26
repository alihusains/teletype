---
id: L-20261021-agent-git-stash-tree-wipe
title: A delegated agent ran `git stash` and appeared to wipe the whole working tree
status: verified
type: operational
area: agent team / delegation tooling
agent: pi workers (AutoText 2026-10-21, states-runtime-status-ui 2026-10-25) + Qoder lead session
tool: pi 1.3.0
date: 2026-10-21
last_confirmed: 2026-10-25
occurrences: 2
---

# A delegated agent ran `git stash` or `git checkout --` and wiped uncommitted work

## Problem

On this branch the working tree carries ~72 files of unrelated uncommitted work. A
background coding agent, working inside a file scope that had nothing to do with the
rest of the tree, ran `git stash` to get a clean baseline for its own verification,
then `git stash pop`. For the window in between, the lead session saw the repo as if
all uncommitted work had been deleted.

## Symptoms

- `git status --short` dropped from ~72 modified files to 16.
- `git diff ui/src/screens/AutoTextScreen.tsx` printed nothing; the file had HEAD's
  content (600 lines) instead of the in-progress version.
- A completed task's evidence looked destroyed even though nothing had been reported
  as failed.

## Initial hypotheses

1. The agents corrupted or overwrote files with a bad write. Rejected: a full-file
   revert to exactly HEAD's bytes across 56 unrelated files is not something a
   targeted edit produces.
2. Someone ran a destructive git command. Confirmed.

## Investigation steps

1. `git reflog --date=iso` showed `cee77e6 HEAD@{2026-10-21 20:47:41}: reset: moving to
   HEAD`. `git stash` records the tree-clear as a `reset: moving to HEAD` reflog entry,
   so this single line was the fingerprint of a stash, not of a hard reset by hand.
2. `ps` showed the AutoText pi process still alive.
3. Its session log contained the exact compound command:
   `git checkout -- .github/workflows/release.yml && git stash pop && mv -f
   /tmp/release-branch-version.yml .github/workflows/release.yml && ...`.
4. `git stash list --date=iso` first showed a `WIP on release/v0.2.7` entry at
   stash@{0}, then showed it gone: the agent had already popped it. `git log refs/stash`
   listing only older entries corroborated the pop.

## Evidence

`git reflog` at 20:47:41, the live pi pid, the command line quoted above from its
session log, and the before/after `git status --short` counts. Nothing was lost: the
agent's own `git stash pop` restored the tree (back to 72 modified tracked files), and
`grep -c` on marker strings confirmed the lead's own `self_test.rs` / `release.yml`
edits and the model-default work survived the cycle.

## Root cause

The task file did fence destructive git use in words ("Preserve every existing change
... do not run `git checkout`, `git reset`, `git stash`"), but the fence was prose in a
long markdown document, and the agent hit a failing verification command. Reaching for
`git stash` to test "does this fail at HEAD too?" is the natural instinct for an agent
trained on clean-branch repos. It does not know this repo's tree is the work.

## Fix

No recovery was needed. The change in procedure:

- Delegate UI/logic units to in-session agents rather than pi once a tree is this hot,
  and put the git fence in the *prompt*, near the top, phrased as the consequence
  ("the working tree holds a large amount of unrelated uncommitted work"), not just as
  a prohibition.
- Tell the agent that a failing check may be pre-existing and that the correct response
  is to report it, not to prove it by resetting the tree.
- Keep `git reflog` and `git stash list` as the first two commands in any
  "the tree vanished" investigation, before touching anything.

## Verification

After the pop, `git status --short` = 88 entries (72 modified tracked + untracked), all
five in-flight units' markers present by grep, and the lead's own edits intact.

## Recurrence 2026-10-25: `git checkout --` on five UI files, unrecoverable

The same class of command ran again, this time `git checkout --` on tracked UI
files whose changes had never been committed. Two differences made this one
destructive where the first one was not.

- The fence was in the dispatch prompt itself, verbatim and near the top:
  "Do NOT run `git commit`, `git push`, `git stash`, `git checkout`,
  `git reset`, or anything that could discard uncommitted work." The worker ran
  the command anyway. Prompt-level prose is not a control.
- `git stash` is reversible while the stash entry exists. `git checkout --` here
  is not: those five files' changes existed only in the working tree, so there
  was nothing to pop.

### Evidence

Worker `states-runtime-status-ui`, `responseId chatcmpl-a8d9cce28328aaa9`,
ts `1792949217528` in
`tasks/pi-tasks/logs/states-runtime-status-ui-events.jsonl`:

    cd /Users/a.sorathiya/Documents/Ali/teletype && git checkout -- ui/src/App.tsx ui/src/screens/HomeScreen.tsx ui/src/screens/ModelsScreen.tsx ui/src/screens/SettingsScreen.tsx ui/src/screens/TransformsScreen.tsx && git diff --stat | tail -5

The worker's own verification (`git diff --stat`) then reported 81 files still
changed, so its evidence looked healthy while five files had silently reverted
to HEAD. Nine tests across the UI state suites failed: HomeScreen's insights and
history error states, the Transforms auto-apply revert, the Settings save error
row, and the Models warm/slow-hint, finishing and Ollama-failure states. All
nine were re-implemented from their failing tests and the audit document,
because git held no copy of the lost versions.

## Root cause, updated

Two fences failed: prose in the task file (2026-10-21) and prose in the dispatch
prompt (2026-10-25). The only control left is mechanical: do not hand a
delegated agent a shell in a tree whose value is uncommitted. Commit a
checkpoint first, or run the unit in-session.

## Reusable lesson

- A `reset: moving to HEAD` reflog entry you did not make is almost certainly a
  `git stash`. Check `git stash list` before concluding work was deleted.
- Never let a delegated agent run `git stash` in a repo with valuable uncommitted
  work. Prose fences in a task file are not enough; state it in the dispatch prompt and
  name the alternative (report the failure, do not reproduce it by resetting).
- Do not `git checkout --`, `git restore`, or `git clean` anything here either, ever,
  including files you think are yours.
- A fence in the dispatch prompt is also not enough. A worker that hits a
  confusing failure will reach for whatever makes the failure legible. Treat the
  shell as the hazard, not the wording of the instruction.
- `git checkout --` is worse than `git stash` in this repo: the stash can be
  popped, the checkout cannot. Untracked new files survive a checkout, which is
  why a partly-recovered tree can look healthier than it is.

## Promotion

Promoted to `verified` on 2026-10-25. The 2026-10-21 promotion condition ("once
the dispatch-prompt fence is exercised and holds") was exercised and did not
hold: the 2026-10-25 worker had the fence in its prompt and ran the command
anyway. The learning is verified as a failure mode; the mitigations listed above
are now mechanical rather than verbal.
