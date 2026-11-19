# Hyperframes Composition Brief: Teletype

## Objective

Create a 22.37-second launch-style brag video for Teletype that explains the cloud-dictation problem, shows Teletype solving it on-device, and lands the "why now" with time saved and ownership.

## Output

- Composition directory: `brag-output/composition/`
- Rendered video: `brag-output/brag.mp4`
- Format: landscape — 1920x1080
- Duration: 22.37 seconds

## Source Material

- Project root: `/Users/alihusainsorathiya/Documents/projects/teletype`
- Primary files read: `README.md`, `usp.md`, `DESIGN.md`, `ui/index.html`, `ui/public/fonts/*`, `ui/public/teletype-app-icon-purple.png`
- Product name: Teletype
- Tagline / strongest claim: "Talk instead of type. Private, fast, and entirely on your machine."
- Key UI or visual moment to recreate: the floating dictation pill (Listening → Transcribing → Done) typing a sentence into a document, plus the on-device pipeline.
- Copy that must appear verbatim:
  - "Talk instead of type."
  - "The deploy went out this morning and it's all good."
  - "Your audio never leaves this Mac."
  - "21 minutes saved per 1,000 words"
  - "No cloud. No account. No subscription."
  - "Free · open source · on your Mac"

## Creative Direction

- Tone preset: `polished`
- Creative direction: quiet premium product film with a paranoid hook
- Interpretation: long holds, restrained motion, one violet accent, no frantic cuts. The dark hook earns the paper-white product reveal.
- Angle: cloud dictation leaks your voice; Teletype keeps the whole pipeline inside the Mac, then gives you the record and the time back. The closing chips and the metric are the "why now".
- Hook: "Your words are leaving your Mac." over a Mac→cloud data path.
- Outro / punchline: "No cloud. No account. No subscription." then the Teletype lockup.
- Avoid:
  - Generic SaaS language
  - Abstract filler visuals
  - Unrelated visual redesign
  - Purple as a large flooded surface

## Visual Identity

- Background: `#f5f6f8` (paper canvas); `#101014` for the dark hook
- Text: `#141821`; inverse `#ffffff`
- Accent: `#702fd2`; light violet `#b69cff` on dark; soft `#f3edfd`
- Secondary text: `#6b7280`; tertiary `#9aa1ad`; borders `#e6e9ef` / `#eef0f4`
- Success: `#16a34a`
- Pill shell: `#181820`; recording red `#ff453d`; activity cyan `#1fd5f9`
- Display font: Open Runde (files in `assets/fonts/OpenRunde-*.woff2`)
- Body font: Inter (file `assets/fonts/InterVariable.woff2`)
- Visual references from the project: floating pill, paper-white settings surfaces, violet accent dots/checks, large Insights metric numeral, app icon (`assets/brand/teletype-icon.png`).

## Storyboard

Use the storyboard in `brag-output/brag-plan.md` as the creative contract.

Scene summary:
1. The leak — 3.27s — dark Mac→cloud path, headline "Your words are leaving your Mac."
2. The loop — 5.47s — pill states + sentence types into Draft.md.
3. Stays home — 4.37s — on-device pipeline inside a Mac boundary, packet bounces, cloud slashed, "NEVER UPLOADED".
4. Clean, kept, counted — 5.45s — fillers struck, day-wise archive, 21 minutes saved metric.
5. Why now — 3.81s — three chips, then icon + "Teletype." + tagline + "Free · open source · on your Mac."

Scene boundaries: 0 / 3.27 / 8.74 / 13.11 / 18.56 / 22.37.

## Audio

- Audio role: warm steady bed with sparse motion-matched accents
- Audio arc: quiet unease → lift at the product reveal → dry proof accents → bell + fade at the lockup
- Music: `assets/music/happy-beats-business-moves-vol-12-by-ende-dot-app.mp3`
- Music treatment: volume 0.32, 0.5s fade-in, 1.4s fade-out; start at track 0:00
- Music cue guidance: bundled preset `cues/happy-beats-business-moves-vol-12-by-ende-dot-app.music-cues.json`; strong cues at 8.74, 13.11, 17.47, 19.66; beat grid at 3.27, 3.82, 4.39, 5.34, 6.00, 7.64, 8.74, 9.83, 10.93, 12.02, 13.11, 14.20, 15.84, 16.93, 17.47, 18.56, 19.66, 20.19, 21.28, 22.37.
- Audio-reactive treatment: subtle; a radial violet glow breathes with bass at each scene entry; the pill's cyan dot picks up treble. No waveform/equalizer visuals.
- Audio-coupled moments:
  - Scene 2 pill pop — soft drop
  - Scene 2 typing — two quiet keypress ticks
  - Scene 3 cloud block — switch
  - Scene 4 filler removal — click
  - Scene 4 record card — soft drop
  - Scene 4 metric landing (17.47) — glass tick
  - Scene 5 lockup (19.66) — single bell
- SFX selection guidance: sparse and low high-frequency risk; match the motion, do not score every element.
- SFX analysis guidance: `/Users/alihusainsorathiya/.agents/skills/brag/assets/sfx/sfx-analysis.md`
- Audio files copied into `assets/`.

## Hyperframes Instructions

Load the composition-building Hyperframes skills — `hyperframes` (composition contract + `data-*` timing, `references/format.md`; motion, eases and seek-safe keyframes in `references/motion.md`; look in `references/look.md`), `hyperframes-registry`, `media-use`, and `hyperframes-cli`. /brag is its own workflow: skip the storyline step, do not read `references/types/*` or `references/story.md`, do not propose storylines, and do not write `SPEC.md`. The brag plan and this brief replace them, including the LOOK line. Skip `usage`, `history`, `feedback`, and REVIEW.md; `check` is the gate.

Requirements:
- Show at least one real UI, copy, or visual element from the source project.
- Keep all text readable in the final render.
- Keep the video within 15-25 seconds.
- Include the planned music/SFX layer.
- Run `npx hyperframes check` before render.
- Keep creation and rendering local.

## LOOK line

LOOK: paper `#f5f6f8` ground, ink `#141821`, one violet `#702fd2` accent (light `#b69cff` on the `#101014` hook), Open Runde display at 600, Inter body at 400/500, 1px hairline borders `#e6e9ef`, 24-32px cards, 999px pills, low blue-tinted shadows, the dark `#181820` floating pill as the one inverted signature surface. BANNED: purple floods, glassmorphism, multi-pixel borders, generic gradient meshes, equalizer bars, whoosh transitions.
