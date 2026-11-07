# Data-driven, updatable model catalog

Design date: 2026-11-07. Status: proposed, awaiting review.

## Problem

Adding one model today is a multi-file Rust change. A single new ASR model
(for example Ivy, a Qwen3-ASR fine-tune) touches the speech catalog
(`crates/teletype-speech/src/catalog.rs`, ~2700 lines of hardcoded
`SpeechModel` rows plus an `Engine` enum, language tables, and artifact
pins), the provider (`crates/teletype-inference/src/qwen3_asr.rs`), the
download command (`crates/teletype-desktop/src/commands/models.rs`), and two
installed-checks (`dictation.rs`). The same is true of the LLM catalog
(`crates/teletype-inference/src/catalog.rs`). Every addition is a build, a
review, and a release.

The data (which models exist, their links, sizes, hashes, capability flags)
changes often. The logic (availability, URL building, provider paths, the
command-mode gate) is small and stable. They are currently fused into one
Rust file, so a data change forces a code change.

## Goals

1. **Adding a model is a data edit, not a code change.** One entry in a JSON
   file: links, the engine it works with, and its capability flags.
2. **A new model can ship without a new app build.** The catalog is loaded
   from disk at runtime and updated silently, so publishing an updated JSON
   file is enough.
3. **A bad or missing catalog never breaks the app.** The app always has a
   working set of models; a failed update keeps the last good catalog.
4. **Capabilities are first-class and drive the UI.** Each model declares
   `asr` / `polish` / `tool_calling`. Command Mode is offered only when the
   selected LLM supports tool calling **and** the selected ASR model is
   usable; otherwise the user sees a plain message saying which part is
   missing.
5. **Existing guarantees stay enforced.** The invariants the ~40 catalog
   tests already check (unique ids, one recommended per domain, well-formed
   hashes, purpose-label vocabulary, language-table consistency) are re-run
   against the data, so a bad entry fails the build, not the user.

## Non-goals

- A plugin system that lets data declare a brand-new engine loader. Engines
  stay Rust. When a model needs an engine the app has never run, that is a
  separate, evaluated decision to implement the engine; the catalog entry
  then routes the model to it.
- User-authored model files. The catalog is maintained by the app team and
  published by us; users pick among offered models, they do not add them.

## Architecture

### Data files

Three JSON files, one per pipeline domain:

- `speech.json` — ASR models (turns voice into text).
- `llm.json` — polish/LLM models (clean up transcribed text).
- `command.json` — command-mode decider models (today the
  `polish_capable: false` LLM rows such as NeuDecide; they get their own
  file because they have their own pipeline role).

All three live in `crates/teletype-catalog/` in the repo as the **shipped
defaults**, and are the same files that get published for runtime updates.

### Entry schema (speech, representative)

```json
{
  "id": "qwen3-asr",
  "name": "Qwen3-ASR 1.7B",
  "engine": "llama-server",
  "capabilities": ["asr"],
  "languages": ["ar", "bg", "cs", "pl", "en", "de"],
  "english_only": false,
  "artifacts": [
    { "file": "Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 2165034944,
      "sha256": "58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57" },
    { "file": "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 355709344,
      "sha256": "46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d" }
  ],
  "base_url": "https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main",
  "size_mb": 2404,
  "min_ram_gb": 4,
  "speed": 6.0,
  "accuracy": 9.0,
  "recommended": false,
  "auxiliary": false,
  "runs_on": ["macos"],
  "purposes": ["Multilingual", "Long recordings"],
  "description": "..."
}
```

- `engine` is a name from a fixed, small set the app implements
  (`whisper`, `parakeet`, `parakeet-coreml`, `parakeet-cpp`, `sherpa`,
  `llama-server`, `apple-speech`, `transcribe-cpp`, `onnx`). It maps to the
  Rust loader. An unknown engine name is a validation error, not a runtime
  surprise.
- `capabilities` is the user-facing capability set: `asr`, `polish`,
  `tool_calling`. The LLM and command entries use the same field; an ASR
  model carries `["asr"]`, a polish model `["polish"]`, a command decider
  `["tool_calling"]`.
- `artifacts` generalizes the single-file vs two-file vs multi-shard shapes
  already in the code (Whisper single file, Qwen3-ASR two-file, LLM
  multi-shard). One file = one artifact; the llama-server ASR pair = two.
- `runs_on` is the platform set the row is offered on. The existing
  `Platform`/`available_on` logic reads this instead of an `Engine` match.

The LLM and command entries add the fields those catalogs already carry
(`size_label`, `specialty`, `license_*`, `runs_on_cpu`, `sort_order`,
`verdict`), so no data is lost in the migration.

### What stays in Rust

The logic does not move. It becomes a thin layer over the parsed entries:

- **Engine registry.** A `match` on the `engine` name that returns the
  loader/provider (the current `Engine` enum, keyed by string instead of by
  variant). Unknown engine = validation error at load.
- **Availability / offered-on.** Reads `runs_on` + the engine's
  `available_on` + `auxiliary`. Same predicate as today
  (`SpeechModel::offered_on`).
