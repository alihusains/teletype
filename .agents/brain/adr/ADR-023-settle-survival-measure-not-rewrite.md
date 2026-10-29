# ADR-023 - Survival evidence is measured and shown; auto-applied text rewriting is not

- Status: accepted
- Date: 2026-10-27
- Deciders: Ali (dispatch), inline implementation (lead)
- Related: `.agents/brain/learnings/2026-10-26-accuracy-history-dead-in-prod.md`,
  `.agents/brain/learnings/2026-10-23-personalization-measured-or-not.md`,
  Vocs `src-tauri/src/session/learning.rs` (the mechanism ported here)

## Context

The learning loop could promote a correction after seeing it twice
(`promote::OBSERVATIONS_TO_PROMOTE`) but had no way to know whether the user
*kept* the correction after it landed. The brain already recorded that the
accuracy-feedback side was dead: `run_replay` had zero runtime callers, so the
per-term hit/false-alarm counters and the demotion rule were unreachable.

The ported mechanism judges by survival instead of by a click: a correction
counts as accepted only if the corrected form is still present in the field at
the anchored span once the user has had time to notice and undo it (30
minutes). Vocs goes one step further and *applies* a fix that survived three
times without an undo, as a deterministic text rewrite in the pipeline.

Shipping the whole mechanism in one pass meant adding a new stage that rewrites
user text based on accumulated pairs, at the same moment two other units were
changing the pipeline and the polish guard. A wrong learned pair would then
silently corrupt every future dictation, and the pair space recorded by the
sweep is coarser than the judge's: a settle pair is a whole changed span
(`"he go"` -> `"He goes"`), and on a take where polish ran it also includes
filler removal and AutoText expansion.

## Decision

Ship the measurement, not the rewrite.

1. `learning/settle.rs` carries the whole judgement: `SettleEdit`, `judge`,
   `should_auto_apply`, and `sweep()`, which settles rows past their deadline,
   tallies kept/undone on the `learn_candidate` row for the pair, recomputes
   `auto_apply`, and closes the row so it is judged exactly once.
   `Verdict::Unlocatable` increments neither tally.
2. The producer is `dictation.rs::record_dictation_row`, gated on polish having
   actually run (the same `TraceStageMarks` test the post-insertion card uses).
   A take that never entered polish records nothing: an AutoText expansion is
   the user's own shortcut, not a correction to grade.
3. The caller is lazy and already exists: the sweep rides
   `observe_learning_store` (the observation flush, triggered by the next
   dictation) through `edit_watch::StoreSink::settle_sweep`. No new timer.
4. The consumer is measurement: `insights::compute_learning` sums the tallies
   into `kept_corrections`, `undone_corrections`, and `auto_apply_fixes`, all
   returned by the existing insights command.
5. Nothing reads `auto_apply` to change text. The flag is maintained so the
   evidence is ready, and the UI line for the new Insights fields is a
   follow-up in Ali's in-progress Insights redesign.

## Consequences

- The app can now answer "does polish's rewriting survive contact with the
  user" with counts from the local store, which is what the two learnings above
  asked for.
- The auto-apply threshold is exercised by tests (three keeps and no undo sets
  it; one undo after three keeps clears it; a `manual` row is never flipped by
  arithmetic) but has no runtime effect, so a future rewriting stage lands on
  evidence that is already accumulating rather than starting from zero.
- The `reverted` tri-state (`settle_status = 2`, "the user chose the original")
  has no producer in the app yet, so `judge()`'s `reverted` argument is dead in
  production; the sweep only ever reads status 0 rows. An explicit revert
  affordance is what would light it up.
- The pair space recorded is spans, not words, and is contaminated by
  AutoText/filler changes on polish takes. Any future consumer that *acts* on
  these pairs needs a per-stage attribution of changes (the same closed-set
  accounting U4 builds for the polish guard) before it can be trusted.

## Evidence

`cargo test -p teletype-core --lib learning::settle` - 27 passed, including the
four self-cleaning guards from the task spec. Production wiring proven by grep:
`settle::sweep` <- `edit_watch.rs:317` <- `dictation.rs:5203`;
`set_settle_edits` at `dictation.rs:5111`; tallies read at
`commands/history.rs:214`.
