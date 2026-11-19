# Report: catalog-b1-finish

Status: DONE_WITH_CONCERNS

## What I implemented

Finished the Task 7 consumer ripple so `teletype-eval` and `teletype-desktop`
compile and test green against the already-migrated speech catalog.
Compiler-driven loop: `cargo check`, fix the exact reported lines, repeat.

### crates/teletype-eval
- `src/bakeoff.rs` (test module only):
  - `engine_kind(m)` / `asr_language(m)` / `model_path_for(dir, m)` /
    `model_present(dir, m)` now pass `&m` at ~14 sites (lines 851-887, 1339).
  - `.map(engine_kind)` on `Result<SpeechModel, _>` became
    `.map(|model| engine_kind(&model))` (line ~1317).
  - `m.engine()` on `BakeoffMetrics` became field access `m.engine` (line ~966).
- `src/driver.rs`: `model_path_for(&models_dir, &model)`,
  `engine_kind(&model)`, and `model.id` field reads became `model.id()`
  method calls (the wrapper exposes `id` as a method).

### crates/teletype-desktop
- `src/commands.rs`: `catalog::find(default_speech_model_id())` and
  `catalog::find(id)` now pass `&` (find takes `&str`, helpers return
  `String`).
- `src/commands/models.rs`:
  - `download_speech_model`: `entry` is now owned (`find` returns
    `SpeechModel`), so the `spawn_blocking` closure captures a clone
    (`entry_for_closure`) and the post-join `entry.install_path(...)` borrow
    still works. E0382 fixed.
  - `speech_model_ready`: `entry.is_some_and(...)` moved `entry` twice;
    switched to `entry.as_ref().is_some_and(...)` for both `ready` and
    `exists`.
  - `speech_runtime`: `entry.map(...)` moved `entry`; switched to
    `entry.as_ref().map(...)`.
  - `speech_model_list`: `m.id == recommended` became `m.id() == recommended`
    (method on the wrapper).
  - `ComponentRuntime::new("missing"/"loaded"/"ready", name, ...)` now passes
    `&name` (label is `&str`).
  - Test module: removed the parens that a previous worker wrongly added to
    the desktop DTO fields. `SpeechModelStatus`/`ModelStatus` have plain
    fields: `m.engine() -> m.engine`, `m.recommended() -> m.recommended`,
    `m.id() -> m.id` (~14 sites, lines 2790-3030, 3189).
  - Test module: added the missing parens on the wrapper:
    `sherpa.file -> sherpa.file()`, `find(id).unwrap().file -> .file()`
    (kept in a binding to avoid E0716), `speech_model_installed(ggml/sherpa/
    fluid, ...) -> (&ggml/&sherpa/&fluid, ...)`,
    `speech_runtime(id, ...) -> speech_runtime(&id, ...)`,
    `remove_speech_model_files(dir, entry) -> (&entry)`,
    `SpeechFileSpec { sha256: Some(sha) } -> Some(sha.to_string())`
    (field is `Option<String>`).
- `src/dictation.rs`:
  - `resolve_speech_model_for_id`: `entry.map(|m| m.engine())` became
    `entry.as_ref().map(|m| m.engine())` (E0382: the later
    `entry.as_ref()` borrow of the moved `Option<SpeechModel>`).
  - Tests: `find(...).unwrap().english_only` -> `english_only()` (6 sites,
    lines ~9226-9230, 10841-10845).
  - `let file = models_dir.join(entry.file)` -> `entry.file()` (line 14954).

## TDD evidence

This task is a compile-level consumer ripple against an already-green
migration, not new behavior: no new tests were added and no test logic was
changed, only call-site syntax (method vs field, owned vs reference).

RED (initial state, from the compiler):
```
$ cargo check -p teletype-eval -p teletype-desktop --all-targets
EXIT=101
error[E0308]: mismatched types (36)
error[E0599]: no method named `recommended`/`engine`/`id` (13)
error[E0615]: attempted to take value of method `english_only`/`file` (9)
error[E0382]: borrow of moved value: `entry` (2)
error[E0631]: type mismatch in function arguments (1)
error: could not compile `teletype-eval` (lib test) due to 17 previous errors
error: could not compile `teletype-desktop` (lib) due to 6 previous errors
error: could not compile `teletype-desktop` (lib test) due to 45 previous errors
```

GREEN:
```
$ cargo check -p teletype-eval --all-targets
EXIT=0
$ cargo check -p teletype-desktop --all-targets
EXIT=0
$ cargo test -p teletype-speech -p teletype-inference -p teletype-eval -p teletype-desktop
EXIT=0
```

## Files changed (git diff --stat, owned files only)

```
crates/teletype-desktop/src/commands.rs        |  42 +-
crates/teletype-desktop/src/commands/models.rs | 323 +++++++------
crates/teletype-desktop/src/dictation.rs       |  55 +-
crates/teletype-eval/src/bakeoff.rs            |  57 +-
crates/teletype-eval/src/driver.rs             |  22 +-
5 files changed, 256 insertions(+), 243 deletions(-)
```

All work is UNCOMMITTED, as required.

## Verification (real output, exit codes captured directly)

```
$ export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
$ cargo test -p teletype-speech -p teletype-inference -p teletype-eval -p teletype-desktop > /tmp/finish_test.out 2>&1; echo "EXIT=$?"
EXIT=0
```
Per-crate results (all `test result: ok`, 0 failed):
- teletype-desktop: lib 591 passed (3 ignored), edit_watch_index 4,
  file_transcribe 2, migration 19, settings_reachability 1.
- teletype-eval: lib 115 passed, eval_cli_instrument_validity 3,
  polish_adversarial 1.
