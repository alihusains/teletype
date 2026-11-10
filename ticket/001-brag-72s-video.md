# 001 — Teletype 72s brag video

Status: ready-for-agent

## Request

Use the `/brag` skill to produce a ~72-second launch video for Teletype that:
1. Identifies the problem (cloud dictation sends your voice to a server)
2. Shows the real pain points of the user (privacy, cost, dependency)
3. Shows how Teletype solves it and why you should use it right now
4. Features (the product's capabilities)

Format: landscape 16:9. Tone: blended — polished/defiant spine, cinematic opening,
app-store clean feature cards.

Deliverables (user confirmed all three):
1. `brag-plan.md` — full 72s 4-act plan + audio/music cue guidance
2. `~25s` companion social cut plan (compressed version of the same material)
3. Rendered `brag.mp4` + poster `brag.jpg` + `share-copy.txt` in `brag-output/`

## Constraints (from PRODUCT.md / README — do not fabricate)

- No testimonials, no press, no case studies. None exist.
- Only stat allowed: ~150 wpm speaking vs 40 wpm typing; ~21 min saved per 1,000 words.
- Real model names only: Parakeet TDT, Whisper. Polish: S1-mini / SpeakoFlow Mini.
- Tagline verbatim: "Talk instead of type. Private, fast, and entirely on your machine."
- No generic SaaS language ("streamline your workflow" is banned).
- Show the real dictation flow (hotkey -> pill -> transformation), not a diagram.
- macOS primary target; Windows untested. Do not claim cross-platform parity.

## Comments

Status: **complete** (2026-11-08)

### Deliverables (all three requested)
1. **72s plan** — `brag-output-2026-11-08-133506/brag-plan.md` (4-act: problem / pain / solution / features; blended polished+cinematic+app-store tone)
2. **~25s companion social cut** — `brag-output-2026-11-08-133506/brag-plan-25s-companion.md`
3. **Rendered video** — `brag-output-2026-11-08-133506/brag.mp4` (1920×1080, 72.0s, 6.7 MB, audio) + `brag.jpg` (poster, baked as frame 0) + `share-copy.txt` + `composition-brief.md`

### Evidence
- **Gate:** `npx hyperframes check` → **Check passed, 0 errors** (6 warnings: nested-structure Studio-UX suggestions + first-frame, addressed by poster bake).
- **Render:** `npx hyperframes render --quality delivery` → 7.1 MB, 1m 12.0s, 2160 frames, audio present.
- **Poster:** extracted at 33.5s (title fully settled), baked as frame 0 via ffmpeg overlay.
- **Frame verification (read back):**
  - 9s — hook "…send your voice to a **server**" (server glows purple), redaction bar ✓
  - 15s — Privacy pain card ✓
  - 33s — "Teletype. Talk instead of type." + tagline ✓
  - 40s — recreated dictation flow: `⌘⇧Space` chip, pill "transcribing…" with dot grid, raw transcript typing in ✓
  - 54s — feature cards revealing in sequence ✓
  - 71.5s — punchline "your voice to a ~~server~~ → **your machine**" + logo ✓
- **Grounding:** all on-screen copy is from README.md / PRODUCT.md (tagline verbatim, real model names Parakeet/Whisper, the only stat is the product's own ~150 vs 40 wpm → ~21 min/1000 words). No fabricated testimonials/press.

### Notes / open
- Font: Open Runde (the app's display face) is not bundled in the composition, so the render falls back to Inter (declared via `@font-face local()`). The type is clean but not the exact brand face. If the exact Open Runde face is wanted, drop the `.woff2` into `composition/assets/fonts/` and point an `@font-face` at it, then re-render.
- The 25s companion is a **plan only** (not rendered) — say the word and I'll build + render it too.
