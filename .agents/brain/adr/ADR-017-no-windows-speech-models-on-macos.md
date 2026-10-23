# ADR-017 - Do not offer Windows (sherpa-onnx) speech models on macOS

- Status: accepted
- Date: 2026-10-23
- Deciders: Ali (request), inline implementation (pi provider was down)
- Supersedes part of: L034 ("sherpa = all platforms"), the 2026-10-21 platform-aware defaults decision

## Context

Teletype's UI does not filter the speech model list by OS; it renders exactly
what the `list_speech_models` IPC returns, and that list is `CATALOG.filter(|m|
m.offered_on(host_platform, fluid_available))`
(`crates/teletype-desktop/src/commands/models.rs:832-834`). The single
"may we show this row" predicate `offered_on` delegated the OS decision to
`Engine::available_on`, which returned `true` for `SherpaParakeet` on every
platform. So on a Mac, onboarding listed ~15 models including two sherpa-onnx
rows whose own descriptions read "Default on Windows" and "sherpa-onnx build
for Windows".

Ali: "for mac we shouldn't show windows models during the onboarding" and "Too
many models cause a lot of confusion". The 2026-10-23 bake-off confirmed these
rows are the worst measured engines on the Mac host (sherpa v3 int8 loses to
every native engine on every set; sherpa v2 int8 is English-only). There is no
Mac use case where they are the right pick: the platform default is Core ML
(macOS 14+) or ggml Parakeet (Intel/older), and uncovered languages route to
Whisper.

## Decision

Stop **offering** the sherpa-onnx rows on macOS. Implemented at the *offering*
predicate, not the *capability* predicate:

- `SpeechModel::offered_on_with` (`crates/teletype-speech/src/catalog.rs:190`)
  now additionally requires `!(platform.is_macos() && engine == SherpaParakeet)`.
- `Engine::available_on` is left truthful: sherpa-onnx still reports `true` on
  macOS because the dependency links and loads there. Capability and surfacing
  are kept as separate concerns.

Consequences of this choice:
- The macOS default and recommendation are unchanged (`pick_engine` never named
  a sherpa row on macOS, so `recommended_speech_model_id`'s offerable-guard is
  still satisfied and no invariant breaks).
- `select_speech_model` still permits sherpa on macOS (it checks `available_on`),
  so a Mac user who already downloaded sherpa is not locked out at the IPC layer;
  `migrate_unavailable_speech_model` (which uses `offered_on`) moves them to the
  macOS default on next launch, which is the intended cleanup.

## Alternatives considered

1. Flip `Engine::available_on(SherpaParakeet)` to `!is_macos()`. Rejected: that
   function is documented to mirror Cargo.toml linkability, and sherpa genuinely
   links/runs on macOS; making it report `false` would misstate capability and
   hard-reject the model at `select_speech_model`, a harsher behavior than
   needed for "don't show".
2. Filter only inside `OnboardingScreen.tsx`. Rejected: duplicates platform
   logic in the UI, contradicts the "backend list is the single source of truth"
   design, and would still show the Windows rows in Settings.

## Verification

- `cargo test -p teletype-speech --lib catalog` -> 32 passed, 0 failed (includes
  new `sherpa_is_never_offered_on_macos`; updated `offered_on_is_the_list_predicate`).
- `cargo test -p teletype-desktop --lib speech_model_list` -> 4 passed (updated
  `speech_model_list_only_offers_what_the_host_can_load` to assert sherpa is
  *absent* on macOS and present on Windows).
- `cargo test -p teletype-desktop --test migration` -> 18 passed (Mac+sherpa saved
  default migrates to a valid macOS row).

## Follow-ups (not in this change)

- `fluid-speech-mini` (parakeet.cpp) remains on the Mac list but cannot be
  measured: the eval harness has no ParakeetCpp provider seam. Wiring it is a
  prerequisite before its accuracy claim is trusted.
- The wider "show only the top Mac models" shortlist (Ultra / ggml Parakeet v3 /
  Whisper for long-tail languages) is proposed in `.agents/brain/asrs.md` but not
  yet applied; a shipped-recommendation change is gated on the human corpus
  (still 0/120) per asrs rule 1.