- **URL / provider path / install path.** Reads `artifacts` + `base_url`
  instead of hardcoding per-id. The `llama_asr_artifacts_for` /
  `llama_asr_provider_dir_for` helpers collapse into "read the entry's
  artifacts and derive the directory from the id."
- **Command-mode gate.** A pure function:
  `command_mode_available(selected_llm, selected_asr) =
  selected_llm.capabilities.contains("tool_calling")
  && selected_asr is usable on this host`. The UI renders the chips and the
  "why not" message from this.

### Delivery: shipped default + silent update

The app **always** has a working catalog:

1. **Shipped default.** The three JSON files from the repo are bundled in
   the app bundle (read-only) as the fallback. A fresh install, or a machine
   that has never updated, uses these.
2. **On-disk working copy.** On first launch the app copies the shipped
   defaults into the data dir
   (`~/Library/Application Support/com.teletype.app/catalogs/`) if none are
   present. This directory is the live catalog.
3. **Silent update.** On launch (and periodically), the app fetches a small
   **manifest** from a GitHub release (latest release asset
   `catalog-manifest.json`: `{ "version": N, "files": { "speech":
   "<sha256>", ... }, "url": "<base>" }`). If `version` is newer than the
   installed one, it downloads the changed files, verifies each against the
   manifest sha256, **validates** (schema + invariants), and only then swaps
   them into the working copy. Any failure (network, hash, schema,
   invariant) leaves the last good catalog in place and logs a non-fatal
   notice. The app never reads a catalog that failed validation.
4. **Rollback is automatic.** Because the working copy is only replaced
   after a full validate, a bad published file cannot take the app down; the
   next launch simply retries the update.

Adding a model without an app update = push the updated JSON files as a new
GitHub release asset (or update the latest release) with a bumped
`version` and the files' sha256s. Users pick it up on next launch.

The model **weights** are unchanged: each entry's `artifacts` carry
`sha256` + `bytes`, and the download command verifies them exactly as today.
Only the small catalog files move to the update channel.

### Validation (build-time and update-time)

One validator, run in two places:

- **CI / cargo test** (build-time): parses the three repo JSON files and
  re-runs the invariants the existing ~40 tests assert, now against data:
  unique ids per domain, exactly one `recommended` per domain, sha256 is
  64-hex or absent (and absent only for the multi-file/OS-managed shapes),
  purpose labels are from the fixed vocabulary, the `languages` table agrees
  with `english_only`, every `engine` name is one the app implements, and
  `size_mb` is at least the sum of its artifacts. This is what keeps the
  existing test coverage alive after the literals are deleted.
- **Update-time** (runtime): the same validator runs on a downloaded catalog
  before it is swapped in. Fail = keep the old catalog.

The existing per-domain test functions are kept but pointed at the parsed
data instead of the `const CATALOG` arrays, so coverage is preserved, not
dropped.

## Migration

Full migration, no hybrid fallback of two sources of truth:

1. Stand up `crates/teletype-catalog/` with the three JSON files containing
   **every** current model, field for field (no data loss).
2. Add the loader + validator + engine registry; make the speech, LLM, and
   command code paths read parsed entries.
3. Delete the hardcoded `const CATALOG` arrays and the now-redundant
   per-id helpers (`llama_asr_artifacts_for`, etc.).
4. Re-point the existing tests at the parsed data.
5. Add the command-mode gate function + its UI message.
6. Add the silent-update path (manifest fetch, verify, validate, swap) with
   the shipped-default fallback.

Ivy (the in-flight addition) becomes the first model added *under the new
system*: one entry in `speech.json`, engine `llama-server`,
`capabilities: ["asr"]`, `english_only: true`, its two artifacts. It is the
acceptance test for the whole design.

## Capability model and the two pipeline shapes

The catalog's `capabilities` array is what the UI and the pipeline both read
from. The values are:

- `asr` — transcribes voice to text.
- `polish` — cleans up transcribed text.
- `tool_calling` — can emit structured tool calls (command mode).
- `omni` — one model does ASR + polish + tool-calling + intent in a single
  pass (a multimodal "virtual assistant" model such as LFM3-Omni or
  Qwen3-Omni). This is a **role**, not just a bag of the other three: an omni
  model runs its own pipeline, so it is marked `omni` in addition to the
  capabilities it has.

Two pipeline shapes result, and the user can see which one is active:

1. **Multi-model (default).** Separate stages, each a different model:
   ASR model -> intent router -> command decider (if command) -> polish LLM
   (if dictation). This is the existing architecture, now driven by data.
2. **Omni (experimental).** The user selects an `omni` model and the whole
   take runs through it in one pass: it transcribes, decides intent, and
   emits the tool call or the polished text. No separate router, decider, or
   polish model. Clearly labeled experimental, for people who want the
   one-model-does-everything feel.

### Command-vs-dictation intent routing

A per-take decision, made by a small model on the **transcript** (not the
audio), that answers: is this a command, or is it speech to insert as text?

