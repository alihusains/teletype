# T4 — Deterministic backtrack resolver

Status: verified in main tree.

Added `crates/teletype-core/src/transforms/backtrack.rs` (160 unit tests) and wired it in `pipeline.rs` at stage 0.55: voice-only, English-gated, after filler/self-repair, before ITN/gate/model. `raw_text` remains the ASR output; the resolved text reaches the model and is recorded as an `EditKind::Correction` stage.

Safe fallback: when a punctuation-free clause is not an unambiguous fact (number, day/time, proper name, code-like token), the resolver keeps the take unchanged. Comma removal after a replacement is limited to corrected names, so original punctuation such as `thursday, at ten` survives. Guardrails prevent suffix deletion (e.g. `9:15 @ the office`) and sentence-final stop loss.

Added 8 adversarial behavior cases and matching correct-output fixtures; updated the eval self-test to accept >=17 corpus cases and skip content-drop detection for backtrack cases (retraction intentionally removes words).

Verification: backtrack 160/160; pipeline 74/74; final combined core 1368 lib, eval self-test PASS, fmt clean. No brain change needed.
