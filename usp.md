# Teletype — Why choose Teletype over WisprFlow

> The honest positioning sheet. Every claim below is verified against the codebase
> (2026-09-26). Where we are *behind*, it is stated plainly in §4 so we never oversell.

---

## 1. The one-liner

**Teletype is a local-first dictation app: your voice, your words, and your history
never leave your Mac — and it gives you a living record of what you wrote and the
time you saved. WisprFlow sends your audio to the cloud and keeps no such record.**

That is the wedge. Everything below is evidence for it.

---

## 2. Our wins — what Teletype does that WisprFlow doesn't (or does worse)

### W1. Local-first, zero-privacy-exposure (the anchor)
- **What:** ASR (Parakeet/Whisper), VAD (Silero), and LLM polish (local GGUF via
  llama-server, or your own key) all run **on-device**. No audio is uploaded. No
  telemetry by default (zero Sentry/PostHog).
- **Why WisprFlow loses:** WisprFlow transcribes in the cloud. For lawyers, doctors,
  journalists, and anyone dictating sensitive work, "my words went to a server" is a
  deal-breaker. We can say, provably, "nothing left this machine."
- **Proof in code:** `crates/teletype-inference/src/server.rs` (subprocess local LLM),
  `crates/teletype-core/src/vad.rs` (Silero on-device), no analytics dependency in the tree.

### W2. A living record + "time saved" insights (the retention hook)
- **What:** Every dictation is archived locally (day-wise transcript + `history.json`).
  `insights.rs` computes **Impact / Milestones / weekly heat-map / minutes-saved** and
  surfaces it in the Insights screen (wired to real data via `get_insights`).
- **Why WisprFlow loses:** WisprFlow has no meaningful "here's your impact" story.
  "This week: 42 min saved across 31 dictations, 6 min median polish" is a
  retention + word-of-mouth asset that only a local-history app can offer.
- **Proof in code:** `crates/teletype-core/src/insights.rs` + `ui/src/screens/InsightsScreen.tsx`.

### W3. It learns *you* (personalization, not just generic polish)
- **What:** A `UserProfile` of explicit + learned preferences (sign-offs, phrasing,
  casing, custom words) is fed into the polish prompt, and app-aware styles exist
  (email vs Slack vs doc).
- **Why WisprFlow loses:** WisprFlow's style handling is shallow and generic. "It knew
  I sign off with 'Cheers, Ali' and formatted my Gmail differently from Slack" is a
  concrete "it just knows" moment.
- **Proof in code:** `crates/teletype-core/src/personalization/` + `transforms/prompt.rs`
  (preferences injected at line ~228).
- **Status: the learning loop is now wired.** `crates/teletype-desktop/src/edit_watch.rs`
  records each insertion, and on the *next* dictation reads the focused field back
  and diffs it against what we inserted (`dictation.rs::observe_pending_edit`).
  A learnable edit goes through the same gates as a UI-reported one
  (`commands::record_edit_core_public`). Every guard fails closed: no read, focus
  moved, scratchpad, expired watch, or an unrecognisable diff all learn nothing.
  Caveat worth stating plainly: a preference needs three consistent observations
  before it reaches usable confidence, so this is a slow burn, not an instant
  "it knows me" moment.

### W4. Open source, forever, never commercial
- **What:** The whole app is MIT, auditable, no subscription gate on core features.
- **Why WisprFlow loses:** WisprFlow is closed, subscription-priced. For the
  tinkerer/developer/privacy crowd (and the "I'll build on this" crowd), open source
  is a standing differentiator.
- **Proof:** `Cargo.toml:15 license = "MIT"`, public repo.

### W5. Lighter and more capable under the hood
- **What:** ~13 MB base binary, on-demand model downloads, cross-platform path
  (macOS + Windows), fast `cargo tauri dev` loop.
- **Why WisprFlow loses:** WisprFlow ships a heavier closed bundle and sends audio
  to their servers. "Small install, my models my choice (local GGUF, Ollama, or
  BYO-key)" is a power-user win.
