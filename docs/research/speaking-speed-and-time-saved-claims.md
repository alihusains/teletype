# Speaking speed and "time saved": what can be honestly measured and claimed

Date: 2026-09-28
Author: research subagent (read-only)
Scope: (1) the `ew/EnviousWispr` reference implementation, (2) published research and first-party product claims.

Everything below is either quoted code with a file:line pointer, or a figure with a URL.
Where I could not verify something, it says so explicitly. I have not invented any citation.

---

## Part 1. What EnviousWispr actually does

### 1.1 Headline finding

**EnviousWispr has no speaking-speed feature, no words-per-minute feature, and no "time saved" feature at all.** There is no such code in the macOS app, the Windows port, or the website. I searched the whole `ew/` tree for `wpm`, `wordsPerMinute`, `words_per_minute`, `timeSaved`, `time_saved`, `speedup`, `speechRate`, `faster than typing`, `minutes saved`, `hours saved`, `words per minute`.

The only hits in product source are incidental comments:

`ew/EnviousWispr/Sources/EnviousWisprAppKit/App/Overlay/PillCatalog.swift:289`

```swift
// **The 8 seconds is READING TIME, and the reason moved here from the
// dead table it used to live in** (#2376 C3). `NotificationStyle`
// carried an `autoDismissSeconds` table with no reader at all, and its
// #1891 note is the only place this number was ever justified: the
// advisory sentence is ~23 words, which at roughly 200 wpm needs about
// seven seconds to read, so the 3-second error dwell would show a
// message the user physically cannot finish.
```

This is the only "wpm" in the entire application. It is a UI-dwell-time justification, not a user-facing metric.

`ew/EnviousWispr/Sources/EnviousWisprAppKit/App/LastRecordingResult.swift:35` matches only on the word "speedup" inside a CI comment about build caching. Not relevant.

Every other hit is **marketing blog copy on the marketing website**, and the copy is internally inconsistent:

- `website/src/content/blog/async-communication-better-when-you-speak.md:12` -> "Most people type at 40 words per minute but speak at 150."
- `website/src/content/blog/async-communication-better-when-you-speak.md:36` -> "Most people speak at 130-150 words per minute and type at 40-80."
- `website/src/content/blog/essay-outline-by-talking-it-out.md:25` -> "Most people speak at 130-150 words per minute but type at 40-60."
- `website/src/content/blog/mac-dictation-for-productivity-the-complete-guide.md:11` -> "Typing a 200-word email takes about 90 seconds at **120 wpm**." (120 wpm for typing, which is wrong by any standard)
- `website/src/content/blog/welcome-to-enviouswispr.md:77` -> "three to five times faster than typing"
- `website/src/content/blog/mac-dictation-for-content-creators-the-complete-guide.md:11` -> "faster than typing by roughly 2x on first drafts"

Four different speedup ratios (2x, 3x, 3-4x, 3-5x) and two different typing speeds (40 wpm, 120 wpm) across six blog posts. **None are sourced.** This is exactly the pattern Teletype is trying to escape.

### 1.2 Does EnviousWispr record the actual DURATION of a dictation?

**Yes, partially, and the field is semantically unreliable.** This is the important structural finding.

The persisted record is `Transcript`:

`ew/EnviousWispr/Sources/EnviousWisprCore/Transcript.swift:339-350`

```swift
/// A completed transcript with metadata.
public struct Transcript: Codable, Identifiable, Sendable {
  public let id: UUID
  public let text: String
  public private(set) var polishedText: String?
  public let language: String?
  public let duration: TimeInterval
  public let processingTime: TimeInterval
  public let backendType: ASRBackendType
  public let createdAt: Date
```

So `Transcript` has `duration: TimeInterval` and `createdAt: Date`. There is **no `endedAt`, no `startedAt`, no `wordCount`, and no `recordingDuration`** persisted on the history row.

`duration` is filled from whatever the ASR engine happened to report:

`ew/EnviousWispr/Sources/EnviousWisprPipeline/KernelFinalizationWiring.swift:554-562`

```swift
    store = { text, transcriptID, disposition in
      let transcript = Transcript(
        id: transcriptID,
        text: outcome.rawText ?? text,
        polishedText: outcome.polishedText,
        language: adapter.lastResult?.language,
        duration: adapter.lastResult?.duration ?? 0,
        processingTime: adapter.lastResult?.processingTime ?? 0,
        backendType: adapter.engineIdentity.backendType,
```

**This field means different physical things depending on which engine is loaded.**

WhisperKit batch path, `ew/EnviousWispr/Sources/EnviousWisprASR/WhisperKitBackend.swift:873-878`:

```swift
    let duration: TimeInterval =
      if let lastSeg = results.last?.segments.last {
        TimeInterval(lastSeg.end)
      } else {
        0
      }
```

That is the **end timestamp of the last decoded speech segment**, not the wall-clock length of the recording, and not the total audio duration. It is short whenever the take ends in silence, and it is `0` whenever the engine returns text with no segment timestamps.

Parakeet streaming path, `ew/EnviousWispr/Sources/EnviousWisprASR/ParakeetBackend.swift:694-703`:

```swift
    let finalizeElapsed = finalizeEnd - finalizeStart

    return ASRResult(
      text: text,
      language: nil,
      duration: totalElapsed,
      processingTime: finalizeElapsed,
```

`totalElapsed` is **transcription pipeline wall-clock latency**, i.e. how long the decode took. It is not speech duration at all.

The project is aware of the WhisperKit problem and patches around it in the adapter, but the patch is local to the adapter and does not change what the field means across engines:

`ew/EnviousWispr/Sources/EnviousWisprPipeline/WhisperKitEngineAdapter.swift:1243-1263`

