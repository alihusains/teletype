# catalog-e: silent-update path (plan Task 11)

## What I implemented

1. `crates/teletype-catalog/tests/update.rs` (create): the four stub-fetch
   tests from plan Task 11 Step 1, with concrete helpers built from the real
   schema + sha2 + tempfile:
   - `a_newer_manifest_updates_the_working_copy`: seed bundled, install v1
     (Updated, version file = 1), same version again (Current), reset
     throttle, v2 (Updated, version file = 2, working copy replaced,
     last_check written).
   - `a_wrong_sha256_keeps_the_old_catalog`: manifest claims all-zero
     hashes, fetch returns good bodies, result Failed, working copy
     byte-identical, version not bumped.
   - `a_schema_error_keeps_the_old_catalog`: hashes match, but llm.json has
     a duplicate id, result Failed, working copy byte-identical, version
     not bumped.
   - `a_recent_check_is_skipped`: fresh last_check, result Skipped.
   Plus a lib-level unit test `swap_renames_staged_files_into_place`
   (MemFetch) that proves the atomic-swap mechanics: only the changed file
   is renamed into place, unchanged files stay, version bumped, no `.tmp`
   files left behind.
2. `crates/teletype-catalog/src/update.rs` (create): `Manifest` (serde,
   R11 contract), `UpdateResult { Updated, Current, Skipped, Failed }`,
   `Fetch` trait, `MANIFEST_URL` (fixed contract URL for the release asset
   `catalog-manifest.json`), `working_copy_dir`, `last_check_path`,
   `should_check`, `seed_from_bundled`, `write_last_check`, `try_update`
   implementing the seven-step flow (throttle, fetch manifest, compare
   version, download changed files to temp + sha256 verify, parse +
   validate_catalog, atomic swap via write-temp + rename, bump version +
   write last_check). Any failure keeps the last good working copy.
   R11 honored: files keys exactly speech/llm/command, each fetched from
   `{url}/{key}.json`, installed version u64 text at
   `working_copy_dir/version` (absent => 0), version monotonic u64.
3. `crates/teletype-catalog/src/lib.rs`: added `pub mod update;`.
4. `crates/teletype-catalog/Cargo.toml`: added `tempfile = "3"` dev-dep
   (same version as the other crates). Cargo.lock updated by cargo.

## TDD evidence

RED (before src/update.rs existed):
```
$ cargo test -p teletype-catalog --test update
EXIT=101
error[E0432]: unresolved import `teletype_catalog::update`
  --> crates/teletype-catalog/tests/update.rs:10:23
   |
10 | use teletype_catalog::update::*;
   |                       ^^^^^^ could not find `update` in `teletype_catalog`
error: could not compile `teletype-catalog` (test "update") due to 1 previous error
```

GREEN (after implementation):
```
$ cargo test -p teletype-catalog --test update
EXIT=0
running 4 tests
test a_recent_check_is_skipped ... ok
test a_wrong_sha256_keeps_the_old_catalog ... ok
test a_schema_error_keeps_the_old_catalog ... ok
test a_newer_manifest_updates_the_working_copy ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Verification (real output + exit codes)

```
export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/targets/catalog-a
cargo test -p teletype-catalog --test update > /tmp/e_green.out 2>&1; echo EXIT=$?
EXIT=0
cargo test -p teletype-catalog > /tmp/e_all.out 2>&1; echo EXIT=$?
EXIT=0
cargo clippy -p teletype-catalog --all-targets > /tmp/e_clippy.out 2>&1; echo EXIT=$?
EXIT=0
```

Whole-crate test tail (all suites):
```
running 4 tests
test a_recent_check_is_skipped ... ok
test a_wrong_sha256_keeps_the_old_catalog ... ok
test a_schema_error_keeps_the_old_catalog ... ok
test a_newer_manifest_updates_the_working_copy ... ok
test result: ok. 4 passed; 0 failed
...
running 9 tests
test bad_purpose_label_fails ... ok
...
test result: ok. 9 passed; 0 failed
Doc-tests teletype_catalog: 0 tests, ok
```
Full-crate count: all suites 0 failed (lib unit tests including the new
swap test, 7 integration test files, 4 new update tests).

Clippy: only the pre-existing workspace warning "profiles for the non
root package will be ignored" (tinydecide). Zero warnings from
teletype-catalog code.

## Files changed (git diff --stat)

```
 Cargo.lock                         |  1 +
 crates/teletype-catalog/Cargo.toml |  3 +
 crates/teletype-catalog/src/lib.rs |  1 +
 crates/teletype-catalog/src/update.rs (new, untracked)
 crates/teletype-catalog/tests/update.rs (new, untracked)
```
Work left UNCOMMITTED for the lead, as required.

## Self-review

- Ownership respected: only the four owned files (plus Cargo.lock, which
  cargo updates automatically for the dev-dep).
- No debug tests left: the swap unit test has real assertions; the
  temporary eprintln debug lines used during diagnosis were removed before
  the final run.
- No em/en dashes in new code (grep verified).
- Every test asserts; no `dbg!`/`eprintln!`/`println!` in test code.
- R11 honored: exact keys speech/llm/command, `{url}/{key}.json` fetch
  shape, u64 version file at working_copy_dir/version absent => 0,
  monotonic u64 (never parsed from a semver tag),
  should_check(Option<SystemTime>, SystemTime) >= 1h.
- No real network: Fetch is stubbed in all tests.
- Failed updates provably leave the working copy unchanged: the two
  failure tests byte-compare the working copy before/after and assert the
  version file did not bump.

## Concerns / notes

- The plan's literal Step-1 test would install v2 directly from a seeded
  (version 0) working copy; I kept the same four scenarios but made the
  first test install v1 first so "newer" is genuinely newer, and added a
  Current assertion for the same version. The plan also writes
  last_check only on the success path, which means consecutive in-test
  calls must reset the throttle; the test does that by writing an old
  timestamp (2h back) between calls. This is a test-harness detail, not a
  contract change.
- `write_last_check` persists epoch seconds as text; `read_last_check`
  parses it back. Sub-second precision is not needed for a 1h throttle.
- The `MANIFEST_URL` constant is a fixed contract URL
  (`https://teletype-catalog/manifest/catalog-manifest.json`) so tests can
  stub it by exact URL and by suffix (the plan's StubFetch matches on the
  `catalog-manifest.json` suffix, which also works). Task 12's real
  HttpFetch should fetch this same URL; if CI publishes the manifest under
  a different fixed URL, only this constant changes.
- Out of scope (per task file): real reqwest HttpFetch + startup wiring
  (Task 12), CI release.yml manifest step (Task 12), manifest signing
  (follow-up, not v1).
