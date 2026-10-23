# ADR-020 - Polish models are offered per host engine build, with a first-run shortlist

- Status: accepted
- Date: 2026-10-23
- Deciders: Ali (request: "show polish models for platform wise ... show the
  models which they can download" + "AI Models tab ... make it simple, easy to
  understand"), inline implementation
- Implements: the platform-aware polish catalog; extends ADR-019 (speech
  shortlist) to the LLM side; supersedes the anonymous "Local model" onboarding
  card

## Context

The speech model side was already platform-aware (ADR-017, ADR-019): one
`offered_on` predicate and a backend `shortlist_ids` decide what a first-run
surface shows. The polish (LLM) side was not. Two problems:

1. The onboarding Polish step showed a single anonymous "Local model" card that
   happened to be `s1-mini`, with no way to see the other options and no
   platform awareness.
2. The same catalog was offered on every host. But the bundled `llama-server`
   is built differently per OS: Apple Silicon builds it with Metal (GPU),
   Windows builds a CPU-only static binary, and Intel Macs are documented as not
   performance-competitive. On a CPU-only host the multi-billion-parameter rows
   (Qwen2.5 3B / 7B, Gemma 2 2B, the "quality" 4B) take a minute per polish, so
   offering them as first-run choices is the same "too many models" confusion
   ADR-019 fixed for speech.

Separately, Ali asked the Settings -> AI Models screen to be "properly
arranged ... simple, easy to understand". It was a hand-rolled 216px left rail
plus a detail pane: the common task "get me a model that works" took a click on
a row before any Download button existed, and the state of the other rows was
invisible until you clicked each one.

## Decision

Make the polish catalog platform-aware, mirroring the speech pattern, and
rebuild the AI Models screen on the shared settings primitives.

### Catalog (crates/teletype-inference/src/catalog.rs)

- Each `CatalogEntry` carries `runs_on_cpu: bool` (true for the small rows that
  answer in seconds on a CPU-only build: s1-mini, speakoflow-mini,
  qwen3-0.6b, neudecide-q4; false for the multi-billion-parameter rows).
- `runs_on_host(gpu_accelerated)` = `gpu_accelerated || runs_on_cpu`.
- `host_gpu_accelerated()` reads `teletype_speech::catalog::host_platform()`,
  so the axis is the llama-server build, not a raw OS check.
- `polish_shortlist_ids(gpu)` returns the first-run ids: GPU
  (s1-mini, speakoflow-mini, qwen2.5-3b), CPU (s1-mini, speakoflow-mini); each
  id is re-checked against `polish_capable && runs_on_host(gpu)` so a stale id
  cannot leak a non-polish or host-hostile row.

### IPC (crates/teletype-desktop/src/commands/models.rs)

- `model_list(gpu, models_dir, selected_id, shortlist)` is a pure helper that
  mirrors `speech_model_list`. The full list is always offered (every row is
  downloadable) but each `ModelStatus` now ships `runsOnHost` so the UI can
  label the slow ones. `shortlist: true` swaps the list for
  `polish_shortlist_ids(gpu)` plus the user's stored selection (kept even when
  it is off-shortlist, so a returning user is never shown a screen that omits
  their own model).
- `list_models` exposes `shortlist: Option<bool>` defaulting to `false`.

### UI

- `OnboardingScreen.tsx` calls `list_models` with `shortlist: true` and renders
  the named `recommendedLlm` (the shortlist head) plus a "Show N other models"
  disclosure for the rest, with a caret and `aria-expanded`. The anonymous
  "Local model" fallback label is gone. Three rules keep the step from
  contradicting itself:
  - The local card is open by default whenever the backend has a row to offer
    (`polishPicked = polishChoice ?? (models.length > 0 ? "local" : null)`),
    because the card's own action was previously hidden behind a click on the
    card, on the step whose whole job is that action.
  - The expander lists `others` / `otherSpeech` (the list minus the card's
    model), so a model name appears once per step.
  - One "Recommended" badge per surface, on the row the list leads with
    (`polishLead` / `speechLead`), never per catalog row: three polish rows and
    two speech rows carry `recommended: true`, and the badge stopped meaning
    "start here" when several rows wore it.
