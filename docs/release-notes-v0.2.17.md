# Teletype v0.2.17 release notes (draft)

Status: draft for the release captain. Nothing here is published until Ali
approves the tag.

## What ships

### Before you upgrade (behaviour changes worth knowing)

- Dictation audio is kept by default. Each take is written as a 16 kHz mono WAV
  in the app's recordings folder so it can be played back from the Dictation
  tab. The setting (`save_recordings`, `crates/teletype-desktop/src/commands.rs`)
  defaults on, and it is on for a settings file that predates the key, so
  upgrading starts keeping audio without asking. The files stay on this Mac and
  are never uploaded. Turning the toggle off stops new recordings; existing
  files stay until you delete them. This was an explicit product decision on
  2026-10-31, and this note is the disclosure for it.
- The crash-recovery banner is currently one-shot in intent but not in
  behaviour: the notice copy is persisted in settings and dismissing it clears
  only the screen, so after one crash the banner returns on every launch until
  the fix lands. See Known limits.

### Reliability

- No panic in production paths. Every site the HARD-0 audit marked "can fail
  at runtime" now returns a typed error that surfaces as a message or a quiet
  end. `clippy::unwrap_used`, `expect_used` and `panic` are deny for non-test
  code in the four pipeline crates, with a one-line reason on every allow.
- Crash recovery. A panic writes a local crash report with lengths and counts
  only (never transcript text) and the next launch says "Teletype hit a
  problem and recovered". The pipeline worker and model child processes are
  supervised: one restart, then a visible error.
- Fault injection. A test-only injector drives the edge cases that a Rust
  probe can reach: speech model load failure, SQLite lock, command tool error,
  and quitting mid-take. It lives behind the `fault-injection` cargo feature,
  is off by default, and a release binary contains none of it (verified: the
  module and every probe site are `#[cfg(feature = "fault-injection")]`).
  Because the gated tests compile out of a default `cargo test`, CI runs them
  in their own step rather than counting them as part of the default suite;
  the long-take memory-bound tests are ungated and do run by default. Four
  scenarios live in the platform layer rather than in Rust (the accessibility
  tree, the pasteboard and the audio callback), so they are documented as not
  injectable instead of tested as no-ops.

### Dictation pipeline

- Recovery spool. A quit mid-take leaves a recoverable spool, a valid spool
  round-trips, and a corrupt one is rejected rather than half-read (three
  tests in `crates/teletype-desktop/src/recovery.rs`). What is NOT claimed in
  this release: that a clean spoken take always drains it, and that a 6 s take
  always commits at least one streaming segment. F-02c's live behaviour is
  still open, see Known limits.
- The 500-take soak harness ships (`tasks/selftest/soak.sh`, with `lib.sh`,
  `run_take.sh` and a README). The run has not happened yet: "500-take soak
  passes" is gate 4 below, not a claim about this build.

### Prompt Engineer

- The built-in instruction is the 10-section template: Title, Role & stance,
  Task, Context, Inputs available, Output requirements, Constraints /
  Do-nots, Examples / References, Execution checklist, Conflict resolution.
  Anything the speaker did not say becomes a `[FILL: ...]` placeholder.
- Users who never edited the built-in get the new instruction on upgrade.
  Users who customised it keep theirs.
- The card shows a worked example and a "Customize prompt" editor with Reset.

### Command Mode

- Command Mode is now a sidebar tab under Process. The screen shows the
  enable toggle, its hotkey, the decider in use and whether it is installed,
  the 14 tools with a plain-English line and an example phrase each, and the
  existing trace log.
- Every tool is labelled truthfully as acting immediately or opening a draft.
  `send_message` sends immediately; the UI says so.

### Speech models

- Purpose chips on every speech row, using the fixed vocabulary: Everyday
  dictation, Fastest, Most accurate, Multilingual, English only, Live
  streaming, Context-aware, Low memory, No download, Long recordings, Indian
  languages, Experimental. Measured chips name their eval file; a test fails
  if a measured chip has no evidence.
- Apple on-device speech (`SpeechTranscriber` and `DictationTranscriber`) on
  macOS 26+ Apple Silicon: 0 MB download, assets installed by the OS, hidden
  on every other platform. The Rust/Swift result-code contract that blocked it
  is fixed in this build, but no live microphone take has run yet, so "it
  works" is not a claim this release makes. See Known limits.

## Model scope for this release

The ASR model research behind this section is a working note kept outside this
repository snapshot (`research/models/ASR_UNDER_2GB.md` in the development
tree), so the rules below are stated here rather than linked. A model ships only
when a native runtime exists in our stack, the download is under 2048 MB, and
the licence allows redistribution.

### Shipping in this release

