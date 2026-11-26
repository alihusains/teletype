# Plan 003: Downmix multi-channel audio files correctly

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- crates/teletype-desktop/src/commands/models.rs crates/teletype-desktop/tests/file_transcribe.rs
> If either file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit 8746c1c, 2026-11-24

## Why this matters

transcribe_file is a shipped feature (the Dictation screen Choose file button). For any multi-channel file, decode_audio_file sums all n samples of each channel plane and pushes one value per channel — so a 3-second stereo clip produces 2 samples instead of roughly 48,000. The ASR engine receives a tiny, meaningless buffer and the transcript is garbage. Mono files are unaffected, which is why the existing tests (both mono) pass.

## Current state

- crates/teletype-desktop/src/commands/models.rs:2688 — fn decode_audio_file(path: &Path) -> CommandResult<Vec<f32>>. Caps: 512 MB file, 2 hours duration. Uses symphonia and teletype_core::audio::resample_to_target(&pcm, from_rate) (audio/mod.rs:981), which returns 16 kHz mono.
- :2740-2759 — the broken multi-channel branch:

      let n = f32_buf.frames();
      if channels <= 1 {
          pcm.extend_from_slice(&f32_buf.chan(0)[..n]);
      } else {
          let planes: Vec<&[f32]> = (0..channels).map(|c| f32_buf.chan(c)).collect();
          for sample in planes.iter().map(|p| p.iter().take(n)) {
              let sum: f32 = sample.sum();
              pcm.push(sum / channels as f32);
          }
      }

  The iterator is over CHANNELS, not frames: one sample is pushed per channel per decoded packet.
- :2776-2780 — decode_audio_file_public is the test seam; lib.rs:45 re-exports it as teletype_desktop_lib::decode_audio_file_public.
- crates/teletype-desktop/tests/file_transcribe.rs — integration tests. make_wav(path, secs, rate, hz) writes a MONO 16-bit PCM WAV (header channels 1, byte rate rate*2, block align 2). Tests decodes_wav_to_16khz_mono and resamples_44khz_wav_to_16khz assert decoded duration 2.8..=3.2 s and a non-silent peak.
- Test seam convention: no live model needed; tests exercise decode/resample only.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| File-transcribe tests | cargo test -p teletype-desktop --test file_transcribe | exit 0, 3 tests pass |
| Desktop crate tests | cargo test -p teletype-desktop | exit 0 |
| Format check | cargo fmt --all -- --check | exit 0 |

## Scope

In scope:
- crates/teletype-desktop/src/commands/models.rs (only the else branch at :2753-2757)
- crates/teletype-desktop/tests/file_transcribe.rs

Out of scope:
- transcribe_file, validate_audio_path, the size/duration caps, and resample_to_target.
- self_test.rs / e2e.rs fixtures — the existing mono WAV fixtures stay as they are.
- No change to the return type, error messages, or the mono path.

## Git workflow

- Branch: advisor/003-stereo-file-decode
- Commit message style: fix(audio): downmix multi-channel files frame by frame
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Iterate frames, not channels

Replace the else branch at :2753-2757 with a frame loop:

    let planes: Vec<&[f32]> = (0..channels).map(|c| f32_buf.chan(c)).collect();
    for frame in 0..n {
        let sum: f32 = planes.iter().map(|p| p[frame]).sum();
        pcm.push(sum / channels as f32);
    }

Keep the explanatory comment but rewrite it to say one mono sample is emitted per frame, averaging the channel planes. Do not change the channels <= 1 branch.

Verify: cargo check -p teletype-desktop -> exit 0.

### Step 2: Add a stereo fixture generator and regression test

In crates/teletype-desktop/tests/file_transcribe.rs:
1. Change make_wav to take a channels: u16 argument, or add make_stereo_wav alongside it leaving the two existing calls intact. For stereo: header channels 2, byte rate rate*2*2, block align 2*2, and interleaved left/right 16-bit samples (e.g. left = 440 Hz tone, right = the same tone at half amplitude, so the downmix is non-silent and not identical to either channel).
2. Add fn decodes_stereo_wav_to_16khz_mono(): generate 3 seconds at 16 kHz stereo, call decode_audio_file_public, assert duration in 2.8..=3.2 s and peak > 0.1.

This test fails on the old code (duration would be about 0.0001 s) and passes after step 1.

Verify: cargo test -p teletype-desktop --test file_transcribe -> 3 tests pass, including the new one by name.

## Test plan

- New test: decodes_stereo_wav_to_16khz_mono in file_transcribe.rs.
- Pattern: the existing decodes_wav_to_16khz_mono and resamples_44khz_wav_to_16khz.
- Edge cases: 2-channel file at 16 kHz; mono regression tests unchanged; optional third test for 4 channels if cheap.
- Verification: cargo test -p teletype-desktop --test file_transcribe -> all pass.

## Done criteria

- [ ] grep -n 'for sample in planes' crates/teletype-desktop/src/commands/models.rs returns no matches.
- [ ] cargo test -p teletype-desktop --test file_transcribe exits 0 with the new stereo test listed by -- --list.
- [ ] cargo test --workspace exits 0.
- [ ] Only models.rs, file_transcribe.rs, and plans/README.md are modified.

## STOP conditions

Stop and report if:
- f32_buf.chan(c) returns fewer than n samples for any channel at the call site (would indicate a different symphonia API than assumed).
- The new stereo test fails on duration after the fix by more than the existing resampler margin.
- A fix appears to require changing resample_to_target or the mono branch.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- The chosen downmix is an unweighted average, matching the previous intent (the old code divided by channels). If the product later wants per-channel gain or mid/side handling, change it here only — there is one implementation.
- The duration cap at :2760 is checked against pcm.len(); after this fix that is the true mono sample count, so the 2-hour cap becomes accurate for stereo for the first time.
