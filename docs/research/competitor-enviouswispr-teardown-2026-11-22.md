# Competitor teardown: EnviousWispr (macOS) + EnviousWispr for Windows

**What the other side has built, and what "parity" means for Teletype on macOS and Windows**

Date: 2026-11-22
Method: public GitHub repositories, release notes, and the product website. **No local clone and no source reading** — every claim below is a public artifact claim, i.e. a prior, not Teletype evidence. Nothing here is copied code; the macOS and Windows repos are GPLv3 and Teletype must stay clean of it.
Sources:
- `github.com/saurabhav88/EnviousWispr` — macOS app, GPLv3, created 2026-02-18, 2,595 commits on `main`, latest release **v2.6.1 (2026-11-20)**, v2.6.0 before it (2026-11-05)
- `github.com/Envious-Labs-LLC/enviouswispr-windows` — Windows app, GPLv3, created 2026-08-24, 420 commits, **pre-release, no public artifact**
- `github.com/Envious-Labs-LLC/EnviousWispr-Android` — a third native app (Android 13+), same org
- `enviouswispr.com` — product site, help centre, pricing ("free"), download

---

## 1. Short answer

They are not ahead on the dictation heart. They are ahead on **surface area around it**: a second product (meeting notes), a polish model family they own, and a History surface that behaves like a real library. On Windows they are pre-release with no public download — that is the one place Teletype can still be first, and it is the place where losing would hurt most, because their Windows port is real (377 production tests, measured numbers, Store pipeline) while ours is a compile gate with stubbed platform layers.

The honest read: **the two products have converged on the same architecture** (record → transcribe → deterministic cleanup → polish → tiered paste, with every optional step failing open). That is validation, not a gap. The gaps are feature-visible ones a reviewer will tick off in five minutes.

---

## 2. What they have actually shipped (macOS)

| Area | EnviousWispr v2.6.x | Teletype today | Gap |
|---|---|---|---|
| Core dictation, hotkey, paste | Push-to-talk, toggle, hands-free lock; tiered paste with fallback | Same shape, tiered paste | none material |
| ASR | Parakeet Ultra (25 EU languages, ANE) + Whisper Large v3 Turbo (99+, auto-detect) | Parakeet TDT + Whisper, ANE/GPU paths | none material |
| Live preview | **Live Preview on by default**: words appear in the pill while speaking; safe no-op where unsupported | Interim text in the pill, not a streaming decoder | **real gap** |
| AI polish engines | EG-1 (own model, 3 sizes, MLX helper), S1-mini, Apple Intelligence, Ollama, BYOK OpenAI/Gemini/Claude | S1-mini / local GGUF / Apple Intelligence / BYOK | **gap on owned model**; otherwise parity |
| Polish productisation | `Show edits (Polished / Marked up / Original)`, `Restore original text`, re-polish **with preview**, live memory bar, "Best for this Mac", "won't fit" warning | Diff card, feedback learning loop, capability-scoped model menus | gap on review/undo surface |
| Deterministic layer | Filler, custom words, spoken punctuation/emoji, ITN, cursor-aware repair (ported from their Swift) | Same layer, same lineage | none |
| History | Rebuilt as a **SQLite catalog** with Dictations / Transcripts / Meetings tabs, day grouping, per-tab search, date + tab filters, bulk select, playback with waveform scrub, keep/delete audio separately from text, target app shown | History list, saved take audio + player, day-wise text archive | **real gap: browsing, search, review actions** |
| Transcripts (files) | Import audio/video (M4A/MP3/WAV/MP4/MOV/FLAC…), own card + page, docked player with speed and ±15 s, `Transcribe again` with any installed engine, macOS **Share extension** | File transcription exists; share extension, per-file library, re-transcribe with another engine | partial gap |
| **Meetings (EnviousMeet, Beta)** | Whole second product in the same app: local call + room recording, no bot, who-said-what labels with real names, mute-following in Zoom/Teams/Meet, calendar naming, notes beside live transcript, AI summaries with compare/keep, playback markers | **nothing** | **largest single gap** |
| Self-learning | "Self-Learning" card; learned-word checker bundled per EG-1 size; correction judge (8 deterministic rules) decides mishearing vs restyle | Learning loop (feedback/promote/settle/bandit) without their judge's classification quality | gap in precision, not in concept |
| Escape Recovery | Cancel keybind keeps text 24 h; Cancel button discards on purpose | Recovery spool (same idea, `9a1969d`) | none material |
| Setup | Guided flow: Welcome → Language → Engine → Polish (memory-aware recommendation) → Permissions → Key → **live practice** → film; resumable | Onboarding wizard with permissions | gap in polish/measurement |
| Model delivery | Downloads start per confirmed choice; remove/keep per engine; progress bars; background upgrades | Downloads + SHA-256 pins | partial |
| Cloud model list | Ships in app **and refreshes from their website**, so new models appear without a release; each added after 3 real dictations through the connector | Hardcoded catalog | **cheap gap to close** |
| Reliability doctrine | Public "Reliability is a feature" table: delivery survives non-critical failure, best-effort history, plain-language AI errors, no silent freeze after idle, signed/notarized, redacted diagnostics | Same invariants in the brain (D001, fail-open polish) | none — but theirs is *marketed* |
| Distribution | Signed, notarized, Gatekeeper-checked, Sparkle-style updates, website + help centre + blog | Signed/notarized pipeline, updater | none material |
| Platforms | macOS 14+ Apple Silicon; Android app; Windows pre-release | macOS 15+ AS; Windows compile-only | **Windows** |