| Model | Runtime | Download |
|---|---|---|
| Apple SpeechTranscriber | Apple framework (macOS 26+) | 0 MB. Bridge contract fixed here; no live take yet. |
| Apple DictationTranscriber | Apple framework (macOS 26+) | 0 MB. Bridge contract fixed here; no live take yet. |
| Nemotron 3.5 ASR Streaming 0.6B | transcribe.cpp (macOS, Apple Silicon) | 496 MB Q4_K_M, OpenMDW-1.1 |
| Moonshine streaming medium | transcribe.cpp (macOS, Apple Silicon) | 296 MB Q8_0, MIT, English only |
| Granite Speech 5.0 470M TurboCTC | transcribe.cpp (macOS, Apple Silicon) | 279 MB Q4_K_M, Apache-2.0 plus NOTICE |

The three transcribe.cpp rows are opt-in only: each carries an Experimental
chip, none is the default selection, and each is gated on macOS and on the RAM
minimum (2 GB, 1 GB and 1 GB). They are not offered on Windows or Intel, because
the native dependency is wired macOS-only in this build.

What we measured on the 52-clip human corpus (real speech, one Apple Silicon
Mac, engine run through the same code path the app uses):

| Model | Clips that produced text | ASR p50 | ASR p95 |
|---|---|---|---|
| Nemotron 3.5 ASR Streaming | 51 of 52 | 79 ms | 367 ms |
| Moonshine streaming medium | 50 of 52 | 111 ms | 1049 ms |
| Granite Speech 470M TurboCTC | 52 of 52 | 41 ms | 215 ms |

The two and one clips that produced nothing are short (2.4 s and 2.8 s) and the
engines returned "no speech detected" rather than inventing text. No accuracy
number is available for any of these: every reference in that corpus is still
pending, so the plan's rule 1 blocks any ranking or default change based on them.

### Deferred (ADD-AFTER) and why

| Model | Why it is not in this release |
|---|---|
| LFM3-Omni-2B | Needs a llama-server re-ship at b11960+ (the bundled b11488 rejects the `lfm3` architecture), Ali's sign-off on the LFM Open License v1.0 revenue gate, and an asrs.md rule-1 run. |
| Underdog Bark 3.0 Pro | Needs a `FluidVersion::BarkPro` case in the Core ML bridge, a Rust-side downloader for the ConwayResearch repos, and a per-model `min_macos: 15` gate. |
| Apple rows (above) | Listed here for completeness: they ship in this release only on macOS 26+ Apple Silicon. |

### Excluded and why

| Model | Why it is excluded |
|---|---|
| Qwen Omni (Qwen2.5-Omni-3B) | The smallest ship set is 3642.9 MB, over the 2 GB budget before any runtime question. No smaller Omni exists. |
| Underdog Bark 3.0 (163 MB tier) | Its Core ML is driven by a Python 3.11 script, not Swift or FluidAudio. No native runner exists. |
| Cohere Transcribe 03-2026 | 4134 MB in the only available format, gated on Hugging Face, and no timestamps. Revisit only if a quant appears. |
| Whissle Arc v1.1 | CC BY-NC 4.0 plus Whissle terms: non-commercial. Commercial use needs a written licence. |
| Phonon 2.5 | MLX or Python only. No Core ML or C artefact for 2.5. |
| HAAS-ja v1.0 | Japanese only, and there is no Japanese human corpus to validate against. |
| IndiCodec | A neural speech codec, not an ASR model. |
| Whisperstream stream-1-mini | A dictation-cleanup LLM, not ASR. A polish-lane A/B is a separate decision. |
| talkdat | A dictation app, not a model. Competitive reference only. |

## Known limits

- F-02c is not closed. The take traces show streaming rarely committing a
  segment on real takes of 4 s or longer: 0 of 12 on 2026-10-28, 2 of 5 on
  2026-10-31, 0 of 2 on 2026-11-01. A take that commits nothing still delivers
  text, but it decodes the whole clip after release, so long takes feel slower
  than the streaming path is meant to. Dictation works; the streaming promise
  is the open part.
- Apple speech has no live verification, on macOS 26 or 27. The code defect that
  blocked it is fixed in this build, and the fix is proven by contract tests
  that run without the Swift side compiled in, not by a take: the Swift helper
  returns the number of bytes it wrote (or needs), the Rust readers previously
  treated any positive code as "buffer too small", doubled their buffer and
  asked the framework again up to thirteen times before giving up, so every
  locale read as "not installed" and `tta_transcribe` returned a flat `1` that
  the same reader could not accept. The readers now accept a positive code at
  or below the capacity as success, the Swift transcribe call reports the
  needed capacity instead of `1`, and seven tests pin the exact call counts and
  returned bytes (1 call when the text fits, 2 when it needs one grow). What
  remains unproven is the real framework on a real machine: asset install,
  `isAvailable`, locale status and an actual transcript. "Apple speech works"
  waits for a live take.
- New engines ship as opt-in with an "Experimental" chip until they are
  measured on the 52-clip human corpus. None is default.
