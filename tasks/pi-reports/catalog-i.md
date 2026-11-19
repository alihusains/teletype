# catalog-i report: route instrumentation (plan Task 15, R1 + R13)

Status: DONE

## What was implemented (per task)

- `crates/teletype-core/src/trace.rs`: added `route_confidence: Option<f32>` to
  `teletype_core::trace::DictationTrace` (the struct at :36), placed after
  `take_kind`, with the exact sibling pattern
  `#[serde(default, skip_serializing_if = "Option::is_none")]` and an R1 doc
  comment. No `route` field added (R13: `take_kind` already carries the route
  label). No derives changed.
- `crates/teletype-core/src/trace.rs` test module: added
  `dictation_trace_carries_route_confidence` (asserts both `take_kind` and
  `route_confidence`).
- `crates/teletype-desktop/src/dictation.rs`:
  - `PipelinePayload` gained `route_confidence: Option<f32>` (the worker's
    `PipelineResult` is consumed on the worker thread; the payload is the
    channel to the controller, so the confidence rides there next to the
    existing decider fields).
  - Both `PipelinePayload` construction sites (command take and dictation
    take in the pipeline worker) stamp `route_confidence: result.route_confidence`.
  - `apply_decider_marks` (the single funnel the controller uses to stamp
    decider fields onto the trace) now takes `route_confidence` and sets
    `t.route_confidence` unconditionally, so every take that ran a measuring
    decision provider records it; `None` otherwise.
  - All five call sites of `apply_decider_marks` (cancelled, test-inject,
    refused-delivery, finish refused, finish success) pass
    `payload.route_confidence` through.
  - Test helper `test_payload` updated with `route_confidence: None`.
- `crates/teletype-desktop/src/command_mode.rs`: NOT modified. It never stamps
  the trace (grep: no `PipelineResult`, no `DictationTrace` field writes; its
  only reference is a doc comment about `DictationTrace::decider`). Per the
  task file, left alone.

Gate logic, confirm-on-external dry-run behavior, and `take_kind` decision
logic are untouched. This task only records the confidence.

## TDD evidence

RED: `cargo test -p teletype-core dictation_trace_carries` (target dir
`/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a`)

```
EXIT=101
error[E0609]: no field `route_confidence` on type `trace::DictationTrace`
    --> crates/teletype-core/src/trace.rs:1238:11
     |
1238 |         t.route_confidence = Some(0.92);
     |           ^^^^^^^^^^^^^^^^ unknown field
error: could not compile `teletype-core` (lib test) due to 2 previous errors
```

GREEN: `cargo test -p teletype-core -p teletype-desktop`

```
EXIT=0
test trace::tests::dictation_trace_carries_route_confidence ... ok
test result: ok. 19 passed; 0 failed; ... (settings)
test result: ok. 1 passed; 0 failed; ... (settings_reachability)
Doc-tests teletype_core / teletype_desktop_lib: 0 failed
```
20 "test result: ok" lines, 0 failures across both crates.

## Files changed

```
crates/teletype-core/src/trace.rs        | 16 ++++++++++++++++
crates/teletype-desktop/src/dictation.rs | 22 ++++++++++++++++++++++
2 files changed, 38 insertions(+)
```

Work left UNCOMMITTED for the lead (no git mutations run by me).

## Verification (real output, exit codes captured directly)

```
export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
cargo test -p teletype-core -p teletype-desktop > /tmp/i_test.out 2>&1; echo "EXIT=$?"
TEST_EXIT=0
cargo clippy -p teletype-core -p teletype-desktop --all-targets > /tmp/i_clippy.out 2>&1; echo "EXIT=$?"
CLIPPY_EXIT=0
```

Clippy tail:

```
warning: `teletype-desktop` (lib) generated 2 warnings
warning: `teletype-desktop` (lib test) generated 2 warnings (2 duplicates)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.54s
```

The 2 remaining desktop warnings are pre-existing at HEAD and in files I do
not own: `useless_conversion` in `commands/models.rs:539` and
`commands.rs:1318` (verified against `git show HEAD:` baseline; both present
before my change). Zero new warnings; the one my initial test draft
triggered (`field_reassign_with_default`) was fixed by rewriting the test to
struct-literal form.

## Self-review

- Ownership: only the two owned files were written. `command_mode.rs`
  confirmed not to stamp the trace; left untouched.
- No `route` field; `take_kind` reused as the route label (R13).
- No derive changes (struct already derives `Default`; `Option<f32>` is
  fine).
- No debug tests, no `dbg!`/`eprintln!`/`println!`, every test asserts.
- No em/en dashes in added code or comments (checked the diff).
- `route_confidence` is `None` for takes that never ran a measuring provider
  (payload defaults to `None`; only stamped from `PipelineResult` where the
  pipeline set it).

## Concerns / blockers

None.
