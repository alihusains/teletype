# BUG-020 — Stale SHA-256 pins on both sherpa-onnx rows: every Windows first-run model download is discarded after a full download

**Severity:** P1 (blocks Windows onboarding entirely)
**Area:** speech / catalog.rs pins vs k2-fsa/sherpa-onnx release assets
**Status:** fixed (2026-11-10, re-pin in `release/v0.2.17` working tree — uncommitted)
**Found:** 2026-11-10, user report: onboarding "Download & set up" button reverts after a full download, nothing installed, retry loops forever.

---

## Description

`download_speech_model` (`teletype-desktop/src/commands/models.rs`) verifies the
pinned SHA-256 after streaming and discards the file on mismatch:

```text
Parakeet v3 (sherpa-onnx) failed its checksum. The download was discarded;
please try again.
```

Both Windows catalog rows were pinned on 2026-10-22 (`5098409`):

| Row | Pinned (stale) | Live asset (2026-11-10) |
|---|---|---|
| `sherpa-parakeet-v3-int8` | `5793d0fd…b16bf` | `1495e91b59d7fc53ddca91f3431758ec99f8fa253fdb0e1e851be0f72b14b0f9` |
| `sherpa-parakeet-v2-int8` | `157c157b…1e1ad` | `6fc4426b54aa549a0019b8a75b8dea59164e71790a37ac56a045ad57f915f714` |

The `asr-models` release assets were re-uploaded upstream around 2026-11-07
(mtimes inside the archives are Nov 7) — after the pins were set and before the
v0.2.18 build (Nov 7 16:49), so v0.2.18 shipped already broken for Windows
first-run setup. The archive contents are otherwise correct: same four model
files (`encoder/decoder/joiner.int8.onnx`, `tokens.txt`) the loader expects.

## Symptom (user-visible)

1. Onboarding step "Set up your dictation model" → "Download & set up (465 MB)".
2. 487 MB downloads with a live progress bar.
3. "Verifying…" → checksum mismatch → file deleted.
4. Button reverts to "Download & set up" with the red error line; retry
   re-downloads everything and fails identically. The "Show other models" v2
   row fails the same way.

## Why CI missed it

`scripts/models-check.sh` only does a HEAD request per URL ("full-file SHA
verification of multi-GB GGUFs is a nightly job") — the URL still resolves
200 after a re-upload, so the check stayed green. `speech_pins_hold_against_
hugging_face` (models.rs) covers the 5 HF-pinned files only, not the two
GitHub-release sherpa archives.

## Fix

Re-pin both rows in `crates/teletype-speech/src/catalog.rs` to the live
digests (done, uncommitted on `release/v0.2.17`).

## Follow-ups (suggested, not done)

- Extend `models-check.sh` (or the nightly job) to full-file SHA-256 for the
  two GitHub-release sherpa archives — they are ~465 MB, not multi-GB.
- Consider surfacing the checksum-failure error more prominently in
  onboarding (11 px red text under the button is easy to miss).

## Verification

- Downloaded both live assets on 2026-11-10; `sha256sum` matches the new
  pins exactly (v3: 487,211,443 bytes; v2: 482,440,461 bytes).
- `tar tjf` on the v3 archive lists the four `MODEL_FILES` plus README/test
  wavs; extraction to
  `%APPDATA%\com.teletype.app\models\sherpa\sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8\`
  satisfies `speech_model_installed` (all four files present, non-empty).
- `cargo test -p teletype-speech` could not be run on the reporting machine
  (no MSVC linker; only the stable-msvc toolchain and no Visual Studio
  installed). The change is two string literals; no test references the old
  digests (grep-verified).
