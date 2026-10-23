# ADR-019 - First-run model surfaces are shortlisted by the backend, not filtered by the UI

- Status: accepted
- Date: 2026-10-23
- Deciders: Ali (request: "do the shortlist first"), inline implementation
- Implements: the "Confusion-reduction shortlist for Mac" read in `.agents/brain/asrs.md`, and the follow-up left open by ADR-017

## Context

ADR-017 removed the two Windows (sherpa-onnx) rows from the Mac list, but the
Mac onboarding still offered the whole remaining catalog: three Core ML
variants, three ggml Parakeet quantizations, four Whisper quality tiers, the
unwired `fluid-speech-mini` row. Ali's original complaint was "Too many models
cause a lot of confusion", and the 2026-10-23 macOS scoreboard shows most of
those rows are strictly dominated duplicates:

| Row | Close-talk mean WER | p50/p95 ms | Verdict |
|---|---|---|---|
| `parakeet-ultra-coreml` | **3.68** | **48/114** | Mac winner; only engine inside the 120 ms budget |
| `parakeet-v3-coreml` | 4.00 | 45/113 | dominated by Ultra on every set, 26.4 s cold load |
| `parakeet-tdt-v3` (q8) | 3.83 | 602/1218 | the no-Neural-Engine / non-Core-ML fallback |
| `parakeet-tdt-v3-q4` | 5.25 | 129/321 | 356 MB but loses real accuracy on `cv` |
| `large-v3-turbo-q5` | 5.14 | 702/764 | the only row covering the ~74 extra languages |
| `fluid-speech-mini` | not measurable | - | eval harness has no ParakeetCpp seam |

Three of those six are quantization duplicates of a row already shown, one is
dominated, one cannot be measured. The set of *different decisions* a Mac user
can make is: the recommended engine, the fallback for a machine without the
Neural Engine, and language coverage.

## Decision

Shortlist in the backend and let the caller ask for it.

- `shortlist_ids(platform, language, fluid_available)` + `SHORTLIST_MAX = 3`
  (`crates/teletype-speech/src/catalog.rs:885,906`) return the ids a first-run
  surface may show: the recommendation always leads (seeded unconditionally, so
  onboarding can never render zero cards for an uncovered language), then at
  most two more rows, each gated by `offered_on` *and* `entry_supports(language)`.
  - macOS 14+ Apple Silicon: Ultra, ggml Parakeet v3, Whisper large-v3 turbo.
  - Older macOS / Intel: ggml Parakeet v3, Whisper large-v3 turbo.
  - Windows: sherpa v3 int8, sherpa v2 int8.
- `speech_model_list` takes a `shortlist: bool` (`crates/teletype-desktop/src/commands/models.rs:832`), and the `list_speech_models` command exposes it as `Option<bool>` defaulting to `false` (`:887`). With the flag on, the response is the shortlist plus, when it is not already one of those ids, the user's stored selection, so a prior pick is never un-reachable or silently dropped.
- Only `OnboardingScreen.tsx` asks for it (`:129`, `:182`, `:820`). Settings →
  Models keeps the full offered catalog.
- The dev browser preview mirrors the contract (`ui/src/dev/browserPreview.ts`)
  and gained a `?onboarding` param to force the first-run view.

## Alternatives considered

1. Slice the list in `OnboardingScreen.tsx`. Rejected for the same reason ADR-017
   rejected a UI-side OS filter: it duplicates platform knowledge in the front
   end and contradicts "the backend list is the single source of truth".
2. Permanently cut the catalog to three Mac rows (delete the duplicates).
   Rejected: the quantization rows are the escape hatch for a machine that
   cannot run Ultra, and Settings must still be able to reach them; the data
   also says the recommendation itself is not yet recommendation-grade
   (`human_case_count: 0`, asrs rule 1).
3. A ranked "top N by accuracy" rule computed at runtime. Rejected: the ratings
   live in a markdown register, not in the catalog, and a generic ranking
   predicate would silently reorder rows when a new model is added. An explicit
   per-platform id list is auditable and fails loudly.

## Consequences

- Onboarding answers "which of these three things do I want" (best on this Mac /
  works without the Neural Engine / speaks my language) instead of presenting
  six SKUs.
- The shortlist is a product opinion held in the catalog, so it needs the same
  care as `pick_engine`: adding a Mac model means deciding whether it belongs
  in `shortlist_ids`, and the `shortlist_is_the_measured_mac_triple` test fails
  when the list changes without that decision being made.
- Rows excluded from the shortlist are still selectable: the IPC only *narrows
  what is shown*, `select_speech_model` keeps using `offered_on`.
- `fluid-speech-mini` remains visible in Settings while being unmeasurable; the
  ParakeetCpp eval seam is still the prerequisite for trusting its accuracy
  claim (unchanged follow-up from ADR-017).

## Verification

- `cargo test -p teletype-speech --lib catalog` -> 34 passed, 0 failed (new
  `shortlist_leads_with_the_recommendation_and_stays_small`,
  `shortlist_is_the_measured_mac_triple`).
- `cargo test -p teletype-desktop --lib speech_model_list` -> 5 passed (new
  `speech_model_list_shortlists_for_first_run`: shortlist <= 3, exactly one
  recommendation and it leads, the stored pick survives, the full list stays
  materially larger).
- `cargo test -p teletype-desktop -p teletype-speech` and
  `-p teletype-desktop -p teletype-core` -> green; `cargo fmt --check` clean.
- `npx tsc --noEmit` exit 0; `npx vitest run` -> 172 tests / 20 files passed.
- Live read-back of `http://localhost:1420/?demo&onboarding` (Vite on 1420):
  the Dictation step shows the Ultra hero card and, expanded, exactly three rows
  (Parakeet Ultra, Parakeet v3 (TDT), Whisper large-v3 turbo); Fluid Speech Mini
  and the Whisper quality tiers are gone. Settings → Models on the same build
  still lists the full offered catalog.