```swift
      // reads `adapter.lastResult?.duration` straight into saved History
      // metadata, so storing the backend's own `result.duration` as-is
      // would persist a wrong/zero duration for a retry-rescued dictation
      // whenever `WhisperKitBackend.mapResults` derived it from the LAST
      // DECODED SEGMENT's end time (short on trailing silence, or zero
      // when text exists without segment timestamps). ...
      let audioDurationSec = Double(inputSamples.count) / AudioConstants.sampleRate
      let correctedResult = ASRResult(
        text: result.text,
        language: result.language,
        duration: audioDurationSec,
        processingTime: result.processingTime,
        backendType: result.backendType
      )
```

The robust quantity already exists in the codebase and is already the correct thing: `sampleCount / sampleRate`. It is just not used as the canonical duration, and it is not persisted where the UI can read it as "how long did this take".

### 1.3 The one genuinely good duration source, and what it is used for

There IS a correct monotonic recording-duration measurement in the kernel:

`ew/EnviousWispr/Sources/EnviousWisprPipeline/RecordingSessionKernel.swift:735-740`

```swift
  var recordingElapsedSeconds: TimeInterval? {
    guard state == .live, let start = recordingStartedAtTick else { return nil }
    let now = currentTick()
    guard now >= start else { return 0 }
    return TimeInterval(now - start) * KernelFinalizationWiring.tickDurationSeconds
  }
```

with the design note above it being explicitly about monotonic immunity to clock changes, and about the VAD discard gate. This is exactly the value Teletype would need.

Where it goes: **telemetry, and only telemetry.**

`ew/EnviousWispr/Sources/EnviousWisprServices/TelemetryService.swift:426-444`

```swift
  /// An eligible cancel selected Escape Recovery.
  public func escapeRecoveryStarted(
    asrBackend: String, polishProvider: String, recordingDurationMs: Int, takeID: String
  ) {
    let props: [String: Any] = [
      "asr_backend": asrBackend,
      "polish_provider": polishProvider,
      "recording_duration_ms": recordingDurationMs,
      "take_id": takeID,
    ]
```

So per-take duration in milliseconds is sent to PostHog, keyed by an ephemeral take id, for exactly one funnel event (escape recovery). It is not written to the history row and it is never read by a view.

### 1.4 Is `Transcript.duration` ever displayed?

**No.** I grepped every read of `.duration` on a transcript-shaped value across `Sources/` and `Tests/`. The only non-test readers are the assignment in `Transcript.swift:465` and the comment in `WhisperKitEngineAdapter.swift:1248`. The two test assertions are:

- `Tests/EnviousWisprTests/Pipeline/HeartPathIntegrationTests.swift:73` -> `#expect(result.outcome.transcript?.duration == fixture.durationSeconds)`
- `Tests/EnviousWisprTests/Pipeline/KernelFinalizationWiringMetadataPropagationTests.swift:69` -> `#expect(transcript.duration == 1.5)`

So the field is persisted and tested, and no human ever sees it. That is the cleanest possible proof that the reference implementation deliberately declined to build a speed feature on it.

### 1.5 The one "stats" surface in the app

`ew/EnviousWispr/Sources/EnviousWisprAppKit/Views/Main/SidebarStatsHeader.swift:6-76` is the entire statistics header. It contains:

- a search field
- a row count (`transcriptCoordinator.listedCount`)
- a segmented `HistoryFilter` picker
- a `ModelStatusBar` showing engine name, pipeline status, and configured polish provider

```swift
/// Stats header shown above the transcript list in the sidebar.
struct SidebarStatsHeader: View {
```

There is no rate, no saving, no aggregate word count. `ModelStatusBar` is a health readout, and its own doc comment says so: "Compact two-row status card: transcription engine + configured AI polish."

### 1.6 Does EnviousWispr count words at all?

Yes, in four places, none user-facing as a speed metric:

**a) The canonical word counter** (whitespace-separated runs, explicitly not `NLTokenizer`, with the reason documented):

`ew/EnviousWispr/Sources/EnviousWisprPipeline/TranscriptSplitter.swift:248-268`

```swift
  /// How many words a piece of text contains.
  ///
  /// Whitespace-separated runs, which is the same thing the ceiling was measured
  /// against. Deliberately NOT `NLTokenizer(unit: .word)`: that counts a
  /// hyphenated compound and a contraction as several tokens, so it would report
  /// a different number from the one the 500 was measured with, and the ceiling
  /// would quietly mean something else.
  public static func wordCount(in text: some StringProtocol) -> Int {
```

**b) The file-import chip**, the only place a word count reaches a human:
`ew/EnviousWispr/Sources/EnviousWisprAppKit/Views/Settings/TranscribeFileView.swift:1679`

```swift
      chip("\(coordinator.wordCount) words")
```

**c) An LLM-polish short-input guard**, `Sources/EnviousWisprPipeline/LLMPolishStep.swift:622-626`, and a cloud-prompt guard at `Sources/EnviousWisprLLM/Prompting/CloudFixedPromptBuilder.swift:63-64`.

**d) A Sentry crash-diagnostics struct** with defaulted fields, never shown:

`ew/EnviousWispr/Sources/EnviousWisprServices/SentryBreadcrumb.swift:15-24, 46-55`

```swift
  public struct RecordingSnapshot: Sendable {
    public let backend: String
    public let audioRoute: String
    public let wasStreaming: Bool
    public let startTime: Date
    public let durationMs: Int
    public let targetAppBundleID: String?
    public let transcriptCharCount: Int
    public let transcriptWordCount: Int
```

