# catalog-h report: routing confidence signal (plan Task 14, R1 + R12)

## What was implemented

Followed R12 exactly (not the plan's stale noul/desktop pointers). The existing
TinyDecide `choice` question is the single source of the confidence; no new
question, no second inference, no new threshold, no new crate.

1. `crates/teletype-core/src/llm.rs`: added ADDITIVE trait method
   `classify_intent_with_confidence(&self, text) -> (Intent, Option<f32>)`
   with a default impl `(self.classify_intent(text), None)`, so every existing
   impl (MockClassifier, MockDecision, Needle, desktop mocks) keeps compiling
   unchanged.
2. `crates/teletype-inference/src/tinydecide.rs`:
   - `classify_raw` now returns `(Intent, Option<f32>)`; the f32 is
     `answer.probs.get(pick).map(|p| *p as f32)` from the EXISTING choice.
     Every failure path that previously returned `Intent::Dictation` now
     returns `(Intent::Dictation, None)`.
   - New private `classify_with_confidence` holds the three-layer logic
     (raw model, heuristic post-filter, keyword boost) and threads the model's
     raw prob through unchanged (one-line comment states the prob stays the
     model's raw confidence in its own pick, not of the final intent).
   - `classify` delegates to `classify_with_confidence(text).0`, so
     `classify_intent` returns byte-identical intents as before.
   - `DecisionProvider` impl overrides `classify_intent_with_confidence`.
   - Model runs exactly once per call either way.
3. `crates/teletype-inference/src/combined.rs`: `CombinedDecisionProvider`
   overrides `classify_intent_with_confidence` to delegate to
   `self.classifier`; `classify_intent` untouched.
4. `crates/teletype-core/src/pipeline.rs`:
   - `PipelineResult` gains `pub route_confidence: Option<f32>` next to
     `intent` with a doc comment.
   - Route call site now:
     ```rust
     let (intent, route_confidence) = if self.force_command {
         tracing::info!("intent_routing_forced_command");
         (crate::llm::Intent::Command, None)
     } else {
         dp.classify_intent_with_confidence(&input_text)
     };
     ```
     The tuple becomes 4-wide (`(intent, route_confidence, embedding, command)`);
     `None` arms and the non-voice arm all initialize `route_confidence = None`.
   - The single `PipelineResult { ... }` construction site (pipeline.rs:797
     region) sets `route_confidence`. Grep confirmed this is the only
     construction site in the file; no other file constructs `PipelineResult`
     (all other hits are return types/reads).

## TDD evidence

RED (test added first, method not yet defined):
```
cargo test -p teletype-inference classify_intent_exposes
EXIT=101
error[E0599]: no method named `classify_intent_with_confidence` found for struct
`tinydecide::TinyDecideProvider` in the current scope
   --> crates/teletype-inference/src/tinydecide.rs:471:40
error: could not compile `teletype-inference` (lib test) due to 1 previous error
```

GREEN (after implementation):
```
cargo test -p teletype-inference -p teletype-core
EXIT=0
test tinydecide::tests::classify_command_examples ... ok
test tinydecide::tests::classify_intent_exposes_a_confidence_in_unit_range ... ok
... all 20 "test result: ok" lines, 0 failed everywhere
```
Test count: teletype-core lib 1104 passed (same suite as before; the new test
lives in teletype-inference). teletype-inference lib gains exactly 1 new test
(`classify_intent_exposes_a_confidence_in_unit_range`).

## Files changed (uncommitted, worktree `feat/data-driven-catalog`)
```
crates/teletype-core/src/llm.rs             |  6 ++++
crates/teletype-core/src/pipeline.rs        | 19 +++++++----
crates/teletype-inference/src/combined.rs   |  4 +++
crates/teletype-inference/src/tinydecide.rs | 50 ++++++++++++++++++++++-------
4 files changed, 60 insertions(+), 19 deletions(-)
```

## Verification (real output, exit codes captured directly)

```
export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
cargo test -p teletype-inference -p teletype-core > /tmp/h_test.out 2>&1; echo "EXIT=$?"
EXIT=0
tail:
    Doc-tests teletype_inference
    running 0 tests
    test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

cargo clippy -p teletype-inference -p teletype-core --all-targets > /tmp/h_clippy.out 2>&1; echo "EXIT=$?"
EXIT=0
tail:
    Checking teletype-core v0.2.7 (...)
    Checking teletype-inference v0.2.7 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.02s
```
Clippy warnings present in the log are pre-existing build-script noise
(parakeet-cpp-sys archive lines) and a workspace-level profile warning, not
from the changed crates.

## Model presence (honesty note)

The TinyDecide model was ABSENT: `models/tinydecide/` contains only
`meta.json` (187362 bytes), no `model.bin`. Per the repo convention,
`model_dir()` returns `None` and the new test early-returns. Therefore
`classify_intent_exposes_a_confidence_in_unit_range` was
SKIPPED-BY-GUARD: the test ran (reported ok) but the confidence assertion
did NOT execute. The change is verified by compilation, clippy, and the
existing non-model tests. Same applies to the pre-existing
`classify_command_examples` test (also skipped-by-guard in this worktree).
Do not read this as the unit-range assertion having passed.

## Self-review

- Ownership: only the 4 owned files touched. No other file modified.
- No debug tests, no stray dbg!/println!/eprintln! in new code.
- No em/en dashes in any added line (checked `git diff -U0 | grep '^+'`
  against U+2013/U+2014: none found).
- Every new test has a real assertion (`assert!((0.0..=1.0).contains(&conf))`).
- Additive only: `classify_intent` path unchanged in behavior (same three-layer
  logic, same intents); default trait impl keeps all other providers
  compiling; `route_confidence` is `None` for force_command, no provider,
  typed input, and non-measuring providers.
- No second inference, no new question, no new threshold (R12).

## Concerns / blockers

None. No out-of-scope changes needed. Work left UNCOMMITTED for the lead.
