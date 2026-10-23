---
id: L019
title: A stray installed /Applications/Teletype.app runs in parallel with the dev build and spawns a duplicate llama-server
status: verified
type: debugging
area: dev environment / process hygiene
agent: Codex
tool: ps
date: 2026-10-06
---

# Stray installed Teletype.app duplicates the dev build (and its llama-server)

## Problem

Goal invariant: "always use a fresh build; there should not be duplicate copies
running in parallel." During the 2026-10-06 production-readiness pass, a second
Teletype instance was found running alongside the dev build, each with its own
EG-1 `llama-server`.

## Symptoms

- `ps -eo pid,command | grep teletype` shows **two** app processes:
  `./target/debug/teletype` (the dev build) **and**
  `/Applications/Teletype.app/Contents/MacOS/teletype` (an installed copy).
- Two `llama-server` children, on different random ports, both loading EG-1.
- The stray's PID changes between kills (49929 -> 55167), so it looks like it
  "respawns" — it is a separate manual launch, not a watchdog.

## Root cause

An older **installed** copy of the app (`/Applications/Teletype.app`) was
launched by hand at some point and kept running. It is fully independent of the
dev build: different binary, its own child `llama-server`. Nothing in the repo
auto-launches it (no launchd agent, no login item) — it is simply left open.

## Fix

- Kill the installed instance **and** its llama-server child:
  `kill <app-pid>`; `kill -9 <llama-server-pid>`.
- Re-check after a few seconds; only `./target/debug/teletype` + vite on 1420
  should remain.
- (Optional, user decision) remove `/Applications/Teletype.app` if it is not
  wanted, so it cannot be launched by accident again. **Done 2026-10-06:** the
  installed copy was moved to Trash after it relaunched 3× in one session
  (PIDs 49929, 55167, 62818, 63087), each with its own llama-server. The dev
  build is now the only Teletype on the machine.

## Verification

After the kill, `ps | grep -E "teletype|llama-server"` shows only the dev build
and no llama-server (the dev build loads Parakeet in-process; EG-1's
llama-server is spawned on demand). Re-checked after 6s — stayed clean.

## Reusable lesson

Before any live-app check in this repo, run
`ps -eo pid,command | grep -E "teletype|llama-server" | grep -v grep` and kill
anything that is not `./target/debug/teletype` / vite. A duplicate llama-server
from a stray installed copy wastes RAM and can confuse "which model is loaded"
diagnostics. The dev build's Parakeet is in-process; a separate llama-server
process is the tell-tale sign of a second app instance.