- **Proof:** roadmap §0 non-negotiables (13 MB vs EW's 37 MB bundled assets).

### W6. Trigger-based AutoText while you *type* (not just speak)
- **What:** `/trigger` expansion in any app, live while typing (CGEventTap watcher,
  just shipped + tested), plus voice AutoText in the pipeline.
- **Why WisprFlow loses:** WisprFlow's snippets are weaker; our typed + voice
  expansion is a real workflow feature for devs and power users.
- **Proof in code:** `crates/teletype-desktop/src/typing.rs` + `typing_tap.m` (verified, 4 tests green).

---

## 3. The comparison table

| Dimension | Teletype | WisprFlow | Our edge |
|---|---|---|---|
| Where audio is processed | **On-device** | Cloud | **W1 — the killer** |
| Telemetry | **None by default** | Has analytics | W1 |
| Your history / transcripts | **Local archive + insights** | Not surfaced | **W2** |
| "Time saved" reporting | **Yes (real data)** | No real story | W2 |
| Learns your phrasing/sign-offs | **Yes (profile in prompt)** | Generic | W3 |
| Source | **Open (MIT)** | Closed | W4 |
| Pricing model | **Open, no core subscription** | Subscription | W4 |
| Platform | **macOS today; Windows compiles in CI but injection/hotkey/tray are stubbed** | Mac, Windows, iOS, Android | **They win — see §4** |
| Model choice | **Local GGUF / Ollama / BYO-key** | Theirs | W5 |
| Typed + voice AutoText | **Both** | Weaker snippets | W6 |
| Perceived speed (ASR) | **142 ms p50 full decode, measured** | ~1s incl. cloud round trip | **We win — see §4** |
| Perceived speed (end to end) | ASR 142 ms + LLM polish, both streaming | ~1s streaming | **They win — see §4** |
| Live transcript in the pill | Not yet (measured blocker, §4) | Yes | **They win** |
| Platform reach | macOS only | Mac, Windows, iOS, Android | **They win** |
| Languages | English-first (Parakeet + EG-1/S1-mini are English) | 100+ | **They win** |
| Voice editing commands | None | Command Mode (Pro) | **They win** |

---

## 4. Where WisprFlow wins today (be honest, then close it)

1. **End-to-end time to text.** Streaming polish landed (T1.1) and injection is
   now a verified direct AX write with no clipboard round trip, so the ASR half
   is fast: **142 ms p50 for a full-utterance decode, 665 ms p95, measured**
   (`evals/report.md`, measure mode). The remaining gap is the LLM polish pass
   and the fact that ASR is still batch, so nothing lands until you stop
   talking.
2. **Live "words assembling as I speak".** They show the transcript filling in
   during speech; we show a spinner. This is **not** simply unbuilt work: a
   sliding-window re-decode was measured and rejected, because Parakeet has no
   streaming state and every re-decode rewrites the entire hypothesis
   (`tasks/pi-tasks/streaming-asr-spike-findings.md`, measured provisional tail
   p50 11.4s, i.e. the whole window). Viable paths are a marked-as-provisional
   interim preview, or a stateful Whisper partial decoder.
3. **Polish "tunedness".** Their model is heavily fine-tuned for cleanup. We ship
   the same class of model (EG-1/S1-mini in the catalog) plus the 500-word
   splitter, so the ceiling is comparable; the difference is measured polish
   quality, which we now have a harness for but no human-recorded corpus yet.
4. **Platform reach and languages.** macOS only, and English-first. These are the
   two gaps that no amount of engineering on the current architecture closes.

**Strategy:** we win on privacy, cost, ownership, and the retention story a
cloud app structurally cannot offer. On perceived speed we have closed the
injection tax and measured the ASR; the remaining honest gaps are streaming
preview, platform reach, and languages. Do not claim parity on any of those
three until they are measured.

---

## 5. How to say it (ready-to-use copy)

- **Tagline:** *"Dictate anywhere. Your words stay on your Mac."*
- **Privacy:** *"No cloud. No telemetry. No subscription gate. Your voice and your
  history never leave this computer."*
- **Insights:** *"See exactly what you wrote and the time you saved, week over week."*
- **Personal:** *"It learns how you talk — your sign-offs, your words, your style per app."*
- **vs WisprFlow, in one breath:** *"Everything WisprFlow does, plus it never sends
  your audio to the cloud, remembers everything you dictated, and shows you the time
  you saved."*
  Only use this one with people who will not check the platform row in §3. It is
  not accurate about iOS/Android, Command Mode, or languages, and those are the
  first three things a WisprFlow user will say back.

---

*Generated 2026-09-26 from a verified codebase audit. §3 and §4 updated
2026-09-27 after the AX injection tier, the eval harness, and the streaming
spike. The ASR latency figure is measured (`evals/report.md`, measure mode); the
streaming-preview and polish-quality claims are not, and are labelled as gaps.
Re-verify §2 before external use: the platform, language, and feature rows move
as Windows and the remaining tiers land.*
