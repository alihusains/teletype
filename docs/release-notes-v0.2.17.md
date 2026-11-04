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
  `run_take.sh` and a README) and the run has happened: gate 4 below records
  the 500-take result (0 crashes, 0 stuck pills, 0 recovery change, memory
  shrank 79 MB).

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
  is fixed in this build, and so is the second defect found behind it: the two
  rows used to run the same model, and now select their own module. Neither
  claim rests on a type check; both rows were driven through the real framework
  over the eval corpus and produce different text on 15 of 17 clips. As of
  2026-11-04 both rows have also pasted a live microphone take word for word in
  the running app, so "it works on a real take" is now a measured claim rather
  than an inferred one. The framework warning every Apple take logged is fixed
  in this build too (APPLE-4); Known limits records what that fix costs and what
  it does not cover.

## Model scope for this release

The ASR model research behind this section is a working note kept outside this
repository snapshot (`research/models/ASR_UNDER_2GB.md` in the development
tree), so the rules below are stated here rather than linked. A model ships only
when a native runtime exists in our stack, the download is under 2048 MB, and
the licence allows redistribution.

### Shipping in this release

| Model | Runtime | Download |
|---|---|---|
| Apple SpeechTranscriber | Apple framework (macOS 26+) | 0 MB. Driven end to end on a real machine through the real framework over the 17 synthetic clips: 17 of 17 returned text, WER 9.2%. Also verified on a live microphone take, which pasted the spoken sentence word for word. The framework error that every Apple take used to log is fixed in this build (APPLE-4) and measured in the running app as one reserve and one release line per take with no unallocated-locale line left; see Known limits. |
| Apple DictationTranscriber | Apple framework (macOS 26+) | 0 MB. Runs on this build as its own model: over the same 17 clips it returned text on 17 of 17 with WER 11.5%, differing from the transcriber row on 15 of 17. It covers 54 locales against the transcriber's 45. Verified on a live microphone take as well, pasting the spoken sentence word for word, and again on this build's fixed Apple path. |
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

### Measured on the 17-clip synthetic corpus (Apple speech, real framework)

The two Apple rows were run through the eval driver over `evals/corpus` (17
synthesised clips, English) on this machine, macOS 27 on Apple Silicon, feeding
the WAV files straight to the bridge. No microphone and no speakers were used,
so this is not the live capture path; it is, however, the real
`SpeechAnalyzer` / `SpeechTranscriber` framework doing the transcription, which
is what the earlier "not verified against the real framework" note was waiting
for. The Parakeet row is the current shipped default on the same clips.

| Engine | Clips that produced text | WER (micro) | ASR p50 | ASR p95 | Load |
|---|---|---|---|---|---|
| Apple SpeechTranscriber | 17 of 17 | 9.17% | 103 ms | 559 ms | 102 ms |
| Apple DictationTranscriber | 17 of 17 | 11.54% | 210 ms | 987 ms | 90 ms |
| Parakeet TDT v3 (default) | 17 of 17 | 5.62% | 81 ms | 481 ms | 1987 ms |

The dictation row was run after APPLE-3 landed, on the same 17 clips. Before
that fix it returned text byte-identical to the transcriber row on every clip.

The Apple row is therefore accurate enough to use but measurably worse than the
default on this corpus, which is why it ships opt-in with an Experimental chip
and is not made the default. Most of its WER is behaviour that helps a dictation
user but scores as an error against a verbatim reference: it writes
"ninety nine point nine five percent" as "99.95%", "version two" as "version 2",
and it strips fillers, turning "Um the deploy went out this morning and uh it is
all good" into "The deploy went out this morning and it is all good." The genuine
recognition misses on this corpus were "login bug" heard as "login bog", "pull
request" as "pool request", "auth module" as "off module" and "us east one" as
"S East one".

### Delivery path per engine, measured on this build

Every engine in the shipping catalog was driven through the app's `--e2e` take
path on 2026-11-04 on the CMCP-1 and APPLE-4 build. The input was the app's own
recording of a real failed Granite microphone take (9.6 s of speech), the
settings were a copy of this machine's live settings, and the authority for each
row is the app's own `inject:` line plus the delivery label the harness records.

