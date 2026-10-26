# L049: Insights numbers have three different scopes — label each one

**ID:** L049
**Date:** 2026-10-26
**Agent:** architect (PandaCode) + Insights backend worker
**Project area:** teletype-core / insights.rs, teletype-desktop / commands/history.rs, ui/src/screens/InsightsScreen.tsx
**Status:** verified
**Evidence:** `get_insights` implementation in `crates/teletype-desktop/src/commands/history.rs`, `Insights` struct in `crates/teletype-core/src/insights.rs`, and the redesigned screen + `ui/src/test/insightsProof.test.tsx`

## Problem

A dashboard that mixes all-time, rolling-window and "last 7 days" numbers without
saying which is which reads as wrong. Users compare two figures that were never
comparable ("words this week" vs "filler words removed" which is a lifetime
counter), and a redesign that "fixes" a number by windowing an all-time counter
would silently destroy data.

## The three scopes

1. **Selected window** — rolling, not calendar: week = 7×86 400 000 ms,
   month = 30 d, year = 365 d. Covers `total_words`, `top_apps`,
   `app_categories`, `top_phrases`, `top_words`, `habit_words`, `top_days`,
   `hour_bands`, `impact`, `prior_window_words` / `words_change_pct`.
2. **Fixed last 7 days** — `active_days_last_7`, `learning.edit_rate_7d`,
   `learning.terms_learned_week`. These do not follow the range control.
3. **All time, no timestamps** — `records`, `milestones`, `heatmap`,
   `longest_streak_days`, `filler_removed`, `autotext_expansions`,
   `top_fillers`, `top_autotext`, `dictionary_words`.
   `usage.rs` persists the filler/AutoText counters as plain totals, so they
   *cannot* be windowed. Rendering them as "this week" would be a lie.

`learning.daily` and `learning.accuracy` are also all-time series (last 30 days
of edit activity, up to 10 replay points) and are independent of the range.

## Non-obvious details that bite

- **`stats::day_start_ms` is UTC**, not local:
  `(secs - secs % 86_400) * 1000`. `wordsToday` therefore rolls over at UTC
  midnight. A 2026-10-26 audit claimed it used local midnight; that is wrong.
  Do not repeat it in copy, and do not "fix" it without deciding the product
  behaviour first.
- **`RankedItem.share` is relative to the most-used item**, not a percentage of
  the total. Rendering `share * 100` as "% of all dictations" overstates every
  row except the first.
- **`impact.words_per_minute` is `null`** until a take longer than
  `MIN_DURATION_FOR_RATE_MS` (1200 ms) exists. The hero shows `—`, and the
  "times faster" multiple falls back to a 150 wpm *estimate*, not a
  measurement. Keep the estimate labelled.
- **`learning.accuracy` WER values are 0..1 fractions**, rendered ×100 as
  percentages; lower is better.
- The UI range buttons offer week/month/year/lifetime/custom. The backend also
  computes a `today` range internally, but no button selects it.

## Reusable lesson

Before adding a metric to a dashboard, write down (a) which stored field feeds
it, (b) whether that field has timestamps, and (c) which window it belongs to.
Then put that scope in the UI next to the number. If a field has no timestamps,
it is all-time by construction — say so instead of inventing a window.