**Router: TinyDecide** (https://huggingface.co/TheREZOR/TinyDecide).
- A "Jev-style" decision model: reads one message, answers plain-English
  questions, returns a **probability for every answer** (no text generation).
- **6.2 MB** (10.4M params, 4-bit), text-in (first 127 tokens), Apache-2.0.
- **Rust engine ships in the repo** (`rust/`, path dependency, conformance
  tested to 1e-6). Latency is single-digit ms on M4 (1.1 s on an ESP32
  Cardputer).
- **Zero-shot and open-vocabulary**: no fine-tuning. At call time the app
  asks, in one pass: `noul("This is a command, not dictation.")` for the
  route, `choice("Which tool?", [...])` for the tool, and `span(...)` for
  slots. One encoder pass answers routing + tool + slots together.
- Accuracy: 82.8% held-out on "known question types, new messages" (a
  generalist floor across many task families), 92.4% on numbers/times/
  contacts. **English only.** The self-scored "Decision Index" is not
  independently verified; the held-out table is the trustworthy signal.
  **Must be verified on Teletype's own 185-clip command suite before trust.**

**Conservative gate (the asymmetry that makes this safe):**
- A false *command* (dictation routed as a command) is dangerous: it can
  perform an action the user didn't ask for. A false *dictation* (command
  routed as text) is harmless and easily redone.
- So: route to command only when TinyDecide's command-confidence clears a
  **high threshold**; otherwise insert as text. The threshold is tuned on
  real takes, not guessed.
- Any action that **leaves the machine** (send message, send email, post,
  open an external app) **always confirms** via the existing dry-run Effects
  preview, regardless of confidence. Never fire-and-forget a low-confidence
  external action.
- **Instrument every decision** (transcript, confidence, route taken,
  user-confirmed-or-not) so the threshold is tuned from data.

**NeuDecide (43 MB, audio-in)** remains available as an optional
audio-direct path (decide straight from audio, skipping ASR) and as the
fallback when the transcript is unavailable. TinyDecide is the primary
router because it is smaller, faster, zero-shot, and composes with any ASR
model including the omni models.

## Testing

- Validator unit tests: a good catalog passes; each invariant has a fixture
  that violates it and fails.
- Migration equivalence: for every current model, the parsed entry's
  `url()`, `provider_path()`, `install_path()`, `available_on()`, and
  `offered_on()` match the old hardcoded behavior (a diff test across all
  existing rows and platforms).
- Command-mode gate: table test over (LLM has tool_calling?, ASR usable?)
  -> available / which-part-missing.
- Update path: a newer manifest updates; a wrong sha256, a schema error, and
  an invariant error each leave the old catalog in place (tested with a
  temp data dir and a stub manifest server).
- The full existing test suites of the three crates stay green.

## Risks and mitigations

- **Runtime parse failure bricking the app.** Mitigated by the shipped
  default + swap-only-after-validate. The app reads the working copy only if
  it is a known-good (validated) catalog; otherwise it falls back to the
  bundled defaults.
- **A published catalog that is valid but wrong** (a model that does not
  actually load). Mitigated by keeping the per-model `sha256`/`bytes`
  verified at download, and by the engine `available_on` gate. A model that
  fails to load at runtime surfaces the existing load-error path, not a
  crash.
- **Two sources of truth during migration.** Avoided by the full-migration
  approach: the JSON is the only source once the Rust literals are deleted.

## Decisions (locked 2026-11-07)

1. **Manifest host:** latest GitHub release asset, reusing the existing
   release pipeline. The release job emits `catalog-manifest.json` plus the
   three catalog files as assets; the app fetches the latest release's
   manifest.
2. **Update cadence:** check on every launch, throttled to at most once per
   hour. Launch is the natural moment; the throttle avoids hammering the
   release API on rapid relaunches.
3. **Command deciders:** a separate `command.json`. Keeps each pipeline
   domain self-contained and matches the ASR / polish / command three-role
   model. Folding into `llm.json` later is a trivial refactor if it ever
   feels redundant.
4. **Catalog signing:** **sha256-only for v1.** The manifest's per-file
   sha256s stop a corrupted download, and the per-model weights are already
   sha256-verified at download. The catalog is published only from the team's
   own release pipeline, so a tampered-manifest attack is not the gating risk
   for v1. A minisign/ed25519 signature over the manifest (key embedded in
   the app) is recorded as a **follow-up hardening item**, not a v1 blocker.
5. **Engine set is closed.** Engines stay Rust. Data routes a model to an
   already-implemented engine by name; a genuinely new engine is a separate,
   evaluated code change. This is the intended boundary, confirmed.

## Follow-ups (not v1)

- Sign the manifest (minisign/ed25519) once the catalog update channel is in
  production and the app is signed for distribution.
- Measure TinyDecide on the 185-clip command suite to set the routing
  confidence threshold from data.
- Re-host NeuDecide (Gate A) so the audio-direct path is available to compare
  against TinyDecide.
