# L053 — A fuzzer's red can be the harness, not the product

- **ID:** L053
- **Date:** 2026-10-23
- **Agent:** OpenCode (nightly triage)
- **Area:** `fuzz/fuzz_targets/hotwords_file.rs`
- **Status:** verified

## Problem

Nightly run `39906998002` reported overall `success`, but the `Fuzz
(hotwords_file)` job was red with:

```
thread '<unnamed>' panicked at fuzz_targets/hotwords_file.rs:35:9:
lossy decode must not yield NULs
```

and libFuzzer wrote the four-byte input `[0, 0, 0, 4]` (`AAAABA==`) as a crash.
Exit status 77.

## Root cause

The fuzz target asserted a false invariant:

```rust
for line in text.lines() {
    assert!(!line.contains('\0'), "lossy decode must not yield NULs");
}
```

`String::from_utf8_lossy` replaces only *invalid* UTF-8 sequences with U+FFFD.
NUL (U+0000) is valid UTF-8, so it is preserved. Any input containing a NUL
therefore failed the assertion. Nothing in the application was involved: the
app only passes a hotwords file *path* to sherpa-onnx, it never parses the
contents in Rust.

## Fix

Assert the invariant that actually holds:

```rust
if let Ok(valid) = std::str::from_utf8(data) {
    assert_eq!(&*text, valid, "valid UTF-8 must pass through unchanged");
}
```

Keep exercising `text.lines()` for totality, but do not assert that legal
characters are absent. Add the discovered input to
`fuzz/corpus/hotwords_file/` as a regression seed.

## Verification

- The exact crash input (`cargo +nightly fuzz run hotwords_file <file>`) exits 0.
- 30 s of fuzzing: 133,954 runs, exit 0.
- The red job was non-blocking (`continue-on-error: true`), which is why the run
  itself still concluded `success` — see L050's "a green job is only evidence
  about the command that actually ran". Read job conclusions, not the run dot.

## Rule

- When a fuzz target panics, read the panicking line before filing a product
  bug. An assertion about standard-library behavior is testing the standard
  library.
- NUL is valid UTF-8. `from_utf8_lossy` preserves it; only invalid sequences
  become U+FFFD.
- Seed every fuzzer-found input back into the committed corpus, even when the
  finding was a harness bug — it keeps the path exercised cheaply.
- A `continue-on-error` matrix leg can be red while the run is green. The run
  conclusion is not the job conclusion.
