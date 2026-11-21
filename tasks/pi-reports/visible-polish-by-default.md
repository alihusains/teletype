# Visible polish by default

Status: implemented in the main working tree, uncommitted.

## What changed
- `polish_skip_when_clean` now defaults to `false`, including legacy settings that omit the key. Polish runs on every take unless another gate legitimately skips it. Existing explicit opt-ins still work.
- Added an Advanced Polish toggle: "Skip polish when text already looks clean", with honest copy.
- `polish-diff` now carries `ran`, `model`, and `original`. `original` is the raw ASR transcript after filler-word removal.
- When polish runs but changes nothing, the card says "Polish ran · 0 changes · already clean". If it was skipped, it says "Polish skipped · reason".
- Added "View original" showing the filler-cleaned original; falls back honestly to "Raw transcript" if unavailable.
- Added "Rewrite stronger": calls `rewrite_with_transform` with the Transform profile and returns text for display/copy only. It never injects into the focused app and never overwrites the delivered text.
- The Why dialog shows the real model id.

## Verification
Core 1390, desktop 630, eval self-test PASS, tsc clean, vitest 58 files / 610 tests, fmt clean, brain freshness PASS. Brain updated with D050.
