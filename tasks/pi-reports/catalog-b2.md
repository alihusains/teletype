# catalog-b2: inference LLM/decider catalog re-point (plan Task 8)

Status: DONE_WITH_CONCERNS

## What was implemented (plan Task 8)

1. `crates/teletype-inference/src/catalog.rs` rewritten, compiler-driven:
   - `const CATALOG` deleted (grep exit 1).
   - `CatalogEntry` is now a wrapper over the parsed `LlmEntry` (same pattern
     as b1's `SpeechModel` over `SpeechEntry`), built from
     `Catalog::bundled().llm()`. `catalog()` returns owned entries; `find(id)`
     returns an owned `Option<CatalogEntry>`.
   - The `polish_capable` bool is gone from the row; `CatalogEntry::polish_capable()`
     is now `capabilities.contains(&Capability::Polish)` (the data carries
     `["polish"]` per R1).
   - `polish_shortlist_ids` keeps its literal id lists and re-checks each id
     against the parsed entry's capability + `runs_on_host`. Returns
     `Vec<String>` (owned; no `&'static str` to borrow from).
   - `ModelVerdict` stays in this crate (the Ollama path in models.rs builds
     it from canonical names); it gains `as_data`/`from_data` mapping to the
     `llm.json` strings, unknown strings fall back to `NotTested`.
   - `Specialty` (tag + measured) is gone: `llm.json` stores the role key
     (`cleanup`, `all_round`, `instruct`, `fast_small`); the desktop maps key
     to the UI label + measured flag (`specialty_status` in models.rs).
   - `CatalogShard` now wraps the catalog `Artifact` (owned strings); shard
     `url` is `String::new()` because `Artifact` has no per-shard URL field.
     All 9 LLM rows are single-file, so nothing reads shard URLs; the
     download path uses the row `url` for single-file and the shard list for
     multi-file layout.
   - `host_gpu_accelerated()`, `POLISH_SHORTLIST_MAX`, `has_plausible_size`,
     `is_downloaded`, `entrypoint`, `runs_on_host` kept with the same
     semantics.
2. `crates/teletype-inference/Cargo.toml`: added `teletype-catalog` path dep.
3. `crates/teletype-inference/src/lib.rs`: dropped `pub use catalog::CATALOG`.
4. `crates/teletype-inference/src/download.rs` + `src/bin/download.rs`:
   field reads became accessor calls, `find` now owned, shard field reads
   borrowed. (These are inside the crate I own; not listed in the task Owns
   but required by the compiler.)
5. `crates/teletype-desktop/src/commands/models.rs`:
   - `model_list` builds from `catalog()` (owned) instead of `CATALOG`;
     sort by `sort_order()`; selected-entry re-append uses the owned entry.
   - `specialty_status(tag)` maps the data role key to the UI label and
     measured flag (cleanup/all_round are the measured bakeoff roles).
   - `select_model` / `delete_model` / `download_model` / `ensure_local_provider`
     re-pointed to accessor methods; `download_entry_with_cancel(&entry, ...)`.
   - `ModelStatus.polish_capable` field KEPT: it is the IPC/TS contract
     (`polishCapable`, pinned by `model_status_ships_the_polish_role_under_
     the_key_the_ui_reads`), now computed from the capability check.
6. `crates/teletype-desktop/src/commands.rs` (NOT in my Owns list, required
   by the compiler, flagged per _common.md):
   - `command_decider_rows` reads the decider row from
     `teletype_catalog::Catalog::bundled().command_by_id("neudecide-q4")`
     (R3: the decider is a `CommandEntry` in command.json, not an LLM row).
   - `local_llm_installed` uses `catalog()` + `polish_capable()`.
   - `migrate_removed_models`: `neudecide-q4` added to `REMOVED_DECIDERS`
     (it is no longer an LLM row, so a saved pick of it is refused as a stale
     pick, same as `needle3`; the existing test
     `migrate_removed_models_moves_a_decider_out_of_the_polish_slot` covers it).
   - Test `polish_model_label...` re-pointed to owned `catalog()`.
7. `crates/teletype-desktop/src/commands/personalization.rs` (one line,
   required by the compiler, also outside Owns): `entry.name()` accessor.
8. `crates/teletype-desktop/Cargo.toml`: added `teletype-catalog` path dep
   (required for the `command_by_id` call in commands.rs).

## TDD evidence

RED (before implementation, after adding the plan's smoke test to the old
file): the old file had no `Catalog::bundled` backing; the smoke test
`llm_find_is_backed_by_the_parsed_catalog` was added with the rewrite and
confirmed failing against the pre-rewrite state is not separately captured
because the rewrite replaced `const CATALOG` atomically. The honest RED
evidence is the first `cargo check -p teletype-inference --all-targets`
after the rewrite, which enumerated the exact ripple (exit 101, 27 errors):

```
$ cargo check -p teletype-inference --all-targets
error[E0615]: attempted to take value of method `id` on type `&CatalogEntry`
   --> crates/teletype-inference/src/download.rs:306:27
... (27 errors)
EXIT=101
```

GREEN:
```
$ cargo test -p teletype-inference -p teletype-desktop > /tmp/b2_test.out 2>&1; echo "EXIT=$?"
EXIT=0
test result: ok. 130 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1.26s   (teletype-inference lib)
test result: ok. 591 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 5.03s   (teletype-desktop lib)
```

## Test counts (honest)

- catalog.rs tests: 18 before -> 19 after. The plan's Task 8 smoke test
  `llm_find_is_backed_by_the_parsed_catalog` was added (step 1 of the plan).
  No test was deleted. `neudecide_q4_pins_all_six_files` was re-pointed at
  the parsed `CommandEntry` (it previously read the const's shards; the
  decider row now lives in command.json).
- teletype-inference lib: 130 passed, 2 ignored (same ignores as before:
  network-bound).
- teletype-desktop lib: 591 passed, 3 ignored.
- teletype-catalog: 9 passed (precondition, unchanged).

## Files changed (git diff --stat, work left UNCOMMITTED)

```
Cargo.lock                                         |   2 +
 crates/teletype-desktop/Cargo.toml                 |   1 +
 crates/teletype-desktop/src/commands.rs            |  40 +-
 crates/teletype-desktop/src/commands/models.rs     |  93 +-
 crates/teletype-desktop/src/commands/personalization.rs |   2 +-
 crates/teletype-inference/Cargo.toml               |   1 +
 crates/teletype-inference/src/bin/download.rs      |  16 +-
 crates/teletype-inference/src/catalog.rs           | 940 ++++++-----------
 crates/teletype-inference/src/download.rs          |  66 +-
 crates/teletype-inference/src/lib.rs               |   1 -
 10 files changed, 475 insertions(+), 687 deletions(-)
```

## Verification (real output, exit codes captured directly)

```
$ export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
$ cargo test -p teletype-inference -p teletype-desktop > /tmp/b2_test.out 2>&1; echo "EXIT=$?"
EXIT=0
$ grep 'test result' /tmp/b2_test.out
test result: ok. 591 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 5.03s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.18s
test result: ok. 130 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1.26s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.13s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.95s
test result: ok. 0 passed; 0 failed; ... (doc/integration binaries)

$ cargo clippy -p teletype-inference -p teletype-desktop --all-targets > /tmp/b2_clippy.out 2>&1; echo "EXIT=$?"
EXIT=0
$ tail -3 /tmp/b2_clippy.out
warning: `teletype-desktop` (lib) generated 2 warnings (run `cargo clippy --fix --lib -p teletype-desktop` to apply 2 suggestions)
warning: `teletype-desktop` (lib test) generated 2 warnings (2 duplicates)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.34s

$ grep -n 'const CATALOG' crates/teletype-inference/src/catalog.rs; echo "GREP_EXIT=$?"
GREP_EXIT=1   # clean

$ cargo test -p teletype-catalog > /tmp/b2_cat.out 2>&1; echo "CATALOG_EXIT=$?"
CATALOG_EXIT=0   # 9 passed
```

The 2 clippy warnings are pre-existing (present in f16ed80, the b1 commit,
at the same two `default_speech_model_id().into()` sites; b1's commit
message records them as pre-existing). teletype-inference clippy is fully
clean.

## Self-review

- Ownership: I edited `commands.rs`, `commands/personalization.rs`,
  `teletype-desktop/Cargo.toml`, `download.rs`, `bin/download.rs`, and
  `inference/lib.rs` beyond the task's Owns list. All were strictly
  compiler-enforced ripples of the `const CATALOG` deletion (field reads
  became method calls; the decider moved to command.json; the new
  `command_by_id` call needs the dep). No behavior change beyond the
  re-point, except the two deliberate test/behavior adjustments below.
- No debug tests, no `dbg!`/`eprintln!`/`println!` added, no scratch
  modules.
- No em/en dashes in any diff line (checked with a unicode scan of
  `git diff`; pre-existing dashes in untouched lines are left alone).
- English only.
- Work left uncommitted; no git mutations attempted.

## Concerns / deliberate behavior adjustments (need lead ruling)

1. `ModelStatus.polish_capable` (IPC field `polishCapable`) is KEPT as a
   bool on the desktop DTO, computed from
   `capabilities.contains(&Capability::Polish)`. The task says to report
   rather than silently keep a Rust bool if a UI/IPC field legitimately
   exposes it. The UI splits the Models rail on `polishCapable` (pinned by
   `model_status_ships_the_polish_role_under_the_key_the_ui_reads`), so
   removing it is a UI contract change (Task 10 territory). I kept it.
2. `migrate_removed_models`: `neudecide-q4` is no longer an LLM row, so a
   saved `selected_llm_model == "neudecide-q4"` no longer hits the
   `!entry.polish_capable()` branch. I added it to `REMOVED_DECIDERS`
   (alongside `needle3`) so the existing test
   `migrate_removed_models_moves_a_decider_out_of_the_polish_slot` passes
   and a stale pick still migrates to speakoflow-mini with the notice.
3. Test `model_list_shortlists_polish_rows_for_the_hosts_engine_build`
   asserted the full (non-shortlist) LLM list still shows the decider
   ("Settings still has to show the decider it uses for Command Mode").
   The decider is now a command row, not an LLM row, so the polish list
   never shows it. I flipped that assertion to negative. The decider is
   still visible in the Command Mode decider rows (command.json, R3).
4. `neudecide-q4` `size_mb` is 43 in command.json (the b1-committed data),
   while the old const and an old desktop test fixture used 41. I kept the
   data (43) and corrected the re-pointed test to assert 43 with a comment
   (decimal-MB display of 42,915,902 bytes = 42.9 MB). The desktop fixture
   at models.rs:3685 still says 41 but is a synthetic DTO literal, not
   data-backed; left as-is.
5. `CatalogShard.url` is `String::new()` for parsed entries (the
   `Artifact` schema has no per-shard URL; `CommandEntry`/`LlmEntry` have a
   single row `url` plus `base_url`). No current row uses multi-shard LLM
   downloads, so nothing reads shard URLs. If a future row needs
   per-shard URLs, the schema needs that field.
6. Pre-existing target-dir breakage (not mine): a stale
   `parakeet-cpp-sys` build dir in the shared catalog-a target had a
   truncated parakeet.cpp checkout (missing ggml/src); I removed that one
   build dir and the rebuild succeeded. No source change.

## Out of scope (untouched)

Speech catalog (b1), qwen3_asr.rs, gate/update/UI/Ivy/routing (Tasks 9-15).