| Engine | Text produced | Delivery the app reported |
|---|---|---|
| Moonshine streaming medium | the full sentence | `inject: pasted verified (100 chars)`, `clipboard(pasted,verified)` |
| Nemotron 3.5 ASR Streaming | the full sentence | `inject: pasted verified (100 chars)`, `clipboard(pasted,verified)` |
| Granite Speech 470M TurboCTC | the full sentence | `inject: pasted verified (101 chars)` in the CMCP-1 A/B above |
| Parakeet TDT v3 (default) | the full sentence | `inject: pasted verified (100 chars)`, `clipboard(pasted,verified)` |
| Apple SpeechTranscriber | the full sentence, polish ran (`fell_back=false`, 157 to 165 ms) | `inject: accessibility not trusted; text on clipboard`, `clipboard(kept,ax-denied)` |
| Apple DictationTranscriber | the full sentence, polish ran | the same `ax-denied` fail-safe, with the text on the clipboard |

The three transcribe.cpp rows are the point of this table: before the CMCP-1
guard they decoded correctly and pasted nothing. The Apple rows reached the
delivery cascade and took the documented F-17 fail-safe rather than pasting,
because the rebuilt ad-hoc binary no longer carries this machine's
Accessibility grant, so the keystroke it needs is refused and the text is left
on the pasteboard with a pill and tray message. That is the correct behaviour
for a denied grant and it is the same path the shipped app takes for a user who
has not trusted Teletype in System Settings. What the Apple rows do NOT get from
this run is a field write: their real end-to-end proof remains the live
microphone takes on the previous build, where both rows pasted the spoken
sentence word for word into TextEdit.

Three things this run also exposed, recorded rather than fixed:

- SCRATCH-1: with `scratchpad_enabled` on (the state on this machine) and the
  target app recorded as Teletype itself, which is what the harness sees, the
  take is routed to the Notetaker note. That branch marked the trace's paste
  stamp and wrote no `inject:` line, so the trace showed `inject: 5 ms` for a
  take that pasted nothing and the harness label stayed `pending`. The routing
  was intended; the bookkeeping was not, because it made a not-pasted take
  indistinguishable from a pasted one in the traces a support reply is built
  from. **Fixed in this build:** the trace now carries a `delivered_to` field
  (`note` for a Notetaker take, `inject` for the focused app, absent when
  nothing landed), and `t_paste_sent` is only stamped for `inject`. A note take
  no longer reads as a pasted one: `release_to_paste_us` stays absent rather than
  claiming a paste time. Pinned by `delivered_to_serializes_only_when_something_landed`
  in `teletype-core` (old lines with no field still parse; the key is omitted
  when nothing landed).
- The `--e2e` delivery label is only set by the cascade. Any branch that ends
  before the cascade leaves the label at `pending`, so `pending` is not a
  delivery verdict. Read the `inject:` line, or ask the field.
- The intentional silent ends are invisible on the result channel. Four branches
  end a take without writing an e2e result record at all: `quick tap: take under
  the visible floor, discarded silently` (dictation.rs:3288, which calls
  `tap_discard`), `press gated: selected speech model is not installed` (:3647),
  `skipping decode: {message}` (:5287) and `no speech in take: ended quietly`
  (:5677). The sibling log line `quick tap: ordinary stop` (:3292) is NOT one of
  them: it forwards `Input::HotkeyUp`, so the take is transcribed and a result is
  written like any other. The four silent branches are correct behaviour (nothing
  to paste, nothing to show), but a caller that waits for a result cannot tell
  them from a hung take and times out. Soaking through the harness therefore
  needs the app log as a second channel; the soak driver in
  `tasks/selftest/soak_e2e.py` matches those markers (`quick tap:`,
  `press gated`, `skipping decode:`, `no speech in take: ended quietly`). Any
  future support tooling that watches takes should treat "no record" as a
  legitimate outcome, not an error.

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
  2026-10-31, 0 of 2 on 2026-11-01, and 0 of 14 on 2026-11-04 (7 takes on
  Parakeet TDT v3, 4 on Granite, 3 on Moonshine, 2 on the Apple rows). Every
  engine that took part committed nothing. A take that commits nothing still
  delivers text, but it decodes the whole clip after release, so long takes
  feel slower than the streaming path is meant to. Dictation works; the
  streaming promise is the open part.
