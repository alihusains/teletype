---
id: L088
title: A real-app E2E matrix fails on harness assumptions long before it fails on the app
status: verified
type: debugging
area: e2e / delivery matrix / macOS injection
agent: opencode
tool: shell + teletype --e2e
date: 2026-10-29
---

# A real-app E2E matrix fails on harness assumptions long before it fails on the app

## Problem

The real 32-case delivery matrix (`scripts/e2e/delivery_matrix_macos.sh`)
reported failures across TextEdit, Notes, Safari, Chrome, Terminal, Slack and
Word even though the dictation pipeline itself was healthy. Several failures
were harness bugs, not product bugs.

## Symptoms

- Short fixtures produced "no result" because the tap ended before the
  controller had a take to finish.
- A case read a stale result from the previous case and passed or failed on
  the wrong text.
- `status` was probed once, before the controller had published its initial
  state, and the case aborted.
- Clipboard assertions disagreed with the app because the harness assumed
  the text was always cleared.
- TextEdit could not be read back through the AX text attribute.
- `keystroke delete` did not clear the field reliably.
- ASR capitalization differences made an otherwise correct transcript fail.

## Investigation

1. Re-ran each app case with `TELETYPE_E2E_LOG` enabled and compared the
   harness transcript with the app log.
2. Checked whether the failure was in capture, decode, injection or read-back
   by looking at the result id and the delivery mode in `status`.
3. Reproduced the short-fixture case with a 0.8 s clip and a 2.5 s clip.
4. Read TextEdit's document through AppleScript `text` instead of AX.
5. Compared the transcript case-insensitively.

## Evidence

- A 2.5 s padded fixture made the quick-tap case deterministic.
- Tracking `LAST_ID` and accepting only a newer result removed the stale-read
  passes.
- Retrying the initial `status` removed the startup race.
- `keepTextOnClipboard` in `status` explained the clipboard mismatch; the
  assertion now follows `keep_text_on_clipboard`.
- AppleScript `text` returned the document; the AX attribute did not.
- `key code 51` cleared the field where `keystroke delete` did not.

## Root cause

The harness encoded assumptions that were true for a fast fake backend but not
for the real app: take length, result ordering, status availability, clipboard
policy, and how each app exposes its text.

## Fix

`e49908d` changed the harness, not the app:

- pad fixtures shorter than 1.2 s to 2.5 s;
- track `LAST_ID` so each case only accepts a newer result;
- retry the initial `status` probe;
- expose `keepTextOnClipboard` in `status` and follow it;
- read TextEdit via AppleScript `text`;
- match transcripts case-insensitively;
- use `key code 51` instead of `keystroke delete`;
- capture `TELETYPE_E2E_LOG` for diagnosis.

Result: 28 passed, 0 failed, 4 skipped (VS Code not installed).

## Verification

Full matrix run on 2026-10-29: TextEdit 4/4 (`clipboard(pasted,verified)`,
`type`, `setvalue`, `hold`), Notes, Safari, Chrome, Terminal, Slack and Word
all green; dry runs of `delivery_matrix_macos.sh`, `command_mode.sh` and
`offline.sh` also green.

## Reusable lesson

When a real-app E2E case fails, first prove the harness observed the right
take and the right result before changing product code. Fixture length,
result-id ordering, startup status, clipboard policy, and per-app read-back
are the five assumptions that break most often.
