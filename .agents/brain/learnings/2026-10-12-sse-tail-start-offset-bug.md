# Learning: SSE frame parser `tail_start` offset bug

**ID:** L022
**Date:** 2026-10-12
**Agent:** Kiro CLI
**Area:** teletype-inference / server.rs / SseFrameParser
**Status:** verified — bug found by proptest, fixed, all 55 inference tests pass

---

## Problem

`SseFrameParser::push` produced different output when a multi-frame SSE body was
split at certain byte offsets vs fed in one shot. Observed by proptest:

```
tokens = ["日 ", "0", "A"], split_frac ≈ 0.792
expected: ["日 ", "0", "A"]
got:      ["日 ", "0", "0", "A"]   # "0" duplicated
```

## Root cause

In the frame-splitting loop inside `push`, `tail_start` was overwritten on each
iteration with `pos + 2` — but `pos` is relative to `rest` (the slice
**remaining** after previous frame splits), not to `bytes` (the full pending
buffer). So after two iterations:

- Iter 1: `pos=48` in `rest=bytes[0..]`, `tail_start = 50` ✓ (correct by coincidence)
- Iter 2: `pos=45` in `rest=bytes[50..]`, `tail_start = 47` ✗ (should be 97)

Result: `self.pending = bytes[47..]` instead of `bytes[97..]`, so frame 2 was
re-processed on the next `push` call — classic off-by-one with a slice origin.

## Fix

Replace the single `tail_start` that was reset on each iteration with a
`consumed` accumulator that **adds** `pos + 2` on each iteration:

```rust
let mut consumed = 0usize;
while let Some(pos) = find_subseq(rest, b"\n\n") {
    frames.push(&rest[..pos]);
    let step = pos + 2;
    consumed += step;         // ← accumulate, don't overwrite
    rest = &rest[step..];
}
self.pending = bytes[consumed..].to_vec();
```

## How it was found

Phase 1 proptest `sse_arbitrary_single_split_gives_same_output` (added 2026-10-12)
generated a 3-token body and a split at ~79% of its length. The existing hand-crafted
tests (`sse_frame_parser_streams_across_chunk_boundaries`,
`sse_frame_parser_survives_utf8_split_across_reads`) only split within a single frame
and never caught the multi-frame case.

## Verification

`cargo test -p teletype-inference sse` → 10/10 pass.
`cargo test -p teletype-inference` → 55/55 pass.

## Reusable lesson

When a slice-relative position is accumulated in a loop, use an absolute offset
accumulator (`consumed += step`) rather than a resettable variable (`tail = pos`).
The bug is easy to miss because single-frame inputs never exercise the second
iteration.

Proptest strategy: always test "any chunking of the same input gives the same output"
for streaming parsers. Hand-crafted split tests cover only the cases the author
thought of.
