# Teletype macOS Roadmap

This is the plan for the macOS side of Teletype: hardening what already works, closing the
gaps against the reference product (EnviousWispr, `enviouswispr/EnviousWispr/`), and
shipping the remaining differentiators. macOS is the reference platform: it is the one
that is launched and shipping, so its roadmap is about *completing* and *hardening*, not
*porting*.

The plan of record for priorities is `roadmap.md` (P0-P5 + the 2026-09-26 reprioritized
Tiers). This file adds the macOS-specific shape, the EW-parity gaps, and the
platform-specific learnings. `roadmap.md` stays the source of truth for *what* and *in
what order*; this file is the macOS *how*.

---

## 1. Where macOS stands today (2026-09-26)

The macOS app is feature-rich and working (see `PROJECT_BRAIN.md` section 1). Recent
committed milestones:

- P0 Polish batch (subprocess llama-server, OpenAI-compatible provider, keyring secrets,
  expanded catalog) in `3fa2793`; verified P0 fixes on `main` (`9163631`, `e2a9ec9`).
- Personalization feedback loop wired (`38843a8`, T2.1).
- Privacy front-page claim (onboarding step + Home badge) in `b7ff674` (T3.2).
- Autotext placeholders + dictionary import/export + recovery spool in `9a1969d`
  (P3.7/P3.9/P2.3).
- Rule-based polish gate for local LLM providers in `903eab0` (P5.1).
- Emoji restore + per-app language overrides in `3d9f6f7` (P3.2/P3.3).
- Typed AutoText while typing (L007), true network-level streaming LLM + live pill
  preview (L008), VAD auto-stop with the sample-rate fix (G010), 5 pill styles.

The heart path (hotkey -> capture -> ASR -> deterministic cleanup -> paste) is solid on
Apple Silicon. The remaining work is: parity features, reliability hardening, and the
Apple-Silicon-specific optimizations.

---

## 2. The two promises (macOS is the reference for these)

1. **Sub-second transcription.** Mac fleet medians (PostHog, referenced by EW): 0.61s
   no-polish, 1.65s on-device polish. This is the bar the whole pipeline is judged on.
   Polish is OUTSIDE the sub-second bar.
2. **Text lands correctly in the focused app.** The macOS tiered paste cascade is the
   reference implementation: Tier 1 AX direct write with character-count verification ->
   Tier 2 clipboard snapshot + CGEvent Cmd+V + 200ms wait + guarded restore -> AppleScript
   -> AXPress on a Cmd+V menu item -> clipboard-only + notice. Every round logs which tier.

---

## 3. Phases

### Phase M1: Complete the reprioritized tiers (T1-T4)

Track `roadmap.md`'s 2026-09-26 reprioritized plan. macOS-specific notes:

- **T1.3 (500-word transcript splitter):** the biggest perceived win. Long dictations
  degrade quality without it; prerequisite for polishing long takes. Not started.
- **T2.1 (personalization loop):** wired (`38843a8`); verify the diff-based signal
  extraction end-to-end.
- **T2.2 (context-aware auto-style):** context + styles exist; the auto-routing is the
  gap (Gmail -> email polish, Slack -> casual, without asking).
- **T3.3 (ITN phase 2b):** spoken -> written numbers/dates/money. 2a done, 2b deferred.
- **T3.4 (emoji restore + language detection):** emoji restore done (`3d9f6f7`); language
  detection signal still needed (Q006).

### Phase M2: EW parity (the features the reference has that Teletype lacks)

These are the gaps against the shipping macOS reference. Each has an EW source to port
from (see `roadmap.md` "Feature -> EW source map").

- **Apple Intelligence connector (P1.4):** the zero-download system model on macOS 26+.
  This is macOS-exclusive and a real differentiator. Groundwork started in `9163631`.
- **Streaming ASR + live preview (P2):** the reference's Parakeet streaming is a
  sliding-window decoder (11s chunk / 1s hypothesis). This is the hardest macOS feature
  and is explicitly deferred; it powers the "Reading Well" pill's live-preview well.
- **Paste cascade hardening (P2.1):** the AX-write direct-write tier with
  character-count-change verification. Teletype currently uses clipboard-paste; the
  direct-write tier is the faster, more reliable path for standard edit fields.
- **Model delivery checksums (P1.5):** SHA-256 for all models, not just EG-1.