Their own published numbers, for calibration: EG-1 93.7% pass on **their** 1,890-case cleanup exam (GPT-5.4-mini 83.8%, Gemini 3.5 Flash 92.6%) — their benchmark, their judge, stated as such. Whisper Large v3 Turbo English WER 8.63% on a frozen 1,480-clip corpus. A dictation flow quoted at ~400-800 ms transcription + ~200-500 ms polish. None of these are independently reproducible; treat them as targets, not facts.

---

## 3. How good is the work (engineering assessment)

**Strong. Above the bar for a competitor of this size, and worth learning from operationally rather than only feature-wise.**

- **Evidence discipline.** The repo carries `benchmark-results/`, `artifacts/issue-<n>-…/`, frozen corpora with clip counts, and issues that publish WER tables per arm (see #3705, a 1,480-clip, 24-language comparison against two rival engines). Their claims are usually tied to a named artifact. Where a number is theirs, they say so.
- **Fail-open architecture, stated as policy.** "The critical path is deliberately separate from every optional enhancement, so a hiccup in a nice-to-have can never swallow your words." That is the same invariant as Teletype's "AI is a limb, not the heart" — and they ship a table of it.
- **Bounded model risk.** Moving EG-1 from GGUF to a bundled MLX helper, moving both local polish helpers from TCP ports to Unix sockets in 0700 folders, and auto-exiting helpers with the app are all the right hardening calls; the memory bar and per-size recommendations acknowledge that their models are heavy instead of hiding it.
- **Test and gate culture on the Windows repo.** 34 proof + 377 production tests, a canonical `validate.ps1`, 1,000-cycle lifecycle with handle-delta accounting, and a public list of what is *still missing* before release (Store certification, clean-machine install, laptop evidence, CUDA/model licenses, security review, private beta). They refuse to call it shipped.
- **Documentation as product.** Help centre, per-feature pages, privacy pages keyed to code, and a contributing page that explains GPLv3 in plain language. This is a maturity signal a reviewer of *their* app will notice.

**Weaknesses worth exploiting.**
1. **Install weight.** EG-1 full is a 4.7 GB download / 5.5 GB memory; plus 630 MB Parakeet and ~900 MB Whisper. Their own UI warns "Won't fit right now." Teletype's 13 MB app + optional models is a genuinely better story for the median laptop.
2. **GPLv3.** Fine for them as the vendor; it caps third-party embedding and any commercial integration.
3. **One person, 17 modules, two apps, a model family, and a meeting recorder.** Velocity is impressive now; the bus factor and the maintenance load (2,595 + 420 + Android commits in nine months) are real.
4. **Windows is not real yet** — Store-only, no artifact, no public date, and their own gate list is long.
5. **Meeting notes are Beta**, and the honest risk in that feature (consent, mute-following via Accessibility, diarization across apps) is where their bug surface will be for months.

---

## 4. Windows: where the parity question actually bites

Their Windows app is a preserved .NET 8 WPF proof being replaced capability-by-capability by .NET 10 / WinUI 3. Already claimed in the production shape: WASAPI capture with device routing and 16 kHz mono conversion, global push-to-talk with **frozen-target delivery** (the app focused when recording began), Parakeet + Whisper with CPU-safe selection, a separate Whisper *preview* that cannot corrupt the final transcript, deterministic cleanup + ITN + cursor-aware repair, EG-1/Ollama/BYOK polish, tray/overlay/settings/history/dictionary/snippets/updates/diagnostics, and a Store-only distribution plan that asks the Store to update only while dictation is idle.

Measured on one machine (2026-08-26, Win11 25H2 + NVIDIA desktop): WinUI shell warm start p95 609 ms; app + Parakeet worker warm start p95 2,313 ms; combined ready working set 973–975 MB; Parakeet CPU 390 / 696 / 3,786 ms for 10 / 20 / 91.5 s fixtures; 3/3 production journey passes; 1,000-cycle handle delta 9. They explicitly list Whisper CPU latency, CUDA delivery, battery/thermal, and lower-spec laptops as **open**.

Teletype's Windows state (README + `docs/roadmap-windows.md`): compiles on a real Windows host under a CI gate; low-level keyboard hook for typed AutoText landed; sherpa-onnx Parakeet int8 is the engine; **WASAPI capture, global hotkey, clipboard injection with restore, tray, packaging, and any runtime test do not exist yet**. No Windows machine has run the app.

So the Windows comparison is not "feature parity", it is "**they have a product in review, we have a compile gate**". Their own numbers are the target list, and the cheapest way to be in parity is to borrow their *order*, not their features.

---

## 5. What parity should mean, in order

### macOS — must-have before a reviewer compares the two side by side

1. **History as a library, not a list** (L–XL). Tabs (dictations / transcripts), search, filters, day grouping, bulk select, review actions: show edits (polished vs marked up vs original), restore original, re-polish **with preview**, keep/delete audio separately. Teletype already has the rows, the audio files, and the player; this is mostly surface + two commands.
2. **Live preview worth the name** (L). Streaming/sliding-window ASR into the pill, on by default, with the "cannot run here → behaves as off" story. It is their most-touched affordance ("I can see it is working without waiting") and it is the one place our pill currently undersells the engineering.
3. **Polish review + memory honesty** (M). We have the diff card and the learning loop; add `restore original`, and a model recommendation that reads *this* Mac's memory instead of a static table. Skip training our own model for now; instead make the model we do have (Apple Intelligence / S1-mini / BYOK) legible about cost, size, and fit.
4. **Precise learn-from-edits** (M). Their judge exists to answer "mishearing or restyle?" — our loop persists far too eagerly by comparison (`BUG-001` closed the worst of it). Precision here is what makes "self-learning" a promise rather than a nuisance.
5. **File/transcript surface + Share extension** (M). Per-file cards, player with speed, `Transcribe again` with another engine, Share menu action.
6. **Remote model list** (S). Serve the cloud model list from our own endpoint with a shipped fallback; it removes a release from every provider model launch.
7. **Meeting notes — decide, do not drift** (XL). This is a second product with a different trust model (consent, diarization, mute-following, calendar). Either commit to a scoped version (record → transcript → summary, mic only, no call-audio tap) or state publicly that meetings are out of scope. Doing nothing while they market it is the worst of the three.

### Windows — parity means "it runs", then "it is trustworthy"

Order, matching what their evidence proves matters:

1. **A real run on real Windows hardware** before any feature work: WASAPI capture, hotkey via the existing hook, clipboard + `SendInput` injection with restore, tray, MSIX packaging. Until a Windows machine has run the app, every Windows feature claim is a prior.
2. **Their measured budget as our acceptance test**: sub-second for a 10 s clip on CPU, warm start under ~2.5 s, working set under ~1 GB, zero handle growth over 1,000 cycles.
3. **Frozen-target delivery** (paste into the app focused at record start) — they call it out as product shape; it is a one-line user expectation and a real engineering constraint.
4. **The same deterministic layer** already in `teletype-core` (ITN, spoken punctuation, custom words, cursor repair) — this is our home advantage; it is platform-neutral and already written.
5. **Preview that cannot affect the final transcript**, i.e. a separate decode path, exactly as they do it.
6. Store vs direct download: they chose Store-only for signing and updates. We do not have to; Tauri's updater plus an MSIX is enough, but the decision should be made before packaging work starts.

### Explicitly not worth chasing

- Their 4.7 GB polish model. Our differentiator is the small app and the user's choice of model; a memory bar is the honest version of that story, not a bigger download.
- Diarization and mute-following in v1 of any meeting feature.
- Matching their release cadence by shipping half-finished surfaces; their cadence works because the review affordances (preview, restore, compare) exist to catch mistakes.

---

## 6. Open questions for the product owner

1. Do we ship a scoped meeting-notes feature, buy one, or state it is out of scope? (blocks any macOS parity claim to a reviewer comparing feature lists)
2. Is streaming ASR in scope for the next macOS release, or does the roadmap keep it deferred? (it is the visible-not-verifiable gap; `docs/roadmap-macos.md` M2 still defers it)
3. Windows: is the goal "first real Windows dictation release" or "parity with macOS on Windows"? The first is a package + capture + injection job; the second is a quarter.
4. Do we adopt their public "Reliability is a feature" table format for our own README? We already honour the invariants; they are simply the ones telling users.
