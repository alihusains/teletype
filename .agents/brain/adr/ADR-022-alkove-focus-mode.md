# ADR-022 - Alkove Focus + Timbre as an optional "focus on my voice" mode

- Status: accepted
- Date: 2026-10-27
- Deciders: Ali (standing directive: adopt the measured winner for the
  competing-speaker gap), Alkove integration design (worker)
- Implements: the 2026-10-26 P3/P3b spike verdict in `asrs.md`
  ("worth integrating as an optional focus-on-my-voice mode, gated on the
  enrollment UX")

## Context

The 2026-10-26 spike (run log rows in `asrs.md`) measured the one scenario
where both of our ASR engines fail — an equal-loudness second speaker
(`competing-0db`, 200 two-talker cases):

- Raw mix → Fluid Speech Mini: **44.57%** micro WER
- Raw mix → Parakeet Ultra Core ML: **43.58%**
- Focus+Timbre-extracted → Fluid Speech Mini: **30.83%**
- Focus+Timbre-extracted → Parakeet Ultra Core ML: **29.93%** micro / 30.61% macro
- DFN3 denoiser → Fluid Speech Mini: **54.05%** (worse than the raw mix)

Focus is the first thing measured that actually solves the equal-loudness
two-talker case instead of softening it. The denoiser alternative is
disqualified by measurement, not preference: at 0 dB the interferer is
speech, not noise, so a noise-suppression model has nothing to suppress.
No other local target-speaker-extraction engine exists on Mac (the 2026-10-26
scout closed that lane — see `asrs.md` "Scouted and rejected").

## Decision

**Adopt Alkove Focus + Timbre as an optional "focus on my voice" mode,
off by default, gated on a one-time voice enrollment.** When the mode is
on and a voiceprint exists, every dictation clip passes through Focus
before the existing ASR engine. The mode reuses the app's default engine
(Parakeet Ultra Core ML on Apple Silicon) — the spike showed Ultra on
extracted audio is 0.9 pp better than Fluid Mini at it (29.93 vs 30.83),
so no new ASR engine is introduced.

## Vendoring and pin

Pin `alkove-ai/alkove` at commit
**`ecaec6da2f80a78571bf589a3c9b4aece023ea63`** (2026-10-26, workspace
version 0.3.2; verified by clone on 2026-10-27). The engine is **not on
crates.io** (`alkove-core` is `publish = false`), and the app must not
depend on git at build time. Precedent in this repo: `crates/parakeet-sys`
builds whisper.cpp from a pinned tag fetched into `OUT_DIR` by its build
script, and `crates/parakeet-cpp-sys` does the same for a pinned
parakeet.cpp commit with the pin recorded in the build script and
`THIRD-PARTY-NOTICES.txt`.

For Alkove we go one step further: **vendor a source copy under
`crates/alkove/`** (a workspace member, committed) holding the two needed
crates — `crates/alkove/engine` (upstream `crates/engine`, crate
`alkove-engine`) and `crates/alkove/core` (upstream `crates/core`, crate
`alkove-core`), with the path dependency `alkove-core -> alkove-engine`
retained. `teletype-speech` depends on `alkove-core` by path. The pin
lives in a header comment in `crates/alkove/` (upstream repo + commit +
fetch date), not in a build script, because nothing is fetched at build
time. The `.alk` weights (Focus 1.1 MB, Timbre 0.74 MB, Apache-2.0) ship
with the app data, not in the crate.

Why a committed copy instead of an `OUT_DIR` download like parakeet-sys:
Alkove is 0.x and 4 days old at pin time; a build-time fetch would couple
every build to network + upstream liveness, and a committed copy lets the
upgrade path (below) be a reviewed diff.

## Pipeline position

```
mic capture -> resample 16 kHz -> Focus (with enrolled Timbre print)
           -> existing ASR (Parakeet Ultra Core ML default)
```

Focus runs on the 16 kHz mono clip after the same resample the ASR
already consumes; its output replaces the clip for the ASR stage only.
Measured latency on M4 Pro CPU: **Focus pass ~0.35 s per 10 s clip**
(P3b run log row; the P3 row measured ~0.5 s on the same host). The ASR
stage is unchanged: Ultra p50 **50-70 ms** on the extracted audio. The
extraction pass, not the ASR, is the latency cost of the mode.

## Enrollment UX

The voiceprint is captured from **the user's first >=1.5 s of clean
speech in a dictation session** — the first utterance, taken before any
competing speaker is likely to be present — and stored as a **192-dim
float vector** in the app data directory. Re-enrollment is an explicit
action in settings ("Focus: re-enroll my voice"), not an automatic
overwrite.

Why not per-app voiceprints: the user's voice does not change with the
frontmost app, so per-app prints would multiply enrollment friction
with zero acoustic benefit; a single print is the minimum state that
works. Why not explicit-first-only (a dedicated "record 1.5 s" screen):
the dictation session already contains >=1.5 s of the user's clean
speech as its first utterance in the overwhelming common case, so a
separate enrollment ritual would make the mode's first run cost more
than its first benefit. The first-utterance capture is the cheapest
enrollment that satisfies the model's >=1.5 s requirement; the settings
re-enroll covers the residual cases (voice changed, bad capture, shared
machine).

## 0.x risk policy

The repo is 0.x with no crates.io provenance. **The pinned commit is
final until a manual re-spike says otherwise: no auto-upgrade.** An
upgrade is a deliberate step: re-run the `competing-0db` set against the
candidate commit, require the Focus-vs-raw-mix delta to hold, then bump
the vendored copy in a reviewed commit. A 0.x API break is therefore
expected to be absorbed in the re-spike, never in a routine build.

## Alternatives considered

- **DFN3 denoiser (already in-repo):** measured 54.05% on the same set —
  worse than the raw mix. Rejected by measurement. (The in-repo `dfn3.rs`
  cannot even run the published export — degenerate ERB filter bank;
  see learning `L-20261026-dfn3-rust-filter-bank-degenerate`.)
- **Per-app or wake-gated enrollment:** more state, no measured benefit;
  the single-print model is what the spike validated.
- **Build-time fetch (parakeet-sys style):** rejected for a 4-day-old
  0.x repo with no provenance; a committed vendored copy makes the
  upgrade path a reviewable diff.

## Consequences

- +1.8 MB of weights (Focus 1.1 MB + Timbre 0.74 MB) in app data.
- One new vendored crate pair (`crates/alkove/{engine,core}`) and a path
  dependency from `teletype-speech`.
- One piece of user state to manage: the 192-dim voiceprint in the app
  data dir — set, re-set, and (when we add account sync) never synced.
- The mode is off by default; users who never turn it on pay nothing.
- Focus-mode clips cost ~0.35 s per 10 s of audio on CPU before the ASR
  stage; the mode's value (a 13.7 pp WER cut at equal loudness) does not
  exist for single-speaker dictation, so it stays opt-in.

## Evidence

- Run log rows, `asrs.md` (2026-10-26): P3 spike (Focus + Fluid Mini,
  30.83 vs 44.57/43.58 raw-mix; DFN3 54.05) and P3b (Focus + Parakeet
  Ultra, 29.93 micro / 30.61 macro; Ultra p50 50-70 ms on extracted
  audio; Focus ~0.35 s per 10 s clip).
- Artifacts: `evals/runs/candidates/alkove-focus-0db/` (report.json /
  report.md / meta.json) and `evals/runs/candidates/dfn3-0db/`.
- Learning: `L-20261026-dfn3-rust-filter-bank-degenerate`
  (`.agents/brain/learnings/2026-10-26-dfn3-rust-filter-bank-degenerate.md`).
- Pin verified by clone of `https://github.com/alkove-ai/alkove` on
  2026-10-27: HEAD `ecaec6da2f80a78571bf589a3c9b4aece023ea63`, crates at
  `crates/engine` (`alkove-engine`) and `crates/core` (`alkove-core`,
  `publish = false`).
