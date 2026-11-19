# Brag Plan: Teletype

## What is this app?

Teletype is a local-first voice dictation app: hold `⌘⇧Space`, speak, and your words are transcribed on-device, cleaned up, and typed into whatever app has focus. No cloud, no account, no subscription.

## The angle

**The quiet instrument, with a paranoid hook.**

Every cloud dictation tool quietly turns your voice into someone else's data. Teletype is the opposite: the entire pipeline runs inside your Mac. The film opens on the problem — words leaving the machine — then snaps to the product's paper-white world and shows the same words staying home. It closes on the two things a cloud tool cannot offer: a record you own and the time you get back.

The user asked for three things, in order: **the problem**, **how Teletype solves it**, **why now**. The storyboard is built around exactly that arc.

## Hook (first 2-3 seconds)

Dark frame. A miniature Mac with a live waveform, a dotted path, and violet packets climbing toward a cloud labelled **THEIR SERVERS**, with a small "uploading audio" bar filling underneath.

Headline: **"Your words are leaving your Mac."** — the word *leaving* in Teletype violet.

## Key moments (the middle)

- **The hotkey loop** — `⌘⇧Space` pressed, the dark floating pill appears ("Listening" → "Transcribing" → "Done"), and a real sentence types itself into a document: *"The deploy went out this morning and it's all good."* This is the product doing its thing, not a diagram of it.
- **The proof** — a Mac boundary with a mic → *Parakeet · Whisper* → clean text pipeline inside it; a packet tries to leave, bounces off the boundary, and a violet slash lands on the cloud: **NEVER UPLOADED**.
- **The payoff** — "so the, **uh**, deploy went out, **um**, this morning" loses its fillers, then a record card shows a day-wise `.md` archive next to **21 minutes saved per 1,000 words**.

## Outro / punchline

Three chips land one by one — **No cloud. No account. No subscription.** — then the lockup:

**Teletype.**
*Talk instead of type.*
Free · open source · on your Mac.

## User flow worth showing

1. **Entry:** hold `⌘⇧Space`.
2. **Key action:** speak; the floating pill shows listening → transcribing → done.
3. **Result:** cleaned text is typed into the focused app, archived locally, and counted in Insights.

## Tone

- Preset: `polished`
- Creative direction: *quiet premium product film with a paranoid hook* — serious about privacy, confident enough not to shout.
- Interpretation: long holds, restrained motion, one violet accent, no joke-a-second editing. The dark hook earns the white reveal; the product footage carries the middle.

## Format: landscape — 1920x1080
## Duration: 22.37 seconds

## Visual identity (from the project)

- Background: `#f5f6f8` (DESIGN.md canvas) — paper-white surfaces `#ffffff`
- Text: `#141821` (DESIGN.md primary text)
- Text secondary: `#6b7280`
- Accent: Teletype Violet `#702fd2`; light violet `#b69cff` on dark
- Dark canvas: `#101014`; pill shell `#181820`
- Pill recording red: `#ff453d`; pill activity cyan: `#1fd5f9`; success `#16a34a`
- Display font: **Open Runde** (bundled in `ui/public/fonts/`)
- Body font: **Inter** (bundled in `ui/public/fonts/`)
- Strongest visual elements: the paper-white "quiet instrument" surfaces, the dark floating pill, the violet accent used sparingly, the Insights metric numeral, the app icon.

## Share copy (draft)

Your voice never leaves your Mac. Teletype dictates on-device, cleans up the "um"s, and types straight into any app — free, open source, and offline after setup.

## Audio direction

- Role: warm, steady, professional bed with a small number of motion-matched accents.
- Music: `happy-beats-business-moves-vol-12-by-ende-dot-app.mp3` (steady and clean; the polished/cinematic track). 109.96 BPM.
- Music treatment: start at 0, volume 0.32, 0.5s fade-in, 1.4s fade-out under the final frame.
- Music cue guidance: bundled preset read. Strong cues used: **8.74s** (local proof reveal), **13.11s** (cleanup cut), **19.66s** (brand lockup). Metric count-up lands at **17.47s**. Scene cuts sit on the beat grid at 3.27, 8.74, 13.11, 18.56, 22.37.
- Audio-reactive treatment: subtle. A radial violet glow breathes with the track's bass at each scene entry; the pill's cyan activity dot picks up treble. No waveforms, no equalizer bars beyond the product's own pill.
- SFX posture: sparse, polished. Pill pop, cloud block, filler click, metric glass tick, logo bell.
- Audio-coupled moments: the pill pops on a soft drop; the typed sentence gets two quiet keypress ticks; the cloud block gets a switch; the 21 lands on a glass tick; the logo gets one bell.
- Restraint rule: no whooshes on every transition, no risers, no bass drops. The music carries the arc.

