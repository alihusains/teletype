# T7 — First-run calibration

Status: verified in main tree.

Added `ui/src/components/CalibrationStep.tsx`, a skippable onboarding step with email/Slack/note prompts, editable samples, dictation via the existing path, and a 60-second flow. Added `crates/teletype-desktop/src/commands/calibration.rs`: generates samples through the same `run_transform_blocking` engine as `test_polish_prompt`, offers only profiles the loaded model can honor, returns deterministic text plus a structured error on model failure, and never writes samples to History. No model is downloaded or loaded implicitly.

Saved data: `calibration.json` contains `{completedAt, skipped, profile}` only. Save sets `polish_profile` to the chosen profile and per-app defaults (email Structured; chat/social Natural; coding/terminal Off). Skip records a marker and leaves defaults untouched.

Verification: calibration filter 7/7; final combined desktop 607, vitest 57 files / 587 tests, tsc clean, fmt clean. Assumptions: model setup must have warmed a provider earlier in onboarding; calibration degrades to explanation + skip otherwise. Per-app defaults follow §5.3 (AI chat Natural), which conflicts with §8.2's Cursor → Clean suggestion. No brain change needed.
