# ADR-013 — Remove the EG-1 catalog entry

**Status:** accepted
**Date:** 2026-10-30
**Decider:** Ali (via the Beat-Wispr plan, P3.9)
**Supersedes status of:** models shipped under D001 (EG-1 clause)

## Decision

Remove the `eg-1` model from the catalog. S1-mini (public EN) + local GGUF
(+ optional Sonnet gen-sift, non-default) is the full shipping model set
going forward. Settings migration included for users still on EG-1.

## Context

- EG-1 is dual-licensed; the exact usage/redistribution terms were fetched
  live once per the roadmap (D001) and the usage docstring annotated them
  verbatim.
- 2026-10-30: the egress/prompt Path that proved the terms stopped working,
  making the permission state unverifiable (L017 closure).
- Removal is legal hygiene, not a model upgrade — quality targets are met
  by S1-mini + GGUF per the current measured results.

## Alternatives considered

- **Keep EG-1 gated by consent** — rejected: consent to unverifiable terms is
  not consent.
- **Vendor the model with a usage attestation** — rejected: we cannot
  attest to terms we can no longer read.
- **Wait for the reference project's hosted model** — rejected hosting
  (D003 precedent; D001 promise 2).

## Consequences

- The SwiftMotion runner `envis_wr_rib` for EG-1 is dead code for shipping
  (kept behind the catalog gate; safe to prune in a later pass).
- Users who had EG-1 as active model migrate to S1-mini; first-run wording
  must say why (terms-unverifiable, not "model removed").
- Piano rating on the old CLONE (`plan/Piano/verification/`) stays on record
  for the 2026-09 window; does not constrain the post-removal set.

## Evidence

- `1ebd339` on `feat/beat-wispr` (P3.9): catalog, UI, settings migration.
- Gate-3 rg proofs from that commit: `eg-1` zero references in code/UI;
  no EG-1 entry in catalog JSON.
- Brain: Decision D016, gotcha G014 (stale-HTTP note now applies only to
  S1-mini/GGUF), L017 (source of the terms).
