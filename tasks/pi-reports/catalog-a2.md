# catalog-a2 report: finish Phase A (plan Tasks 2-6 tests + data migration)

Status: DONE (one deviation from the task file, noted below)

## What was implemented

### A. Scaffold tests (plan Tasks 2, 3, 4, Step 1 code)
- Created `crates/teletype-catalog/tests/engine_registry.rs` (3 tests), `tests/validate.rs` (9 tests), `tests/loader.rs` (2 tests) with the exact plan code.
- `tests/loader.rs` was updated after the data migration (as the task file directs): `bundled_catalog_loads_with_empty_domains` became `bundled_catalog_loads_with_populated_speech_rows`, asserting `speech_by_id("parakeet-tdt-v3").is_some()` and `speech().len() == 22`. `load_from_missing_dir_is_an_error` kept as-is.
- All pass against the existing scaffold code. No scaffold bugs found; no `src/*.rs` in teletype-catalog needed changing.
- One lint fix in `tests/engine_registry.rs` beyond the plan code: `name_roundtrips` had an unused `e` variable (clippy warning), so the loop now also asserts `Engine::from_name(name).unwrap() == e`. All plan assertions preserved.

### B. Platform re-export (Task 2, speech side)
- `crates/teletype-speech/Cargo.toml`: added `teletype-catalog = { path = "../teletype-catalog" }`.
- `crates/teletype-speech/src/catalog.rs`: replaced `pub enum Platform { ... }` and its `impl Platform { is_macos }` block with `pub use teletype_catalog::engine::Platform;`. `host_platform()`, `parse_macos_major()`, and the speech `Engine` enum are untouched.

### C. Speech data (Task 5)
- Created `tests/speech_data.rs` with the exact plan code. Confirmed RED before populating (left: 0, right: 22).
- Populated `speech.json` with all 22 source rows, field for field from the source `const CATALOG`:
  - engine string per the variant map (parakeet, parakeet-cpp, parakeet-coreml, sherpa, whisper, llama-server, apple-speech, transcribe-cpp).
  - `capabilities: ["asr"]` on every row.
  - qwen3-asr: two artifacts from `QWEN3_ASR_ARTIFACTS` (2_165_034_944 + 355_709_344 = 2_520_744_288 bytes), size_mb 2521 (R7, re-derived).
  - granite-speech-470m-q4: artifact bytes 279_114_080 from `PINNED_SPEECH_ARTIFACTS`, size_mb 280 (R7, re-derived).
  - Core ML rows and Apple-speech rows: `artifacts: []`.
  - runs_on: `["macos"]` for macOS-only engines, `["macos","windows","linux"]` for the two sherpa rows.
  - purposes with label/basis/evidence, including the two measured chips with `evals/runs/bench-2026-10-28/SUMMARY.md` evidence (parakeet-ultra-coreml, large-v3-turbo-q5).
  - ivy NOT added (R5).
  - Note: single-file rows whose source had no pinned bytes (parakeet ggml, whisper ggml, sherpa tarballs, fluid-speech-mini) carry `bytes = size_mb * 1_000_000`, which satisfies the R7 inequality by construction; all sha256s, base URLs, languages, speed/accuracy, min_ram, and descriptions are verbatim from the source rows.

### D. LLM + command data (Task 6)
- Created `tests/llm_command_data.rs` with the exact plan code. Confirmed RED before populating (0 rows).
- Populated `llm.json` with the 9 `polish_capable: true` rows, `capabilities: ["polish"]`, specialty lowercased const name, verdict `recommended`/`mixed` (gemma2-2b is mixed), the 3 recommended rows keep `recommended: true` (R4), `runs_on: ["macos","windows"]`, all url/size/attribution/license/backup fields verbatim.
- Populated `command.json` with `neudecide-q4`: `capabilities: ["tool_calling"]`, engine `onnx`, 6 shards with bytes + sha256, base_url `https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/`, FULL R3 display fields (size_label `55.5M`, specialty `Command Mode decider`, backup_url, attribution, license_name/url, requires_license_accept, recommended false, verdict, verdict_note, sort_order 10, runs_on_cpu true).

## Deviation from the task file (lead decision needed)
- The task file specifies `size_mb 41` for neudecide-q4, but the 6 shards sum to 42_915_902 bytes. The validator (already committed, R7) enforces `size_mb * 1_000_000 + 1024 >= sum(shard bytes)`, so 41 fails validation and `Catalog::bundled()` panics. I set `size_mb: 43` (ceil of 42.915902 MB). The plan's own R7 rule says "otherwise re-derive from bytes", which this follows. Flagging because the task file's Done-when and Step 3 both say 41.

## TDD evidence

