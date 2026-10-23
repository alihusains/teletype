---
id: L-20261021-agent-wrote-after-failure-signal
title: A delegated agent kept writing after its run reported failure
status: candidate
type: operational
area: agent team / delegation tooling
agent: general-purpose subagents (style-measurable, ai-polish-restructure) + Qoder lead session
tool: Qoder CLI
date: 2026-10-21
---

# A delegated agent kept writing after its run reported failure

## Problem

Two background agents were dispatched to own disjoint files. Both hit an upstream
stream idle timeout and surfaced a "failed" notification. The lead treated the
notification as a stop signal and began re-implementing one agent's Rust work
inline. The large Edit failed with `0 occurrences found`, which is what exposed
the truth: the "failed" agent had kept writing for six more minutes.

## Symptoms

- Failure notification arrives at roughly 21:01.
- `stat` shows the agent's files written at 21:03:37, 21:04:53 and 21:06:21.
- The peer agent likewise wrote at 21:05:20 and 21:08:31 after its own notification.
- The lead's in-flight re-implementation collided with work already on disk.
- A concurrent partial edit by the lead made `npx tsc --noEmit` fail with
  "declared but never used" for symbols the agent was mid-way through wiring up.

## Initial hypotheses

1. The agent crashed and left nothing. Rejected: mtimes postdate the notification.
2. The notification was for a different agent. Rejected: the task ids matched.
3. A stream timeout ends the *reporting* channel, not the *process*. Accepted:
   the agent continued, finished the work, and its real result arrived later in a
   completion notification.

## Evidence

- `find ui/src crates -newermt "21:02"` listing files the "failed" agent owned.
- `grep -n "build_layers\|preview_style_profile"` finding complete implementations.
- The delayed completion report describing edits already visible on disk.
- `crates/teletype-desktop/src/lib.rs` briefly holding `style_resolution` and
  `preview_style_profile` registered twice (agent and lead each added a line),
  which the agent then de-duplicated.

## Root cause

A failure notification reports the state of the event stream, not the lifetime of
the worker. "Reported failed" and "stopped writing" are different facts, and
treating the first as the second causes the lead to duplicate in-progress work.

## Fix

No product code changed. The procedure changed:

1. Before re-implementing anything an agent "failed" at, check file mtimes
   (`find <dirs> -newermt "<dispatch time>"`) and grep for the agent's distinctive
   symbols.
2. Attribute a diff to an agent by mtime, never by a whole-tree `git diff --stat`.
   The tree already carried unrelated branch work, and reading it that way produced
   a false scope-creep accusation against the other agent.
3. Do not hand-edit a file an agent owns while it may still be running.

## Verification

The lead stopped, re-read both files, and confirmed the agent's implementation was
complete rather than replacing it. `cargo test -p teletype-desktop --lib --
commands::personalization` passed 23 tests, `npx tsc --noEmit` exited 0, and
`lib.rs` holds exactly one registration line per new command.

## Reusable lesson

An agent's failure notification is a claim about its output channel. Before
reproducing that agent's work, prove the work is absent with mtimes and symbol
greps; the cheapest check is `find <owned paths> -newermt <dispatch time>`.

Related: [[L-20261021-pi-endpoint-timeout]] (exit 0 with zero work, the mirror
image of this failure), [[L012]] (parallel agents corrupting shared files).
