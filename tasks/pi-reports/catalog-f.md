# Report: catalog-f (Task 12: startup wiring + CI manifest)

Status: DONE_WITH_CONCERNS (one deviation from the plan's literal test, see Concerns)

## What was implemented

1. **Startup wiring in `crates/teletype-desktop/src/lib.rs`**:
   - New `load_catalog(data_dir)` fn: `working_copy_dir(&data_dir)` -> `seed_from_bundled` -> `try_update(&data_dir, &HttpFetch)` (throttled by the persisted last-check timestamp; any failure keeps the last good copy, so it is never fatal) -> `Catalog::load_from_dir(&catalogs_dir)` with `unwrap_or_else` fallback to `Catalog::bundled()` and a `tracing::warn!`. The `UpdateResult` is logged with `tracing::info!`.
   - New `HttpFetch` struct implementing `teletype_catalog::update::Fetch` with `reqwest::blocking::get` (blocking matches the existing codebase usage in `commands/models.rs`; the call site is the synchronous Tauri setup closure).
   - Called once in `run()` setup, right after `models_dir` resolution (near the `data_dir` resolution at former line 742/754). The loaded `Catalog` is stored once in `AppState` as a new `pub catalog: teletype_catalog::Catalog` field (set in both the real `AppState` construction and `AppState::for_tests`, which uses `Catalog::bundled()`).
   - `Cargo.toml` needed no change: `teletype-catalog` and `reqwest` (with `blocking`) are already dependencies; `tempfile` was already a dev-dependency.
2. **Startup test** in lib.rs `#[cfg(test)] mod startup_tests` (matches the crate's existing in-lib test convention, e.g. `command_decider_rebuild_tests`): `startup_seeds_the_working_copy_and_falls_back_on_bad_data`. Drives the real `load_catalog` path: asserts the three working-copy files exist, asserts the seeded catalog is non-empty, corrupts `speech.json`, re-runs `load_catalog`, and asserts the fallback catalog (bundled) is non-empty in all three domains.
3. **CI manifest step in `.github/workflows/release.yml`** (publish job, before "Check release asset names are unique"): "Build the catalog manifest" step computes the sha256 of the three repo JSON files, writes `catalog-manifest.json` with R11 contract (keys exactly `speech`/`llm`/`command`; `version` a monotonic u64 from `CATALOG_VERSION` env, falling back to `GITHUB_RUN_NUMBER`, never parsed from the semver tag; `url` = the release download base), asserts the R11 contract in-script, and prints the manifest. The release asset `files:` list now includes `catalog-manifest.json` plus the three JSON files. No new runner, no release triggered.

## TDD evidence

RED attempt: the plan's literal Step 1 test only exercises `teletype_catalog` APIs (seed/load/bundled), which were already green from Task 11, so it could not fail before the wiring existed. Instead the test drives the new `load_catalog` path (which does not exist before the wiring), so RED = compile failure of the test:

- RED command: `cargo test -p teletype-desktop startup` (test written before `load_catalog` existed)
- RED output (real):
```
error[E0425]: cannot find function `load_catalog` in this scope
  --> crates/teletype-desktop/src/lib.rs:1594:30
   |
1594|         let catalog = super::load_catalog(data_dir);
   |                              ^^^^^^^^^^^^ not found in this scope

error: aborting due to 1 previous error
EXIT=101
```
- GREEN command: `cargo test -p teletype-desktop startup` (after wiring)
- GREEN output (real):
```
test startup_tests::startup_seeds_the_working_copy_and_falls_back_on_bad_data ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 594 filtered out; finished in 10.17s
EXIT=0
```

## Files changed (git diff --stat)

```
 .github/workflows/release.yml      | 46 ++++++++++++++++++++++++
 crates/teletype-desktop/src/lib.rs | 72 ++++++++++++++++++++++++++++++++++++++
 2 files changed, 118 insertions(+)
```

Work left UNCOMMITTED for the lead.

## Verification (real output, exit codes captured directly)

```
$ export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
$ cargo test -p teletype-desktop > /tmp/f_test.out 2>&1; echo "EXIT=$?"
EXIT=0
```
Tail of /tmp/f_test.out:
```
test startup_tests::startup_seeds_the_working_copy_and_falls_back_on_bad_data ... ok
...
test result: ok. 592 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 14.16s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;   (main.rs)
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;   (edit_watch_index)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;   (file_transcribe)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;  (migration)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;   (settings_reachability)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;   (doc-tests)
```
Before: 591 lib tests (590 + 3 ignored baseline). After: 592 lib tests. One new test, no inflation.

```
$ cargo clippy -p teletype-desktop --all-targets > /tmp/f_clippy.out 2>&1; echo "EXIT=$?"
EXIT=0
```
Only the 2 pre-existing `.into()` warnings (lib.rs:539 and :1318, present in base 79524a3 per the scorecard). No new clippy warnings from this change.

### Local dry-run of the manifest generation (placeholder version 1)

```json
{
  "version": 1,
  "files": {
    "speech": "04060e4a5fa36587736268693d27d4f810e8f44b2b55cfe7dc15eb00492c88bd",
    "llm": "ae441db284840915c0a2a6b47d9a8e8ce9522aaa08d4dff624272cb55ceea1d6",
    "command": "9f9435eb99ec5fb9579cc1358d611a77fbb9872962fab899c46fabbc776e166b"
  },
  "url": "https://github.com/alihusains/teletype/releases/download/v0.0.0-dryrun"
}
```
R11 contract assertions (key set, 64-hex values, positive u64 version, https base) all pass. release.yml parses as valid YAML (checked with python yaml). No real release was triggered.

## Self-review findings

- Ownership respected: only `crates/teletype-desktop/src/lib.rs` and `.github/workflows/release.yml` modified. `Cargo.toml` needed no edit (deps already present). No other files touched.
- No debug tests, no stray `dbg!`/`eprintln!`/`println!` in test code.
- No em/en dashes in any added code or comment (grepped).
- Every assertion in the new test is a real assertion (6 asserts, no assert-free probes).
- try_update failure is non-fatal: `load_catalog` never returns a Result; the test proves the corrupt-working-copy fallback to bundled.
- R11 honored in both the CI step (keys exactly speech/llm/command, version from CATALOG_VERSION, url = download base) and the dry-run.
- No back-compat shims, no feature flags, no speculative abstraction.

## Concerns

1. **Deviation from the plan's literal Step 1 test (reported per _common.md, not a silent change).** The plan's test body only calls `teletype_catalog` APIs (`seed_from_bundled`, `load_from_dir`, `bundled`), which Task 11 already made green; it cannot be RED before the wiring. I kept the plan's test name and intent but drove it through the new `load_catalog` function (the actual startup path), which is what makes it a genuine red-green test for this task. If the lead prefers the literal plan test verbatim plus the `load_catalog`-driven test, the literal one can be added; it would be a pure catalog-API smoke test with no dependency on this task's wiring.
2. **`AppState::for_tests` uses `Catalog::bundled()` directly** (no working copy) because it builds a throwaway state without a data_dir startup flow. The real `run()` path always goes through `load_catalog`.
3. **The manifest `url` in CI uses `${GITHUB_REF_NAME}`** (the release tag) as the download base, matching the existing updater-manifest `base` in the same publish job. The app's `MANIFEST_URL` in update.rs is a fixed placeholder (`https://teletype-catalog/manifest/catalog-manifest.json`); until the real release is triggered and that constant is pointed at the real release asset URL, the app's silent update is a no-op (fetch fails, last good copy kept). Firing the release is the deferred human decision; I did not trigger one.
4. Pre-existing 2 clippy `.into()` warnings untouched (per scorecard, in base commit).