```swift
      var ctx: [String: Any] = [
        "backend": backend,
        "audio_route": audioRoute,
        "was_streaming": wasStreaming,
        "start_time": ISO8601DateFormatter().string(from: startTime),
        "duration_ms": durationMs,
        "transcript_char_count": transcriptCharCount,
        "transcript_word_count": transcriptWordCount,
      ]
```

`transcriptWordCount` defaults to `0` in the initialiser and I found **no call site that supplies a non-zero value**. I am flagging that as unpopulated rather than asserting it is dead.

### 1.7 Per-word timing exists, but only for file import

`ASRResult` carries optional per-word timings:

`ew/EnviousWispr/Sources/EnviousWisprCore/ASRResult.swift:80-85`

```swift
  /// Per-word timing over `text`, when the engine was asked for timestamps.
  /// `text` is unaffected either way. Optional + defaulted so existing
  /// callers and old Codable payloads still decode.
  public let wordTimings: [ASRWordTiming]?
```

But every consumer of `wordTimings` is in the **file import** path, for speaker diarization and turn assembly (`FileImportCoordinator.swift:1378-1379, 1910-1926, 1962-2089`). It is never requested for a live dictation, and it is not persisted on `Transcript`. So the raw material for a true measured wpm is already in the codebase and is not being used for this purpose.

### 1.8 Verdict on the reference implementation

| Question | Answer |
| --- | --- |
| Records actual dictation duration? | Yes, in kernel memory (`recordingElapsedSeconds`) and to telemetry (`recording_duration_ms`), but not reliably on the history row. |
| Persists a trustworthy per-dictation duration? | **No.** `Transcript.duration` is engine-dependent: last-segment end time on WhisperKit, decode wall clock on Parakeet. |
| Computes "time saved" or any speed comparison? | **No. Zero lines of code.** |
| Uses any words-per-minute constant? | **No.** Not even as a fallback. |
| Shows a speaking rate to the user? | **No.** |
| Counts words? | Yes, whitespace-delimited, but only for a file-import chip, a polish length guard, and Sentry. |

**The useful lesson from EnviousWispr is negative:** a serious dictation app can sit on a correct monotonic duration measurement for years and still have nothing to show on an Insights screen, because it never decided what the number means. Teletype should decide what it means before it ships, not after.

---

## Part 2. The citable numbers

### 2.1 Typing speed, adults, English

**Best peer-reviewed source I verified. This is the one to cite.**

