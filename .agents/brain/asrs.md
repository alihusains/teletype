# ASR and audio model evaluation register

Living register of every speech (ASR) and audio model Teletype evaluates, with a
row per model and a row per completed run. It exists so a ranking is never based
on a stale vibe: once a model has measured results, its row says so, and the
Teletype recommendation follows the measurements instead of the static bars.

Owner: Ali. Maintained by every agent that runs an eval or changes the speech
catalog. This file is the source of truth for **evaluation state**. The catalog
(`crates/teletype-speech/src/catalog.rs`) remains the source of truth for what
ships.

## Decision this register serves (2026-10-22)

Evaluate ASR and audio models on a regular cadence. After each round, the
**top 3 measured models become the recommended ones** on Teletype, and the
in-app ranking follows the eval results.

Rules that keep that honest:

1. A model can be recommended only after a measured run on the human corpus
   (see "Decision metric" below). No run, no recommendation.
2. Recommendation is per host class, because the catalog is host-gated: only a
   row the host can load and list may be recommended (`offered_on_with` in
   `crates/teletype-speech/src/catalog.rs`). Current host classes: Apple
   Silicon macOS 14+, Intel macOS, Windows/Linux (sherpa only).
3. Top 3 is per host class and per language coverage. An English-only model
   cannot win a slot for a non-English default.
4. Ties break on real-time factor, then RAM, then download size.
5. A candidate outside the catalog can be evaluated (Needle 3 is one), but it
   ships only after its weights license is confirmed and it has a catalog row.

Current implementation state: **not yet wired for top 3.** The speech catalog
has one static `recommended: true` row and a per-host runtime pick
(`recommended_speech_model_id` / `pick_engine`). The first bake-off snapshot
(5 engines, 86 synthesized cells) is recorded below; the human corpus is still
0/120, so the primary metric is unmeasured and no recommendation may change.
Turning measurement into a top-3 recommendation still needs a catalog and UI
change after a human-corpus run.

## Eval state legend

| State | Meaning |
|---|---|
| `untested` | In the catalog or on the candidate list, no run recorded |
| `queued` | Chosen for the next cadence run |
| `running` | Run in progress |
| `measured` | At least one completed run on the human corpus with a report artifact recorded below |
| `measured-synth` | Completed run(s) on synthesized corpora only; human corpus pending, not recommendation-eligible |
| `retired` | Evaluated and dropped; keep the row and its results for history |

## Decision metric

- Primary: `wer_clean_human`, the human-corpus clean WER in
  `crates/teletype-eval/src/bakeoff.rs` (the platform default decision metric).
- Secondary: verbatim WER and its delta vs the baseline row, RTF, peak memory,
  and per-set behaviour (noise, accented speech, vocabulary terms).
- Robustness sets must not be averaged into the primary score; record them as
  their own columns or notes.

## Harness

- `scripts/eval/run_bakeoff.sh` drives `teletype-eval run` over engine x set x
  boost. It is idempotent and resumable: a cell whose `report.json` exists is
  skipped, and a missing model is reported as disk-gated, never as a result.
- Engine list: `parakeet-ultra-coreml`, `parakeet-tdt-v3`, `parakeet-v2-coreml`,
  `sherpa-parakeet-v3-int8`, `sherpa-parakeet-v2-int8`, `large-v3-turbo-q5`.
- Sets: `human`, `librispeech-clean`, `librispeech-other`, `earnings22-vocab`,
  `ami`, `cv`, `noise-10db`, `noise-5db`, `noise-0db`, `fleurs-en`.
- Outputs: `evals/runs/bakeoff/<engine>/<set>-<boost>/report.json`. Record the
  path in the run log below; do not paste whole reports.

## ASR register

Static speed/accuracy are the catalog's UI bars, not measurements. They stay
until a run replaces them in the ranking.

