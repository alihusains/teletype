# Task: T3.2 — Privacy as a front-page claim (onboarding + privacy badge)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Files you may edit
- `ui/src/screens/OnboardingScreen.tsx`
- `ui/src/screens/HomeScreen.tsx` (privacy badge/row only)
- `ui/src/pill.tsx` or the pill component (badge only, if a pill surface fits)
- `ui/src/lib/` (a small shared `PrivacyBadge` component if you create one)
- Do NOT edit any Rust file. This is a copy + UI task.

## Why
Roadmap T3.2 (Tier 3, "zero code risk, positions the whole product"): the
anchor differentiator vs WisprFlow is that audio, transcripts, and history never
leave the Mac (or PC). That claim is true today (local ASR/VAD/LLM, zero
telemetry — verify before writing the copy) but it is not visible anywhere in
the product. This task makes it the first thing a new user sees.

## Read first
1. `ui/src/screens/OnboardingScreen.tsx` (whole file): existing steps, copy
   tone, component patterns, i18n approach if any.
2. `ui/src/screens/HomeScreen.tsx`: where a status row or badge would sit
   (there is a "Time saved" card from T3.1 — do not touch it).
3. Verify the claim before writing it (do this and paste the evidence):
   - `grep -rn "sentry\|posthog\|analytics\|telemetry" crates/ ui/src --include="*.rs" --include="*.ts" --include="*.tsx" -i`
     (expect: no telemetry dependencies; if you find any, STOP and report —
     the copy must be true)
   - Confirm the local providers: `grep -n "local-server\|openai-compat" crates/teletype-desktop/src/commands.rs | head`
     The honest claim must cover the BYO-key path: "when you use a cloud
     provider, only the text you dictated is sent to that provider, under your
     key". Write copy that is true for BOTH paths.

## Build
1. Onboarding: add (or rework the first) step to lead with the privacy claim:
   - Headline: "Your voice never leaves this device."
   - Body: 2-3 sentences max. Local transcription, local polish, local history.
     One honest sentence about the optional cloud-provider path.
   - A small "On-device" badge/shield visual (CSS only, no new image assets —
     use an inline SVG or a unicode shield with the existing design tokens).
2. Home screen: a persistent, dismissible-once privacy row or badge
   ("On-device · nothing uploaded") near the time-saved card, using the same
   visual language. Dismissal persisted in localStorage (check how other UI
   prefs persist in this app; follow the house pattern).
3. Copy rules (Ali's register): warm, plain English, no hype, no em dashes.
   Every factual sentence must be verifiable in the codebase. No "military
   grade", no "100% private" absolutes — "your audio is processed on this
   device and never uploaded" is the claim shape.

## Out of scope
- No Rust changes, no new settings, no telemetry (there is none to remove —
  verify and report).
- No marketing site copy; product surfaces only.
- No new dependencies (no icon library; inline SVG only).

## Verification (paste real output)
1. `cd ui && npx tsc --noEmit 2>&1 | tail -3` (must be clean; note: the known
   pre-existing InsightsScreen errors, if any remain, are not yours — report
   them separately)
2. `cd ui && npm run build 2>&1 | tail -5`
3. Paste the grep evidence from "Read first" step 3 into the report.
4. Screenshot if possible (`cargo tauri dev` + screenshot the onboarding
   screen); if not possible headlessly, say so.