> Dhakal, V., Feit, A. M., Kristensson, P. O., & Oulasvirta, A. (2018). Observations on Typing from 136 Million Keystrokes. *Proceedings of the 36th Annual ACM Symposium on Computer Human Factors in Computing Systems (CHI '18)*, 336-341. DOI: [10.1145/3173574.3174220](https://doi.org/10.1145/3173574.3174220). Best Paper Honorable Mention.

- Design: 168,000 volunteers, 136 million keystrokes, online typing test of short randomised phrases.
- **Mean typing speed: 52 wpm.** (Open access PDF: <https://aaltodoc.aalto.fi/bitstreams/c175f9bb-8c83-40bc-8969-e179f4a3a6e6/download>)
- **Fastest users in the study: 120 wpm.** (Aalto University press release via ScienceDaily: <https://www.sciencedaily.com/releases/2018/04/180405101720.htm>, "The fastest users in our study typed 120 words per minute, which is amazing given that this is a controlled study with randomized phrases", Prof. Antti Oulasvirta)
- Independently corroborated: Pinet et al., "Typing expertise in a large student population", write "the overall mean typing speed was of 52 WPM in Dhakal et al. (2018)" (<https://hal.science/hal-03767064v1/file/2022_PinetZielinskiAlarioLongcamp_CRPI_toshare.pdf>)

**Caveats that matter and must travel with the number:**
- Self-selected online sample, not a probability sample of adults.
- The task is **copying short pre-written phrases**, not composing. Composing is slower. The "32.5 wpm transcribing / 19.0 wpm composing" split circulating online is attributed to Karat and colleagues (1999); I was **unable to verify** the primary source for that pair, so do not cite it.
- 52 wpm is a *typing-test* number on a mix of laptop and full-size physical keyboards (per the authors' own Table 1: laptop keyboard 54.15%, small physical keyboard / on-screen the rest).

**Historical professionally trained typists, for the upper context:** the same CHI paper's related-work slide cites Grudin (1983) for 1920s-80s trained touch typists at an average of 75 wpm with 1.0-3.2% error rates, and a typical 60-90 wpm range. Modern cognitive-lab touch typists average 70-80 wpm (Salthouse 1984, Logan 2018, as cited in the same literature).

**On "40 wpm" specifically.** It is not a nonsense figure, and it is not exactly a study result either. It is the industry shorthand for a casual/inexperienced typist and is roughly the low end of the Dhakal distribution. SuperWhisper's own reference table (below) labels 40 wpm "Average typist" and 70 wpm "Fast office worker". If you want a defensible citable typing constant, use **52 wpm (Dhakal et al. 2018)** and label it as a typing-test mean for everyday users, not as a claim about anyone's personal speed.

### 2.2 Typing record

> Barbara Blackburn (1920-2008), Guinness Book of World Records 1976-1985 editions. Verified via the Guinness *Book* text and Wikipedia's sourced summary: <https://en.wikipedia.org/wiki/Barbara_Blackburn_(typist)>

- **Verified Guinness figure: 150 wpm sustained for 50 minutes (37,500 keystrokes); 170 wpm on the Dvorak Simplified Keyboard.** Quoted from the 1985 edition (p. 467) and 1976 edition (p. 485), both scanned on archive.org.
- **The famous "212 wpm" is a self-reported peak, not a Guinness figure.** Blackburn told the *Seattle Times* (20 May 1985) she had "attained speeds of 212 words a minute for a brief time" on an Apple keyboard. She never specified the time period, only "brief".
- **Guinness removed the category.** The 1986 edition states: *"Records on electric and computer-driven typewriters cannot be compared with any accuracy."* Her records were accepted into the book **without an official test**, lobbied in by a Dvorak keyboard promoter.
- On Letterman in January 1985 she was shown to have transposed her hands on a typewriter. A Navy cryptographer deciphered the typos as a substitution cipher with the right hand offset one key right.

**Takeaway:** the typing "record" is a ~50-year-old, revoked, never-independently-tested claim on a non-standard layout, doing non-standard work (copying a baseball rules book, not composing). If your Insights screen compares Teletype against a typing "world record", that is a red flag, not a benchmark.

### 2.3 Speaking rate, English, conversational

This is the messiest number in the entire space, and the messiness is the finding. There is no single accepted "average speaking rate" because **the measured value depends entirely on what you count.**

**Definitions that must be separated:**

| Measurement | Range in English |
| --- | --- |
| Articulated rate (silences removed) | roughly 150-200 wpm |
| Gross rate (total elapsed wall clock, including pauses) | roughly 100-160 wpm |
| Prepared/monologue delivery (news reading, TED) | roughly 150-200 wpm |
| Dictation into a device, no conversational partner | roughly 120-160 wpm |

**Verified peer-reviewed anchors I can cite:**

1. **Tauroza, S., & Allison, D. (1990). Speech Rates in British English. *Applied Linguistics*, 11(1), 90-105.** Peer-reviewed, 246 citations. ERIC record: <https://eric.ed.gov?id=EJ405462>. The paper measured four registers (conversation, school reading, interview, radio monologue) across a large British corpus and argues the widely quoted radio-announcer figure "does not represent a truly standard range of speech rates."

2. **D. Jones (1967), *An Outline of English Phonetics*, section 43** puts the average conversational rate of native English speakers at **300 syllables per minute**. Quoted verbatim in a Lund University publication on speech tempo: <https://www.lunduniversity.lu.se/publication/01ea84be-7f31-4538-afaf-7bc1e229fc1c> ("D. Jones (An Outline of English Phonetics, Cambridge 1967, §43) put the average conversational rate of native English speakers at 300 syllables/minute"). This is the classic textbook figure every source is ultimately quoting.

3. **Ruan et al. (2018)** (below) gives the cleanest *dictation* measurement: 153 wpm of English speech input.

4. **A 2020-21 replication of Tauroza & Allison** reports contemporary British English at 198 wpm / 265 syllables per minute, versus the 1990 baseline of 170 wpm / 240 syllables per minute. Source: <https://francis-press.com/papers/4225>. **Caveat: Francis Academic Press is a low-tier pay-to-publish outlet. I could not obtain and verify the underlying corpus numbers from a better source. Cite the 1990 Applied Linguistics paper, not this replication.**

5. **Brysbaert, M. (2019). "How many words do we speak per minute? A simple and reliable measure of speech rate." *J. Speech Lang. Hear. Res.*, 62(11), 4686-4700** is the paper I most wanted to cite, because Brysbaert did exactly the meta-analytic work you want. **I could not verify it exists.** Crossref, Europe PMC, Semantic Scholar and Ghent University's own repository all return no hit for the title. Brysbaert's 2019 *reading*-rate meta-analysis is real and verified (*J. Memory and Language* 109:104047, DOI 10.1016/j.jml.2019.104047) but that is a different paper about a different thing. **Do not cite a Brysbaert speech-rate paper.** I may have misremembered the title; if someone can produce the DOI, that is the best citation available.

**The honest summary of the literature:** ~150 wpm is a defensible central estimate for an English adult talking at a normal pace, and it is the right number for *dictation* specifically. It is not defensible as a claim about "conversational speaking rate" with one decimal place, and any source that gives you 150 wpm to three significant figures for "conversation" is over-reporting its precision.

### 2.4 Speaking record, and what a physically impossible number looks like

> **Fastest talker (English)** - Guinness World Records: <https://www.guinnessworldrecords.com/world-records/358936-fastest-talker>

Verbatim from the Guinness page:

> Sean Shannon (Canada) recited Hamlet's soliloquy `To be or not to be' (260 words) in a time of 23.8 seconds (**655 words per minute**) at Edinburgh on 30 August 1995.

Related holders:
- **Steve Woodmore** (UK, 1959-2023): 637 wpm, held the Guinness title for five consecutive years. <https://en.wikipedia.org/wiki/Steve_Woodmore>
- **Fran Capo** (US): 603.32 wpm, Guinness "Fastest Speaking Woman". <https://en.wikipedia.org/wiki/Fran_Capo>

**Critical framing for your purposes:** all three are **recitation of a pre-memorised text**, not spontaneous intelligible speech. Woodmore in particular produced a rapid-fire repeated phrase. The Guinness category was deliberately consolidated onto the Hamlet soliloquy as the measure, which is at least a fixed, uniform benchmark.

**So: physically impossible thresholds for a spontaneous dictation, calibrated against the above.**

- **> 250 wpm**: outside anything measured in spontaneous or conversational English. The 2020-21 British English gross figure of 198 wpm is the highest corpus measurement I found. If a product reports a *user's* spontaneous dictation above ~250 wpm, either the duration denominator is wrong (silence not counted, or an ASR artifact inflating word count) or the number is fabricated.
- **> 400 wpm**: only achievable by recitation or repetition, never by spontaneous dictation. Reserved for the deliberate-practise end.
- **> 655 wpm**: the all-time Guinness ceiling for English. Nothing legitimate exceeds it.
- **Teletype's current "290 wpm"** sits above the highest corpus-measured spontaneous British English figure I could find (198 wpm) and roughly 1.9x the central conversational estimate. It is not physically impossible, which is the problem: it looks like a bad measurement rather than an obvious bug, and that is harder to catch.

### 2.5 The single best head-to-head study, and why its "3x" is actually defensible

> Ruan, S., Wobbrock, J. O., Liou, K., Ng, A., & Landay, J. (2018). Comparing Speech and Keyboard Text Entry for Short Messages in Two Languages on Touchscreen Phones. *Proceedings of the ACM on Interactive, Mobile, Wearable and Ubiquitous Technologies (IMWUT)*, 1(4), Article 23. DOI: [10.1145/3161187](https://doi.org/10.1145/3161187). Preprint: [arXiv:1608.07323](https://arxiv.org/abs/1608.07323). Lab page: <https://hci.stanford.edu/research/speech>

Design: 32 participants, 50 trials per method per participant (3,200 data points), iPhone 6 Plus, English and Mandarin, Baidu Deep Speech 2 vs the built-in Apple keyboards, laboratory upper-bound conditions. Researchers from Stanford, University of Washington, and Baidu.

Published results (from the abstract, verified at arXiv:1608.07323):

> "We found that with speech recognition, the English input rate was **2.93 times faster (153 vs. 52 WPM)**, and the Mandarin Chinese input rate was 2.87 times faster (123 vs. 43 WPM) than the keyboard for short message transcription under laboratory conditions for both methods. Furthermore, although speech made fewer errors during entry (5.30% vs. 11.22% corrected error rate), it left slightly more errors in the final transcribed text (1.30% vs. 0.79% uncorrected error rate)."

Preprint-era numbers (Stanford HCI PDF, <https://hci.stanford.edu/research/speech/paper/speech_paper.pdf>): English keyboard 53.46 wpm (SD 13.97) vs speech 161.20 wpm (SD 16.37), a 3.0x ratio.

**Why this study is methodologically the gold standard, and why almost nobody cites it properly:**

The measured quantity is **entry rate: the elapsed time from starting a message to the final correct text being on screen.** It therefore **includes the correction time.** The study did not stop the clock at first draft. That is the single most important methodological fact in this entire report, and it is the thing the entire dictation marketing industry omits.

**Its limits, which are real:**
- It measures **short message transcription**, not drafting a 600-word report.
- The keyboard is a **miniature touchscreen keyboard**, not a physical desktop keyboard. Using 52 wpm from this study as a *desktop* typing figure is wrong and widespread.
- Upper-bound laboratory conditions, the authors' own framing.
- No "time saved" figure at all. It reports a ratio of two measured rates, nothing more.

### 2.6 What other products actually claim

I read first-party pages. Being blunt about which ones are honest:

**Wispr Flow** - **not measured, constants stated openly.**
<https://wisprflow.ai/post/voice-productivity>, verbatim:

> - **Typing speed:** 40 WPM
> - **Speaking speed:** 150+ WPM
> - **Average time saved:** 3-4x faster document creation

And: "an email that takes five minutes to type takes about ninety seconds to say." Site tagline: "Talk 4x faster". No source is given for either constant. 150/40 = 3.75, which is where "3-4x" and "4x" come from. **This is Teletype's bug with a citation-free constant attached.** The only thing in Wispr's favour is that they *state* the constants, so a reader can audit the arithmetic. Their own number also contradicts itself: the article is titled "Speak 4x Faster" while the body says "3-4x".

**SuperWhisper** - **the most honest implementation I found, on the WPM, and opaque on the saving.**
<https://superwhisper.com/docs/get-started/history>, verbatim:

> Superwhisper tracks summary statistics across your recordings: total dictations, total words, total recording time, and **average words per minute**.

Their iOS stats screen (image alt text on the same page) shows "**words per minute, minutes saved**, and most used mode". Their CLI exposes `stats`: "total recordings, words, time spent, and your average wpm."

This is the model: **wpm = words / actual recorded duration.** They also ship a typing-speed test at <https://superwhisper.com/typing-speed-test> that measures the user's *own* typing wpm against their own speaking wpm, with this reference table:

| WPM | Typing | Speaking |
| --- | --- | --- |
| 40 | Average typist | Slow, deliberate speech |
| 70 | Fast office worker | Audiobook narration |
| 100 | Top few percent of typists | Relaxed conversation |
| 150 | Competitive typist | Normal conversation |

Their prose claim: "The average person types about 40 words per minute and speaks at 120 to 160."

Two things to note honestly:
1. **Their "minutes saved" formula is not published anywhere I could find.** I searched the docs index (`https://superwhisper.com/docs/llms.txt`), the History page, the CLI page, the typing-speed-test page, and the enterprise Usage Analytics page. Usage Analytics explicitly collects "Duration of recordings (in seconds)" but never says what it divides that by. It is almost certainly `words / assumed_typing_wpm - measured_recording_time`, i.e. the standard model, but that is my inference, not their statement.
2. **Their changelog contains the tell:** <https://superwhisper.com/changelog> includes the fix "Fixed **negative time saved** showing on homepage stats". A savings figure that can go negative is one that subtracts a measured duration from an assumed one. That is a good sign about the model and a bad sign about the copy, because the copy evidently has not always handled it.

So: SuperWhisper measures its wpm honestly, personalises the typing assumption by actually testing the user, and discloses neither the saving formula nor its typing constant on the stats screen. **Better than everyone else. Not exemplary.**

**Otter.ai** - **self-reported survey, not a measurement.**
<https://otter.ai/how-otter-ai-can-save-you-time-at-work>, verbatim:

> 62% of users reporting weekly time savings of at least 4 hours - that's over 1 month a year!

And at <https://otter.ai/blog/otter-vs-fireflies>: "62% of users reporting Otter helps them save at least 4 hours per week."

This is a **user-opinion survey of a vague construct**, with no sample size, no methodology, and no control. It is a claim about sentiment, presented next to a very large number. There is no public sample size, question wording, field dates, or N. **I would not put this on a marketing page without the qualifier, and Teletype should not do anything like it.**

**Granola** - **no numeric time-saved claim found.** Its marketing is qualitative ("Give your full attention", "Huge time saver" quoted from a G2 review). Its blog is about *how* to use AI notes, not about how much time that saves. **This is the most restrained of the meeting-note products and it is a legitimate strategy.** I found no first-party numeric claim to evaluate.

**Fireflies.ai** - **I could not verify a first-party numeric time-saved claim.** The "3-5 hours saved per week" figures I found circulating are third-party blog posts and a Reddit user report, not Fireflies' own published methodology. Stating this plainly rather than quoting a number I cannot source.

**macOS / iOS built-in Dictation (Apple)** - **Apple makes no speed claim at all.** I found no Apple first-party page asserting a words-per-minute figure, a speedup multiple, or a time-saved quantity. Every "Apple dictation is 3x faster" statement I found comes from third-party blogs. **This is worth copying.** The world's most-used dictation feature refuses to make the claim.

**EnviousWispr** - no feature, inconsistent unsourced blog copy (see Part 1.1).

**A useful contrast worth recording:** a third-party open-source analytics tool built on SuperWhisper's history export shows exactly the honest shape of this metric and how a third party applies it (<https://github.com/crarau/superwhisper-analysis>):

> Total Recording Time: 46.9 hours; Total Estimated Words: 317,069; **Average Speaking Speed: 126.7 WPM**
> TIME SAVED vs DIFFERENT TYPING SPEEDS: vs Casual Typing (35 WPM): 106.0 hours / vs Professional (60 WPM): 44.2 hours / vs Fast Typing (80 WPM): 23.5 hours

That is the right shape: a **measured** speaking rate, a **band** of typing assumptions rather than one number, and a sensitivity table. Note that the answer moves by 4.5x depending on which typing constant you pick. That spread is the whole problem with "time saved".

---

## Part 3. Recommendation for Teletype

### 3.1 The bug, stated precisely

Teletype computes `total_words / (total_words / 150)` with integer division. That is algebraically `150`, except when integer division floors to 0, at which point it renders the raw word count instead. Your observation is exactly right: 290 words prints "290 wpm". The 150 constant is doing all the work, and the "7x faster" is that same 150 divided by a typing constant you also hardcoded. **Nothing is measured. Delete both lines.**

The non-monotonicity at 149/150 words is the division collapsing to zero at the boundary. Any fix must not reintroduce a division by a duration that can be zero.

### 3.2 What Teletype can claim honestly TODAY, with word count alone

Word count per dictation is real data. It supports exactly one class of claim: **volume**.

| Claim | Verdict |
| --- | --- |
| "You have dictated N words" | **Fully honest.** Directly measured. |
| "You have made N dictations" | **Fully honest.** Directly counted. |
| "Your dictations average N words" | **Fully honest.** Directly measured. |
| "You have spoken N words out loud" | **Honest**, and worth saying, because it is the number people are actually curious about. |
| "You speak at N wpm" | **Impossible today.** You have no duration. Any N is invented. |
| "N x faster than typing" | **Impossible today.** Requires a duration to compute your side of the ratio. |
| "You have saved X minutes" | **Not defensible today**, for reasons in 3.5. |

The honest move for the N figure is: **delete "You speak at N words per minute" entirely.** Do not re-label the constant, do not soften it to "an estimated", do not show it greyed out. It is a measurement slot with nothing in it, and the fastest way to lose a technical user's trust is to show them a number that cannot be true.

**A concrete, shippable, honest Insights screen today:**

- Words dictated (total)
- Dictations (count)
- Average words per dictation
- A weekday or weekly activity chart of dictation count or word volume
- Optionally: total characters, or a "longest dictation: N words" record

That is a genuinely useful screen built entirely on data you actually have. It is also, not coincidentally, a better product than a fabricated speedup multiplier: word volume is something a user recognises their own life in, and a speedup ratio is something they can check.

### 3.3 What Teletype must record to claim more

**Required: a per-dictation duration, in milliseconds, captured with a monotonic clock.**

- Record `startedAt` at the moment recording actually begins (after the VAD/onset gate, matching what the user perceives as "I started talking", and consistent across the pipeline). Record `endedAt` at the moment recording stops.
- Persist both on the dictation row, alongside the word count. `durationMs: u32` plus `startedAt` is enough; do not store audio for this purpose.
- Use a monotonic clock (`std::time::Instant` on the Rust side, or `ContinuousClock` on the Swift side, or `CLOCK_MONOTONIC` on macOS) for the *delta*. Store an epoch `Date` separately for the calendar position. This is the single most important implementation detail: NTP sync, timezone change, DST, sleep/wake, and the user editing the system clock must not be able to produce a negative or 600x duration.
- EnviousWispr got this right and then threw it away. Its `recordingStartedAtTick` plus a checked non-wrapping subtraction (`RecordingSessionKernel.swift:735-740`) is the correct pattern, including the explicit `guard now >= start else { return 0 }` guard against a regressing clock. Copy that guard.
- Reject takes below a floor (EnviousWispr uses a "minimumRecordingTicks" discard gate for exactly this reason). A 0.2-second tap must not enter the rate denominator.

**Also strongly recommended:**

- **Word count on the FINAL text the user received**, not on the raw ASR output. Filler removal and punctuation change the count. If you want "the number of words you got out", that is the number to store. Store both if you like; label clearly which is which.
- **A whitespace-delimited word count, and say so.** Do not use an aggressive tokenizer that splits contractions and hyphens. EnviousWispr documented this exact decision in `TranscriptSplitter.swift:250-254`: `NLTokenizer(unit: .word)` counts "well-known" and "don't" as several tokens, so your number would not be comparable to any published wpm figure, including your own typing comparison. Whitespace-delimited is the convention the whole literature uses.
- **The per-dictation WPM**, computed as `wordCount / durationMinutes`, so the rate is per-take and the aggregate is a **word-weighted** mean, never a mean of means. A mean of per-take rates over-weights one-word dictations.
- Optionally, a **p50 and p90**, not just a mean. A user's dictation rate distribution is long-tailed toward slow.
- Optionally, per-word ASR timings if your engine exposes them. EnviousWispr already has `ASRWordTiming` plumbed for file import (`ASRResult.swift:80-85`) and never uses it for dictation. If Teletype's engine can emit them, they let you compute an **articulated** rate (silence removed) as well as a gross rate, which is the honest way to compare against the 150 wpm literature figure.

**And one more field worth having from day one, even if you never use it in Insights:**

- **Edits.** If you ever want to claim anything net of a correction tax, you need to know how much the user changed after Teletype produced the text. This is the single biggest hole in every other product's claim and it is the difference between "3x" and "2.5x". Ruan et al. solved it by measuring end-to-end entry rate including corrections. You cannot retro-fit it. If you want to earn a corrected number later, start counting edit distance now.

### 3.4 "Nx faster than typing" once you have duration

With measured duration, the calculation becomes:

```
measured_wpm       = total_words / total_recording_minutes
assumed_typing_wpm = 52          (Dhakal et al. 2018, typing-test mean, everyday users)
speedup            = measured_wpm / assumed_typing_wpm
```

This is defensible **only if you name the typing constant and its source in the UI.** Without that, it is the same fabrication with better arithmetic.

Three honest refinements, in increasing order of defensibility:

1. **Show the arithmetic.** "You dictated at 142 wpm. The 2018 Aalto study measured everyday typists at 52 wpm. That's 2.7x." A user who can see the denominator can disagree with it, which is fine. A user who cannot see it has to trust you.
2. **Let the user set their own typing speed.** SuperWhisper's typing-speed test is the right idea. A user who types at 95 wpm is not getting a 2.7x speedup, and showing them 2.7x because you assumed 52 is wrong *about them specifically*. Offer "take a 1-minute typing test" and use the result. This is the single highest-value honesty upgrade available and it costs one screen.
3. **Report a band, not a point.** If the user has not tested, show the speedup at 40 / 52 / 70 wpm: "2.5x to 3.6x faster, depending on how fast you type." The spread is real and the range is more informative than a false precision.

### 3.5 Is "time saved" defensible from word count alone?

**No. Not from word count alone, and not even from word count plus duration, without a stated assumption and a stated scope.**

The honest decomposition of what "time saved" asserts:

```
time_saved = (words / typing_wpm - words / speaking_wpm) - correction_cost
             \_______________________  ______________________/
                  ASSUMED               ASSUMED or MEASURED
```

Three separate claims are bundled together, and they have very different standing:

**(a) `words / typing_wpm` is an assumption, always.** It depends entirely on how fast *this specific user* types on *their* hardware while *composing*, which is a task the Dhakal study did not measure. Real numbers for typing speed vary by 5x across real users. The spread is not an inconvenience to be hidden in a footnote, it is the dominant term.

**(b) `words / speaking_wpm` should be a measurement.** If Teletype records duration, this becomes measured rather than assumed, and the "time saved" figure becomes *much* stronger. Note that with a measured duration, the correct denominator is your own measured rate, not a "typical 150 wpm" constant. That is a meaningful improvement over every competitor.

**(c) `correction_cost` is currently invisible to everyone.** Ruan et al. is the only study in this report that measures it, and it did so by timing to final-correct-text. Every vendor's "3x" omits it, and the industry commentary estimate is that the net advantage falls to roughly 2-2.5x once editing is included. This is the load-bearing omission.

**On your specific question: "is it defensible if the tool genuinely does reduce the effort?"**

Partly, and only with a narrower claim than the one you are currently making. The word "time" is the problem. There are two different quantities and you should not conflate them:

- **Time actually spent on the keyboard**: reduced. You can measure your side. You cannot measure their side.
- **Time actually saved overall**: depends on the delta between what they would have typed and what they actually do, including the edits they now have to make, the corrections, the re-listening, and whether the text they would have typed was even the same text. **This is not derivable from word count. It is not derivable from word count plus duration. It is only derivable from an A/B measurement of the actual user over weeks, or from a stated, sourced, and clearly-labelled estimate.**

The one genuinely defensible move from word count alone is the **volume-equivalent** framing, which sidesteps every assumption:

> "You have dictated 4,200 words. That is about 35 average Slack messages, or 6 blog posts, without touching the keyboard."

"35 Slack messages" is a conversion between two things you both know the size of, not a claim about anyone's speed. It is honest, it is memorable, and it needs no constants at all. If you ship this next to the word count, you lose nothing and you gain credibility.

**What is not defensible, ever, from the data you have:**
- "You have saved X minutes" as a bare number with no constant, no scope, and no measurement
- "Nx faster than typing" when x comes from a hardcoded ratio
- "You speak at N wpm" when N comes from dividing a word count by itself
- Any number that gets smaller as you dictate more (the 149/150 bug is the visible symptom of a model that has no monotonicity in it)
- Any number that can go negative (SuperWhisper's bug). Time saved is not negative. If your model produces a negative, your typing constant is too optimistic for that user, and the honest output is "no measurable difference for this dictation", not "-3 minutes".

### 3.6 Sanity bounds you should enforce in code

Cheap assertions that catch the entire class of bug:

- `duration_ms >= MIN_DURATION_MS` (EnviousWispr uses a discard gate; pick yours, ~500 ms is defensible)
- `word_count > 0`
- `wpm <= 400` for a spontaneous dictation take. If a take exceeds 400 wpm it is not a speaking rate, it is a bug. Log and exclude, never display.
- `wpm >= 20` for anything you intend to aggregate. Below that you are counting keypresses and dead air.
- `time_saved_minutes = max(0, ...)` at the presentation layer, and surface the raw value to a log, not to the user.
- **Monotonicity test:** the aggregate time-saved figure must be non-decreasing as words increase, holding typing_wpm fixed. A unit test that asserts `saved(300) > saved(200)` would have caught today's bug immediately. This is a two-line test and it is the highest-value thing on this list.

---

## Part 4. Proposed honest copy

### 4.1 Today, before any schema change

Replace the three fabricated tiles. Exact wording:

> **Words dictated**
> 4,182
> *All time. Every dictation.*

> **Dictations**
> 96
> *Average 44 words each*

> **Longest dictation**
> 612 words
> *"…" (3 min 41 sec)*, *only if and only if you already store duration; otherwise drop the time*

Section header instead of "Insights":

> **Your dictations**

And one line of framing, which is true and costs you nothing:

> Teletype does not estimate how much time you saved. It counts what you said.

I like that line. It is a differentiator against every competitor in the category, it is literally true, and it buys you permission to add the estimate later once it is earned.

### 4.2 Once you record per-dictation duration

Primary tile, measured:

> **Your speaking rate**
> 142 words per minute
> *Measured across 96 dictations, 3 h 12 min of speech.*

Compare tile, assumption named:

> **Against typing**
> 2.7x faster than typing
> *Your 142 wpm, against 52 wpm for everyday typists (Dhakal et al., CHI 2018, 168,000 people). Take a 1-minute typing test to use your own number.*

Band version, if no test has been taken:

> **Against typing**
> 2.0x to 3.6x faster
> *Your 142 wpm. Everyday typists manage 40 to 70 wpm, so the speedup lands anywhere in that range. Not sure where you sit? Take a 1-minute test.*

Savings tile, honestly scoped:

> **Time back on the keyboard**
> ~81 minutes
> *An estimate. It assumes you would have typed at 52 wpm, and it does not count time you spend editing the result.*

That last clause is the whole ballgame. It is the clause that no competitor prints, and printing it makes the number above it credible instead of suspicious.

### 4.3 Copy that is specifically worth stealing or avoiding

Steal, because it is the only honest framing found in a competitor:

> "The catch with dictation was always the cleanup. Raw transcripts arrived as one long run-on with the ums left in, and fixing them ate the time you saved. That's the part that changed."
> Source: <https://superwhisper.com/typing-speed-test>

This is SuperWhisper naming the correction tax out loud. It costs them the headline and buys them the reader.

Steal, because Apple invented it by not saying anything:

> Say nothing about speed. Apple's built-in Dictation is used by millions and Apple has never published a speedup claim. There is nothing to beat here except your own honesty.

Avoid, each of these is in the wild and each is a lie of a different shape:

- "Talk 4x faster" (Wispr Flow) - a ratio of two constants, one of which is wrong
- "You speak at 290 wpm" (Teletype, today) - a word count wearing a rate's clothes
- "You have saved 3 minutes" from 149 words but 2 from 150 (Teletype, today) - a model with no monotonicity, which is a tell that it has no model at all
- "62% of users save 4 hours a week" (Otter) - a survey of a vague feeling, next to a very large number
- "3x, 4x, and 5x" (EnviousWispr's own blog, variously) - four numbers for one quantity
- Any savings figure that can render negative (SuperWhisper's shipped bug)

### 4.4 One-line summary for the changelog

> Insights now shows what we can actually measure: how many words you have dictated and how fast you speak. It no longer estimates how much time you saved, because estimating that honestly requires knowing how fast *you* type, and we would rather ask than assume.

---

## Appendix: verification notes

**Verified directly, code read line by line:** all Part 1 file:line pointers and quoted code.

**Verified directly, first-party page read:** Wispr Flow's constants; SuperWhisper's stats documentation, WPM test, typing test, changelog, and enterprise analytics docs; Otter's 62%/4-hours claim; Granola's absence of a numeric claim; the Guinness fastest-talker page; the Stanford HCI study page and the Ruan et al. abstract; the Aalto open-access PDF listing and the ScienceDaily press release; the ERIC record for Tauroza & Allison.

**Could not verify, and flagged as such above:**
- A Brysbaert speech-rate meta-analysis. I could not confirm this paper exists. Do not cite it.
- The "196 wpm gross / 236 wpm net over 2,438 Switchboard conversations" pair, which circulates widely in vendor blogs. I could not trace it to a primary source. Do not cite it.
- "Karat and colleagues (1999), 32.5 wpm transcribing / 19.0 wpm composing." Could not trace the primary source. Do not cite it.
- The exact formula behind SuperWhisper's "minutes saved". Not published. The negative-value bug in their changelog tells you the shape but not the arithmetic.
- A first-party Fireflies.ai numeric time-saved claim. Only third-party blog claims were found.
- The 1990 baseline numbers in Tauroza & Allison (170 wpm). These come from a 2020 replication published by Francis Academic Press, a pay-to-publish outlet. Cite the 1990 Applied Linguistics paper itself, which is peer-reviewed and solid, rather than the replication's rendering of its numbers.
- Whether `SentryBreadcrumb.RecordingSnapshot.transcriptWordCount` is ever populated. It defaults to 0 and I found no non-zero call site. Flagged as unpopulated, not asserted dead.