- teletype-inference: lib 129 passed (2 ignored), benchmark 1, needle_e2e 1,
  voice_command_e2e 1.
- teletype-speech: lib 81 passed (1 ignored, regression check: unchanged),
  conformance 1, real_parakeet_cpp 1 ignored, real_transcribe_cpp_words
  1 ignored.
- Doc-tests: 0 across all four crates.

```
$ cargo clippy -p teletype-speech -p teletype-inference -p teletype-desktop -p teletype-eval --all-targets > /tmp/finish_clippy.out 2>&1; echo "EXIT=$?"
EXIT=101
```
Clippy output, the only lints:
1. `error: used expect() on an Option value` at
   `crates/teletype-inference/src/qwen3_asr.rs:88` (deny-level `expect_used`
   lint). This is PRE-EXISTING code from the Task 7 migration, in a file
   outside my Owns list. See Concerns.
2. `warning: constant EVIDENCE_BENCH is never used` in
   `crates/teletype-speech/src/catalog.rs` - pre-existing, read-only file,
   left untouched as instructed.
3. clang linker warnings from the Tauri build script (pre-existing,
   unrelated).
No clippy lints in any of my owned files.

## Self-review

- Ownership: I edited only the five owned files. `git status` shows the
  speech/inference changes as pre-existing uncommitted work from the Task 7
  migration (the "already done and green" baseline my task description
  refers to); I made no edits to `crates/teletype-speech/`,
  `crates/teletype-inference/src/catalog.rs`, or
  `crates/teletype-inference/src/qwen3_asr.rs`.
- No new tests, no debug tests, no assertion-free tests.
- No `dbg!`/`eprintln!`/`println!` introduced (checked the diff).
- No em/en dashes introduced (checked the diff).
- Behavior preserved: all fixes are call-site syntax (parens, `&`,
  `.as_ref()`, one clone for the moved-into-closure `entry`).

## Concerns / blockers

1. `cargo clippy` for the four crates cannot reach EXIT=0 in this worktree
   without touching an out-of-scope file: the Task 7 migration left
   `crates/teletype-inference/src/qwen3_asr.rs:88` with
   `find("qwen3-asr").expect("qwen3-asr in catalog")`, which trips the
   deny-level `expect_used` lint. The task allows touching qwen3_asr.rs
   only "if the compiler reports an error there", and this is a clippy
   error, not a rustc error. One-line mechanical fix:
   `match teletype_speech::catalog::find("qwen3-asr") { Some(m) => m.artifacts().to_vec(), None => return Err(...) }`
   or `unwrap_or_else(|| panic!(...))` is not an option under the lint; the
   clean fix is a `match`/`ok_or` returning a `SpeechError`. Flagging for
   the lead to either apply it or assign it (it is a Task 7 artifact, not a
   Task 8 one).
2. The `EVIDENCE_BENCH` dead-code warning in `crates/teletype-speech/
   src/catalog.rs:598` is pre-existing and in a read-only file; left as-is
   and noted here as instructed.

## Out of scope (not touched)

- `crates/teletype-speech/` (done, green, 81 tests pass).
- `crates/teletype-inference/src/catalog.rs` (Task 8 / catalog-b2).
- gate/update/UI/Ivy/routing (Tasks 9-15).

## Clippy follow-up 2 (catalog-b1-clippy2)

Two new clippy warnings introduced by the Task 7 consumer ripple, in owned
desktop files, removed:

1. `crates/teletype-desktop/src/commands.rs:1452` (needless_borrow):
   `teletype_speech::catalog::find(&previous) else` became
   `teletype_speech::catalog::find(previous) else` (`previous` is already
   `&str`).
2. `crates/teletype-desktop/src/commands/models.rs:1327`
   (useless_conversion): `label: p.label.clone().into(),` became
   `label: p.label.clone(),` (the target field is already `String`).

The two pre-existing `useless_conversion` warnings at `commands.rs:1317` and
`models.rs:527` were left untouched, as instructed.

### Verification (real output, exit codes captured directly)

```
$ export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
$ cargo clippy -p teletype-desktop --all-targets > /tmp/b1c2_clippy.out 2>&1; echo "EXIT=$?"
EXIT=0
```
Remaining `useless_conversion`/`needless_borrow` warnings after the fix
(grep of /tmp/b1c2_clippy.out): only the two pre-existing ones:
```
warning: useless conversion to the same type: `std::string::String`
   --> crates/teletype-desktop/src/commands/models.rs:527:42
warning: useless conversion to the same type: `std::string::String`
    --> crates/teletype-desktop/src/commands.rs:1317:36
```
The 1452 and 1327 warnings are gone.

```
$ cargo test -p teletype-desktop > /tmp/b1c2_test.out 2>&1; echo "EXIT=$?"
EXIT=0
```
Test results (all `test result: ok`, 0 failed):
- lib: 591 passed, 3 ignored
- edit_watch_index: 4 passed
- file_transcribe: 2 passed
- migration: 19 passed
- settings_reachability: 1 passed
- main.rs + doc-tests: 0

Note: the first `cargo test -p teletype-desktop` run failed at link time
with `Undefined symbols ... libggml-metal.a` (stale parakeet-cpp-sys
artifacts in the shared target dir, same failure mode seen during
catalog-b1-clippy). `cargo clean -p parakeet-sys -p parakeet-cpp-sys -p
transcribe-cpp-sys` and a re-run fixed it; no code change was involved.

### Self-review

- Only the two specified lines changed in this follow-up.
- No em/en dashes introduced (checked the diff).
- No behavior change: both fixes are lint-level (needless borrow, identity
  conversion).
- Work left uncommitted for the lead.
