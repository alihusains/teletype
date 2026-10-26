# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Tauri 2 desktop app: React 19 + TypeScript + Vite (UI), Rust workspace (core, inference, desktop crates). The UI runs in a WebView shell; all AI inference is on-device.

## Users

Anyone who types a lot. Professional writers, developers, knowledge workers, and privacy-sensitive professionals all benefit. The primary job is getting words out of the head and into the focused app faster than typing, without sending audio to a server.

## Product Purpose

Teletype is a local-first cross-platform voice dictation app. Hold a hotkey, speak, and your words are transcribed on-device, tidied up, and typed into whatever app your cursor is in. No cloud, no account, no subscription. Success means the user dictates more than they type, and the output reads like something they would have written by hand.

## Positioning

Three things a neighboring product cannot truthfully copy at the same time:

1. **Fully private, no cloud, no account.** Audio and text never leave the machine. No login, no tier, no paywall.
2. **Free and open source.** No subscription, no word limits, no trial. The code is public.
3. **Local AI polish, not just transcription.** An on-device LLM turns speech into clean prose, real lists, and structured text. Not a cloud API call.

Plus: AutoText snippets (speak a phrase, get a full sentence or address) and blazing-fast on-device inference (Parakeet TDT and Whisper).

## Operating Context

- **Workflow:** User is working in any app (editor, browser, terminal, email client). They hold a hotkey (macOS: Cmd+Shift+Space, Windows: Ctrl+Shift+Space), speak, release. Text appears at the cursor.
- **Environment:** Fully offline after one-time model download. Works on a plane, in the subway, in an air-gapped office.
- **Setup:** Onboarding wizard asks for Microphone and Accessibility permissions (macOS) or equivalent (Windows). User downloads a dictation model in Settings.
- **Documents:** Transcripts are written to a day-wise folder of .md files the user chooses. No server-side storage.
- **Tools:** The app itself is the tool. No companion web service, no mobile app (yet).

## Capabilities and Constraints

**Capabilities (confirmed):**
- On-device speech-to-text: Parakeet TDT v3, Whisper (Tiny through Large v3 Turbo)
- Deterministic cleanup: capitalization, punctuation, filler-word removal
- Optional on-device LLM polish (styling, structure, context control)
- AutoText snippets: voice-triggered and typed /shortcut expansion
- Text transforms: sentence case, title case, lower/upper, strip markdown, trim whitespace
- Personal dictionary: learn misheard terms by voice or by typing
- Style profiles: casual, professional, structured; per-app routing
- Insights dashboard: words dictated, speaking speed, streaks, time saved
- Selection transforms with global shortcuts
- User-created vocabulary packs
- History screen: recent dictations, copy or delete
- Plain-text archive: day-wise .md files in a user-chosen folder

**Constraints (binding):**
- No new cloud dependencies. The app must remain fully functional offline after initial model download.
- No account or subscription. No login, no tier, no paywall. This is a product decision.
- Cross-platform: macOS and Windows first with equivalent features. Linux is out of scope.

**Open decisions:**
- Linux support (not currently in scope)
- Mobile app (planned but not started; see mobile-app-plan.md)

## Brand Commitments

- **Name:** Teletype
- **Tagline:** "Talk instead of type. Private, fast, and entirely on your machine."
- **Voice:** Plain, direct, no hype. The README's "Why local" section sets the tone: factual, slightly defiant about cloud dictation, warm about the user's control.
- **Identity constraint:** The product is private-first. Any marketing or UI copy that implies cloud dependency, data collection, or a free tier that could change is a violation of the product's core claim.

## Evidence on Hand

- README.md: full product description, comparison table, quick start
- usp.md: verified competitive sheet (do not describe as "at parity with Wispr Flow")
- PROJECT_BRAIN.md: detailed feature history, test status, architecture decisions
- evals/ and crates/teletype-eval/: ASR benchmark numbers (142 ms p50, micro-WER 0.056 on synthetic corpus)
- No customer testimonials, no press coverage, no case studies exist. Do not fabricate any.

## Product Principles

1. **Private is the product.** Privacy is not a feature toggle; it is the mechanism. Every design decision should reinforce that the user's words stay theirs.
2. **Speed is respect.** The user is mid-thought. Latency between releasing the hotkey and seeing text is the product's most visible quality signal.
3. **Output should read like the user wrote it.** Cleanup, polish, and transforms exist to make spoken text feel like written text, not to make it sound like an AI wrote it.
4. **No dark patterns, no friction.** No account, no onboarding wall, no "premium" tier. The app works after permissions are granted and a model is downloaded.
5. **Local is a constraint, not a compromise.** The on-device requirement shapes what is possible. Design within it; do not design around it.

## Accessibility & Inclusion

- Hotkey-based interaction requires a physical keyboard. No voice-only trigger currently.
- The app requires Accessibility permission on macOS to type into other apps. This is a system-level constraint, not a product choice.
- No specific accessibility requirements have been established beyond standard WCAG for the settings UI.
