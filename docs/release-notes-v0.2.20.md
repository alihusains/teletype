# Teletype v0.2.20 release notes (draft)

Status: draft for the release captain. Nothing here is published until Ali
approves the tag.

This release is about making what Teletype already does visible and dependable:
better out-of-the-box model choices, a mirror we control for the small local
models, an honest account of what polish did to a take, clearer controls for
profiles and learning, and a hotkey that recovers when the OS drops it. No claim
here is a claim of feature-for-feature parity with Wispr Flow; these are changes
to Teletype's own behaviour, each tied to the work that landed in this tree.

## What ships

### Better defaults out of the box

The default models now match the hardware in front of the user:

- **Apple Silicon / macOS:** Parakeet Ultra via Core ML for speech, with
  Qwen3.5 2B for text cleanup and transforms.
- **Windows and other platforms:** Parakeet Ultra via sherpa ONNX in INT8 —
  the same family, on a runtime those platforms actually have.
- The CPU fallback remains on the small cleanup models. It is still the path a
  machine takes when the preferred engine is unavailable; this release does not
  move it onto the larger models.

The practical effect is that a fresh install reaches the intended speech engine
without the user first having to go to the Models screen and pick one.

### Model mirror we control

Four permissively licensed local models now download from a GitHub Releases
mirror in `alihusains/teletype-models`, under the non-`v*` tag `models-v1`:

- S1-mini
- SpeakoFlow Mini
- Qwen3.5 0.8B
- Qwen3.5 2B

`models/manifest.json` in this repository is the source of truth. Each entry
pins an immutable upstream revision, a sha256 and an exact byte count, and keeps
that upstream pinned URL as a backup if the mirror is unreachable. Merging a
change to the manifest runs `.github/workflows/sync-model-mirrors.yml`, which
verifies those pins before uploading, so a mirror update is a reviewed change
rather than a manual copy. Mirror assets are additions only: existing bytes are
never overwritten, and the mirror release is never marked latest, because the
Tauri updater reads `releases/latest/download/latest.json`. Only models that are
permissively licensed and under 2 GiB are eligible.

### Polish is now visible

Polish runs by default again, and the result card now tells the truth about it:

- The skip-when-already-clean option is off by default, including for settings
  files that predate the key. Every take therefore reaches the polish path
  unless a different gate legitimately skips it — a short take, an intent that
  opts out, or no available model.
- When polish ran but genuinely changed nothing, the card says so: **"Polish
  ran · 0 changes · already clean."** A take that needed no work is no longer
  indistinguishable from a take where polish never ran.
- **View original** shows the raw ASR text after filler removal, so the user can
  see what the model started from.
- **Rewrite stronger** previews a second pass through the Transform profile.
  Its output is display-and-copy only: it is never injected into the focused
  field and never overwrites the text that was actually delivered.
- The card's **Why** line now names the model, the profile and the real fallback
  reason when one applied, instead of a generic explanation.

### Trust layer

The text-cleanup safeguards were tightened around the cases that matter to a
user trusting the output with their words:

- Guard removal and case-handling fixes stop cleanup from discarding content or
  mangling capitalisation.
- A deterministic backtrack resolver makes the "walk the change back" path
  reproducible instead of order-dependent.
- Protected spans keep named entities and deliberately preserved text out of
  aggressive rewrites.
- The behaviour-case corpus in `evals/polish-adversarial/` was extended so these
  cases are pinned by data, not only by unit tests.

### Profiles and context

- Text behaviour is organised into five selectable profiles: **Clean,
  Natural, Structured, Transform and Off**. Off is a deliberate choice for
  users who want verbatim output.
- Per-app defaults let a user keep one profile for a chat app and another for a
  code editor, without switching by hand on every take.
- A profile info command exposes which profile and resolution the app is
  actually using; the UI surfaces internal resolution and any fallback that
  occurred, so "why did this sound different in that app" has an answer.

### Personalization v2

- Names confirmed on screen from the transcript are learned, rather than being
  inferred silently.
- Word learning is now visible, with an undo for anything the app got wrong.
- "Never change this" protection keeps explicitly protected terms out of
  rewrites.
- Learned terms now flow into the **post-ASR** correction tiers, where they can
  fix a transcript after decoding. Native ASR vocabulary boosting stays off, per
  decision D029 — the measured bakeoff found no win for it and a regression for
  some engines — so this is a correction path, not a decode-time boost.

### Hotkey reliability

Several ways for the global hotkey to stop working after the OS drops or
changes a grant are now handled:

- A stale modifier latch is reset, so a missed key-up cannot leave the next
  press stuck or silently swallowed.
- Watchdogs detect a hotkey path that has stopped reporting.
- Liveness re-registration brings the hotkey back without an app restart.
- A **Hotkey status** row in Settings shows the current state, so a failure is
  visible rather than a mystery.

### Performance and internal quality

Work that does not change what the user sees, but makes the app cheaper and the
behaviour easier to verify:

- Insights are computed in a single pass instead of repeatedly walking the
  same history.
- The text diff used by the polish card is memoized.
- Calibration packets are reused rather than rebuilt per take.
- Backtrack cues are pre-split, so the resolver starts from stable input.

## Verification

Full local suite on this tree:

| Check | Result |
|---|---|
| `cargo test -p teletype-core` | 1390 passed |
| `cargo test -p teletype-desktop` | 631 passed |
| `cargo test -p teletype-inference` | 133 passed / 135 total (2 ignored) |
| `cargo test -p teletype-speech` | 81 passed / 82 total (1 ignored) |
| TypeScript (`tsc`) | clean |
| UI `vitest` | 610 passed |
| `cargo clippy` | green |
| Eval harness self-test | PASS |

The ignored tests are pre-existing ignores, not new gaps introduced by this
release. This is local verification; it is not a substitute for the signed
release build or for acceptance on the user's own hardware.

## Known limitations

- **Native ASR vocabulary boosting remains off on every engine** (D029). The
  bakeoff measured a regression for Core ML and Whisper and no benefit for ggml
  Parakeet, so learned terms are applied after decoding only. Personalization is
  reachable and covered by tests, but there is still no eval axis that measures
  its end-to-end accuracy.
- **Core ML polish is not adopted.** The Core ML speech path remains an engine
  choice; this release does not route polish through a Core ML model, and no
  accuracy claim is made for it here.
- **Qwen3-ASR stays upstream.** Its artifact is over the 2 GiB per-asset mirror
  limit, so it is not part of the `models-v1` mirror. It is not a new default in
  this release.
- **Live Mac acceptance is still recommended for hardware-dependent paths.**
  The hotkey watchdogs/liveness re-registration and the Core ML / sherpa default
  paths are verified by local tests and code paths, but some only behave
  correctly with the real Input Monitoring grant, accessibility trust and audio
  hardware on a signed build. A short pass on real hardware before the tag is
  the honest way to close those out.
- The updater still depends on repository signing credentials being present for
  a signed, non-empty manifest. Check the signing gate before publishing the
  tag; a release with no signing key stays unsigned and offers no in-app update.
