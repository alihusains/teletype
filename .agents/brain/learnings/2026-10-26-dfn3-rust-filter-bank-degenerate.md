---
id: L-20261026-dfn3-rust-filter-bank-degenerate
title: The in-repo Rust DFN3 cannot run the published dfn3_ll ONNX export
status: verified
type: debugging
area: "teletype-core/src/audio/dfn3.rs"
agent: architect
tool: teletype-eval
date: 2026-10-26
---

# The in-repo Rust DFN3 cannot run the published dfn3_ll ONNX export

## Problem

The P3 spike needed a DFN3 baseline on the `competing-0db` two-talker set,
and the app already ships a DFN3 implementation in
`crates/teletype-core/src/audio/dfn3.rs`. Running it against the published
`DeepFilterNet3_ll_onnx.tar.gz` from Rikorose/DeepFilterNet `models/` fails
at load time:

```
dfn3 enc: Got invalid dimensions for input: feat_erb for the following indices
 index: 3 Got: 96 Expected: 32
```

## Symptoms

- `Dfn3::load` succeeds (all three sessions load), but the first `process()`
  call errors from the encoder: the Rust code feeds a `(1,1,T,32)` ERB input
  while the model declares `feat_erb [1,1,'S',32]` and `feat_spec
  [1,1,'S',96]` — the "96" is the deep-filter bin count, not the ERB count.
- The `DeepFilterNet2_onnx_ll.tar.gz` export has the same shape, so it is
  not a DF3-vs-DF2 mismatch in the ERB channel; the Rust module was written
  against a different (unpublished) export whose `feat_spec` is
  `(1,1,T,2,481)` over the full spectrum.

## Initial hypotheses

1. Wrong weights version (DF2 vs DF3) — ruled out: both published exports
   have identical input shapes, and the Rust constants (960/480, 32 ERB,
   96 DF bins, 5-tap) match the DF3 `config.ini`.
2. ORT input-layout mismatch — ruled out: the Python reference reproduces
   the exact shapes and runs cleanly.

## Investigation

1. Inspected the published `dfn3_ll` export with `onnxruntime`:
   `feat_erb [1,1,'S',32]`, `feat_spec [1,1,'S',96]`, encoder outputs
   `e0..e3 (1,64,'S',32/16/8/8)`, `emb ('Reluemb_dim_0','S','Reluemb_dim_2')`
   = 512, `c0 (1,64,'S',96)`.
2. Replicated the Rust front end in Python
   (`scripts/eval/dfn3_ref.py`): 48 kHz Hann-window STFT (960/480), the
   same 32-band ERB filter bank, the same 5-tap complex deep filter, and
   the same overlap-add back end.
3. Checked the ERB filter bank in isolation: with the Rust edge layout
   (31 edges linear in 0-16 kHz, last band declared to 24 kHz, FFT 960 at
   48 kHz so bin 480 = 24 kHz), **bands 24-31 have zero in-band bins** —
   the bank is degenerate. A correct 32-band ERB bank over 0-24 kHz would
   give every band ~15 bins.
4. The published export's `feat_spec` is 96 bins (the deep-filter range),
   not 481, so even a fixed filter bank would not feed it.

## Evidence

- `onnxruntime` input shapes for `enc.onnx` (both published exports):
  `feat_erb [1,1,'S',32]`, `feat_spec [1,1,'S',96]`.
- Band audit of the Rust `erb_filter_bank(false)`: bands 0-23 have 20-21
  bins each, bands 24-30 have 0, band 31 has 0.
- The Python reference (`scripts/eval/dfn3_ref.py`) runs the published
  export end to end: silence in -> silence out, 440 Hz tone in -> 92% RMS
  out, and produces a full 200-case denoised set in 49 s on the M4 Pro.

## Root cause

`dfn3.rs` was written against a DF3 export that is not in the public
`models/` directory (its `feat_spec` is the full 481-bin spectrum with 2
channels, and its ERB bank presumably covered 0-24 kHz correctly). The
published `DeepFilterNet3_ll_onnx.tar.gz` is a reduced export
(`feat_spec` = 96 DF bins only) whose ERB front end must be computed by a
filter bank that matches the model's training config — which the Rust
module does not implement (its bank is degenerate for the same edge
layout).

## Fix

None in this round. For the spike, the DFN3 baseline was measured with the
Python reference pipeline (`scripts/eval/dfn3_ref.py`) driving the
published ONNX models, which is faithful to the algorithm but not the
shipped code. The in-repo `dfn3.rs` needs either (a) the matching export
from the DeepFilterNet authors, or (b) a rewrite of the front end to feed
the published 96-bin export with a correct 0-24 kHz ERB bank. Until then
the `dfn3` feature in `teletype-core` should be treated as unverified.

## Verification

- `scripts/eval/dfn3_ref.py` self-test: silence -> 0.0 max abs, 440 Hz
  tone -> 0.1956 out RMS vs 0.2121 in RMS.
- 200-case run on `competing-0db`: 49 s wall, 200 wavs written.
- Scored with the eval driver: **micro WER 54.05** (vs 44.57 raw-mix
  Fluid Mini, 43.58 raw-mix Ultra, 30.83 Alkove Focus).

## Reusable lesson

A "we already ship X" assumption is not a baseline: before using an
in-repo implementation as the control arm of a measurement, run it on one
case and check the output is sane. Here the shipped DFN3 could not even
load the published weights, and its filter bank is degenerate — the
baseline had to be rebuilt from the algorithm spec instead.
