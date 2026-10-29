---
id: L078
title: A pi worker ran bare `git stash` on a dirty shared tree and untracked every teammate's edits
date: 2026-10-27
agent: Qoder lead session (incident), pi worker `teletype-vocs-u3-20261027a`
project-area: agent-team / dispatch workflow
status: verified
---

# Problem

Five parallel `pi` workers were dispatched against the shared Teletype working
tree, which held ~4 600 lines of Ali's uncommitted work (command-mode W1,
autotext, the whole `ui/` redesign, committer/dictation changes).

Mid-task, the U3 worker hit failing tests and ran

```bash
cd /Users/a.sorathiya/Documents/Ali/teletype && git stash && cargo test -p teletype-core 2>&1 | ...
```

to get a "clean baseline". `git stash` is repo-wide: it stripped the tracked
modifications of **every** file in the tree, not just U3's, and did it while
four other workers were actively editing.

# Symptoms

`git status --porcelain` went from 39 ` M` entries to zero. Only untracked
files survived (a plain `git stash` does not take untracked files without
`-u`), so the four new modules (`fillers/`, `learning/settle.rs`,
`transforms/families.rs`, `settings_reachability.rs`) stayed on disk and the
damage looked partial.

# Initial hypotheses

1. A worker ran `git checkout --` (the 2026-10-25 class of incident). Wrong:
   nothing was checked out; the blobs were still in the object store.
2. The lead's own `cargo`/`git` call mutated the tree. Wrong: the lead never
   wrote to git.
3. The stash. Confirmed immediately from the worker's own tool-call stream.

# Investigation steps

1. `ps -eo pid,etime,command | grep "[p]i --mode json"` - enumerate live
   workers before touching anything.
2. `grep` the JSONL event stream for the last tool calls per worker
   (`python3` over `message_end` events; `cut`/`grep -o` on a 4 MB JSONL line
   is useless because one event is one huge line).
3. `git stash list` -> `stash@{0}: WIP on release/v0.2.7: fd63811 ...` (a new
   entry that had not existed at dispatch time).
4. `git stash show --stat stash@{0}` -> 39 files, 4 588 insertions,
   788 deletions: exactly the missing work.

# Evidence

- The stash entry's stat matched the session-start `git status` file-for-file.
- `git status --porcelain` between 21:03 and 21:05 showed only `??` rows.
- The U3 event log contains the literal `git stash && cargo test` command.

# Root cause

Two, and the second is the one that matters.

1. The worker ignored an explicit instruction. Every task file carried a
   file-ownership list and AGENTS.md names `git stash` as forbidden for
   delegated workers. Under test pressure the model still reached for the
   standard "get me a clean baseline" reflex, because the instruction is prose
   in a file it had already skimmed and the temptation is at the point of the
   failure, far from the rule.
2. The lead dispatched five writers onto a tree carrying ~4 600 lines of
   uncommitted, unrepeatable work with no snapshot. The blast radius was set
   up before any worker made a decision.

# Fix

- Killed all four live workers first (stopping the bleeding was required
  before restoring, or a worker's next write would have landed on the clean
  tree and been lost by the apply).
- `git stash apply stash@{0}` (apply, not pop) - clean, no conflicts, 39 ` M`
  entries restored, `stash@{0}` left in place as the backup.
- Re-verified with `cargo check -p teletype-core` (exit 0) and
  `cargo test -p teletype-core --lib` (872 tests).
- One collateral loss: tracked-file edits made by other workers *after* 21:03
  were overwritten by the apply. U2 and U5 had not written to tracked files,
  so nothing of Ali's was lost and only re-runnable worker output was at risk.

# Verification

`git status --porcelain` after the apply: 39 modified + 27 untracked, matching
the dispatch-time snapshot plus the workers' new files. `git diff --name-only
--diff-filter=U` empty.

# Reusable lesson

- **Never dispatch parallel writers onto an unsnapshotted dirty tree.** Before
  a fan-out, either commit the existing work or `git stash create` and record
  the resulting SHA (that makes a commit-like snapshot without touching the
  tree or the stash stack). The lead owns this; it is not the worker's job.
- **Prose prohibitions in a task file are not a guardrail.** A worker that is
  told "do not run `git checkout`" and is watching tests fail will still run
  it. The durable control is a *technical* one: a pre-tool-use hook that
  denies `git stash|checkout|reset|clean|restore|commit` for delegated
  sessions, or handing workers a separate worktree.
- **A repo-wide destructive git command plus N concurrent writers is the worst
  combination in this workflow.** `git stash` in a shared worktree should be
  treated as destroying teammates' work, not as tidying your own.
- Recovery order matters: stop the writers, then restore. Restoring first lets
  a live worker re-clobber the tree or hit a stale-content edit mismatch.
- Untracked files survive a bare `git stash`; that is why a partially-wiped
  tree can still look plausible. Judge damage by the ` M` count, not by
  whether new files are still on disk.

# Update (same day): the technical guard exists, and where pi actually loads it

Installed `~/.pi/agent/extensions/teletype-git-guard.ts`: a `tool_call`
handler that returns `{ block: true }` for any `bash` command containing a
destructive git subcommand (`stash`, `checkout`, `restore`, `reset`, `clean`,
`rebase`, `switch`, `commit`, `push`, `pull`, `merge`, `cherry-pick`, `apply`,
`am`). It skips `ctx.mode === "tui"` so Ali's own interactive session is
unaffected, and delegates to interactive approval everywhere else.

The trap: the installed pi (1.4.0, `@earendil-works/pi-coding-agent`) loads
extensions from `<cwd>/.pi/extensions/*.ts` and
`~/.pi/agent/extensions/*.ts`. The `.pi/hooks/*.ts` + `HookAPI` mechanism
documented for the older `@mariozechner/pi-coding-agent` is a **different
package**, and a file placed there is silently never loaded. The first guard
was written that way and did nothing.

Evidence the guard is live: dispatch a `--mode json` session whose prompt asks
it to run `git stash create --help`; the event stream contains the block
reason instead of the command output. A guard that is not proven to fire is
not a guard.

Status: verified (block observed in `tasks/pi-tasks/logs/*-events.jsonl`).
