# ADR-014 — Beat-Wispr plan of record + commit-while-you-go streaming

**Status:** accepted (in progress)
**Date:** 2026-11-01
**Decider:** Ali (root planning docs; desk-checked 2026-11-01)

## Decision

1. `Teletype Agent Plan Beat Wispr Flow.md` + the NeuDecide addendum (repo
   root) are the **plan of record** through gate 0–7. Rules: strict gates
   (no claiming done above a gate), baseline-ways before any latency claim,
   "sufactube countUpper selfness after changes" (baseline measurements are
   re-verified after any perf change), never break delivery.
2. Plan work executes **only on branch `feat/beat-wispr`** (created from
   `main` HEAD per the plan's strict rule). Do not land plan items on
   `refactor` or `main`.
3. The streaming-approach shipped for the gap is **commit-while-you-go**:
   per-segment incremental decode + live preview (P4.1 endpointer/segmenter,
   P4.2 preview resync). This **does not retract** the 2026-09-28
   "streaming ASR rejected" learning — that applies to token-by-token live
   delta streaming; commit-while-you-go is a different mechanism and
   satisfies the two-promises constraints (guard-see-then-ignore
   mishearings; per-segment compact transports under mid-stream change).
4. NeuDecide (Apache-2.0, 55.5M ONNX command decider) is queued behind
   gate 2: hybrid decider — **tool from audio, arguments from transcript**;
   re-host to a HuggingFace or public CAGOS location when it ships.

## Context

- The 2026-09-28 streaming spike measured Parakeet TDT at `1.05 → TTP1.05`
  vs the app's ~1.3s region (`tasks/pi-tasks/streaming-asr-spike-findings.md`)
  and rejected it: "streaming cannot be justified without incremental
  re-decode".
- The Beat-Wispr plan's P4.x re-derives the same blockers and lands a
  scoped mechanism instead of the full token stream: live-preview +
  secondary tail while keeping full-decode as the committed path.
- `fcae04b` (on `feat/beat-wispr`): P4.1 (segmenter 9/9 + committer 9/9
  green; FIRST-REPAIR & guards landed) and P4.2 (live-preview UI in
  `ui/src/pill.tsx`).
- P3.9 (`1ebd339`) removed EG-1 from the catalog during this same plan
  (ADR-013).

## Alternatives considered

- **Full token-streaming preview** (delta runs on partial audio): rejected
  — needs incremental re-decode in Parakeet TDT (no state API; DFN3 re-run
  not justified), and the 2026-09-28 numbers + the root plan's
  pause-detection premise define it as scope creep.
- **Local debounce-only preview** (show tail, wait VAD): rejected — slower
  perceived speed than commit-while-you-go, same two-promises satisfaction.

## Consequences

- `PROGRESS.md` on `feat/beat-wispr` is the single source of truth for
  gate/phase state; brain keeps the summary (Decision D017), not the
  ledger.
- P5.4–P5.7 (audio processing, VAD-1, split-loop avoidance, per-compact
  quality) remain **unstarted** — next batch after gate 4 evidence.
- P3.8 (engine bake-off doc `docs/decisions/engine-defaults.md`) is missing
  as of 2026-11-01 → gate 3 cannot be marked complete until it lands.

## Evidence

- Root planning docs (desk-checked 2026-11-01; both present in the repo
  root).
- `fcae04b`: P4.1/P4.2 implementation, tests green per the commit message.
- `1ebd339`: P3.9 catalog removal.
- `PROGRESS.md` on `feat/beat-wispr` (gate state as of 2026-11-01):
  gate 0 5/5 P0 done (one baseline row outstanding), G1–G2 drained, P3
  7/9, P4.1/P4.2 done, P5.1/P5.3 done.
- `BEFORE_PASTE`/`BEFORE_RESTORE` fixed-sleep deferral nodes at
  `crates/teletype-core/src/injector.rs` L46/L48 and
  `crates/teletype-desktop/src/dictation.rs` sleeps (plan P1.1/P1.2).