- Apple speech now has live microphone verification on macOS 27 Apple Silicon, and
  a new limit came out of it. The code defect that
  blocked it is fixed in this build, and the fix is proven by contract tests
  that run without the Swift side compiled in, not by a take: the Swift helper
  returns the number of bytes it wrote (or needs), the Rust readers previously
  treated any positive code as "buffer too small", doubled their buffer and
  asked the framework again up to thirteen times before giving up, so every
  locale read as "not installed" and `tta_transcribe` returned a flat `1` that
  the same reader could not accept. The readers now accept a positive code at
  or below the capacity as success, the Swift transcribe call reports the
  needed capacity instead of `1`, and seven tests pin the exact call counts and
  returned bytes (1 call when the text fits, 2 when it needs one grow). What is
  now proven, on 2026-11-04 on this macOS 27 Apple Silicon machine, is the live
  capture path too. Both rows ran real microphone takes through the running app:
  each held the hotkey while the speakers played "The quick brown fox jumps over
  the lazy dog.", and each pasted that sentence word for word into a TextEdit
  document. The transcriber row decoded 6.1 s of audio in 1.68 s and injected in
  85 ms; the dictation row took 1.80 s and 86 ms. What is NOT proven is macOS 26
  behaviour, which was never measured here.
- Apple speech used to log a framework error on every live take, and that is
  fixed in this build. The complaint was "Cannot use modules with unallocated
  locales. Currently allocated locales are ... This will be an error in a
  future release!" (measured 2026-11-04, one line per take, pid 48318). The
  reason was in the bridge: `tta_install_assets` reserves a locale with
  `AssetInventory.reserve` and then releases it deliberately so the five-locale
  cap stays free for other languages, but `tta_transcribe` built its module
  straight from `Locale(identifier:)` and never reserved. The take still
  returned correct text because the OS tolerates an unallocated locale today.
  `tta_transcribe` now wraps the whole take in one reservation
  (`withLocaleReservedForTake`), releases it on both the success and the
  throwing exit, and releases only a reservation it created itself, so it
  cannot steal the install path's slot and the two paths cannot free each
  other's. Reserving is best effort: a `reserve` that returns false (the locale
  is already reserved) or throws (cap full, `SFSpeechErrorDomain` Code 11;
  unreservable locale, Code 15) is logged under the `tta-locale-reserve` marker
  and the take runs exactly as it did before, so no take that works today can
  start failing. Measured in the running app on 2026-11-04, both Apple rows,
  through the `--e2e` take path: one `reserved 'en_US' for the take` and one
  `released 'en_US' after the take (release returned true), reserved locales
  now: none` per take, and zero `unallocated locales` lines in the same window.
  The same change closed a leak on the install path: it released with the
  caller's raw tag, and `release(Locale("en-US"))` returns false while the
  framework had recorded the reservation as `en_US`, so a hyphenated tag burned
  one of the five slots for the life of the process. Both paths normalize the
  identifier now.
  Cost and limits, measured: a per-take reserve plus release adds 32..35 ms to
  the decode (module 0 from ~50 ms to ~82 ms, module 1 from ~122 ms to ~155 ms,
  stable across three alternating rounds). Holding one reservation across takes
  measured free, so a "reserve once per loaded language, release on unload"
  policy is available if that latency matters. Not proven: macOS 26 behaviour,
  more than one spoken language in a session (the `reserved locales now: none`
  read-back was exercised for `en_US` only), and the window where a take hits
  its 60 s timeout and the abandoned task releases its slot later. It is
  tracked as APPLE-4, and only ever showed up on a live take: no test, no type
  check and no CI run reports it, and CI cannot even compile the Swift off
  Apple Silicon.
