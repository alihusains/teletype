# Changelog

All notable changes to Teletype are recorded here. Detailed, draft-stage notes
for a release live in `docs/release-notes-v<version>.md` when present.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This file records user-visible changes; it does not claim feature parity with
any other dictation product.

## [0.2.22]

### Added

- A floating learned-word pill with Undo, a hover-pausable countdown and
  added/updated provenance, plus a configurable 12-second undo window shared
  with the tray.
- All keyboard bindings, including transform shortcuts, now live in one
  Shortcuts section inside Settings, with conflict detection and deep links
  from Dictionary, Command Mode and AI Polish.

### Fixed

- Polish capitalizes sentences and repairs unambiguous subject-verb agreement
  on every model path, including card-locked cleanup models.
- The dictionary Forget control now also clears remembered refusals, and the
  Learn-from tab shows the blocked-pair count.

## [0.2.21]

### Fixed

- Deterministic backtrack now retracts a quantity phrase as one fact, so
  "twenty dollars, correction, thirty dollars" becomes "thirty dollars"
  instead of "twenty thirty dollars".
- A replacement's duplicated leading function word is dropped, avoiding
  "on on Tuesday" after a spoken correction.

## [0.2.20]

### Added

- Manifest-driven model mirror at `alihusains/teletype-models@models-v1` for
  S1-mini, SpeakoFlow Mini and Qwen3.5 0.8B/2B, with hash-pinned upstream
  backups and a workflow that syncs on manifest merge.
- Five text profiles (Clean, Natural, Structured, Transform, Off), per-app
  defaults and a profile info command; resolved profiles and fallbacks are now
  visible in the UI.
- Hotkey status row in Settings, with stale-latch reset, watchdogs and liveness
  re-registration.
- Visible word learning with undo, on-screen name confirmation and
  "never change this" protection; learned terms feed post-ASR correction.
- "View original" and "Rewrite stronger" (Transform preview, copy-only) on the
  polish result card; the "Why" line names the real model, profile and fallback
  reason.

### Changed

- Default models: Parakeet Ultra Core ML + Qwen3.5 2B on Apple Silicon/macOS;
  Parakeet Ultra sherpa INT8 on Windows/Other. The CPU fallback stays on the
  cleanup models.
- Polish runs by default, including for older settings files; a take where it
  changed nothing now reads "0 changes · already clean" instead of looking like
  it never ran.
- Trust layer: guard removal and case fixes, a deterministic backtrack resolver,
  protected spans and expanded adversarial behaviour cases.
- Performance: single-pass insights, memoized diff, calibration packet reuse
  and pre-split backtrack cues.

### Verification

- Local suite: core 1390, desktop 631, inference 133 passed (2 ignored), speech
  81 passed (1 ignored), TypeScript clean, vitest 610, clippy green, eval
  self-test PASS.

### Known limitations

- Native ASR vocabulary boosting remains off (D029); Core ML polish is not
  adopted; Qwen3-ASR stays upstream because it exceeds 2 GiB; live Mac
  acceptance is still recommended for hardware-dependent paths.
