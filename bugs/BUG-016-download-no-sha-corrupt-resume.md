# BUG-016 — Interrupted download of a no-SHA catalog entry can leave a corrupt model that passes the size check

**Severity:** P2 (edge, confusing failure)
**Area:** inference / download.rs + catalog.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `download.rs:515-540` and `catalog.rs:11-35`

---

## Description

`download_single` (`download.rs:515-540`) only verifies SHA when
`entry.sha256.is_some()`. The `fast` (Qwen3-1.7B) and `quality` (Qwen3-4B)
catalog entries have `sha256: None`.

If the connection drops mid-transfer, attempt 2 resumes with
`Range: bytes=offset-`. If the server returns 206 honoring the range on a
**renamed/updated** remote file (these are `resolve/main/` HuggingFace URLs,
which can be re-pointed), the resumed tail is appended to a stale prefix;
with no checksum there is nothing to catch it.

A second interrupt can leave a file whose size is within the 5% tolerance of
`size_mb` (expected size is MB-rounded: `size_mb * 1_000_000` vs real
~1.4 GB), so `is_downloaded` passes, `select_model` /
`ensure_local_provider` proceed, and `llama-server` fails to load the GGUF
mid-dictation.

The verified-download fix (f2e6177) only protects entries **with** a
checksum; the two largest catalog entries are unprotected.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-inference/src/download.rs` | 515-540 | resume path, SHA only when present |
| `crates/teletype-inference/src/catalog.rs` | 11-35 | `has_plausible_size` (5% tolerance) |
| `crates/teletype-inference/src/catalog.rs` | — | `fast`/`quality` entries with `sha256: None` |

## Reproduction

1. Start downloading the `fast` or `quality` model.
2. Interrupt the network mid-download.
3. Resume.
4. If the remote file was re-pointed between attempts, the resumed tail is
   appended to a stale prefix.
5. The file size is within 5% of expected, so `is_downloaded` passes.
6. Select the model — `llama-server` fails to load mid-dictation.

## Unit test cases (must pass after fix)

```rust
// 1. A no-SHA catalog entry is NOT considered downloaded unless its size
//    matches EXACTLY (not within 5%).
#[test]
fn no_sha_entry_requires_exact_size() {
    let entry = catalog_entry("fast"); // sha256: None, size_mb: 1400
    assert!(entry.sha256.is_none());
    let size = entry.size_mb * 1_000_000;
    assert!(!is_downloaded_at(size + 10 * 1_000_000, entry),
        "a +10 MB file must not pass for a no-SHA entry");
    assert!(is_downloaded_at(size, entry),
        "exact recorded byte size must pass");
}

// 2. A resume onto a re-pointed remote file is detected.
#[test]
fn resume_detects_repointed_remote() {
    // Prefix written under ETag "A"; remote now serves ETag "B".
    let remote = FakeRemote::etag("B", full_bytes());
    let prefix = Prefix::written_with_etag("A", stale_prefix());
    let decision = resume_decision(&remote, &prefix);
    assert!(matches!(decision, ResumeDecision::Restart),
        "changed remote ETag/size must force a full re-download");
}
```

## Acceptance criteria

- [ ] No-SHA catalog entries are verified by **exact** size (not 5%
      tolerance).
- [ ] A resume that detects a changed remote file re-downloads from scratch.
- [ ] A corrupt model cannot pass `is_downloaded`.

## How to test (manual / smoke)

1. **Smoke (3 min):** download `fast`, interrupt, resume, confirm the final
   file loads in `llama-server` (run it, check `/health`).
   - **Pass:** `/health` returns ok and a test prompt completes.
   - **Fail:** GGUF load error, or the model was silently re-downloaded.

2. **Regression (2 min):** a clean (uninterrupted) download still works.
   - **Pass:** model downloads, loads, and answers.
   - **Fail:** clean downloads now fail or re-verify unnecessarily.

## Fix direction

(a) Add SHA-256 to the `fast` and `quality` catalog entries (fetch the
current hashes and pin them).

(b) For any no-SHA entry, make `has_plausible_size` an **exact** match on the
recorded byte size — record the real byte size, not MB-rounded.

(c) On resume, re-HEAD the remote; if the `content-length` or ETag changed
since the prefix was written, discard the prefix and restart.

## Related

- G009 (0-byte stub shadowed lookup) — a different download bug, already
  fixed.
