# Report: catalog-g (Ivy acceptance, plan Task 13, R5)

## What I implemented

1. **Added the `ivy` row to `speech.json`** using the verified artifacts:
   - main: `ivy-Q8_0.gguf`, 1_834_422_208 bytes, sha256 `da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed`
   - mmproj: `mmproj-ivy-f16.gguf`, 641_774_016 bytes, sha256 `07ed1cc9c96c19aba84354b9135c69332747a98d50363a321bf1418aececfc00`
   - engine `llama-server`, base_url `https://huggingface.co/raj7676/ivy-dictation/resolve/main`
   - english_only true, languages `["en"]`, capabilities `["asr"]`
   - purposes: `English only` (inferred), `Experimental` (inferred)
   - size_mb 2477, min_ram_gb 4, speed 6.0, accuracy 8.5 (display estimates consistent with qwen3-asr row)
   - runs_on `["macos"]`, recommended false, auxiliary false
   - Size check: 2477 * 1_000_000 + 1024 = 2_477_001_024 >= 2_476_196_224 (artifact sum). PASS.

2. **Updated `tests/speech_data.rs`**: renamed test to `speech_json_has_all_23_models`, added `"ivy"` to expected list, changed `is_none()` to `is_some()`.

3. **Updated `tests/llm_command_data.rs`**: completeness speech count 22 -> 23.

4. **Created `tests/acceptance_ivy.rs`** with the exact code from plan Task 13 Step 2.

## TDD evidence

The task file prescribes the exact test code (plan Step 2), so the RED phase is the pre-existing `speech_data.rs` asserting `is_none()` for ivy. After adding the data row:

- RED (pre-change): `speech_json_has_all_22_models` would fail because `speech_by_id("ivy").is_none()` is now false.
- GREEN (post-change): all focused tests pass.

## Files changed

```
crates/teletype-catalog/speech.json               | 904 ++++++++++++++++++----
crates/teletype-catalog/tests/llm_command_data.rs |   2 +-
crates/teletype-catalog/tests/speech_data.rs      |   7 +-
crates/teletype-catalog/tests/acceptance_ivy.rs   |  14 + (new)
```

The large speech.json diff is JSON reformatting (Python `json.dump` with indent=2) plus the new ivy row. Content is identical for the 22 original rows.

## Verification commands and real output

### Focused tests (my owned files)
```
$ cargo test -p teletype-catalog --test acceptance_ivy --test speech_data --test llm_command_data
EXIT=0

running 1 test
test ivy_flows_through_every_layer_from_pure_data ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 3 tests
test bundled_catalog_is_complete ... ok
test llm_json_has_the_polish_rows ... ok
test command_json_has_the_decider_with_display_fields ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 3 tests
test qwen3_asr_row_carries_its_two_artifacts_and_corrected_size ... ok
test every_speech_row_has_asr_capability_and_a_known_engine ... ok
test speech_json_has_all_23_models ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### Full teletype-catalog suite
```
$ cargo test -p teletype-catalog
EXIT=101

test bundled_catalog_loads_with_populated_speech_rows ... FAILED
  assertion `left == right` failed
    left: 23
   right: 22
```

**BLOCKER**: `tests/loader.rs:7` asserts `c.speech().len() == 22`. This file is NOT in my "Owns" list. It needs `22` changed to `23`.

### teletype-speech
```
$ cargo test -p teletype-speech
EXIT=101

test catalog::tests::urls_point_at_the_right_repos ... FAILED
  assertion failed: m.url().starts_with("https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main")
```

**BLOCKER**: `crates/teletype-speech/src/catalog.rs:970` (test at line 952) asserts ALL `Engine::Qwen3Asr` rows have a URL starting with the Qwen3-ASR repo. Ivy is also `llama-server`/`Qwen3Asr` but has a different base_url. The test needs to either skip ivy or check per-row base_url.

### Grep guard (R14)
```
$ grep -rn '"ivy"' crates --include=*.rs | grep '/src/'
GREP_EXIT=1
```
Clean. No Rust SRC hardcodes "ivy".

### Clippy
```
$ cargo clippy -p teletype-catalog --all-targets
EXIT=0
```

### Em dash check
```
$ grep -n '\u2013\|\u2014' <my files>
EMDASH_EXIT=1
```
Clean.

## Self-review findings

- Ownership: I only wrote to my 4 owned files. The two blockers require changes to files I do NOT own.
- No debug tests, no stray eprintln/dbg.
- No em/en dashes.
- All assertions present.
- The JSON reformatting of speech.json is a side effect of using Python to append the row. The 22 original rows are semantically identical.

## Concerns / blockers

1. **`crates/teletype-catalog/tests/loader.rs`** (line 7): `assert_eq!(c.speech().len(), 22)` must become `23`. This file is not in my "Owns" list.
2. **`crates/teletype-speech/src/catalog.rs`** (line 970, test `urls_point_at_the_right_repos`): The Qwen3Asr arm asserts every row's URL starts with the Qwen3-ASR repo URL. Ivy has a different base_url. The test needs updating to handle per-row base_url (similar to the `ParakeetCpp` arm which checks `base_url().is_none()` first). This file is not in my "Owns" list.

Both are mechanical one-line fixes that the lead (or a follow-up task) can apply.

---

## catalog-g2 (follow-up: formatting + 2 test fixes)

### What changed
1. **Restored `speech.json` to original compact formatting.** The 22 original rows are byte-identical to HEAD. Only the ivy row is new (+33 lines, 0 deletions).
2. **`tests/loader.rs`**: count 22 -> 23.
3. **`crates/teletype-speech/src/catalog.rs`** (test `urls_point_at_the_right_repos`, Qwen3Asr arm): now checks the row's own `base_url()` when present, else falls back to the Qwen3-ASR default repo. Mirrors the ParakeetCpp arm pattern.

### Verification (real output)

```
$ cargo test -p teletype-catalog
EXIT=0
(all suites pass: acceptance_ivy 1, speech_data 3, llm_command_data 3, loader 2, validate 9, gate 4, etc.)

$ cargo test -p teletype-speech
EXIT=0
(all 81 tests pass including urls_point_at_the_right_repos)

$ grep -rn '"ivy"' crates --include=*.rs | grep '/src/'
GREP_EXIT=1  (clean, no src hardcodes ivy)

$ cargo clippy -p teletype-catalog -p teletype-speech --all-targets
EXIT=0  (no new warnings)

$ git diff --stat
crates/teletype-catalog/speech.json               | 33 +++++++++++++++++++++++
crates/teletype-catalog/tests/llm_command_data.rs |  2 +-
crates/teletype-catalog/tests/loader.rs           |  2 +-
crates/teletype-catalog/tests/speech_data.rs      |  7 ++---
crates/teletype-speech/src/catalog.rs             |  7 ++---
5 files changed, 43 insertions(+), 8 deletions(-)
(+ untracked new file: crates/teletype-catalog/tests/acceptance_ivy.rs)
```

### Status: DONE
All green. Minimal diff. No concerns.
