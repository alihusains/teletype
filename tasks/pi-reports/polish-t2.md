# T2 — Zero-edit rate

Status: verified in main tree.

Added `ZeroEditRate` to the 7-day insights payload in `crates/teletype-core/src/insights.rs`: settled takes require `polish_skipped == false && observed.is_some()`, zero-edit means `changed_word_count(pasted, observed) == 0`, and the rate is `100 * zero_edit / settled`, rounded to 1 decimal (`None` when denominator is 0). Per-app top-3 breakdown by settled count included. No schema change: `polish_skipped` and `app_id` already exist.

UI: `ui/src/screens/InsightsScreen.tsx` renders a QualityCard row with an honest "—" when there is no settled data.

Verification: 52 insights tests; final combined core 1368 lib, vitest 57 files / 587 tests, tsc clean. No brain change needed.