### Phase M3: Reliability + Apple Silicon optimization

- **Model residency / unload policy (P2.4, model unload timer):** the reference measured
  that launchd reaps the idle Parakeet helper (1.54 reclaims per active user-day, 48% of
  active user-days affected; warm-respawn p50 459ms / p95 1.88s / p99 12.8s). Teletype
  runs the engine in-process, so it avoids the reaper, but the idle-unload setting still
  needs the "getting dictation ready" pill state.
- **Escape Recovery spool (P2.3):** done (`9a1969d`). Verify it returns the last
  successful text on failure (product invariant 5).
- **Deterministic failsafe:** when every AI provider fails, the deterministic output must
  still be useful. The reference has a whole epic on this; Teletype's "AI is a limb, not
  the heart" invariant is the same idea.
- **Apple Silicon optimization:** ANE/Metal are the whole macOS win. Keep the ASR on the
  ANE/Metal path; the thread-pinning lesson from the Windows roadmap does NOT apply the
  same way (no hybrid-core oversubscription on Apple Silicon), but model residency and
  prewarm still do.

### Phase M4: Distribution + release hardening

- **Notarization + Gatekeeper (roadmap P3.5):** every build signed, notarized,
  Gatekeeper-checked. The reference's `scripts/build-release-dmg.sh`,
  `attest.sh`, and `signing/` are the patterns to follow.
- **Updater:** Sparkle (the reference uses it) or Tauri updater, with channel separation
  and hash verification.
- **Opt-in crash reporting (P2.4):** content-free, redacted, off by default (D003).

---

## 4. Measured learnings to carry (from the macOS reference)

- **Model residency is a measured pain, not abstract.** launchd reaps the idle helper by
  design; the warm-respawn absorbs it but shows no UI, so the user sees a silent
  multi-second dead app. Pinning with `xpc_transaction_begin()` was researched and
  REJECTED. Teletype's in-process engine avoids the reaper, but the unload-after-idle
  setting must show a "getting ready" state, not a silent stall.
- **The paste cascade is tiered and every round logs its tier.** The sacred clipboard
  contract preserves every item and every type and restores only if the pasteboard
  changeCount still equals our post-write count. Failures elsewhere must never touch
  clipboard policy.
- **Cursor-aware insertion repair runs AFTER the chain:** one caret-context read, one
  repair candidate, revalidated at each paste route's commit boundary. No caret context
  -> legacy payload (degrades to "paste the text", never breaks it). Terminal payloads
  with a newline are REFUSED (a newline in a terminal submits the command).
- **Casing at the seam needs NSSpellChecker/NLTagger** (12 languages, tiered by measured
  dictionary honesty). The no-context legacy fallback already ships, so this degrades,
  not breaks.
- **Parakeet is English-only; "auto" resolves to "en".** Whisper is the multilingual
  engine with auto-detection. The language setting is model-aware.
- **The 164K-line test corpus is the durable porting spec.** The deterministic chain
  (word correction -> filler -> emoji -> ITN -> polish -> emoji restore) is specified by
  its test corpus, not by its code. Port fixtures before features.
- **Apple Intelligence is macOS-exclusive and the zero-download polish tier.** No Windows
  equivalent exists; on Windows the default local polish is EG-1 instead.

---

## 5. What Teletype does differently from the reference

- **License:** Teletype is MIT, open-source-forever, personal (D001). The reference is
  GPLv3 and commercial (Envious Labs). Do not pull in GPL code or re-host the reference's
  EG-1 distribution build.
- **Engine:** Teletype uses whisper.cpp (Parakeet via parakeet-sys, Whisper) rather than
  the reference's FluidAudio/WhisperKit. The model *weights* are portable; the runtimes
  differ.
- **Architecture:** Rust core + Tauri, platform-neutral `teletype-core` with a
  `Platform` trait. The reference is Swift with 17 modules. The heart path is the same;
  the platform surface is what differs.

---

## 6. Open decisions (macOS)

- Default provider priority chain when Apple Intelligence and the local server both exist
  (Q003).
- The "Detected <Lang>. Lock it?" chip after auto-detect (Q006) needs the detected-language
  signal from ASR.
- Streaming ASR: defer to a later milestone or build the sliding-window decoder now? The
  reference shipped it as a limb that can fail without affecting the final result.
- llama-server distribution: download on first local-model use vs optional installer (Q002).
