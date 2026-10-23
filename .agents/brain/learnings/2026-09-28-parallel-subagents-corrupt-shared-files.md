# Parallel sub-agents corrupting shared hot files

**ID:** L012
**Date:** 2026-09-28
**Agent:** teletype-productivity-monitor (lead session)
**Area:** agent orchestration / process
**Status:** candidate (observed 3× this session, not root-caused)

## Problem

During the 2026-09-28 bug-fix dispatch, parallel `teletype-backend-engineer`
sub-agents repeatedly corrupted the working tree:

1. BUG-009/010 agent: `ast_grep_replace` double-applied a pattern —
   `tray.rs` got duplicate `apply_visibility` blocks and stray `}` lines.
2. BUG-007 agent (1st attempt): `commands.rs` got the same 18-line block
   prepended to *different* doc comments in 4 places.
3. BUG-001 agent (1st attempt): `personalization/mod.rs` ended with three
   duplicate `pub mod learn;` lines and a dangling `pub mod judge;` with no
   `judge.rs`.
4. BUG-012 agent: `App.tsx` got the nav entry added twice with trailing `,,`.
5. Two separate agents (BUG-007, BUG-009/010) each burned 20–40M tokens and
   30–50 min to finish with **zero changes** — tool loops, not work.

## Evidence

- `git checkout` was required to restore `tray.rs` and `commands.rs` twice.
- The lead session had to re-apply the BUG-009/010 `save_settings` hooks,
  the BUG-007 kill-before-spawn block, the BUG-005 echo-guard threshold fix,
  and the BUG-012 nav de-duplication inline.
- All five incidents were in files shared by ≥2 concurrent agents
  (`commands.rs`, `tray.rs`, `personalization/mod.rs`, `App.tsx`).

## Hypotheses (untested)

- The sub-agent `edit`/`ast_grep_replace` tooling matches non-unique regions
  and applies to all matches when the agent's oldText isn't unique enough.
- Agents that hit a compile error they didn't cause (another agent's
  mid-edit state) retry the same edit against a drifting file, compounding.
- Long-running agents (20+ min) drift: they re-read stale file state from
  their own earlier context instead of the live file.

## Reusable lesson (candidate)

For a repo with a small number of hot shared files (`commands.rs` is the
canonical one — settings, model selection, and 40 other commands live in it):

1. **Sequence** agents that touch the same file; only parallelize
   file-disjoint work.
2. **Small well-specified changes go inline** in the lead session (the
   `edit` tool with verified-unique oldText is reliable); reserve
   sub-agents for large, file-disjoint units.
3. After any sub-agent finishes, **verify the actual diff**
   (`git diff --stat` + read the hunk) before trusting its report — a
   "done" report is a claim, and in 5 of 11 dispatches this session the
   claim didn't match the tree.

## How to apply

- Before dispatching: list the files each agent will touch; if two lists
  intersect, sequence them.
- Keep a `git stash`/`git diff` checkpoint before each dispatch so a
  corrupted tree is one `git checkout <file>` away.