- The three transcribe.cpp engines are measured for coverage and latency, not
  accuracy: the corpus has no finished references, so no ranking, no chip
  claiming "Most accurate", and no default change is based on them yet.
- "Streaming" in the Nemotron and Moonshine model names is the upstream model
  family, not a promise in this app. This build has no streaming provider for
  the transcribe.cpp engine, so a take with one of these models decodes the
  whole clip after you release, the same way Qwen3-ASR does. They are also not
  live-verified with a microphone take; the proof so far is the corpus run
  through the same engine code. Their download pins were checked against the
  live repositories on 2026-11-01: all three files still match the pinned
  sha256 and byte count (495831520, 295793568 and 279114080 bytes), so a first
  download today verifies. One caveat stays open: these three rows are fetched
  from a `/resolve/main` branch ref rather than an immutable commit, and no CI
  job watches those repositories, so a republish by the upstream owner would
  surface as a failed install for users rather than a red build.
- Windows and Intel Mac builds compile, but the live self-test ran on Apple
  Silicon only. Note the sequence honestly: TC-1 (the transcribe.cpp seam) broke
  the Windows compile gate, which CI caught on the push after the merge, and the
  stub now compiles. The Windows fix is verified by a cross-target `cargo check`
  for `teletype-speech` and by CI, not by a Windows run of the app; run
  41350456737 reports the Windows compile check green on the fix.
- The pill suites are deterministic now, and the second fix names the input that
  moved. UI-FLAKE-1 removed two runner-only failures by flushing the machine
  dispatch before each assertion, and that was a real defect: the pill machine's
  Tauri event listener resolves asynchronously, so an event emitted during mount
  could land in a window where nothing had subscribed. It did not remove all the
  flakiness. On run 41350456737 the same commit passed the full UI suite in one
  job (528 of 528) and failed one assertion in another:
  `a level event does not force a layout of the frame` counted three reads of the
  pill frame where two were expected. The lead reproduced 20 passes out of 20 in
  isolation locally, so it was load-dependent. UI-FLAKE-2 found the input that
  moves: `useNow` advances the recording clock on a real `setInterval`
  (`ui/src/pill.tsx:1786`), its result feeds `warmingSeconds`, and
  `warmingSeconds` is a dependency of the frame's layout effect
  (`ui/src/pillGlow.tsx:132`), which is the code that reads the bounding rect.
  Under load a tick could land between the baseline count and the level events
  and add a read. The test now fakes that interval, so the only clock advances
  are the ones the test asks for. The guard it enforces (a level event must not
  force a synchronous layout) is unchanged, and the positive control that the
  frame was measured at least once before the baseline is still there.
  Verification limit: the CI failure was never reproduced locally, so this is a
  mechanism-level fix confirmed by a green suite, not by a red-to-green run.
- HARD-4 closed the three failure modes in the plan's section 4 that no branch
  implemented. The device-loss row now reads a flag that the cpal stream error
  callback actually writes, because the `Recording` owns it and the take adopts
  that same `Arc`; the test proves the identity with `Arc::ptr_eq`, so it cannot
  pass against a flag the test invented. The model fallback only fires for a
  model failure now: a silent take or a mic failure no longer pays an extra
  engine load and decode. The crash-recovery spool removes its duration sidecar
  with the spool, and the offer only claims a shortened take when the sidecar
  really says the take was longer. Two mutations of the shipped code (drop the
  no-speech guard, drop the flag adoption) each turn exactly one of these tests
  red, so the tests are attached to the wiring. Known limit: a device that dies
  mid-take is still only noticed when you release, not while the take runs.
- The crash-recovery notice repeats. `dismiss_crash_notice` is a deliberate
  no-op, so dismissing the banner clears only the screen: the copy lives in
  persisted settings and is re-seeded on the next launch and on every
  settings-changed event. One crash means the banner on every future launch
  until CRASH-1 lands. The crash report file itself is consumed after one
  display; it is the settings copy that is stuck.
- The spoken voice-command text is no longer written to the app log on the
  selection command path (PRIV-1, bbb152b). The log now records only the
  command name and a length count, never the dictated text.

## Release gates

- [x] Full local suite green on the merge: fmt, clippy, all crate tests,
      ipc_contract, UI tsc and vitest, test inventory.
- [x] CI green on the pushed release branch: macOS arm64, macOS Intel
      cross-compile, Windows, polish gate "not run" notice, aggregate green.
- [ ] Live self-test with sound: 10 takes across the default model, Apple
      speech, one Omni model and one sherpa model; exact paste each time;
      Prompt Engineer and Command Mode one take each.
- [ ] 500-take soak passes.
- [ ] First run on a clean macOS user account passes.
- [ ] Developer ID signing and notarization, updater manifest non-empty and
      signed.
- [x] Release captain verdict: no CRITICAL or HIGH open.
- [ ] Ali's explicit OK, then tag `v0.2.17`.