## Storyboard

### Scene 1 — The leak — 3.27s
Dark `#101014` frame. A miniature Mac (screen + base) sits left of centre with a mic glyph and a live violet waveform. A dotted path curves up to a cloud silhouette at right labelled **THEIR SERVERS**. Violet packets climb the path; an "uploading audio" bar fills under the cloud. Headline above: **"Your words are leaving your Mac."** with *leaving* in `#b69cff`.
Sequential/interaction: yes — four packets travel the path, staggered; upload bar fills.
Audio intent: quiet unease, no percussion spike; let the bed establish.
Audio-coupled idea: none; the music carries the hook.
Transition mood: dip cut → Scene 2.

### Scene 2 — The loop — 5.47s
Hard cut to paper `#f5f6f8`. Top-left: **Teletype.** + *Talk instead of type.* A document window ("Draft.md") slides up; the floating dark pill pops in over its lower-right corner. Pill states: **Listening** (red dot, waveform) → **Transcribing** → **Done** (green check). The sentence *"The deploy went out this morning and it's all good."* wipes into the document with a violet caret.
Sequential/interaction: yes — hotkey hint, pill state changes, sentence types left-to-right.
Audio intent: the bed lifts; the product feels alive.
Audio-coupled idea: one soft drop for the pill, two quiet keypress ticks during typing.
Transition mood: clean hard cut → Scene 3.

### Scene 3 — Stays home — 4.37s
Paper frame. Headline: **"Your audio never leaves this Mac."** with *never* in violet. A Mac boundary card holds a mic → **Parakeet · Whisper** chip → clean-text card; a packet tries to leave, bounces off the boundary, and a violet slash crosses the grey cloud outside. Label: **NEVER UPLOADED**.
Sequential/interaction: yes — pipeline builds left to right, packet bounces, slash draws.
Audio intent: the argument lands; a dry switch punctuates the block.
Audio-coupled idea: switch sound at the slash.
Transition mood: hard cut on the strong cue → Scene 4.

### Scene 4 — Clean, kept, counted — 5.45s
Paper frame. A cleanup card shows *"so the, uh, deploy went out, um, this morning"*; the filler words strike through and vanish, then *"The deploy went out this morning."* appears with a green check. The card gives way to a record card: left, a day-wise `.md` archive; right, **INSIGHTS** with a large **21**, counting up, and *minutes saved per 1,000 words*. The metric lands on the strong cue at 17.47s.
Sequential/interaction: yes — filler strike → vanish → clean line; archive/insight card swap; numeral count-up.
Audio intent: satisfying, tidy, quiet.
Audio-coupled idea: click for the filler removal; soft drop for the record card; glass tick when 21 lands.
Transition mood: hard cut → Scene 5.

### Scene 5 — Why now — 3.81s
Paper frame with a faint violet glow. Three chips land one by one: **No cloud. / No account. / No subscription.** At the strong cue, the lockup appears: app icon + **Teletype.**, *Talk instead of type.*, and **Free · open source · on your Mac.** A violet underline draws under the wordmark, then the frame holds.
Sequential/interaction: yes — three chips, then icon + wordmark + tagline.
Audio intent: resolution; one bell, then let the bed fade.
Audio-coupled idea: one bell on the lockup; music fades out under the final hold.
Transition mood: end on a settled frame.

**Music mood for this video:** steady, warm, quietly confident.
**Audio summary:** dark low bed under the hook, the same bed lifting as the product appears, dry motion-matched accents through the proof and payoff, one bell and a fade for the lockup.

## Grounding notes

- "Your words are leaving your Mac" is the film's framing of the README's stated problem for cloud tools ("ship your audio to a company's servers").
- "The deploy went out this morning and it's all good." is the README's own example.
- "Parakeet · Whisper", "on-device", "never uploaded", "day-wise archive", "Insights", "21 minutes saved per 1,000 words" are from README.md / usp.md.
- "No cloud. No account. No subscription." is from the README comparison table and the "Why local" section.
- "Free · open source · on your Mac" is from README (MIT, free, macOS).
- The floating pill's listening/transcribing/done states are from README and DESIGN.md.
- The date `2026-11-07.md` is a plausible stand-in for the day-wise archive; no personal data is shown.