- `ModelsScreen.tsx` is rebuilt on `settings/primitives.tsx` (Section/Row/Chip/
  PrimaryButton/SecondaryButton/TextInput) as four task-first groups:
  Dictation models, Polish models, a one-line Command Mode note, and
  Other engines (Ollama + OpenAI-compatible). Every model row carries its own
  inline Use/Download/progress action, so the common task is one pass and the
  state of all rows is visible at once. The column is 860px, the same as
  Keybinds and Command Mode, and the sections rely on `Section`'s own margin
  rather than a flex gap on top of it (the two together doubled every gap to
  56px). A row the host runs slowly gets a "Slow here" chip but stays offered (a
  32 GB machine may still want it). The publisher attributions render once as a
  section footer, deduped, because quantisations of one publisher's model share
  a credit line.
- The dev preview (`ui/src/dev/browserPreview.ts`) mirrors the contract and
  gained a `?cpu` param to review the CPU-only build (it flags rows >900 MB as
  not `runsOnHost` and returns the two-row CPU shortlist).

## Alternatives considered

1. Filter the polish list in the UI by an OS check. Rejected for the same
   reason ADR-017/019 rejected it: it duplicates platform knowledge in the
   front end and contradicts "the backend list is the single source of truth".
2. Permanently delete the large rows from the catalog. Rejected: they are the
   escape hatch for a fast machine, and Settings must still reach them; the
   catalog is a superset, the shortlist is the opinion.
3. Keep the rail-plus-detail-pane AI Models layout and only relabel it.
   Rejected: the two-pass "click then the button appears" flow is the root of
   the "UI doesn't look good" complaint, and it hides the state of the other
   rows. Rebuilding on the shared primitives is what makes it consistent with
   the rest of Settings.

## Consequences

- Onboarding answers "which of these does this machine run well" instead of
  showing one anonymous card; the same three-question shape as the speech step.
- `runs_on_cpu` is a product opinion held in the catalog, so adding a polish
  model means deciding whether it runs on CPU; the
  `runs_on_cpu_withholds_exactly_the_multi_billion_parameter_rows` test fails
  when the list changes without that decision.
- The full catalog is still reachable in Settings; `shortlist` only narrows
  what a first-run surface shows. `select_model` still refuses
  non-`polish_capable` ids, so the decider can never be picked as a polish
  engine.
- The AI Models screen now depends on `settings/primitives.tsx`, so a token or
  primitive change ripples here (and to every other settings screen) by design.

## Verification

- `cargo test -p teletype-inference --lib catalog` -> 14 passed (new
  `polish_shortlist_leads_with_a_recommended_model_and_matches_the_hosts`,
  `runs_on_cpu_withholds_exactly_the_multi_billion_parameter_rows`,
  `polish_shortlist_rows_are_all_polish_models_this_host_can_run`).
- `cargo test -p teletype-desktop --lib commands::models` -> 23 passed (new
  `model_list_shortlists_polish_rows_for_the_hosts_engine_build`).
- `npx tsc --noEmit` exit 0; `npx vitest run` -> 186 tests / 21 files passed.
  `ui/src/test/modelsScreen.test.tsx` (12) covers the grouping, the single
  badge, the deduped attribution line and the per-row "Slow here" chip;
  `ui/src/test/onboardingPolishStep.test.tsx` (8, the first test coverage the
  onboarding flow ever had) covers the `shortlist: true` request, the named
  card, the visible Download action, the one-badge rule, the non-repeating
  expander, and continuing without downloading.
- `impeccable detect --json` on ModelsScreen.tsx + OnboardingScreen.tsx -> [].
- Live read-back against Vite on 1420 (screenshots read back, not inferred):
  `?demo&onboarding` Polish step shows the open S1-mini card with one
  "Recommended" badge and a "Show 2 other models" disclosure that expands to
  SpeakoFlow Mini + Qwen2.5 3B (the card's own model is not repeated). Same URL
  with `&cpu` reads "Show 1 other model" and drops Qwen2.5 3B. `?demo&ready` AI
  Models screen renders at 860px with a single 28px rhythm between sections (the
  flex gap plus `Section`'s margin used to make every gap 56px), and the
  Command Mode note reads "Command Mode uses: NeuDecide Q4 (decider) · installed"
  with no Use control; with `&cpu` the >900 MB rows (Qwen2.5 3B, Gemma 2 2B)
  carry a "Slow here" chip while S1-mini / SpeakoFlow Mini stay clean.