RED (speech data), real output:
```
$ cargo test -p teletype-catalog --test speech_data > /tmp/a2_speech_red.out 2>&1; echo EXIT=$?
EXIT=101
thread 'speech_json_has_all_22_models' panicked at crates/teletype-catalog/tests/speech_data.rs:14:5:
assertion `left == right` failed
  left: 0
 right: 22
failures:
    qwen3_asr_row_carries_its_two_artifacts_and_corrected_size
    speech_json_has_all_22_models
test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

RED (llm/command data), real output:
```
$ cargo test -p teletype-catalog --test llm_command_data > /tmp/a2_llm_red.out 2>&1; echo EXIT=$?
EXIT=101
thread 'bundled_catalog_is_complete' panicked at crates/teletype-catalog/tests/llm_command_data.rs:38:5:
assertion `left == right` failed
  left: 0
 right: 22
failures:
    bundled_catalog_is_complete
    command_json_has_the_decider_with_display_fields
    llm_json_has_the_polish_rows
test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out
```

RED (command size_mb 41 vs R7), real output:
```
thread 'bundled_catalog_is_complete' panicked at crates/teletype-catalog/src/loader.rs:38:53:
bundled catalog passes validation: Validation("neudecide-q4: size_mb 41 does not cover 42915902 artifact bytes")
test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out
```

GREEN (after populating, size_mb 43): see Verification below.

## Files changed (git diff --stat)
```
 Cargo.lock                            |   1 +
 crates/teletype-catalog/command.json  |  59 +++-
 crates/teletype-catalog/llm.json      | 210 +++++++++++-
 crates/teletype-catalog/speech.json   | 614 +++++++++++++++++++++++++++++++++-
 crates/teletype-speech/Cargo.toml     |   1 +
 crates/teletype-speech/src/catalog.rs |  24 +-
 6 files changed, 884 insertions(+), 25 deletions(-)
```
Plus new (untracked): `crates/teletype-catalog/tests/{engine_registry,validate,loader,speech_data,llm_command_data}.rs`. All work left UNCOMMITTED.

## Verification (real output, direct exit codes)
```
$ export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
$ cargo test -p teletype-catalog > /tmp/a2_test.out 2>&1; echo "EXIT=$?"
EXIT=0
     Running unittests src/lib.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/engine_registry.rs ...
test unknown_name_is_an_error_not_a_variant ... ok
test name_roundtrips ... ok
test known_names_resolve_to_engines ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/llm_command_data.rs ...
test command_json_has_the_decider_with_display_fields ... ok
test llm_json_has_the_polish_rows ... ok
test bundled_catalog_is_complete ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/loader.rs ...
test load_from_missing_dir_is_an_error ... ok
test bundled_catalog_loads_with_populated_speech_rows ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/schema_roundtrip.rs ...
test speech_entry_roundtrips_through_json ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/speech_data.rs ...
test every_speech_row_has_asr_capability_and_a_known_engine ... ok
test qwen3_asr_row_carries_its_two_artifacts_and_corrected_size ... ok
test speech_json_has_all_22_models ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/validate.rs ...
test empty_domain_is_valid ... ok
test a_good_speech_set_passes ... ok
test english_only_with_other_languages_fails ... ok
test bad_purpose_label_fails ... ok
test duplicate_ids_fail ... ok
test unknown_engine_fails ... ok
test zero_recommended_in_nonempty_speech_fails ... ok
test size_mb_below_artifact_sum_fails ... ok
test multiple_recommended_is_allowed ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo test -p teletype-speech catalog > /tmp/a2_speech.out 2>&1; echo "EXIT=$?"
EXIT=0
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 39 filtered out; finished in 0.03s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s

$ cargo clippy -p teletype-catalog -p teletype-speech --all-targets > /tmp/a2_clippy.out 2>&1; echo "EXIT=$?"
EXIT=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.44s
(only pre-existing warnings: workspace profiles note, parakeet-cpp-sys build-script archive logs; no new warnings from teletype-catalog or teletype-speech code)
```

Test counts: teletype-catalog went from 1 test (schema_roundtrip) to 21 (engine_registry 3, llm_command_data 3, loader 2, schema_roundtrip 1, speech_data 3, validate 9). All new tests contain real assertions.

## Self-review
- Ownership: only owned files written (5 new test files, 3 JSON files, teletype-speech Cargo.toml + catalog.rs Platform re-export). No other file touched. Cargo.lock updated automatically by cargo (dependency add); flagging for the lead.
- No debug tests, no `dbg!`/`eprintln!`/`println!` in test code (grep clean).
- No em/en dashes in any new or changed file (grep clean; the one source ellipsis char was already in the source row and was rendered as `...` in the JSON).
- Every new test asserts.
- No git mutation attempted; work left uncommitted.

## Concerns
1. neudecide-q4 `size_mb` set to 43 instead of the task file's 41 (see Deviation above). The validator's R7 rule makes 41 impossible.
2. Single-file speech rows without pinned source bytes carry `bytes = size_mb * 1_000_000` (derived, not inventor hashes; all sha256s are verbatim). This keeps the R7 invariant satisfiable and the loader's download-sanity bound unchanged.
