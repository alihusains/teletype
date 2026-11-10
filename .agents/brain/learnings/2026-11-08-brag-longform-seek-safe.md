# L-2026-11-08-brag-longform-seek-safe

- **ID:** L-2026-11-08-brag-longform-seek-safe
- **Title:** Long-form Brag/HyperFrames video (72 s, 8 scenes): what actually renders correctly
- **Date:** 2026-11-08
- **Agent/tool:** Buffy agent, HyperFrames CLI 0.9.23, ffmpeg 7
- **Project area:** marketing artefact (not app code) — `brag-output-2026-11-08-142531/`
- **Status:** candidate (verified by render + frame inspection; not human-reviewed)

## Context

The `brag` skill targets 15–25 s. This task required exactly 72 s, 1920×1080, eight
fixed scenes, voiceover, music and SFX. The composition is one root
`composition/index.html` with eight timed clips, not sub-compositions.

## Durable lessons

1. **One root `index.html` with timed scene clips beats sub-compositions at this length.**
   `check` warns `nested_structure_needs_subcomposition` eight times and
   `composition_file_too_large` once; both are benign. The single file stayed editable and
   the render never diverged from the preview.

2. **Every animation must be seek-correct, or the render silently shows a different film
   than the preview.** Rules that worked:
   - Typing = pre-create one `<span>` per character, then `gsap` stagger their opacity.
     Never `tl.call(() => el.textContent += ch)`.
   - State swaps (Listening → Done, before → after) = two stacked labels crossfading,
     never a text mutation.
   - Count-ups = a column of discrete, masked values crossfading.
   - Scene-in/scene-out = clip `data-start`/`data-duration`, so seeking into a scene
     reconstructs state without replaying earlier tweens.

3. **Snapshot the settled frames, not only the ones mid-reveal.** A heat map built with
   `cells.filter(i => i % 2 === 0)` looked fine mid-animation (scattered cells) but was a
   half-empty checkerboard once settled. Snapshot at `scene_end - 1.5 s` as well.

4. **Kokoro TTS can fail hard and needs a scripted fallback.** Eight consecutive attempts
   returned `Speech synthesis failed: Download failed: HTTP 504`. Fallback that shipped:
   `say -v Samantha -r 168` → 44.1 kHz mono WAV → `loudnorm=I=-16:TP=-1.5:LRA=11` per file.
   Always disclose the substitution in the final report; the voice is audibly less
   produced than a neural TTS.

5. **Poster bake + verification.** Extract the chosen frame with `ffmpeg -ss <t> -frames:v 1
   -q:v 2 brag.jpg`, then overlay it on frame 0 only:
   `overlay=0:0:enable='eq(n,0)'` with `-c:a copy`. Re-encoding means frame 0 will *not* be
   bit-identical to the JPEG; `ssim All:` around 0.99 is the pass signal, not a defect.

6. **Render cost.** 72 s at 1080p30 (2160 frames), 3 workers, `--quality delivery`:
   ~34 s wall clock, ~8 MB h264 + AAC. Cheap enough to re-render after a fix rather than
   preview-only verification.

## Evidence

- `brag-output-2026-11-08-142531/composition/index.html` (final composition)
- `brag-output-2026-11-08-142531/brag-freebuff.mp4` — `ffprobe`: 1920×1080, 30 fps,
  2160 frames, duration exactly 72.000000
- `.hyperframes/artifacts/checks/*.json` — `ok: true`, 0 lint/layout/contrast/audio errors
- Frame dumps at 2.0/9.0/16.5/26.0/30.5/39.0/50.5/61.5/70.5 s inspected for artefacts

## Reusable rule

Write motion as declarative, seek-safe state (spans, stacked labels, discrete values);
verify both mid-animation and settled frames; treat a TTS download failure as expected and
ship a loudness-normalised `say` fallback with a disclosure.

## Scene 4 fix (2026-11-08, pi session)

Scene 4 (The Reveal + Demo, 21–33s) had two render bugs that the draft-quality
render exposed but the snapshot engine masked:

1. **`fromTo` with `opacity: 0` on a flex container** (`#s4-reveal`, `#s4-demo`,
   `#s4-pill`) caused the render to show the elements ~3s late. The snapshot
   engine applied the `from` state at seek time correctly, but the render's
   per-frame seek did not. Fix: make the container CSS-visible by default and
   animate only child transforms + `autoAlpha` for the exit.

2. **`tl.call()` text mutation** (`textContent += ch`) is not seek-safe — the
   render shows empty text at any seeked frame. Fix: pre-create one `<span>`
   per character in the HTML (static, not JS-created), then animate with a
   selector-based stagger: `tl.to("#s4-cleantext span", { opacity: 1, stagger: … })`.
   JS-created spans passed as an array to `tl.to(array, …)` are reported as
   `__unresolved__` by the keyframe engine and never animate in the render.

3. **Timing math**: scene 4 is 21–33s absolute. Local time = abs − 21.
   The clean-text tween starts at local 10.9s = abs 31.9s. Frames at 31s
   (10s local) correctly show no clean text yet. Verify at abs 32.5s+
   (11.5s local, tween complete).

**Rule:** for HyperFrames render (not just snapshot), every element that must
be visible at a seeked frame must be CSS-visible by default. Never rely on
`fromTo({opacity:0})` for the entry of a container that holds other content.
Never use `tl.call()` or JS-created element arrays for text animation.
