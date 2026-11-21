# Teletype v0.2.21 — backtrack quantity fix

## Why this patch exists

A post-release QA pass found a deterministic backtrack bug that could shape the delivered text before any model ran:

```text
"It costs twenty dollars, correction, thirty dollars" -> "It costs twenty thirty dollars"
"we have three servers, correction, four servers"      -> "we have three four servers"
"ship on Monday, correction, on Tuesday"               -> "ship on on Tuesday"
```

Two unit tests had encoded the corrupted output as a "known limitation", which made the bug look intentional. v0.2.21 fixes it.

## Fixed

- Quantity phrases are now retracted as one fact. The comma-bounded clause walk extends left over the number and its unit (`twenty dollars`, `three servers`, `five hundred`), but stops at names and verbs.
- A duplicated leading function word in the replacement is dropped, so `on Monday, correction, on Tuesday` becomes `on Tuesday`, not `on on Tuesday`.
- The two tests that encoded the broken output now assert the corrected text, and new cases cover spelled/digit quantities and multi-word quantity connectors.

## Verification

- `cargo test --workspace`: all green; `teletype-core` 1396 passed, 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- Polish eval self-test: PASS.
- TypeScript and vitest: clean; 58 files / 610 tests.

## Scope

This is a patch on top of v0.2.20. It does not change models, profiles, personalization, or the release pipeline. See `docs/release-notes-v0.2.20.md` for the full feature release.
