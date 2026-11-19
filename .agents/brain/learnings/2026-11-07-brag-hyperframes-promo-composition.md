---
id: L089
title: Hyperframes promo compositions have five non-obvious check gates
status: verified
type: workflow
area: "promo-video"
agent: "opencode"
tool: "brag skill + hyperframes 0.9.19"
date: 2026-11-07
---

# Hyperframes promo compositions have five non-obvious check gates

## Problem

Building the Teletype brag video with the `brag` + Hyperframes workflow, `npx hyperframes check` rejected several patterns that look correct in ordinary HTML/GSAP.

## Symptoms

- `gsap_non_transform_motion` on a caret tweened with `left: 0% → 100%`.
- `gsap_repeated_fromto_without_baseline` on a state label with show/hide `fromTo` calls.
- `text_below_floor` craft warnings on UI chrome (14–22 px labels) and `first_frame_empty` on a scene that faded in from nothing.
- WCAG failures for `#9aa1ad` tertiary text on `#f5f6f8` / `#ffffff`.
- The check failed even though layout, runtime, motion, and audio were clean; lint was the only blocker.

## Root cause

Hyperframes treats compositions as a seek-by-frame render target, not a normal web page:

1. Animating the element that itself carries `class="clip"` is flagged; the clip is the visibility window, not the animated content.
2. Layout properties (`left`, `top`, `width`) snap to integer pixels and stutter under the frame-capture engine. Transforms (`x`, `y`, `scale`, opacity) are required.
3. Multiple `fromTo` calls on one element apply their `from` values immediately at authoring; later calls overwrite the resting state. The later call needs `immediateRender: false`.
4. The craft reviewer enforces a 28 px type floor at 1080p and a non-empty frame 0. UI chrome can opt out with `data-layout-ignore`; key copy should not.
5. The design token `#9aa1ad` is too light for small text on `#f5f6f8` / `#ffffff`. Use `#5f6673` (≈5.3:1) for small secondary labels on paper/white.

## Fix

- Wrap clip content: `<div id="x-clip" class="clip">…<div id="x">…</div></div>`; animate `#x`, never `#x-clip`.
- Replace `left` tween with a constant `x` distance measured before timeline registration.
- Add `immediateRender: false` to the later of two `fromTo` calls on the same element.
- Keep each scene's first frame populated (opacity 1 + a small transform), not blank paper or black.
- Use `#5f6673` for small secondary text; keep `#9aa1ad` for text on the dark pill only.
- Mark diagram/window chrome with `data-layout-ignore`; mark intentional crossfades with `data-layout-allow-overlap`.
- Run `npx hyperframes check`, then `npx hyperframes render --quality delivery --output ../brag.mp4`, bake `brag.jpg` into frame 0, and inspect the scene-cut frames.

## Verification

- `npx hyperframes check`: 0 errors, 0 warnings in lint/runtime/layout/motion/craft/audio; 20/20 text contrast checks passed.
- Render: 672 frames, 1920×1080, 30 fps, 22.4 s, AAC 48 kHz stereo.
- Frame inspection at 3.28, 8.75, 13.12, 18.57, 21.5 confirmed content at every cut and a settled final lockup.
- Output: `brag-output/brag.mp4`, `brag-output/brag.jpg`, `brag-output/share-copy.txt`.

## Reusable lesson

For the next Teletype promo, start from `brag-output/composition` and reuse the palette/fonts. Treat `check` as the gate, not an afterthought: wrap clip elements, animate transforms only, keep frame 0 and every cut populated, use `#5f6673` for small labels on paper, and reserve `data-layout-ignore` for genuine UI chrome. The 21-minutes-per-1,000-words claim comes from `README.md` §The impact and `crates/teletype-core/src/insights.rs` (40 WPM typing baseline).

## Evidence / sources

- Check report: `brag-output/composition/.hyperframes/artifacts/checks/` (latest passing run).
- Render log: `/tmp/tt-render2.log` (local).
- Composition: `brag-output/composition/`.
- Source claims: `README.md` §The impact, `crates/teletype-core/src/insights.rs`.
- Skill: `.agents/skills/brag/SKILL.md`, `~/.agents/skills/hyperframes-cli/SKILL.md`.
