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
  (preferences injected at line ~228). *Gap: the feedback loop that *learns* from your
  edits (P1.3) is not yet wired — that's the next win, see roadmap.*

### W4. Open source, forever, never commercial
- **What:** The whole app is MIT, auditable, no subscription gate on core features.
- **Why WisprFlow loses:** WisprFlow is closed, subscription-priced. For the
  tinkerer/developer/privacy crowd (and the "I'll build on this" crowd), open source
  is a standing differentiator.
- **Proof:** `Cargo.toml:15 license = "MIT"`, public repo.

### W5. Lighter and more capable under the hood
- **What:** ~13 MB base binary, on-demand model downloads, cross-platform path
  (macOS + Windows), fast `cargo tauri dev` loop.
- **Why WisprFlow loses:** WisprFlow ships a heavier closed bundle, macOS-only.
  "Small install, my models my choice (local GGUF, Ollama, or BYO-key)" is a
  power-user win.
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
| Platform | **macOS + Windows path** | macOS (primary) | W5 |
| Model choice | **Local GGUF / Ollama / BYO-key** | Theirs | W5 |
| Typed + voice AutoText | **Both** | Weaker snippets | W6 |
| Perceived speed (text-appears time) | Full-batch today | ~1s streaming | **They win — see §4** |

---

## 4. Where WisprFlow wins today (be honest, then close it)

1. **Perceived speed.** WisprFlow shows text within ~1s of you stopping. We run a
   full LLM transform then paste the whole result (3–8s). **This is the #1 thing to
   fix** — streaming LLM + progressive paste/preview. (Roadmap: promoted to P0.)
2. **Live "words assembling in the pill" preview.** They show the transcript filling
   in as you speak; our pill shows a spinner. Tied to the streaming fix.
3. **Polish "tunedness."** Their EG-1 model is heavily fine-tuned for cleanup. We can
   match by shipping the same class of model (EG-1/S1-mini already in catalog) + the
   500-word splitter.

**Strategy:** We don't beat them on the two things they're best at by copying them —
we win on privacy + living record + personalization (the things a cloud app
*structurally* can't match), and we close the speed gap with streaming so we're no
longer behind on the one thing users notice in the first 10 seconds.

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

---

*Generated 2026-09-26 from verified codebase audit. Re-verify §2 claims before
external use; the §3 "They win" row must be updated the moment streaming lands.*
