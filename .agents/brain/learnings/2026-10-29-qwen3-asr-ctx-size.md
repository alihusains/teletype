# L089: --ctx-size 2048 is required for Qwen3-ASR

**Date:** 2026-10-29
**Agent:** main agent
**Project area:** speech / llama-server / memory
**Status:** verified

## Problem

`llama-server` allocates the full KV cache at load. The Qwen3-ASR 1.7B model's
trained context is far larger than any dictation needs, and the default
allocation costs 9.6 GB of RSS on an M4 Pro 24 GB machine.

## Investigation

Spawned `llama-server` with the production args plus `--reasoning off` and
measured peak RSS with a 0.25 s sampler while a 33 s clip transcribed.

| Context | Load RSS | Peak RSS | Decode (33 s clip) | Words |
|---|---|---|---|---|
| default | 9,582 MB | 9,622 MB | 2,160 ms | 120 |
| `-c 2048` | 2,795 MB | 2,825 MB | 2,162 ms | 120 |
| `-c 4096` | 3,020 MB | 3,048 MB | 2,118 ms | 120 |

All three produced the identical 120-word transcript.

## Fix

`qwen3_asr.rs` passes `--ctx-size 2048` to the spawn. The catalog row's
`min_ram_gb` is 4 (measured 2.8 GB, rounded up for comfortable use).

## Verification

Live test `qwen3_asr_live_transcribes_a_real_clip` (ignored, env-gated)
passes with the `--ctx-size 2048` arg in place. 5 corpus clips, WER 0.

## Reusable lesson

When spawning llama-server for a narrow-use model (ASR, not chat), always set
`--ctx-size` to the minimum the use case needs. The default is the model's
trained context, which can be 32k-128k tokens and allocates the full KV cache
at load. For dictation, 2048 tokens is ample.
