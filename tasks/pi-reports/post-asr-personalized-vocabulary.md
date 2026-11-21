# Post-ASR personalized vocabulary

Status: implemented in the main working tree, uncommitted.

## What changed
- Native ASR boost remains OFF (`boost_capacity == 0`) per D029/ADR-018.
- Learned lexicon terms now affect the **final text after decode** through the existing dictionary/pack correction tiers:
  - text-only `correct_with_dictionary` path in `Pipeline`;
  - confidence-aware `rescore_with_confidence` path when the engine supplies word timings.
- Filters: `source == learned`, non-tombstoned, at most 64 terms, at most 2 aliases each, app-scoped terms only in their own app category.
- `dictionary_enabled` gates the confidence-aware path, respecting the existing dial.
- Added a unit test proving source/scope/tombstone filtering and canonical/alias mapping.

## Why this path
D029's measured bakeoff found no native-boost win: Core ML WER 6.88% → 10.61%, entity accuracy 84.5% → 75.4%, p50 2.6 s → 19.7 s; Whisper lost entity accuracy; ggml Parakeet was a no-op. The user chose post-ASR-only rather than a new bakeoff or a default-off toggle.

## Verification
Core 1390, desktop 628, eval self-test PASS, tsc clean, vitest 58 files / 601 tests, fmt clean, brain freshness PASS. Brain updated with D048.
