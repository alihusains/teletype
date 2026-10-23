# ADR-018 - Vocabulary boosting ships off by default on every engine

- Status: accepted
- Date: 2026-10-23
- Deciders: Ali (decision), inline implementation
- Supersedes: the "no boost default changes" clause of L047; the §7.5 per-engine
  list sizes in `teletype-core/src/learning/boost.rs` as *shipped* defaults

## Context

Teletype biases the ASR decode toward terms the user is likely to say. At
take-start, `dictation.rs` captures the focused-app context, scores the learning
lexicon plus on-screen terms, and hands the engine a per-take boost list
(`build_boost_terms` -> `take_boost_terms` -> `set_boost_terms`, in both the
streaming committer and the whole-take `transcribe`). The list size came from
`boost_capacity`, which mapped Fluid 100 / sherpa 50 / Whisper 30 / Whistle 50
and 0 for the ggml engines.

The 2026-10-17..22 bake-off measured this axis (engine x set x boost) on the one
term-heavy set, `earnings22-vocab`, after `extract_vocab_terms` was fixed at
`981900e` (L047). Verbatim, from `.agents/brain/asrs.md`:

| Engine | Boost path | off -> on | Entity accuracy | ASR p50 |
|---|---|---|---|---|
| `parakeet-ultra-coreml` | native `ttf_set_vocab` | 6.88% -> 10.61% | 84.5% -> 75.4% | 2.6 s -> 19.7 s |
| `parakeet-tdt-v3` (ggml) | `rescore_text` | 12.18% -> 12.18% | unchanged | unchanged |
| `large-v3-turbo-q5` (Whisper) | `initial_prompt` | 69.62% -> 68.94% | 28.8% -> 19.8% | 31.2 s -> 34.6 s |
| sherpa-onnx | post-ASR rescorer | no cell (ONNX aborts on 21-minute calls) | | |

So on the only set where the terms do anything, the boost either has no effect
(ggml Parakeet, which returns no word timings so the rescorer is inert), is
measurably harmful (Core ML: worse WER, worse entity placement, ~7x slower
decode inside the Neural Engine; Whisper: 0.7 pp of WER for 9 pp of entity
accuracy), or has never been measured (sherpa). No measured win exists.

## Decision

Keep the boost list empty for every engine, in one place.

- `boost_capacity(kind)` (`crates/teletype-desktop/src/dictation.rs`) returns 0
  for all engines. That is the single gate: `build_boost_terms` short-circuits
  on `n == 0`, so `take_boost_terms` stays empty and both `set_boost_terms`
  call sites are skipped.
- The infrastructure stays: the `N_*` sizes, the lexicon scoring, the
  take-scoped cache, the screen-context capture, and the engine-side setters are
  all left in place so a booster that is *shown to help* can be switched back on
  by editing that one function. The `N_*` doc now says they are sizes to
  restore, not defaults.
- Learning keeps working through the text paths. `commands::record_dictation_edit`
  writes learned terms into the D006 dictionary, and the pipeline's
  `correct_with_dictionary` plus `rescore_with_confidence` apply them after the
  decode. What turns off is biasing inside the ASR decode, not the user's
  vocabulary.
- The bake-off keeps its `--boost on|off` axis. It is a measurement instrument,
  not a product setting; `RunConfig.boost` already defaults to `false`.

## Alternatives considered

1. Delete the boost machinery entirely. Rejected: the lexicon, the replay
   before/after WER in Insights, and the `N_*` plan sizes are consumed by other
   code, and the honest read is "not yet worth it", not "never".
2. Add a Settings toggle. Rejected: a default-off switch for a feature with zero
   measured wins is a dead control, which this repo's dead-controls audit says to
   fix or remove rather than ship.
3. Disable only Fluid and Whisper, leaving sherpa and Whistle listed as boosted.
   Rejected: sherpa's `set_boost_terms` is already a documented no-op stub
   (`crates/teletype-speech/src/sherpa.rs:253`) and Whistle has never been
   measured, so the "capability" map was describing boosting that does not happen
   while hiding the two engines where it does happen and hurts.

## Consequences

- Personalized vocabulary has no ASR-level effect today; it is a
  post-transcription correction. Any copy that promises the model will hear your
  contacts or terms better is ahead of the measurement
  (`DictationPolishSections.tsx` "learns them as vocabulary hints").
- Dictation latency is lower and word placement is not degraded by a term list.
- The `boost` column in future bake-off snapshots is a research axis for fixing
  the booster, and `asrs.md` rule "leave boost off until measured" now matches
  the product.

## Verification

- `cargo test -p teletype-desktop --lib boost` -> 3 passed (new
  `boost_capacity_is_zero_for_every_engine` replaces
  `boost_capacity_matches_the_plan_sizes`).
- `cargo test -p teletype-desktop -p teletype-core` -> 776 + 322 lib tests plus
  all integration suites passed, 0 failed, so no take pipeline, learning, or
  replay test depended on a populated boost list.

## Re-open when

A booster is worth switching back on only when a `earnings22-vocab` (or, better,
a human-corpus term-heavy) cell shows WER or entity accuracy *improving* with
boost on, at an acceptable decode cost, with the term list produced by the
fixed extractor.
