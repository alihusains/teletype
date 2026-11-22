---
id: L096
title: A frozen main thread is invisible to stack sampling; a cooperative probe plus a log marker is what makes it observable (and gateable)
status: verified
type: debugging
area: "teletype-desktop/main_thread_watchdog.rs, scripts/check-main-thread-responsive.sh, scripts/qa.sh, .github/workflows/release.yml"
agent: Buffy
tool: Freebuff
date: 2026-11-22
---

# A frozen main thread is invisible to stack sampling

## Problem

v0.2.21 shipped a main-thread hang (see L095): after a first launch and a trip
to the background, the app stopped responding, and the only trace was a hang
report a user produced by Force Quitting. No QA check could catch it, and the
product itself never said "I am stuck".

## Symptoms

- `spindump` says `Event: hang` and the main thread is parked in
  `__ulock_wait` (an unbounded `pthread_join`).
- A *healthy idle* main thread is also parked, in `__mach_msg2_trap` inside the
  event loop. Nothing in a stack sample separates "waiting for the next event,
  as designed" from "will never return".

## Initial hypotheses

1. A watchdog that samples the main thread's stack and reports where it is
   stuck. Rejected: hypothesis 0 above — an idle event loop and a deadlock
   look the same, so the sampler cannot produce a verdict, only a clue.
2. Drive the app with Apple Events (`System Events` keystrokes/clicks) and
   assert on window state. Rejected: needs Automation/Accessibility TCC
   consent, which cannot be granted in CI and is fragile on a dev box.
3. Poke the app through the existing `--e2e` socket. Rejected: the socket
   server runs on its own thread (it answers precisely when the main thread
   cannot) and `--e2e` deliberately skips hotkey registration, so it cannot
   exercise the path that froze.

## Investigation

The workable signal is *cooperative*: ask the main thread to do something and
watch whether it does it. Tauri already exposes the primitive —
`AppHandle::run_on_main_thread` — and the main thread drains that queue between
event-loop iterations.

Two details made the difference:

- **Cadence vs. grace.** Probing every 5 s with a 5 s grace turns a busy but
  healthy main thread into a false alarm only if it is blocked for a full
  grace period, which is already a hang by any user-facing definition.
- **Reporting must not need the log lock.** The unresponsive line cannot go
  through `crate::log_entry`: that takes `LOG_BUFFER`'s mutex, which the wedged
  main thread may be holding. Writing to stderr directly is what makes the
  watchdog useful in exactly the case it exists for.

## Evidence

Real app, main thread blocked on purpose for 8 s (temporary patch, since
reverted), gate cadence 300 ms / 4 s grace:

```
Info main-thread watchdog: first probe answered in 0 ms
Info main-thread watchdog: 12 probes answered, main thread responsive
TEMP-GATE-SELFTEST: blocking the main thread for 8 s
teletype-watchdog: main thread unresponsive for 4005 ms (1 probe(s) unserved)
RESULT: FAIL — the main thread stopped answering probes after the
background/foreground cycle
```

The same build without the block passes three real background/foreground
cycles (`open -a Finder` / `open -a <bundle>`):

```
  cycle 3/3: app alive (heartbeats before: 6)
verdict: no unresponsiveness, watchdog still answering (heartbeats 9 -> 14)
RESULT: PASS
```

## Root cause (of the missing signal, not of the hang)

The app had no main-thread liveness contract at all: nothing asked the main
thread to prove it was running, so a hang could only surface through a user's
hang report, minutes later, with no timeline.

## Fix

- `main_thread_watchdog.rs`: a thread that queues a no-op closure every
  `TELETYPE_MAIN_THREAD_PROBE_MS` (default 5 s), waits
  `TELETYPE_MAIN_THREAD_TIMEOUT_MS` (default 5 s) on a condvar latch, reports
  the first miss of an episode to stderr as
  `teletype-watchdog: main thread unresponsive for <ms> ms (...)`, then once a
  minute while it lasts, and logs a recovery summary (with the Developer-tab
  entry) once the probe is served again. Optional heartbeat lines
  (`TELETYPE_MAIN_THREAD_HEARTBEAT_PROBES`, default off) give a gate positive
  proof that probes are still being answered.
- `scripts/check-main-thread-responsive.sh`: launches the built bundle with a
  temporary `HOME`, waits for the first answered probe, cycles
  background/foreground with plain `open -a` (no TCC), then fails on an
  unresponsive line, an app death, or a stalled heartbeat. `--dry` prints the
  plan; it skips when it cannot observe and `--strict` turns those skips into
  failures.
- Wired as `scripts/qa.sh` stage 7 and as a release.yml step right after the
  bundle build.

## Verification

- 4 unit tests on the module: report cadence, env-seam clamping/garbage,
  served-vs-timed-out latch (own latch per test — the first version shared the
  process-global latch and parallel tests satisfied each other's probes).
- Synthetic bundles (health / hang / death / stall): exit 0 / 1 / 1 / 1, with
  the expected messages.
- Real app bundle: PASS across 3 cycles; FAIL with the unresponsive line when
  the main thread was blocked for 8 s.
- `cargo clippy -p teletype-desktop --all-targets` clean, `cargo fmt --check`
  clean.

## Reusable lesson

**To gate a hang, add a probe with a caller-visible deadline; do not sample the
stack.** A stack sample describes where a thread is, not whether it will ever
move, and an idle event loop is indistinguishable from a deadlock. A
cooperative probe that must be *answered* converts the question into an
observation anyone (or any script) can read from a log line.

Three corollaries found the hard way:

1. Report a hang on a path that cannot itself be blocked by the hang (here:
   stderr, not the mutex-protected log buffer).
2. A gate that can only sometimes observe must say which case it is in; a
   silent skip is what ships broken releases. Give the caller `--strict`.
3. macOS single-instance (`tauri-plugin-single-instance`) makes a second launch
   exit instantly, so any script that launches the app must check for a running
   copy first or it will report "the app died". The plugin keys on the
   *compiled-in* identifier, not on the built bundle's `Info.plist`.
