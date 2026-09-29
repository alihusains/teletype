# BUG-005 — Splitter re-join can duplicate text when the model partially echoes a chunk

**Severity:** P0 (text corruption on long dictation, narrow trigger)
**Area:** transforms / splitter + engine re-join
**Status:** open (trigger confirmed by code trace; not yet reproduced with a live model)
**Found:** 2026-09-28 QA audit (text-pipeline agent), verified by reading `engine.rs` + `validator.rs`

---

## Description

For dictation over ~500 words, `run_transform_blocking` (`engine.rs:177-196`)
splits the protected input into sentence-aligned chunks, transforms each
independently, and **concatenates the chunk outputs with a single space**.

The validator's echo check (`validator.rs:127-144`) treats a model that
returns the *full* chunk as a **valid pass-through**, not an echo:

```rust
let covers_most_of_input = cleaned.len() as f64 >= input.len() as f64 * 0.9;
if !covers_most_of_input {
    return ValidatedOutput::Fallback(input.to_string(), Some(Failure::PromptEcho));
}
// ... falls through to ValidatedOutput::Transformed(cleaned)
```

So a model that returns the chunk **unchanged** is accepted (correct — it's a
no-op). But a model that returns the chunk **plus extra text** (a partial
echo: it re-states the chunk and then adds a polished version, or it re-states
the chunk with a small edit) is also accepted, because `cleaned` covers ≥ 90%
of `input`. The re-join then concatenates the echoed portion with the rest,
**duplicating the overlapped text**.

### Why it is narrow

- Only fires for >500-word dictation (the split threshold).
- Only fires when the model *partially* echoes (returns the chunk plus extra),
  not on a clean no-op (returns the chunk exactly) or a clean transform
  (returns only the polished text).
- The prior audit's splitter test used a mock that returned only the changed
  text per chunk, so the duplication was invisible.

### Why it is still P0

When it fires, the user's 700-word email becomes ~1200 words with a duplicated
middle section. That is visible, unrecoverable text corruption in the user's
document. The fix is cheap (a word-count-ratio guard in the re-join), so it
should be added now as insurance even though the trigger is narrow.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-core/src/transforms/engine.rs` | 177-196 | `run_transform_blocking` — the re-join |
| `crates/teletype-core/src/transforms/engine.rs` | 185-189 | the concatenation loop |
| `crates/teletype-core/src/transforms/validator.rs` | 127-144 | echo check (too permissive for re-join) |
| `crates/teletype-core/src/transforms/splitter.rs` | 21-90 | `split_for_polish` |

## Reproduction (requires a model that partially echoes)

1. Dictate a >500-word text (or use the eval harness with a long corpus entry).
2. Use a model/prompt that tends to re-state the input before transforming
   (some models do this, especially with a "here is the cleaned version:"
   preamble that the validator's preamble check does not catch when the
   preamble is short).
3. **Symptom:** the output contains a duplicated section.

A deterministic repro is not possible with a mock (the mock either echoes
exactly or transforms cleanly). The fix is a guard that catches the partial
echo regardless of model.

## Unit test cases (must pass after fix)

```rust
// 1. A chunk whose output is >1.3x its input AND shares >70% LCS with the
//    input must be treated as a partial echo and fallen back to the input.
#[test]
fn partial_echo_chunk_falls_back_to_input() {
    let input = "alpha beta gamma delta epsilon zeta eta theta";
    // Model returns the input plus a short addition (partial echo).
    let model_output = format!("{input} and also iota kappa");
    let result = validate_chunk(input, &model_output);
    assert_eq!(result.text, input, "partial echo must fall back to input, not concatenate");
}

// 2. A clean no-op (output == input) is still accepted.
#[test]
fn clean_noop_is_accepted() {
    let input = "alpha beta gamma delta";
    let result = validate_chunk(input, input);
    assert_eq!(result.text, input);
    assert!(!result.fell_back);
}

// 3. A clean transform (output is a real rewrite, not an echo) is accepted.
#[test]
fn clean_transform_is_accepted() {
    let input = "alpha beta gamma delta";
    let output = "the alpha and beta with gamma, plus delta";
    let result = validate_chunk(input, output);
    assert_eq!(result.text, output);
    assert!(!result.fell_back);
}

// 4. The re-join does not duplicate when all chunks transform cleanly.
#[test]
fn rejoin_no_duplication_on_clean_transform() {
    let input = make_600_word_input(); // helper: 600 distinct words
    let result = run_transform_blocking(&clean_mock(), &transform(), &input, &ctx(), &mut None);
    let words: Vec<&str> = result.text.split_whitespace().collect();
    assert_eq!(words.len(), 600, "no word lost or duplicated");
}
```

## Acceptance criteria

- [ ] In `run_transform_blocking`, after each chunk is transformed, the
      re-join checks: if `chunk_output.len() > chunk_input.len() * 1.3` AND
      `longest_common_substring(chunk_input, chunk_output).len() >
      chunk_input.len() * 0.7`, treat the chunk as a partial echo and use the
      **input** for that chunk (not the output).
- [ ] A clean no-op (output ≈ input) is still accepted (no false positive).
- [ ] A clean transform (output is a real rewrite) is still accepted.
- [ ] The existing test `long_input_is_split_and_rejoined` (`engine.rs:1051`)
      still passes.
- [ ] `cargo test -p teletype-core` passes.

## How to test (manual / smoke)

1. **Smoke (5 min, requires a long dictation or eval harness):**
   - Dictate (or paste via the eval harness) a 600+ word text.
   - Enable Polish with a local model.
   - **Pass:** the output has the same word count as the input (±5%), no
     duplicated section.
   - **Fail:** the output is ~1.3x the input length with a repeated block.

2. **Eval harness (CI):**
   - Add a 600-word corpus entry to `evals/`.
   - Run `cargo test -p teletype-eval` in measure mode.
   - **Pass:** no duplication in the output (assert word count within 5%).

## Fix direction

In `run_transform_blocking` (`engine.rs:185-189`), after `run_single_chunk`
returns for a chunk, add a guard:

```rust
let r = run_single_chunk(provider, transform, chunk, ctx, on_token);
// Partial-echo guard: if the output is much longer than the input and
// mostly overlaps it, the model re-stated the chunk. Use the input.
if r.text.len() > chunk.len() * 13 / 10
    && longest_common_substring(chunk, &r.text).len() > chunk.len() * 7 / 10
{
    tracing::warn!(chunk_len = chunk.len(), out_len = r.text.len(),
        "partial echo detected; using input for this chunk");
    full.push_str(chunk);
} else {
    full.push_str(&r.text);
}
```

`longest_common_substring` already exists in `validator.rs` (used by the echo
check) — export it or duplicate the small helper.

## Related

- BUG-004 (ITN "second" — a different text-corruption bug in the same pipeline)
- `validator.rs:127-144` — the echo check is correct for the single-chunk
  case; the bug is that the re-join does not re-apply a similar check across
  the chunk boundary.