| id | Engine | Platforms | Languages | Size | Static speed/acc | Eval state | Notes |
|---|---|---|---|---|---|---|---|
| `parakeet-tdt-v3` | parakeet (ggml) | macOS | 25 European (`PARAKEET_V3_LANGUAGES`) | 640 MB | 9.7 / 9.2 | measured-synth | Static catalog head; first snapshot below. The old 100% boost-on earnings22 cell was a pre-fix harness bug (empty word list, fixed in `93c4c85`); a 2026-10-22 re-run with the fixed vocab confirms boost is a no-op (12.18% both ways) |
| `parakeet-tdt-v3-q4` | parakeet (ggml) | macOS | 25 European | 356 MB | 9.7 / 8.8 | measured-synth | Compact build. Measured 2026-10-23: clean 1.85%, but `cv` regressed to **10.04%** (vs q8's 6.03%) — the q4 quantiser loses real accuracy on conversational audio. Fast (129 ms p50), tiny cold load (0.16 s), small download. Good size/speed trade only if you accept the `cv` hit. |
| `parakeet-tdt-v3-f16` | parakeet (ggml) | macOS | 25 European | 1260 MB | 9.5 / 9.4 | untested | High-precision build |
| `fluid-speech-mini` | parakeet.cpp | macOS | English | 347 MB | 9.7 / 9.5 | untested (harness gap) | Claims best English accuracy in noise, but **cannot be measured yet**: `teletype-eval run` has no ParakeetCpp provider seam (`--engine fluid-speech-mini` errors "ParakeetCpp provider is not wired into the eval harness"). Needs the engine wired into `crates/teletype-eval/src/driver.rs`+`bakeoff.rs` before its static bars can be trusted. |
| `parakeet-ultra-coreml` | Core ML | Apple Silicon, macOS 14+ | 25 European | 600 MB | 9.9 / 9.6 | measured-synth | Provisional platform default on Apple Silicon; first snapshot below. Fastest measured engine |
| `parakeet-v2-coreml` | Core ML | Apple Silicon, macOS 14+ | English | 600 MB | 9.9 / 9.5 | untested | English recall build |
| `parakeet-v3-coreml` | Core ML | Apple Silicon, macOS 14+ | 25 European | 600 MB | 9.9 / 9.3 | measured-synth | Fallback Neural Engine build. Measured 2026-10-23: clean 1.73%, other 4.23%, cv 6.02% — **loses to Ultra on every close-talk set**, and cold load is the worst of any engine at **26.4 s**. Fast steady-state (45 ms p50) but strictly dominated by `parakeet-ultra-coreml`; keep only as the macOS 14+ fallback when Ultra is unavailable. |
| `sherpa-parakeet-v3-int8` | sherpa-onnx | Windows, macOS, Linux | 25 European | 465 MB | 8.5 / 9.0 | measured-synth | Provisional platform default on Windows/Linux; first snapshot below. Loses to v2 int8 on every English set, wins on coverage |
| `sherpa-parakeet-v2-int8` | sherpa-onnx | Windows, macOS, Linux | English | 461 MB | 8.5 / 9.0 | measured-synth | English-only; first snapshot below. Strongest measured CPU engine on English |
| `large-v3-turbo-q5` | whisper.cpp | macOS | 99 Whisper languages | 547 MB | 7.5 / 9.4 | measured-synth | Routed for languages Parakeet does not cover; first snapshot below. Best on fleurs-en, trails on ami/cv |
| `large-v3-turbo` | whisper.cpp | macOS | 99 | 1624 MB | 7.0 / 9.5 | untested | |
| `small-en` | whisper.cpp | macOS | English | 466 MB | 8.0 / 8.5 | untested | |
| `base-en` | whisper.cpp | macOS | English | 142 MB | 9.0 / 7.5 | untested | |
| `base` | whisper.cpp | macOS | 99 | 142 MB | 9.0 / 7.3 | untested | |
| `tiny` | whisper.cpp | macOS | 99 | 75 MB | 9.5 / 6.0 | untested | Test-only quality level |

## Measured results: first bake-off snapshot

Source: 86 completed cells under `evals/runs/bakeoff/` (5 engines x 8-9 sets x
boost off/on). Commits in the reports: `d4b433b` (17 Oct), `51241ad` (Ultra
clean re-run), `00f8a76` (19 Oct: all sherpa cells plus the `cv` re-runs).
Every report has `human_case_count: 0`, so these are **synthesized-corpus**
results and `wer_clean_human` is null in every cell.

The six `earnings22-vocab` cells were re-run on 2026-10-22 at `981900e` after
`extract_vocab_terms` stopped treating sentence-initial capitals and function
words as vocabulary. The pre-fix cells are preserved under `*-pre-vocab-fix/`
for history; every number below comes from the re-run. Recall is scored against
the corrected term list, so it is not comparable across the two rollups.

Numbers are `wer_verbatim_micro` (the metric in
`docs/decisions/D-engine-defaults.md`), not the report's `wer.micro_wer`.
`a -> b` means boost off -> boost on; a single value means both cells equal.
Lowest is best.

### Verbatim WER (%)

| Engine | librispeech-clean | librispeech-other | fleurs-en | ami | cv | noise-10db | noise-5db | noise-0db | earnings22-vocab |
|---|---|---|---|---|---|---|---|---|---|
| `parakeet-ultra-coreml` | **1.61** | 3.56 | 4.95 | 10.46 | **5.89** | **4.34** | 5.25 | **7.18** | **6.88** -> 10.61 |
| `parakeet-tdt-v3` | 1.68 | 3.78 | 5.49 | **10.20** | 6.03 | 4.76 | 5.67 | 7.54 | 12.18 -> 12.18 |
| `sherpa-parakeet-v2-int8` | 1.66 | **3.42** | 6.03 | 11.39 | 6.99 | 4.58 | **4.95** | 8.14 | not run |
| `sherpa-parakeet-v3-int8` | 1.83 | 4.46 | 10.53 | 11.66 | 10.15 | 8.62 | 10.31 | 15.98 | not run |
| `large-v3-turbo-q5` | 1.76 | 3.98 | **4.05** | 12.98 | 9.67 | 5.79 | 6.39 | 9.77 | 69.62 -> 68.94 |

### ASR latency, boost off (p50/p95 ms)

| Engine | librispeech-clean | librispeech-other | fleurs-en | ami | cv | noise-10db | noise-5db | noise-0db | earnings22-vocab |
|---|---|---|---|---|---|---|---|---|---|
| `parakeet-ultra-coreml` | **48/114** | **43/100** | **55/72** | **38/57** | **57/207** | **47/76** | **46/77** | **46/74** | 2592/3192 |
| `parakeet-tdt-v3` | 602/1218 | 280/456 | 223/315 | 238/378 | 193/664 | 231/364 | 168/302 | 156/317 | 15261/21073 |
| `sherpa-parakeet-v2-int8` | 366/1134 | 328/975 | 598/843 | 104/439 | 813/3436 | 289/944 | 290/956 | 294/944 | not run |
| `sherpa-parakeet-v3-int8` | 363/1133 | 327/965 | 602/847 | 110/481 | 937/3584 | 474/1526 | 441/1536 | 297/953 | not run |
| `large-v3-turbo-q5` | 702/764 | 681/750 | 704/737 | 657/706 | 685/1571 | 683/757 | 676/729 | 678/728 | 31166/39556 |

Cold model load (`librispeech-clean` cell): Ultra 12,501 ms; Whisper 2,321 ms;
sherpa v2 851 ms; sherpa v3 778 ms; Parakeet TDT v3 494 ms.

### What the data says

- **No recommendation change is allowed yet.** Rule 1 requires the human
  corpus and it is still 0/120; `wer_clean_human` is null for all 86 cells.
  The static recommendation and the provisional host picks stay as they are.
- **Apple Silicon: Parakeet Ultra is the speed leader and wins 5 of 9 sets
  outright** (clean, cv, noise-10db, noise-0db, earnings22 off). It beats ggml
  Parakeet TDT v3 on 8 of 9 sets, losing only `ami` (10.46 vs 10.20), at
  48/114 ms p50/p95 vs 602/1218 ms. It is not best everywhere: sherpa v2 int8
  edges it on `librispeech-other` (3.42 vs 3.56) and `noise-5db` (4.95 vs
  5.25), and Whisper on `fleurs-en` (4.05 vs 4.95). It is the only engine that
  meets the macOS p95 budget (120 ms) on the clean set (114 ms); it misses
  only on `cv` (207 ms). Cost: cold load 12.5 s vs 0.5 s, which matters for
  first-use latency.
- **Windows/Linux: sherpa v2 int8 is the stronger English engine.** It beats
  sherpa v3 int8 on all eight English sets (clean 1.66 vs 1.83, other 3.42 vs
  4.46, fleurs-en 6.03 vs 10.53, cv 6.99 vs 10.15, noise-0db 8.14 vs 15.98).
  v3 int8 remains the only measured option with 25-language coverage, so the
  Windows default is a coverage-vs-English-accuracy choice, not a quality
  ranking. No sherpa engine meets the 600 ms Windows p95 budget on clean
  (1134/1133 ms).
- **Whisper large-v3-turbo-q5 is competitive only on `fleurs-en`** (4.05,
  best) and trails on `ami` (12.98, worst) and `cv` (9.67) at ~700 ms p50.
  Keep it for the 99-language route, not as an English default.
- **Boost only did anything on `earnings22-vocab`.** Every other set carries
  no vocab terms (`vocab_recall` null), so its off/on cells are identical.
  On earnings22 the two boost paths diverge:
  - **Ultra (native FluidAudio `ttf_set_vocab`): real regression, keep boost
    off.** With the corrected vocab, verbatim 6.88% -> 10.61%, vocab recall
    83.8% -> 95.3% but entity accuracy 84.5% -> 75.4% (terms land in the wrong
    places), and asr p50 2.6 s -> 19.7 s. The fix shrank the damage (pre-fix
    on: 14.42%) but did not remove it. The slowdown is inside the CoreML
    decode, not the harness: Ultra reports `native_boost: true`, so no
    rescoring runs.
  - **TDT v3 (`rescore_text` path): boost is a no-op, confirmed.** ggml
    Parakeet returns no word timings, so `transcribe_detailed` yields an
    empty word list; before commit `93c4c85` rescoring an empty list returned
    `""`, making every hypothesis empty (the recorded 100.00% cell). The guard
    now returns the transcript unchanged. The 2026-10-22 re-run measures
    12.18% -> 12.18% verbatim with entity accuracy 84.5% both ways.
  - **Whisper (native `initial_prompt`, top 30 terms): not a dependable
    win.** On the corrected vocab, verbatim 69.62% -> 68.94% (-0.7 pp) but
    entity accuracy falls 28.8% -> 19.8% and asr p50 grows 31.2 s -> 34.6 s.
    The pre-fix apparent gain (69.62% -> 65.98%) was measured against the
    polluted term list, whose common-word overlap flattered WER.
  - **Sherpa: unmeasured on a term-heavy set.** Sherpa reports word timings
    and no native boost, so it takes the rescorer, where weight 2.0 waives
    the confidence gate (a phonetically close term can replace a
    high-confidence word). No sherpa `earnings22-vocab` cell exists (ONNX
    aborts on the 21-minute calls). Leave boost off until that cell runs.
- **`earnings22-vocab` latency is not comparable to the other sets**: 6 long
  calls, p50 15-34 s. Compare WER within that row only.
- **Product outcome (2026-10-23): vocabulary boosting now ships off.** No engine
  had a measured win, so `boost_capacity` returns 0 for every engine and a take
  never sends a boost list to `set_boost_terms`
  (`crates/teletype-desktop/src/dictation.rs`, `ADR-018`, D029). This replaces
  the "no boost default changes" clause of L047. Learned terms still land through
  the text paths (the D006 dictionary, then `correct_with_dictionary` /
  `rescore_with_confidence`); what is off is biasing inside the ASR decode. The
  `--boost` axis stays in the harness as a research instrument, and every cell
  recorded in this file was measured with the default (`off`) except the
  `earnings22-vocab` on-cells that motivated this decision.

### macOS scoreboard (measured 2026-10-23)

Boost off, synthesized corpora, `human_case_count: 0` throughout (so these are
`measured-synth`, not recommendation-grade). Fresh cells for `parakeet-v3-coreml`
and `parakeet-tdt-v3-q4` under `evals/runs/bakeoff-2026-10-23-mac/`; the other
rows are re-read from the first-snapshot cells under `evals/runs/bakeoff/`.

Rating rubric (deterministic, auditable): **accuracy** from the close-talk mean
of {librispeech-clean, librispeech-other, cv} (desk dictation profile; `ami`/
`fleurs`/noise are reported but not averaged in, per the "robustness sets must
not be averaged" rule) — <=3.7->9, <=4.2->8, <=5.0->7, <=5.6->6, else 5.
**latency** from clean p50 ms against the macOS 120 ms p95 budget — <=60->10,
<=120->9, <=200->8, <=350->7, <=600->6, <=900->5, else 4.

| Model | clean | other | cv | ami | fleurs-en | noise-0db | close-talk mean | p50/p95 ms | cold load | acc /10 | lat /10 | size | langs |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `parakeet-ultra-coreml` | **1.61** | 3.56 | **5.89** | 10.46 | 4.95 | 7.18 | **3.68** | **48/114** | 12.5 s | **9** | **10** | 600 | 25 |
| `parakeet-v3-coreml` | 1.73 | 4.23 | 6.02 | 11.79 | 4.68 | 8.08 | 4.00 | 45/113 | 26.4 s | 8 | 10 | 600 | 25 |
| `parakeet-tdt-v3` (q8) | 1.68 | 3.78 | 6.03 | **10.20** | 5.49 | 7.54 | 3.83 | 602/1218 | 0.49 s | 8 | 4 | 640 | 25 |
| `parakeet-tdt-v3-q4` | 1.85 | 3.87 | 10.04 | 9.93 | 5.31 | 8.32 | 5.25 | 129/321 | 0.16 s | 6 | 7 | 356 | 25 |
| `large-v3-turbo-q5` | 1.76 | 3.98 | 9.67 | 12.98 | **4.05** | 9.77 | 5.14 | 702/764 | 2.3 s | 6 | 4 | 547 | 99 |
| `sherpa-parakeet-v2-int8` | 1.66 | **3.42** | 6.99 | 11.39 | 6.03 | 8.14 | 4.02 | 366/1134 | 0.85 s | 8 | 5 | 461 | EN |
| `sherpa-parakeet-v3-int8` | 1.83 | 4.46 | 10.15 | 11.66 | 10.53 | 15.98 | 5.48 | 363/1133 | 0.78 s | 6 | 5 | 465 | 25 |
| `fluid-speech-mini` | — | — | — | — | — | — | — | — | — | — | — | 347 | EN |

The two sherpa rows are now **hidden on macOS** (see the onboarding decision);
listed here only for cross-platform completeness. `fluid-speech-mini` is blank
because the eval harness cannot load the ParakeetCpp engine yet.

**Reading:** `parakeet-ultra-coreml` is the clear Mac winner — best clean and
close-talk accuracy, the only engine that meets the 120 ms interactive budget
(48/114 ms), and 25-language coverage. Its one cost is a 12.5 s cold load
(offset by keeping the model resident after first use). `parakeet-v3-coreml` is
fast but strictly dominated by Ultra on every set and loads even slower. The
ggml `parakeet-tdt-v3` (q8) is the fallback for machines without the Neural
Engine (Intel / macOS <=13) but its 602 ms p50 is not interactive. q4 is small
and quick but loses real accuracy on `cv`. **Confusion-reduction shortlist for
Mac:** Ultra (default), ggml Parakeet v3 (no-Neural-Engine / small-download
fallback), Whisper large-v3-turbo-q5 (only for the ~74 languages Parakeet lacks)
— drop the q4/f16/v2-coreml/v3-coreml duplicates, the unwired fluid-mini row,
and the two sherpa Windows rows from the Mac onboarding list.

**Applied 2026-10-23:** that shortlist is now backend code, not a proposal.
`shortlist_ids(platform, language, fluid_available)` with `SHORTLIST_MAX = 3`
(`crates/teletype-speech/src/catalog.rs`) is what onboarding renders when it
calls `list_speech_models` with `shortlist: true`; see
`adr/ADR-019-first-run-model-shortlist.md`. The polish (LLM) side got the same
treatment the same day (`adr/ADR-020-polish-models-per-host.md`). Do not
re-propose it; extend those functions when the measured table changes.

### Evidence gaps this snapshot does not close

- Human corpus: 0 cells, 0/120 utterances. The primary metric cannot be
  computed; no model is recommendation-eligible.
- RTF: reports carry `latency_asr` percentiles only, no audio duration, so the
  tie-break rule (RTF, then RAM, then size) cannot be applied yet. Add
  duration or RTF to the run report before relying on it.
- Unmeasured engines/rows: `parakeet-tdt-v3-q4`, `parakeet-tdt-v3-f16`,
  `fluid-speech-mini`, `parakeet-v2-coreml`, `parakeet-v3-coreml`,
  `large-v3-turbo`, `small-en`, `base-en`, `base`, `tiny`, and Needle 3.
- Missing cells: no sherpa `earnings22-vocab` rows (ONNX abort on 21-minute
  calls, see `evals/RESULTS.md`); Parakeet v2 ggml was disk-gated (truncated
  orphan); `cv` had a no-speech case-error cluster (3/3/10).
- Boost coverage: only `earnings22-vocab` carries vocab terms, and sherpa has
  no cell for it, so the sherpa rescoring path is still unmeasured on a
  term-heavy set. TDT v3 boost-on has been re-run with the fixed vocab and is
  a no-op (12.18% -> 12.18%), so the rescorer path on ggml Parakeet is
  measured and inert.

## Candidate lane (not in the catalog)

Candidates live here until a measured run and a license check justify a catalog
row. The 2026-10-22 catalog cleanup removed the `needle3` **catalog row**; the
model stays in this register because evaluation continues.

| Candidate | What it is | Runtime seam | License | Eval state | Notes |
|---|---|---|---|---|---|
| Needle 3 speech (`whistle.cact`) | Cactus Compute on-device speech model, 16.9 MB | `teletype-speech/src/whistle.rs` (`WhistleProvider`), loads `models/whistle.cact` through `needle-sys` | Repo Apache-2.0; **weights license unverified** (see `THIRD_PARTY_MODELS.md`) | queued | The ASR half of the Needle 3 family. Evaluation only. Do not ship or offer a download until the weights license is confirmed. Needs a catalog row before it can be recommended. |
| Needle 3 text (`needle3.cact`) | 121M tool-calling, extraction and embedding model | `teletype-inference/src/needle.rs` (`NeedleProvider`); not consumed by the desktop app today | Repo Apache-2.0; **weights license unverified** | queued | Not an ASR model. Kept here because Ali asked to keep it in the register for evaluation; evaluate it as a Command Mode / extraction component, never as a dictation engine. |
| TinyDecide | 10.4M classifier (intent, not transcription) | `teletype-inference/src/tinydecide.rs` | Apache-2.0 (verified) | untested | Not an ASR row; evaluate only as an intent-routing component, never as a dictation engine. |
| Qwen3-ASR-1.7B | 2.04B speech-LLM (Qwen3-Omni audio encoder + LLM decoder), 30 languages incl. Polish, streaming and offline | llama.cpp GGUF (`ggml-org/Qwen3-ASR-1.7B-GGUF`: Q8_0 + mmproj, ~2.6 GB), MLX 8-bit, transformers >= 5.13 | Apache-2.0 (verified) | untested (top candidate for next cadence) | Open ASR Leaderboard average 5.40, #1 open weights in the 2026-10-21 snapshot; card (2026-06-26) LS clean 1.24, LS other 2.92, AMI 9.26, Earnings22 9.88, mean 5.59. Not wired into `teletype-eval`; needs a new engine seam and a human-corpus run. 0.6B sibling has a GGUF and MLX 4-bit port if 1.7B latency is too high. |
| Cohere transcribe 03-2026 | 2.07B Conformer encoder + Transformer encoder-decoder, 14 languages incl. Polish, non-streaming | `transcribe.cpp` GGUF (Q4_K_M 1.56 GB, Q8 2.41 GB), community ONNX / CoreML / MLX ports | Apache-2.0 (verified; official weights gated) | untested | Open ASR Leaderboard average 5.77, #3 open weights in the 2026-10-21 snapshot (launch claim was #1 at 5.42 in March); GGUF port reports 1.24-1.27% LS test-clean. Not llama.cpp: needs the project's own runtime or a community ONNX/CoreML port, so provenance is weaker than Qwen's ggml-org path. |
| Granite Speech 5.0 2B | 2.31B speech-LLM, en/fr/de/es/pt/ja only, no Polish; 2,000-term keyword biasing | llama.cpp official GGUF (`ibm-granite/granite-speech-5.0-2b-GGUF`), MLX via mlx-audio | Apache-2.0 (verified) | untested (English/Latin-European tier only) | Open ASR Leaderboard average 5.68 in IBM's 2026-10-21 chart; CommonVoice WER CV-EN 6.02, CV-DE 4.10, CV-ES 3.50, CV-FR 6.49, CV-PT 2.60, CV-JA 10.74. llama.cpp support postdates the pinned commit `f95b0d9` (2026-09-22), so adopting it would require a pin bump. No Polish means it cannot replace the current coverage rows. |
| Phonon-2 (`FermionResearch/Phonon-2`) | ~0.6B Parakeet-tdt-v3 teacher distilled to 5-level ~2.1-bit QAT; 164 MB of weights (316 MB Core ML package) | **CoreML port** (`FermionResearch/Phonon-2-CoreML`) + official MLX + pure-C; runs on Apple Silicon | CC-BY-4.0 (code Apache-2.0) — weights license needs the same confirm we give Needle before shipping | measured-synth (2026-10-23, Mac; human 0) | Scored on the same six Mac sets as Ultra, replay mode: LS clean **1.68** / other **5.19** / cv **7.14** / fleurs-en **4.77** / ami **10.86** / noise-0db **9.65**; close-talk mean **4.67** vs Ultra **3.69**. It wins only FLEURS-en (4.77 vs 4.95) and loses entity accuracy on cv (70.6 vs 82.4) and fleurs-en (20 vs 80). Warm latency is good (p50 9-34 ms, p95 22-135 ms), but the first call into each Core ML encoder function (5/10/15/35 s buckets) blocked 60-90 s per process, so the RTF in `hypotheses.meta.json` is cold-start-dominated, not throughput. **English only.** Verdict: keep Ultra as the Mac default; Phonon-2 is a size-tier fallback only, and only after a warm-up or a single-35 s-bucket path makes cold start acceptable. |
| Parakeet Redux (`moondream/parakeet-redux`) | 0.6B arch, 1.58-bit ternary; 178 MB | Photon runtime (Metal + ARM-NEON/CPU); **no GGUF/ONNX/CoreML port documented** | CC-BY-4.0 | queued (multilingual slot) | Sep/Oct-2026. 25 languages incl. Polish. LS 1.96/4.34; OAL avg 6.55 (vs v3 6.26); beats v3 on FLEURS/TED-LIUM but **worse in noise**. Tiny footprint. Blocked on runtime: Photon is exotic for our llama.cpp+FluidAudio stack, so integration cost is high — evaluate only after the CoreML Phonon path. |
| Sophea ASR K1 (preview) | Undisclosed | Unknown | **Unresolved: paper says "no weights released", org profile claims MIT open weights** | blocked (license/repo unverified) | Would top the board (OAL avg 4.26, LS clean 1.18) but no verifiable downloadable repo as of 2026-10-23. Re-check when it ships as a real repo. |
| Canary-1B-v2 (`nvidia/canary-1b-v2`) | 978M FastConformer encoder + Transformer decoder; ASR + speech translation (AST), 25 European langs incl. Polish | NeMo / HF Transformers; **NVIDIA CUDA only (A100/H100/L4, Linux), no GGUF/ONNX/CoreML/MLX port, no CPU-on-device path** | CC-BY-4.0 (verified) | blocked (runtime: no Mac-native path) | Ruled out 2026-10-23 scout. The 749x RTF is a GPU number, not a Mac one. English does not beat our Mac default: LS clean 2.18 / other 3.56, Earnings-22 11.79, AMI 16.01 vs Parakeet Ultra measured clean 1.61 / other 3.56 / cv 5.89; the paper's own table has Parakeet-TDT-0.6B-v3 beating Canary on English (6.32 vs 7.15) and 550x faster RTF. Only real edge is the AST feature, which is not a dictation engine. Re-open only if a Mac-native (CoreML/MLX/GGUF) port lands. |

## Audio and front-end register

| Component | What it is | Where | Eval state | Notes |
|---|---|---|---|---|
| Silero VAD (pure Rust) | Auto-stop on silence | `teletype-core/src/vad.rs` | unit-tested | Latency gate < 10 ms per 32 ms chunk; utterance-level stop accuracy not yet measured |
| DFN3 denoiser | Noise suppression before ASR | `teletype-core/src/audio/mod.rs` | untested | Record WER with and against the `noise-*` sets before enabling by default |
| Parakeet CTC 110M | Neural Engine vocabulary spotter | `speech/catalog.rs`, `auxiliary: true` | untested | Evaluate through the bake-off `boost` on/off cells, not as a dictation engine. Inert in the shipped app while boosting is off (ADR-018): the Core ML bridge only loads it when a non-empty boost list arrives |

## Run log

Newest first. One row per completed cell or coherent group of cells; link the
report path and keep numbers verbatim. Do not record a cell that was skipped
because the model was absent.

| Date | Model | Set(s) | WER (clean human) | RTF | Notes | Artifact |
|---|---|---|---|---|---|---|
| 2026-10-23 | `phonon2` (CoreML port) | 6 Mac sets, replay-scored | n/a (synth clean 1.68%) | not usable (warm p50 9-34 ms; 60-90 s first call per encoder function) | Candidate, not in the catalog. Loses to Ultra on 5 of 6 sets and on entity accuracy; close-talk mean 4.67 vs 3.69. Hypotheses from `scripts/eval/bench_candidate_asr.py --model phonon` under Homebrew Python 3.11 + coremltools 9.0, scored by `teletype-eval run --engine none --hypotheses`. First-call stalls are one per 5/10/15/35 s Core ML function, reproduced by re-timing clips serially. | `evals/runs/candidates/phonon2/` |
| 2026-10-23 | `parakeet-v3-coreml`, `parakeet-tdt-v3-q4` | 8 English+noise sets x boost off = 16 cells | n/a (synth clean: v3-coreml 1.73%, q4 1.85%) | not reported (clean p50 45 ms / 129 ms) | Sequential live bake-off on the M4 Pro host. v3-coreml loses to Ultra on every close-talk set and cold-loads in 26.4 s; q4's `cv` regressed to 10.04% (q8 is 6.03%), so the compact quantiser costs real accuracy. Per-case fails (invalidAudioData on 4 ami cases, No-speech on 3 cv cases) recorded in the reports. | `evals/runs/bakeoff-2026-10-23-mac/{parakeet-v3-coreml,parakeet-tdt-v3-q4}/` |
| 2026-10-23 | `fluid-speech-mini` | (attempted) | n/a | n/a | **Not measured** — `teletype-eval run --engine fluid-speech-mini` exits 1: "the ParakeetCpp (native C++) provider is not wired into the eval harness". Needs the engine seam added to `crates/teletype-eval/src/driver.rs`+`bakeoff.rs` before it can be scored. | none |
| 2026-10-22 / 2026-10-23 | `parakeet-ultra-coreml`, `parakeet-tdt-v3`, `large-v3-turbo-q5` | `earnings22-vocab` x boost = 6 cells re-run | n/a (synth clean: Ultra 6.88%, TDT 12.18%, Whisper 69.62%) | not reported (asr p50 2.6-34.6 s) | Re-run at `981900e` with the corrected `extract_vocab_terms` (sentence-initial capitals and function words excluded). Pre-fix cells kept under `*-pre-vocab-fix/`. TDT boost is a no-op; Ultra still regresses under boost; Whisper gains only 0.7 pp and loses entity accuracy | `evals/runs/bakeoff/*/earnings22-vocab-{off,on}/` |
| 2026-10-17 / 2026-10-19 | `parakeet-ultra-coreml` | 9 sets x boost = 18 cells | n/a (synth clean 1.61%) | not reported (clean p50 48 ms) | Synthesized corpus, human 0; commits `d4b433b`, `51241ad`, `00f8a76`. Best WER on 5 of 9 sets; beats ggml TDT v3 on 8 of 9; meets macOS p95 budget on clean | `evals/runs/bakeoff/parakeet-ultra-coreml/` |
| 2026-10-17 / 2026-10-19 | `parakeet-tdt-v3` | 9 sets x boost = 18 cells | n/a (synth clean 1.68%) | not reported (clean p50 602 ms) | Synthesized corpus, human 0; commits `d4b433b`, `00f8a76` (cv). Boost on earnings22 is inert today (no word timings); the recorded 100% cell predates the `93c4c85` empty-words guard | `evals/runs/bakeoff/parakeet-tdt-v3/` |
| 2026-10-19 | `sherpa-parakeet-v2-int8` | 8 sets x boost = 16 cells (no earnings22) | n/a (synth clean 1.66%) | not reported (clean p50 366 ms) | Synthesized corpus, human 0; commit `00f8a76`. Strongest measured CPU engine on English | `evals/runs/bakeoff/sherpa-parakeet-v2-int8/` |
| 2026-10-19 | `sherpa-parakeet-v3-int8` | 8 sets x boost = 16 cells (no earnings22) | n/a (synth clean 1.83%) | not reported (clean p50 363 ms) | Synthesized corpus, human 0; commit `00f8a76`. 25-language coverage; loses to v2 int8 on every English set | `evals/runs/bakeoff/sherpa-parakeet-v3-int8/` |
| 2026-10-17 / 2026-10-19 | `large-v3-turbo-q5` | 9 sets x boost = 18 cells | n/a (synth clean 1.76%) | not reported (clean p50 702 ms) | Synthesized corpus, human 0; commits `d4b433b`, `00f8a76` (cv). Best on fleurs-en (4.05), trails on ami (12.98) and cv (9.67); keep for 99-language coverage | `evals/runs/bakeoff/large-v3-turbo-q5/` |
| 2026-10-15 | `parakeet-tdt-v3` (ggml q8_0; report label `parakeet-ultra`) | 16 on-disk sets, 1306 cases | n/a (human 0/120) | not reported (asr p50 433 ms) | Gate 0 baseline, commit `8197681`; overall verbatim WER 7.89%; only multilingual measurement on disk (8 fleurs languages, per-tag WER in `evals/RESULTS.md`) | `evals/runs/baseline/` |

## Cadence

1. Pull the eval corpora and put the target models on disk.
2. Run `scripts/eval/run_bakeoff.sh` (it builds `teletype-eval` with LTO off if
   needed, then plans/skips completed cells).
3. Fill the run log for every completed cell.
4. Update the register state: `queued` -> `running` -> `measured-synth` (no
   human cases) or `measured` (human cases scored).
5. Recompute the top 3 per host class. If the ranking changes, update the
   catalog recommendation and the UI in the same change, and record the decision
   in `PROJECT_BRAIN.md`.
6. Record anything surprising as a learning under `.agents/brain/learnings/`.
