# T3b — Profiles UI and honest capability messaging

Status: verified in main tree.

Added `ui/src/lib/profileLabels.ts` (shared vocabulary, budgets, effective-budget rule, per-app table, reused capability map), `ui/src/components/ProfilePicker.tsx`, and new/updated settings, Transforms, and Onboarding copy. Cleanup-only models enable only Clean/Off and show the fallback banner; unsupported profiles are disabled and show effective Clean. Onboarding states the model contract and recommends a capable model for Structured. No new network calls.

Verification: final combined tsc clean; vitest 57 files / 587 tests, including 5 new profile UI tests. Lead decisions: SettingsScreen's settings type lacks the two new optional fields (owned files use optional extras); Transform on cleanup-only remains enabled in core `decide_profile` although the UI disables it; T7 reconciles the lightweight intent selector. No brain change needed.