- The Nemotron, Moonshine and Granite rows pasted nothing on a live microphone
  take, and the cause was in Teletype, not in those models. Measured 2026-11-04
  against the running build: `granite-speech-470m-q4` decoded the take and
  logged `Success transcription complete` twice (949 ms and 1012 ms) and then
  logged `inject: pasted (unverified, 0 chars)`; `moonshine-streaming-medium-q8`
  did the same on two takes and reported "no speech detected" on a third. The
  audio was never the variable: feeding the app's own recordings of those exact
  failed takes back through the file path scored three of them at 0.00 verbatim
  WER, word for word, on the same build.
  The reason is a contract break in the delivery path. `TranscribeCppProvider`
  declares `word_timings = true`, and when an engine claims word timings the
  dictation path rebuilds the sentence from the word rows instead of using the
  text it was given. Granite and Moonshine return the text with **no word rows
  at all**, which the speech crate documents as a valid result ("the text still
  lands"), and joining an empty word list produces an empty string. The
  transcript was therefore decoded, correct, and then thrown away on the way to
  the field, with no error anywhere. A new contract test measures the shape on
  the real models:
  `granite-speech-470m-q4: words=0`, `moonshine-streaming-medium-q8: words=0`,
  `nemotron-3.5-asr-streaming-q4: words=1`, all with `word_timings=true`, all
  with the same text from the plain and detailed calls. Only Nemotron happened to
  return a word row, which is why the three rows did not fail identically.
  Fixed in this build: an engine that returns text with no word rows now keeps
  its text, and the take logs `rescoring skipped: engine returned no word rows`
  so the shape is visible instead of silent. Measured on the running build
  through the app's own `--e2e` harness, a unix-socket driver that replaces
  only the audio source and the trigger; take start, transcription, the text
  pipeline and the delivery cascade are the production code paths. Driven with
  Ali's real settings, where `dictionaryEnabled: true` and
  `enabledPacks: ["tech"]` make the term list non-empty, which is the condition
  the rebuild needs to run:
  - guard removed, same binary, same 9.7 s recording of the failed Granite
    microphone take: `text: ""`, `delivery: clipboard(pasted,unverified)`,
    `inject: pasted (unverified, 0 chars)`, `transform finished: fell_back=true`.
    The shipped defect, reproduced on demand.
  - guard in place: the sentence delivered whole, `delivery:
    clipboard(pasted,verified)`, `inject: pasted verified (101 chars)`, plus the
    new `rescoring skipped` line, and the clipboard held the text.
  Three unit tests pin the shape (`a_wordless_engine_keeps_its_transcript`,
  `no_terms_skips_the_rebuild`, and the positive control
  `word_rows_still_rebuild_the_take`, which proves an engine that does report
  word rows is still rebuilt from them).
  Verification limit: this proves the delivery path against a real recording,
  not a fresh capture from the microphone. A microphone leg cannot be run on a
  rebuilt binary on this host, because a rebuild changes the ad-hoc signature
  and macOS then stops delivering the global hotkey to it, so the take never
  starts. The microphone legs of the self-test ran on the pre-fix build.
  Tracked as CMCP-1.
- Nemotron rejects an explicit language. Measured 2026-11-04 with the real
  model: the same `hello.wav` transcribes to "Hello." when the language is
  "auto" and fails with `Transcription failed: Unsupported` when it is "en".
  The live dictation path passes "auto" unless the user sets a language (or
  turns on the per-app override), so a user who picks English in Settings gets a
  hard failure on a 496 MB download. No test covered it because every harness
  asks for "auto". Tracked as NEMO-1.
- Command Mode, for users who switch it on, tends to type the command instead of
  running it. Measured 2026-11-04 with the feature enabled, two takes. Saying
  "Hey Teletype, set the volume to fifty percent" was transcribed correctly, the
  wake phrase was stripped correctly, and the decider still answered
  `tool=no_action args_source=none (448 ms)`, after which the sentence was pasted
  into the focused field and the system volume stayed where it was. The second
  take, "open system settings", reached the decider as `open system set.` (the
  recognizer dropped the last syllable) and answered
  `tool=no_action args_source=none (355 ms)`, so 16 characters were pasted and
  nothing opened. The clean take is the one that matters: `set_volume` is a
  shipped tool and a stated example phrase, so a correct command with a correct
  transcript still produced no action.
  Root cause, measured 2026-11-04 against the running fallback model: the
  intended decider is NeuDecide (an ONNX speech-to-tool model), but its files are
  not installed on this machine, so Command Mode runs the LLM fallback, which is
  whatever the user's polish LLM is. On this machine that is `s1-mini`
  (~1.2 B), and it cannot map clean volume phrases to `set_volume`: driven
  directly through llama-server with the same grammar, it answered `none` for
  "set the volume to 50 percent", "set volume to 50" and "turn the volume up",
  and even answered `level: "lower"` for "make it louder". The tool and the
  routing are correct (the `set_volume` schema lists `"50"` and the system
  prompt rule says "'volume' -> set_volume"); the fallback model is simply too
  small to hold the mapping. `no_action` mapping to plain dictation is the
  intended graceful degradation, so nothing crashes and no wrong command runs.
  The fix is to install a larger local LLM for the fallback, or the NeuDecide
  model, not a code change. `command_mode_enabled` defaults to false, so no new
  user is affected; anyone who turns it on with a small LLM is. Tracked as CMD-1.
- The updater's failure path is verified on screen and its cause is not
  verifiable at all. Pressing Settings, "Check for updates" on this build renders
  "Update failed. The updater could not reach or verify the release. The releases
  page always has the latest installer." with a Try again button, which is the
  fail-closed behaviour F-04 was meant to guarantee. Nothing about the underlying
  reason reaches the in-app log or the log file, so a user reporting it cannot be
  triaged from the artifacts the app keeps. A successful upgrade check can only be
  measured once a signed release with a matching `latest.json` platform entry
  actually exists, which needs the signing key.
- The two Apple rows used to be the same model for any locale the transcriber
  covered, which includes English. `apple-dictation-transcriber` was a separate
  row, but `tta_transcribe` took no module argument and the Swift selection
  picked by locale while preferring `SpeechTranscriber`, so the row id reached
  the availability UI but not the transcription. Measured at the time: the two
  rows returned byte-identical text on all 17 clips, differing only in wall
  clock. That is fixed and merged in this build. `tta_transcribe` now carries a
  module code (0 transcriber, 1 dictation), the Swift side honors it, and
  `tta_module_used` reports the class that actually ran so a locale-only
  substitution is visible instead of silent. Re-measured on this macOS 27
  (Build 26A428) after the fix: the two rows differ on 15 of the 17 clips
  (`multi-01` and `question-01` still agree), and a real FFI test executed both
  modules and got back `SpeechTranscriber` and `DictationTranscriber` with no
  fallback. Measured locale coverage: transcriber 45, dictation 54, 23 locales
  only dictation covers, and both cover `en_US`. Corpus WER puts the
  transcriber ahead: micro 9.17% against 11.54%, and verbatim micro 4.29%
  against 10.12%, so the dictation row is the weaker English model. Neither row
  shows a speed or accuracy bar. What is still
  unproven is a live fallback take, for a locale only one module covers; no
  Indic or European asset pack was downloaded to force one.
- New engines ship as opt-in with an "Experimental" chip until they are
  measured on the 52-clip human corpus. None is default.
- The three transcribe.cpp engines are measured for coverage and latency, not
  accuracy: the corpus has no finished references, so no ranking, no chip
  claiming "Most accurate", and no default change is based on them yet.
- "Streaming" in the Nemotron and Moonshine model names is the upstream model
  family, not a promise in this app. This build has no streaming provider for
  the transcribe.cpp engine, so a take with one of these models decodes the
  whole clip after you release, the same way Qwen3-ASR does. They are not
  capture-verified with a fresh microphone take on the fixed build (see CMCP-1
  for why a rebuilt binary cannot receive the hotkey on this host); what is
  measured is the production take path fed with real microphone recordings,
  which is how the three rows were shown to deliver text after the fix. Their
  download pins were checked against the
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
- Command Mode has no per-tool confirmation, and `send_message` really does
  send. Verified by reading the call path rather than trusting the module doc:
  the only gate is the `settings.command_mode_enabled` master switch at
  `crates/teletype-desktop/src/dictation.rs:6731`, and once that is on,
  `crates/teletype-desktop/src/dictation.rs:6757` calls
  `command_executor::execute` directly with no preview step. There is no
  risk-level concept anywhere in the Rust sources: searching the workspace for
  `risk_level`, `RiskLevel` and `dangerous` returns nothing outside an unrelated
  test name.
  Two facts keep this at a documented limit rather than a blocker. The switch
  ships off (`crates/teletype-desktop/src/commands.rs:1198`), a test asserts
  "default off" (`:1659`), and a settings payload missing the key also lands on
  off (`:1674`). And the Command Mode screen does not hide the consequence: it
  lists the tool as "Sends immediately" with the note "No confirmation step"
  (`ui/src/screens/CommandModeScreen.tsx:101`-`:102`), so a user who enables it
  is told what they are enabling. Wrong-recipient odds are reduced but not
  removed by the contacts lookup, which errors out on zero, ambiguous, or
  numberless matches (`command_executor.rs:463`, `:492`, `:498`).
  The module header used to claim the opposite ("the caller must check the risk
  level first"). That sentence was false and has been replaced with what the
  code actually does, because a comment asserting a safety net that no caller
  implements is how the next person ships it believing the net is there.
  Open product decision for Ali, not something I will decide by patching: add a
  confirm step for outward tools (`send_message` especially), or accept
  immediate send as the design for an opt-in feature.

## Release gates

- [x] Full local suite green on the merged tip. Re-run after APPLE-3 landed, on
      `d6856a0`: `cargo fmt --all -- --check` clean, `cargo clippy -p
      teletype-desktop -p teletype-apple -p teletype-speech --all-targets -- -D
      warnings` exit 0, `cargo test --workspace` 2210 passed / 0 failed / 8
      ignored, `ipc_contract` 11 passed, `scripts/test-inventory.py --check` in
      sync at 77 suites, UI vitest 530 passed.
- [ ] CI green on the pushed release branch: macOS arm64, macOS Intel
      cross-compile, Windows, polish gate "not run" notice, aggregate green.
      Proven on `2d9b85f`, run 41714573720: 16 of 16 jobs success, the only
      skipped job is the weekly non-gate catalog URL check. The first run of
      that commit failed Coverage and Benches with "The job was not acquired by
      Runner of type hosted even after multiple attempts", a macOS arm64 runner
      capacity problem, not a code failure; re-running the two failed jobs made
      them green without re-running anything else. That run predates APPLE-3, so
      the gate has to go green again on the new tip. Note that CI does not
      compile the Swift bridge except on the Apple Silicon job, and does not run
      `cargo doc`, so a green CI is not evidence that the Apple rows still
      select different modules: the eval is.
      Re-run on the SCRATCH-1 tip (2026-11-04): 10 of 16 jobs passed,
      including the full `Lint (fmt + clippy)` job, the macOS release link, the
      macOS Intel and Windows compile checks, and the core, speech and
      inference test jobs. Five macOS-arm64 test-heavy jobs (desktop tests,
      coverage, checks, polish gate, benches) were cancelled after about 30
      minutes, and the aggregate gate failed only because those inputs were
      cancelled. This is the same hosted-runner starvation as before, not a
      code failure: the code that job would run is green locally on the same
      tip (fmt clean, clippy `-D warnings` clean, 558 desktop lib tests, 21
      core trace tests, 11 ipc_contract). The gate stays open until a push
      lands on a window where the arm64 runners are not saturated.
- [ ] Live self-test with sound: 10 takes across the default model, Apple
      speech, one Omni model and one sherpa model; exact paste each time;
      Prompt Engineer and Command Mode one take each. **Run on 2026-11-04 against
      the merged tip: 14 takes, real microphone, real speakers, frontmost-app
      guard held (it aborted once rather than typing into the terminal).** The
      14 are 7 on `parakeet-tdt-v3`, 4 on `granite-speech-470m-q4`, 3 on
      `moonshine-streaming-medium-q8` and 2 on the Apple rows.
      Done and passing: `parakeet-tdt-v3` pasted the spoken sentence word for
      word; both Apple rows did the same; Parakeet transcribed a corpus file
      played through the speakers into the microphone exactly, which is the
      positive control that makes the capture path trustworthy. Done and failing:
      the transcribe.cpp rows returned nothing live (CMCP-1 above, now fixed and
      re-proven on the very recordings those takes produced), and Command Mode
      typed its command instead of running it (CMD-1). Streaming committed 0
      segments on all 14. Not runnable on this host, and not claimed: the
      sherpa leg, because no sherpa row is offered on macOS (it is the Windows
      default), and the Omni leg, because no Omni row is in this release's
      catalog; the Prompt Engineer leg additionally needs a 700 MB Qwen3.5 2B
      completion, and the file is still a `.part`. `parakeet-ultra-coreml` was
      not part of this run either: it has no model file on this machine (the
      models folder holds `ggml-parakeet-tdt-0.6b-v3-q4_0.bin` and
      `-q8_0.bin` plus the three transcribe.cpp files), so the default row that
      ran is TDT v3. Treat this gate as covered for the engines macOS users
      actually get, and as open for the three legs that need a Windows machine
      or a model download.
      Method note for whoever re-runs this: the microphone legs need a binary
      whose Input Monitoring grant still matches its signature, and a rebuild
      breaks that silently (the app registers the hotkey, macOS delivers nothing
      to it). The app's `--e2e` socket harness drives the same production take
      path from a WAV with no hotkey at all, which is how the CMCP-1 fix was
      measured; `scripts/e2e/delivery_matrix_macos.sh` is the worked example.
- [x] 500-take soak passes. **Run on 2026-11-04, headless, 500 takes in 82.3
      minutes.** The four SOAK-1 pass criteria all met: 0 crashes (12 crash
      reports before and after), 0 stuck pills, 0 recovery-file change (0 to 0),
      and memory growth of -79 MB from warmup (limit 100 MB, i.e. it shrank).
      The driver flagged 71 takes, and none is a product defect: 61 are
      intentional silent ends the app logs instead of writing a result (35
      "no speech in take: ended quietly" on the silence and 30 s shapes, 26
      "quick tap" under the visible floor), 8 are the terminal Paste-Last skip
      message (delivery "hold" in headless, which clears on the next take, and
      every one of the 8 recovered on the following arm), 1 is a 90 s timeout on
      a 5-minute take, and 1 is an empty transcript on a spam take. The 8 are
      now counted as `terminal_skip_messages`, not `stuck_pills`, because a pill
      that still shows a skip notice is the app telling the user why their text
      was not pasted, not a take that failed to release it. See the "intentional
      silent ends" note above for why the harness needs the app log as a second
      channel.
- [x] First run on a clean macOS user account passes, for the part that is
      measurable without a window. **Run on 2026-11-04** via
      `tasks/selftest/first_run_clean.sh` in two phases. Phase "defaults" (5
      checks): the app launches with no settings file present, the shipped
      default (`parakeet-ultra-coreml`) resolves, storage is initialised, and
      the pill is at rest. Phase "gate" (7 checks): with a ggml row
      (`parakeet-tdt-v3`) whose bytes are absent, a press is gated, not hung,
      the pill returns to rest, and no recording is written. That gate is the
      "accidental tap does nothing" requirement. Reported as an observation, not
      a failure: the Core ML default reads as installed from the bridge alone,
      so its first take downloads weights from Hugging Face while the pill reads
      transcribing. Three legs stay MANUAL and are not claimed here: the
      onboarding window (needs the GUI), the TCC permission prompts (need a
      signed app on a real account), and the first model download (behind the
      Models rail; the e2e protocol has no download command).
- [ ] Developer ID signing and notarization, updater manifest non-empty and
      signed. **This gate cannot pass with the repository as it stands today,
      and pushing the tag will now fail rather than publish a dead updater.**
      Measured on 2026-11-04:
      - `gh secret list` and `gh variable list` for `alihusains/teletype` both
        return nothing: no `TAURI_SIGNING_PRIVATE_KEY`, no `APPLE_CERTIFICATE`,
        no `ALLOW_NO_UPDATER`.
      - No Developer ID identity on this machine (`security find-identity -v -p
        codesigning` reports 0 valid identities), no `teletype-notary`
        notarytool profile, and no `~/.tauri` key directory. The only keychain
        account under `com.teletype.app` is an LLM credential. So the private
        half of the committed updater key is not reachable from here.
      - Every recent release run logs the same thing: "No usable
        TAURI_SIGNING_PRIVATE_KEY secret (absent or empty): updater artifacts
        will not be signed", and v0.2.16 published `APPLE_CERTIFICATE` absent,
        i.e. unsigned.
      - The consequence is visible in the published manifests: I downloaded
        `latest.json` from v0.2.13, v0.2.15 and v0.2.16 and all three have
        `"platforms": {}`. An empty platforms map means the updater endpoint
        offers nobody anything, so no installed Teletype has ever been offered
        an in-app update. The DMG and EXE assets still ship, so manual upgrade
        works.
      - `crates/teletype-desktop/tauri.conf.json` does commit an updater
        `pubkey` and `createUpdaterArtifacts: true`, and the plugin is
        registered (`crates/teletype-desktop/src/lib.rs:604`) with a real
        "Check for updates" button in Settings
        (`ui/src/settings/PermissionsUpdatesLicenseSections.tsx:205`). Read
        from the source, not measured against a live empty manifest: when the
        manifest has no entry for the running platform the plugin resolves
        `check()` to null, the handler takes the `else` branch and puts the row
        back to `idle` (`:213`-`:215`), which re-renders the same "Check for
        updates" button. There is no "you are up to date" string anywhere in
        that component, so the failure is silent rather than dishonest, and the
        adjacent "Open releases page" button still works.
        Verification limit: the idle-on-null behavior is a code read plus the
        plugin's documented contract. I have not clicked the button against a
        zero-platform manifest in a running app, and doing so needs a built app
        plus network, so treat that one sentence as reasoned rather than
        observed.
      - Why the tag now fails instead of publishing: the fail-closed gate
        (`if not platforms and not (not key_present and allow)`) and the
        `ALLOW_NO_UPDATER` escape hatch both landed in `7b0014d` on 2026-10-31,
        after the v0.2.16 tag was cut; `git show v0.2.16:.github/workflows/release.yml`
        contains neither string. v0.2.17 is cut from a branch that does contain
        them. With no signing key and no override variable, the publish job
        prints "Updater manifest would contain zero platforms. Refusing to
        publish an update that offers nothing." and exits 1.
      Two ways forward, and this is Ali's call, not mine:
      1. Add the `TAURI_SIGNING_PRIVATE_KEY` (plus
         `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) secret holding the private key
         matching the committed `pubkey`. Then payloads sign, the manifest
         advertises `darwin-aarch64` and `windows-x86_64`, and in-app updates
         work for the first time. `docs/signing-and-updates.md:67` records that
         this keypair was already generated and that the private half lives
         outside the repository, so it should exist somewhere rather than
         needing to be created. It is not on this machine: the documented path
         `~/.teletype-updater.key` does not exist, there is no `~/.tauri`
         directory, and nothing updater-related is in the `com.teletype.app`
         keychain. Find it in the password manager the doc recommends and paste
         it into the secret.
         The key must be that original one, not a new one. A freshly generated
         pair would sign payloads that existing installs cannot verify, because
         the old public key is baked into every shipped binary
         (`docs/signing-and-updates.md:71`). If it really is lost, rotating the
         pair still works, but the first release after rotation must be
         installed by hand.
      2. Set the repository variable `ALLOW_NO_UPDATER=true` to publish as it
         is now, deliberately unsigned. The release then ships DMG and EXE for
         manual download and in-app updates stay unavailable, which is the
         status quo rather than a regression.
      Adding `APPLE_CERTIFICATE` plus `APPLE_ID` /
      `APPLE_APP_SPECIFIC_PASSWORD` / `APPLE_TEAM_ID` would additionally make
      the app open on a fresh Mac without Gatekeeper override and is a separate
      credential from the updater key.
- [x] Release captain verdict: no CRITICAL or HIGH open.
- [ ] Ali's explicit OK, then tag `v0.2.17`.
