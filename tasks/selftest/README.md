# Soak harness (500 takes)

Drives the real dictation path end to end, 500 times: a fresh TextEdit
document per take, the Control hotkey held by the real `holdctrl` binary,
speech from macOS `say` through the real microphone. Per take it records
counts and booleans only. Dictated text, screen text and field text are
never written anywhere.

## Commands

From the worktree or checkout root:

    # Check the environment, print the plan, press nothing.
    bash tasks/selftest/soak.sh --dry-run --takes 5

    # The full 500-take run (about 3 hours):
    bash tasks/selftest/soak.sh --takes 500 --seed 4242

    # A subset, e.g. 25 takes:
    bash tasks/selftest/soak.sh --takes 25 --seed 4242

    # Continue an interrupted run in the same out dir:
    bash tasks/selftest/soak.sh --takes 500 --seed 4242 --out <OUT DIR> --resume

    # One take, for a quick manual check:
    bash tasks/selftest/run_take.sh 6 "The quick brown fox jumps over the lazy dog."

`run_take.sh` prints what landed in TextEdit (a short fragment match, not
the full text) and the numeric fields of the latest take trace.

## Prerequisites

- The Teletype app is running. If its process name does not match
  `Teletype.app/Contents/MacOS/` or `teletype-desktop`, export
  `TELETYPE_APP_PID=<pid>` before running.
- TextEdit installed.
- `ocu` on PATH (Open Computer Use CLI).
- `say` on PATH (macOS built in).
- The `holdctrl` binary. Resolution order: `TELETYPE_HOLDCTRL`, then
  `tasks/selftest/holdctrl` next to the scripts, then
  `/Users/a.sorathiya/Documents/Ali/teletype/tasks/selftest/holdctrl`
  (the main checkout). Copy it into the worktree or export
  `TELETYPE_HOLDCTRL` if neither exists.
- Accessibility permission for the terminal running the scripts
  (System Events frontmost checks and the HID key stream).

Other environment variables: `TELETYPE_TRACES` (trace dir),
`TELETYPE_APP_SUPPORT` (recovery spool dir), `TELETYPE_PGREP_PATTERNS`
(colon separated pgrep patterns for the app process).

## The plan

`--seed 4242` is deterministic. Every 50th take is a 5 minute speech take
(`speech300`). The other shapes: `speech2` (short phrase), `tap`,
`silence`, `spam` (5 quick presses), `app_switch` (switch to Finder and
back mid take).

## Reading the results

The out dir defaults to `tasks/selftest/soak-<timestamp>/` (pass `--out`
to choose it; `--resume` needs the same dir).

- `takes.jsonl`: one JSON line per take. Fields: `index`, `shape`,
  `start`, `end`, `frontmost`, `trace_file`, `committed_segments`,
  `audio_ms`, `tail_audio_ms`, `t_paste_sent`, `recovery_before`,
  `recovery_after`, `recovery_delta`, `pid_before`, `pid_after`,
  `rss_before_kb`, `rss_after_kb`, `pasted`, `pass`, `reason`.
- `summary.md` and `summary.json`: written at the end of the run.
  `summary.json` has `overall_pass`, `first_fail_index`, per shape
  counts, and the RSS growth from take 10 to the last take.

### Pass criteria

A take passes when: the Teletype process stayed alive with the same pid;
the expected paste happened (exact fragment for short speech takes, new
committed segments for the 5 minute takes); the recovery spool did not
gain or lose files; no stuck pill overlay window is visible.

A run passes when: every take passed, and the RSS growth from take 10 to
the last take is under 100 MB.

### Fail reason tags

- `crash:no_app_process`, `crash:pid_changed`: the app died or restarted.
  Check the app logs. The run continues.
- `paste_missing`: the phrase did not appear in TextEdit within 75 s.
  Look at the take's `trace_file` for the stage timings.
- `transcript_empty`: the 5 minute take committed no new segments within
  90 s.
- `recovery_delta:N`: the recovery spool changed by N files during the
  take. A clean take must not touch the spool.
- `pill_window`: an overlay window whose name contains "pill" was left
  open. Needs a live check, see below.

## When a take fails

1. Find the line in `<OUT>/takes.jsonl` with the failing `index`; its
   `trace_file` points at the take trace (JSON lines, one row per
   stage). The trace rows contain stage timings and counts, not text.
2. The per take record also has `recovery_before`/`recovery_after`; a
   nonzero delta means inspect
   `~/Library/Application Support/com.teletype.app/recovery/` for a new
   spool file from that time window.
3. A stuck pill flag means open the Teletype app and look for a leftover
   overlay window, then report it with the take index.
4. To resume after an interruption (not a failure), rerun with the same
   `--out` dir and `--resume`. It continues from the last recorded
   take index. A crashed app does not abort the harness; the run marks
   the affected takes and keeps going.

## Safety

The harness checks the frontmost app before every key press. Anything
other than TextEdit aborts the whole run without pressing, and writes
`<OUT>/aborted.txt`. The dry run presses nothing.
