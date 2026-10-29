# L088: Worker reported done with failing tests, wrong platform gate, and a 307 MB pin for a 2.16 GB model

**Date:** 2026-10-29
**Agent:** main agent (verification phase)
**Project area:** speech / catalog / worker verification
**Status:** verified

## Problem

A delegated `pi` worker was tasked with creating the `Engine::Qwen3Asr` catalog
row and the `Qwen3AsrProvider` in `teletype-inference`. The worker reported
completion. Re-running its test commands and reading the real diff found three
defects:

1. **Two failing tests.** `cargo test -p teletype-inference` showed 112 passed,
   2 failed: `bare_language_tag_becomes_empty` and `strips_language_word_prefix`.
   Root cause: `is_language_tag` had a `t.len() > 4` early return that
   short-circuited the `LANGUAGE_WORDS` lookup, so "english" (7 chars) was
   checked for length first and rejected before the allowlist was consulted.

2. **Wrong platform gate.** The worker set
   `Engine::Qwen3Asr => platform.is_macos() || platform == Platform::Windows`
   in `available_on`. The Windows release job builds llama.cpp at the same
   pinned commit but omits `-DLLAMA_BUILD_TOOLS=ON`, so the Windows
   llama-server has no mtmd library and does not accept `--mmproj`. Offering
   the row on Windows would produce a dead download.

3. **Wrong size pin.** The worker recorded the main GGUF as 307,018,165 bytes.
   The Hugging Face API reports 2,165,034,944 bytes for
   `Qwen3-ASR-1.7B-Q8_0.gguf`. The 307 MB figure is consistent with a truncated
   download (the mmproj pin was also off by ~600 KB from the actual 355,709,344
   bytes). A download that fails its SHA-256 check forever is a dead row.

## Investigation

- Re-ran `cargo test -p teletype-inference --lib` (112/114, 2 failures).
- Read the `is_language_tag` source; the `t.len() > 4` guard was the bug.
- Checked `.github/workflows/release.yml` line 639: the Windows cmake command
  has no `-DLLAMA_BUILD_TOOLS`; `scripts/build-llama-server.sh` line 113 has
  `-DLLAMA_BUILD_TOOLS=ON`. The macOS binary contains `tools/mtmd/models/qwen3a.cpp`
  strings; the Windows build was never inspected.
- Queried `https://huggingface.co/api/models/ggml-org/Qwen3-ASR-1.7B-GGUF/tree/main?recursive=true`
  for authoritative sizes and LFS OIDs. The LFS OID is the SHA-256 of the
  resolved bytes.

## Fix

- `qwen3_asr.rs`: rewrote `strip_language_prefix` to check the `<asr_text>`
  marker first (authoritative), then fall back to a strict allowlist
  (`LANGUAGE_WORDS` + `LANGUAGE_CODES`). No length heuristic. 11/11 qwen3
  tests pass.
- `catalog.rs`: `Engine::Qwen3Asr => platform.is_macos()` (macOS only).
  `QWEN3_ASR_ARTIFACTS` re-pinned from the HF API: main 2,165,034,944 bytes,
  sha256 `58e22d05...`; mmproj 355,709,344 bytes, sha256 `46c1d533...`.
- Added `qwen3_asr_published_bytes_are_the_pinned_ones` (ignored, env-gated)
  to catch a republished HF file.
- Downloaded both files; both hash-match the pins.

## Verification

- 726 tests green across teletype-speech (68), teletype-inference (116),
  teletype-desktop (427), teletype-eval (115).
- Live transcription of 5 corpus clips (plain-01, question-01, awkward-02,
  list-01, long-01) all matched their references with WER 0.
- `cargo check --workspace --all-targets` clean. `npx tsc --noEmit` clean.

## Reusable lesson

Exit 0 is not verification. When a delegated worker reports "done":
1. Re-run its test commands.
2. Read the real diff (not the worker's summary).
3. Cross-check any numeric pin (size, checksum, URL) against a primary source
   before it ships in a download path.
4. Check platform gates against the actual build flags in CI, not against the
   assumption that "the same commit builds the same binary everywhere."
