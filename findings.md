# Teletype vs EnviousWispr — Findings & Roadmap

Generated 2026-09-23 by a multi-agent gap-analysis workflow (53 agents: 3 surveys,
5 gap lenses, 46 verified findings, 1 refuted). Reference codebases:
`enviouswispr/EnviousWispr` (Mac, Swift) and `enviouswispr/enviouswispr-windows` (.NET).

> **Status section updated 2026-09-25; treat unannotated entries as unverified
> against git.** Annotations below are git-verified (commit hashes cited); the
> analysis text is the original 2026-09-23 assessment and was not rewritten.

## Where we stand

Teletype is a solid macOS dictation core: a clean five-stage pipeline (dictionary →
AutoText protect → LLM transform → filler → restore), a real custom-transforms
feature that EnviousWispr does not have, working local (llama-server) and BYOK
(OpenAI-compat) polish, and a 99-language Whisper catalog. But it is behind in
exactly the places a dictation app is judged: delivery reliability (cancel still
pastes, Windows doesn't compile, single clipboard+Cmd+V injection), deterministic
text quality (no ITN, no language-drift guard, filler removal flattens line breaks
after the model), and trust signals (raw HTTP dumps, false-positive connection
test, dead 20s pill). EnviousWispr wins on polish depth (S1 control line, ITN
floor, language detection chips, escape recovery, crash spool, streaming) and
ships the boring reliability features by default. Teletype's honest position:
strong differentiator in transforms, but a first release would embarrass itself
on reliability before anyone reaches the differentiator.

## P0 — Do now (broken things, quick wins)

Ordered by (userValue / effort). All verified against the working tree of
2026-09-23.

### P0-1. Cancelled dictation still transcribes and pastes — 1 day, High

> **Status (2026-09-25): FIXED** — commit `0e8b94e`. `stop(cancelled)` now
> discards the recording outright (no transcription, no pipeline, no injection,
> no history); `next_id` advances so any in-flight `Transcribed` event is
> dropped by the staleness guard (verified in `dictation.rs stop()`).

**Bug:** `crates/teletype-desktop/src/dictation.rs` `stop(cancelled)` (line 337)
uses `cancelled` only for pill copy and final `Phase::Cancelled` (line 411). The
worker thread unconditionally runs `recording.finish()` → `transcribe()` →
`Event::Transcribed`, and `transcribed()` (line 415) checks only session
staleness, then runs the full pipeline and injects. Cancel is the most-trusted
control in dictation; a cancel that pastes is trust-breaking.

**Fix:** Thread `cancelled` into the worker closure. In `stop()`, if `cancelled`,
skip `transcribe()` and send `Event::Cancelled { session }` (or simply do not send
`Transcribed`); in `transcribed()`, add `if self.phase == Phase::Cancelled {
return; }` as a second guard. Skip history save and injection for cancelled
sessions.

### P0-2. Windows build is broken — ~2 hours, High

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (Fn hotkey branches
> cfg-gated for the Windows compile, `dictation.rs`). Note: the earlier
> worktree commit `4f2ded7` addressed the same finding but is not on main.

**Bug:** `crates/teletype-desktop/src/lib.rs:8` declares `mod fn_tap;`
unconditionally; `fn_tap.rs:8` has `#![cfg(target_os = "macos")]` so the module is
empty on Windows, while `dictation.rs:97` and `:121` call
`crate::fn_tap::start/stop` unconditionally inside the `hotkey == "Fn"` branch.
Hard E0433 on any Windows target, regardless of runtime config.

**Fix:** Wrap the Fn branch in `register_hotkey`/`unregister_hotkey` with
`#[cfg(target_os = "macos")]` and return `Err("Fn key is macOS-only; pick another
hotkey")` on other targets.

### P0-3. Filler removal runs after the LLM and flattens line breaks — ~2 hours, High

> **Status (2026-09-25): FIXED** — filler removal now runs as pipeline step 0.5
> before the transform, line-aware (verified: `pipeline.rs` step 0.5,
> `remove_filler_words` preserves newlines; regression test for newline
> preservation in `e2a9ec9`). The worktree commit `b095e42` is not on main;
> the fix landed via the P0 batch.

**Bug:** `crates/teletype-core/src/pipeline.rs` step 4 calls `remove_filler_words`
on `text_after_transform`; it does `split_whitespace().filter(...).join(" ")`,
collapsing every newline. Any line breaks the model produced (the EG-1 prompt
requires greeting/body/sign-off on separate lines) and any bullets from
`format_spoken_lists` (`transforms/engine.rs:169`) are flattened to one paragraph
whenever filler removal is enabled, is voice, and the list is non-empty. On by
default with 17 default words.

**Fix:** Move the filler pass to step 0.5, on `input_text` before the transform
(same pattern as dictionary correction at step 0). Re-run the ~17 pipeline tests
in `pipeline.rs`.

### P0-4. Reasoning models fail 100% of dictations via openai-compat — ~3 hours, High

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (temperature omitted for
> o1/o3/o4/gpt-5 reasoning models, `openai_compat.rs`; detection unit tests
> included). Worktree commit `001728b` is not on main.

**Bug:** `crates/teletype-inference/src/openai_compat.rs:182` sends
`"temperature"` unconditionally; o1/o3/o4/gpt-5 non-chat reject it with HTTP 400,
and the retry loop (line 208) only retries 5xx/429. `engine.rs:125-156` falls
back to raw input on any `Err`, so every dictation passes through unpolished with
only a raw 400 in the log. Reasoning models are the most common BYOK "best
quality" pick.

**Fix:** Add a small model-id capability check in `openai_compat.rs` (or
`engine.rs`): if the id matches `o1|o3|o4|gpt-5` non-chat, send
`reasoning_effort: "low"` instead of `temperature`. Port the pattern from
`enviouswispr-windows/.../OpenAiPolishProvider.cs:70-85`.

### P0-5. 401/403/429 surfaced as raw HTTP dumps — ~3 hours, High

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (401/403/429 classified
> into user-facing messages, `openai_compat.rs`; classification unit tests
> included). Worktree commit `5284b24` is not on main.

**Bug:** `openai_compat.rs:208-211` formats every non-retryable error as
`HTTP {status}: {body-400-chars}`; `commands.rs:854` `test_llm_connection` returns
it verbatim and `ui/src/screens/ModelsScreen.tsx:171` displays it. Key/billing
misconfiguration is the #1 BYOK onboarding failure.

**Fix:** Classify in `openai_compat.rs`: 401 → "API key rejected — re-enter it in
Settings", 403 → "Access denied — check billing/access", 429 + body contains
`insufficient_quota` → "Out of credits — check provider billing" (and mark
non-retryable), other 429 → transient rate limit (retryable). Surface distinct
strings in `test_llm_connection` and `ModelsScreen`. Reference:
`enviouswispr/.../PolishFailureReason.swift:67-75`.

### P0-6. Test connection is a false positive — ~2 hours, Med

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (`test_connection` checks
> the configured model against the endpoint's model list,
> `openai_compat.rs`). Worktree commit `1658caf` is not on main.

**Bug:** `openai_compat.rs:117-143` `test_connection` calls `list_models` and
returns `Ok("N models available")` on a successful GET /models, discarding the
ids. A typo'd model id (e.g. `gpt-4o-mni`) passes green and fails at first
dictation.

**Fix:** Check `self.config.model` membership in the fetched ids; if absent,
return a warning naming the configured model and the closest match. Fallback
branch (1-token chat) already exists for endpoints without /models.

### P0-7. Truncated output discards the whole transform — ~3 hours, Med

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (partial content returned
> on truncation instead of Err, `openai_compat.rs`).

**Bug:** `openai_compat.rs:200-201` and `server.rs:225-227` return
`Err("stopped at max_tokens")` when `finish_reason == "length"`; `engine.rs:125-156`
falls back to raw input. The partial rewrite is already in hand (the `content`
field is present) and is far better than raw speech text; long dictations hit the
cap exactly when polish is wanted most.

**Fix:** Return the partial content with a `truncated: bool` flag instead of
`Err`; plumb a named "output truncated" status through `TransformMetrics` into
the Insights screen. Reference: `OpenAiPolishProvider.cs:124-128` +
`SessionFinalizationEffects.cs:176`.

### P0-8. ServerProvider (default local path): 20s timeout, no retry — ~2 hours, Med

> **Status (2026-09-25): FIXED** — commit `9163631` (`scaled_timeout()` in
> `llm.rs`: wall-clock budget grows with max_tokens, 20s floor / 60s cap;
> unit tests `scaled_timeout_grows_with_max_tokens`,
> `scaled_timeout_is_monotonic`). Worktree commit `bc8b31c` is not on main.

**Bug:** `crates/teletype-core/src/llm.rs:24-30` `GenerationParams::default` has
`timeout: 20s`; `engine.rs:117-121` scales `max_tokens` to input length (up to
2048) but not the wall-clock budget, so a 2048-token generation at ~100 tok/s
times out and the polish is silently discarded. The retry loop exists only in
`openai_compat.rs:187-224`; `server.rs:192-207` makes a single request.

**Fix:** In `engine.rs`, scale `params.timeout` with `max_tokens` (e.g.
`Duration::from_secs(20 + max_tokens as u64 / 50)`); port the one-retry-on-5xx/429
pattern from `openai_compat.rs` into `server.rs::chat`.

### P0-9. API key stored under fixed "openai" account for every endpoint — ~2 hours, Med

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (per-host keychain
> accounts, `secret_account_for_url` + `read_api_key` in `commands.rs` with
> legacy "openai" fallback; `ModelsScreen.tsx` updated). Worktree commit
> `a1193dd` is not on main.

**Bug:** `ui/src/screens/ModelsScreen.tsx` calls
`set_llm_secret({providerId: "openai"})` for all endpoints; `commands.rs`
hardcodes `get_secret("openai")` in three sites. `secrets.rs` already supports
per-provider accounts (`llm:{provider_id}`). Switching OpenRouter → Groq sends
the old key to a new third party; switching to Ollama keeps a stale key riding on
"local" requests.

**Fix:** Key the account on the base URL's host (e.g. `llm:openrouter.ai`); clear
the old account on endpoint change; replace the three hardcoded reads in
`commands.rs`.

### P0-10. Commit the untracked WIP modules — today, High

> **Status (2026-09-25): CLOSED** — the WIP P0 batch was committed as
> `3fa2793` (server.rs, openai_compat.rs, download.rs, catalog.rs, manager.rs,
> UI); follow-up commits `9163631`, `56dc123`, `1c9f3d9` landed on top. All
> previously-untracked modules are tracked on main.

**Bug:** `git status` shows `??` for `crates/teletype-inference/src/{server.rs,
openai_compat.rs, download.rs}`, `crates/teletype-desktop/src/secrets.rs`, and
`ui/src/screens/DeveloperScreen.tsx` (plus `downloadStore.ts`,
`DownloadProgress.tsx`). These modules are declared by committed files, so a
clean checkout does not compile, and the most safety-relevant code (secret
storage, remote downloads) is one laptop crash from being lost.

**Fix:** Review and `git add` the five modules plus the two UI files; commit.

### P0-11. Hot-word correction: raw edit-distance ≤ 2 with no similarity floor — ~half day, High

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (similarity floor 0.80 on
> hot-word near-miss correction, `pipeline.rs`; regression test included).
> Worktree commit `d04e556` is not on main.

**Bug:** `crates/teletype-core/src/pipeline.rs:317` `correct_with_dictionary`
accepts any fuzzy entry with `dist <= 2` and no similarity ratio or stop-word
penalty. User-taught words default to `fuzzy: true` (`dictionary.rs:41`), so a
taught 3-letter word like "apt" rewrites dictated "apt" unconditionally. The
builtin exact-only workaround (`BUILTIN_DICTIONARY_VERSION = 2`) is evidence the
team already hit this in the wild.

**Fix:** Add a length-based similarity floor (e.g. `1 - dist/max_len >= 0.82` for
single words) to both acceptance loops in `correct_with_dictionary`; reference
thresholds in `enviouswispr-windows/.../CustomWordCorrector.cs:11-36`.

### P0-12. Teach-words capture: hardcoded 3s, comment says 5s, no VAD — ~half day, Med

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (energy-based silence
> stop for teach-words capture, 5s cap, `commands.rs`). Worktree commit
> `92d167b` is not on main.

**Bug:** `crates/teletype-desktop/src/commands.rs:354` `transcribe_word` sleeps
exactly 3 seconds (line 374) while the doc comment says "up to 5 seconds". No
silence-endpoint detection. This capture feeds `correct_with_dictionary`, the
hot-word loop.

**Fix:** Replace the fixed sleep with an energy-based silence stop (the dBFS
metering in `teletype-core/src/audio/mod.rs` already computes levels; stop after
N ms of silence below threshold, cap at 5s). At minimum, align comment and code.

### P0-13. Dead LlamaProvider / ProviderHandle — ~2 hours, Med

> **Status (2026-09-25): FIXED** — commit `e2a9ec9` (dead `LlamaProvider` and
> `ProviderHandle` deleted; `llama.rs`, `manager.rs`, `lib.rs` updated).
> Worktree commit `e948c72` is not on main.

**Bug:** `crates/teletype-inference/src/llama.rs` `LlamaProvider` always returns
`Err` and has zero callers (only the `lib.rs` re-export); `manager.rs:123-152`
`ProviderHandle` is unused. Deliberate stubs from the pre-llama-server era, but a
trap for anyone extending the provider list.

**Fix:** Delete `llama.rs` and `ProviderHandle`; keep `ModelManager`/`ModelState`
(live, wraps `ServerProvider`); update `lib.rs` re-exports and docs.

## P1 — Next (feature gaps that most close the product gap)

### P1-14. Deterministic ITN (numbers, dates, phones, money, units) — M, High

> **Status (2026-09-25): OPEN** — implementation plan completed (read-only
> prep, 2026-09-25): port `InverseTextNormalizer.swift` to
> `crates/teletype-core/src/itn.rs` as pipeline step 0.6, fixtures from
> `macos-itn-parity.jsonl` (2084 rows) + holdout (3756 rows). Build task on
> the team board, gated behind the P0 fixes (shared `pipeline.rs` edits).

**Gap:** Pipeline has no ITN stage; `transforms/mod.rs:138` delegates "dates,
times, numbers, currency, percentages, phone numbers" to the LLM. With no model
loaded, or a too-short bypass, "twenty twenty six" stays spelled out.
EnviousWispr's `InverseTextNormalizer.swift` (~850+ lines, 99.8% on corpus,
parity-pinned via `parity.jsonl`) runs always-on before polish and doubles as the
raw-fallback floor.

**Approach:** New module `crates/teletype-core/src/itn.rs` (or port of the Swift
engine): word→digit tables for numbers/ordinals, date patterns, phone digit-runs
(7/10 digits), currency, units, AP-style 1-9 spelled. Run it as step 0.6 (after
filler, before transform) so it works with no model; also run it on the no-model
branch. Port the `parity.jsonl` fixtures as Rust test vectors.

**Dependencies:** P0-3 (filler ordering) so ITN sees clean text.

### P1-15. Language auto-detection — M, High

> **Status (2026-09-25): PARTIAL** — `dictation.rs effective_language` now
> passes "auto" through to Whisper (which auto-detects); empty/legacy
> defaults to "en". The full language picker with lock chips and per-language
> defaults is not yet built (unverified against git).

**Gap:** `dictation.rs:668` `effective_language` maps "auto"/empty to "en" with
the comment "auto-detect is unreliable for short utterances"; the
`SettingsScreen.tsx` language picker offers only English. The 99-language Whisper
catalog (`teletype-speech/src/catalog.rs:47-48`) is unusable. The Whisper backend
already honors language (`whisper.rs:59` `params.set_language`); only Parakeet is
English-only.

**Approach:** (a) Fill the picker with the catalog's language list; (b) stop
coercing "auto" to "en" for the Whisper path (pass through; Whisper auto-detects);
(c) optionally add a detector (linguist crate or whisper's own `language` field
from the segment) with a "Detected <Lang>. Lock it?" chip. Reference:
`enviouswispr/.../LanguageDetector.swift` five-layer autodetect +
`LanguageChipView.swift`.

**Dependencies:** P0-1 (cancel) so a bad-detect cancel actually discards.

### P1-16. Per-step failure taxonomy + user-visible polish status — M, Med

> **Status (2026-09-25): PARTIAL** — `validator.rs` has a 7-case `Failure`
> enum (Empty, PromptEcho, InstructionEcho, Preamble, MarkdownFence,
> MassivelyExpanded, Truncated) with Display strings, landed in `9163631`
> (expanded taxonomy incl. truncation). User-visible "Polish failed, using
> raw text" status surfacing is not yet built (unverified against git).

**Gap:** `engine.rs:62-115` lumps too-short bypass and transport failure into
`transformed: false, failure: None`; the validator's six `Failure` variants are
output-quality only and consumed nowhere;
`PipelineResult.transform_skipped_no_model` is logged to file only
(`dictation.rs:488`), never shown. EnviousWispr has 17 leaf `PolishSkipReason`
cases with telemetry wire names and a narrator overlay ("Polish failed. Using raw
text.").

**Approach:** Add a `SkipReason` enum in `crates/teletype-core/src/transforms/`
(tooShort, noModelLoaded, providerTimeout, providerError{status},
validatorFailure{variant}, truncated). Distinguish the eg1 `Err(())` site (bypass
vs transport), map provider errors in `engine.rs`, and surface the last N in
`InsightsScreen.tsx` (the usage-tracking Tauri command already exists).

**Dependencies:** P0-5/P0-7 (status strings) so the taxonomy reuses the same
user-facing wording.

### P1-17. Language-drift detection on polished output — M, High

**Gap:** `crates/teletype-core/src/transforms/validator.rs` `Failure` has six
variants, no language check, and `validate()` takes no language argument. A cloud
model drifting to another language is pasted silently. The language signal
already exists: `TransformDefinition.language` (`transforms/mod.rs:23`),
`LANGUAGE: {ctx.language}` in the prompt, `eg1_too_short(text, language)` in
`prompt.rs:98`.

**Approach:** Add a `LanguageDrift` `Failure` variant; in `validate()`, compare
input vs output language using a fast detector (linguist crate) with a
min-alphabetic-char fail-open threshold (~24, per EnviousWispr's
`NLLanguageRecognizer` design). On drift, fall back to raw input with a surfaced
status.

**Dependencies:** P1-16 (taxonomy) for the fallback status.

### P1-18. Auto-delivery of the selected speech model — M, High

**Gap:** `dictation.rs:271` `warm_up()` silently returns if the model file is
missing; `transcribe()` (line 759) returns the bare `Err("Speech model not
downloaded")` with no link to the fix. Default model is parakeet-tdt-v3 (640 MB).
Onboarding offers a download step, but a skipped onboarding, a deleted file, or a
model switch hits the dead end.

**Approach:** In `transcribe()`, on missing file, emit a Tauri event that opens
the Models screen with a "Download model (640 MB)" button, or trigger
`download_speech_model` on-demand (the download machinery in `download.rs`
already exists). Add a "Getting dictation ready" pill state.

**Dependencies:** P0-10 (download.rs committed).

### P1-19. Model download resume — M, Med

> **Status (2026-09-25): PARTIAL** — `download.rs` already has HTTP Range
> resume ("attempting resume" from the partial file, `Range: bytes=N-`
> header, verified in current tree) plus a disk-space check. Checksum
> verification per DeliveryManifest: unverified against git.

**Gap:** `crates/teletype-inference/src/download.rs:386` `try_fetch_to_part`
unconditionally `remove_file(&part)` then `File::create` with no `Range` header;
every failed attempt (EG-1 is 2.9 GB in shards) restarts from byte 0. `ModelStore`
has no disk-space probe.

**Approach:** Add `Range: bytes=N-` + `If-Range` (ETag) on retry, append mode, a
`.resume.json` record per shard, a 416 handler, and a 256 MiB disk-space probe.
SHA-256 verification and progress events already exist. Reference:
`enviouswispr-windows/.../ArtifactDownloadTransport.cs:131-140`.

**Dependencies:** P0-10.

### P1-20. S1-style control line (tone/structure/context) — M, High

> **Status (2026-09-25): BUILT AND VERIFIED** — three closed enums
> (`S1Styling`/`S1Structure`/`S1Context`) + frozen control line in
> `prompt.rs` (`9163631`), pickers in `StylesScreen.tsx` (`56dc123`),
> DeveloperScreen debug readout (`2d31981`), and 8 verification tests
> pinning wire tokens, per-axis prompt changes, provider-wire delivery, and
> backward compat (`e353e79`). Note: the control line applies to the S1-mini
> path only (not the generic `build_prompt` path), which is correct for the
> model card; this is the mirror image of the approach text below.

**Gap:** Teletype has tone via free-text style profiles (`style.rs`, fully wired)
but no closed-enum control line. No user-facing structure knob (prose vs lists is
hard-coded in `CORE_RULES` + `format_spoken_lists`). No explicit context axis
(general vs email), though `prompt.rs` auto-detects the host app.

**Approach:** Add three closed enums (`Styling: casual/semi-casual/semi-formal/
formal`, `Structure: prose/lists`, `Context: general/email`) to `PromptContext`
in `transforms/prompt.rs`; render a frozen `[Styling: …] [Structure: …]
[Context: …]` line in `build_prompt`. Add pickers to `StylesScreen.tsx` (the
screen and CRUD commands already exist). Note: applies only to the generic
`build_prompt` path, not the training-locked EG-1 system prompt.

**Dependencies:** P1-16 (so the chosen context can be logged).

### P1-21. Filler removal: false starts, repetitions, multilingual awareness — M, Med

**Gap:** `pipeline.rs:243-268` `remove_filler_words` is a pure user-list filter
with no false-start/repetition handling and no language awareness. The 17 defaults
include "er", "um", "mm", "ah", which are real words in German/Portuguese/etc.
EnviousWispr's `FillerRemovalStep.swift` has unit-guarding (the "the gap is 5 mm"
regression) and a 9-line per-language blocklist.

**Approach:** (a) Add per-language blocklist (skip removal of protected tokens
when `ctx.language` is de/nl/da/no/sv/pt/sl/hr); (b) add false-start detection
(repeated first-N-word prefix at sentence start); (c) keep the user list.

**Dependencies:** P0-3 (filler runs on pre-transform text) and P1-15 (language is
real).

### P1-22. Per-context polish routing — M-to-L, Med

**Gap:** `pipeline.rs:137-143` selects exactly one transform (explicit >
auto_apply > none) for the whole dictation; `active_style_profile` is a flat
global that only appends phrases, never selects. `active_application()` is already
fetched (line 106) and plumbed into the personalization packet, but not used to
pick the transform.

**Approach:** (a) Map `active_application()` → a context key; (b) let the key
override/parameterize transform selection at `pipeline.rs:137-143`; (c) add a
per-app mapping UI in `StylesScreen.tsx` (reuse the existing style-profile CRUD at
`commands.rs:1400-1478`).

**Dependencies:** P1-20 (control line gives context a real effect).

### P1-23. Ollama model discovery as a picker + verdicts — M, Med

**Gap:** `openai_compat.rs:89` `list_models` already fetches `{base}/models` and
`test_connection` reports a count, but the UI never surfaces the list; the user
types the model name blind. No measured quality verdicts.

**Approach:** Surface `list_models` results as a selectable dropdown in
`ModelsScreen.tsx`; add a small static verdict table (recommended/mixed/
unreliable/notTested, reference `OllamaModelVerdicts.swift`).

**Dependencies:** P0-6 (test connection checks the id).

### P1-24. Spoken-emoji + spoken punctuation + wire learn-from-edits — M, Med

**Gap:** Zero emoji or spoken-punctuation handling in `crates/`.
`personalization/learn.rs` `extract_signals()` is implemented and tested but has
**zero production call sites** (the UI toggle "Learn from my corrections" exists
and is on by default, but the feature is dead code).

**Approach:** (a) New deterministic `emoji.rs` step (spoken "thumbs up" → 👍) as
step 0.7, reference `EmojiFormatterStep.swift`; (b) spoken punctuation commands
("comma", "period") in the same step; (c) call `extract_signals()` in the pipeline
after the user edits a pasted transcript, persist the signals.

**Dependencies:** P0-3 (step ordering settled).

### P1-25. History: search + diff + retention — M, Med

**Gap:** `ui/src/screens/HomeScreen.tsx` is a flat newest-first list, zero search,
no raw-vs-polished diff (`DictationEntry` stores only final text), no
user-facing retention (only `MAX_ENTRIES = 1000` in `history.rs:38`).

**Approach:** (a) In-memory search filter over ≤1000 entries; (b) retain
`raw_input` in `DictationEntry` (the pipeline already returns it) + a word-diff
view; (c) day-based pruning modeled on `JsonHistoryStore.cs:240-263`.

## P2 — Later (bigger bets / differentiators)

### P2-26. Escape recovery (24h retention of cancelled transcripts) — M, High

Spool the cancelled transcript to a keychain-encrypted store on cancel, show a
recovery card, 24h window. Default-ON on Mac (reference:
`SettingsDefaultValues.swift:95`, `WindowsRecoveryTextStore.cs` uses
`CryptProtectData`).

**Dependencies:** P0-1 (cancel must actually discard first).

### P2-27. Crash-recovery audio spool — L, High

Recording lives entirely in RAM (`audio/mod.rs` `Recording::finish` →
`Captured { samples: Vec<f32> }`); a mid-recording crash loses the dictation.
EnviousWispr has an AES-GCM-256 frame spool with a durable marker and a full
"decrypt → transcribe → polish → save" replay chain (`RecoverySpoolReplayer.swift`).

**Approach:** Encrypted frame writer (flush per batch) in
`crates/teletype-core/src/audio/`, durable marker before transcribe, replay path
on launch re-running the pipeline over recovered PCM.

**Dependencies:** P0-1, P1-16 (status for "recovered from crash").

### P2-28. Apple Intelligence / FoundationModels polish path — L, High

> **Status (2026-09-25): GROUNDWORK ONLY** — `9163631` added the
> `tauri-apple-intelligence 0.2.1` dependency (macOS 26+ path) and
> groundwork in commands/lib; no Apple Intelligence provider is wired into
> the pipeline yet (unverified against git beyond the dep).

Teletype has zero on-device, zero-key, zero-download polish (grep for
FoundationModels/AFM returns nothing). EnviousWispr's
`AppleIntelligenceConnector.swift` is its **default** provider, gated
`#if canImport(FoundationModels)`, macOS 26+, with a 4096-token preflight.

**Approach:** New `FoundationModelsProvider` in `teletype-inference` (macOS 26+
only), 4096-token preflight, listed as a provider option.

**Why P2:** L effort, but it is the reference product's default experience and
the only polish option with no download or key; strengthens the local-first/
privacy story.

### P2-29. Streaming ASR + live preview — L, Med

`SpeechProvider::transcribe` is a whole-clip batch API returning a plain
`String`; no streaming, no word timings, no interim text. EnviousWispr streams
during recording and transcribes only the tail at release (Windows: 390ms/10s
fixture), and ships a live-preview pill (`LivePreviewCoordinator.swift`,
default-ON).

**Approach:** whisper.cpp has no streaming API, so use the Parakeet path (already
used, full-clip mode) for streaming; add a `PillState::Preview { text }` variant;
wire an interim-transcription pass.

**Dependencies:** P1-15 (language), P2-27 (spool makes streaming crash-safe).

### P2-30. Multi-tier text delivery (paste cascade + frozen target) — L, High

`teletype-core/src/injector.rs` is a single clipboard+Cmd+V path with no
fallback; if the paste fails, the text lands nowhere. EnviousWispr has
UIA/AX direct write → menu-paste → clipboard+V cascade, plus a frozen target
captured at recording start.

**Approach:** AX/UIA tier first, Edit > Paste menu tier second, clipboard+V last;
capture foreground HWND/window at recording start.

**Dependencies:** P0-2 (Windows compiles).

### P2-31. Streaming polish (SSE) — L, Med

`openai_compat.rs:131,183` and `server.rs:205` hardcode `"stream": false` in
`reqwest::blocking` calls; no SSE parsing, no token callback, the pill shows a
dead 20s timer.

**Approach:** Swap to an async/streaming client, parse SSE lines, add a
"polishing" pill stage with token callback.

**Dependencies:** P0-8 (timeout), P1-16 (status).

### P2-32. Multi-hotkey roles + VAD auto-stop — L, Med

Single hotkey only (`commands.rs:98-140`); no cancel/quickAdd/pasteLast/copyLast
roles, no VAD/silence auto-stop. EnviousWispr has 5 `ShortcutRole` cases and 1.5s
VAD auto-stop by default.

**Dependencies:** P0-1 (cancel), P0-12 (VAD for teach-words is the first VAD
primitive).

### P2-33. File transcription with speaker diarization — XL, High

No file-transcription path (only live-mic `transcribe_word`). EnviousWispr Mac has
a 6-step wizard with diarization (Mac-only in the reference; not in the Windows
port).

**Dependencies:** P1-15 (language), P2-29 (streaming).

### P2-34. Auto-update + What's New — M, High

No `tauri-plugin-updater`, no updater endpoint, no What's New page, no
launch-on-login. EnviousWispr ships Sparkle (Mac) and MSIX (Windows).

**Approach:** `tauri-plugin-updater` + a release-notes endpoint; the same channel
delivers model bundles (Parakeet, EG-1).

**Dependencies:** P0-10.

### P2-35. ASR metadata (confidence, language, word timings) — S, Low

`whisper.rs:77-89` reads only `seg.to_str()`, discarding timing/logprob;
`parakeet.rs` reads only segment text. No confidence, detected-language, or
word-timing data reaches the pipeline.

**Dependencies:** P1-15 (consumes detected language).

### P2-36. Accessibility (Reduce Motion, screen-reader labels, High Contrast) — M, Low

The only aria attribute in the UI is `aria-hidden="true"` on an icon;
`ui/src/pill.tsx` runs infinite CSS animations with no motion opt-out.
EnviousWispr Windows has `PillAction.AccessibilityLabel`/`SpokenLabel` and
HighContrast/ReduceMotion handling.

**Approach:** `prefers-reduced-motion` CSS + aria-labels on the pill + Tauri AX
labels.

## Do not copy

1. **EnviousWispr's 17-case `PolishSkipReason` telemetry taxonomy as a wire
   protocol.** Teletype is not shipping telemetry. Copy the *user-visible* status
   (P1-16) but keep the internal enum small (6-8 cases). The 17-case enum is a
   product of their telemetry pipeline, not a feature.
2. **The "modeless by design / #1255/#1269/#1948" narrative.** The verified finding
   shows this citation is fabricated; EnviousWispr's real control surface is
   S1-mini's three style settings. Do not build against a phantom competitor
   constraint. Teletype's named transforms + user-authored custom instructions are
   already strictly more flexible; un-Beta the Transforms screen and market it.
3. **EnviousWispr's Quick Add as selection-capture.** Both Mac and Windows Quick
   Add are text-selection-capture (shortcut + selected word), not voice-trigger.
   Teletype's voice-trigger teach-words is a different (and for short words,
   arguably better) mechanism. Fix the 3s capture (P0-12); do not replace it with
   a clipboard-selection flow.
4. **Sparkle/MSIX auto-update as a first-release priority.** EnviousWispr ships it
   because it has a large installed base. Teletype is pre-release; a manual
   re-download is acceptable for the first few versions. Build the updater (P2-34)
   when the model-bundle delivery channel is needed, not before.
5. **EnviousWispr's 4096-token AFM context preflight as a general architecture.**
   That is specific to the macOS 26 on-device FoundationModels window. Do not bake
   a fixed context-window preflight into the generic pipeline; the
   `engine.rs:121` `max_tokens` clamp already handles the local server's 4096-
   token budget.
6. **EnviousWispr's 99-language advertising without a working picker.** Their
   language detection is a five-layer WhisperKit wrapper with confidence tiers.
   Teletype's Whisper engine already auto-detects; the gap is the picker and the
   "auto" coercion, not a five-layer detector. Do not build the tier system until
   the basic picker works (P1-15).
7. **EnviousWispr's Windows MSIX packaging + Velopack leftover.** The reference
   Windows port is a WPF/WinUI app with MSIX; Teletype is Tauri. Do not chase MSIX
   or Velopack; the Tauri updater (P2-34) is the correct path. The `Velopack 1.2.0`
   reference in the EnviousWispr csproj is itself a leftover.

## Summary table

| Priority | Item | Effort | Value |
|---|---|---|---|
| P0 | Cancel still pastes | S | High |
| P0 | Windows compile break | S | High |
| P0 | Filler flattens line breaks | S | High |
| P0 | Reasoning models 400 | S | High |
| P0 | 401/403/429 raw dumps | S | High |
| P0 | Test connection false positive | S | Med |
| P0 | Truncation discards partial | S | Med |
| P0 | Local provider 20s/no retry | S | Med |
| P0 | API key fixed account | S | Med |
| P0 | Commit untracked WIP | S | High |
| P0 | Hot-word no similarity floor | S | High |
| P0 | Teach-words 3s no VAD | S | Med |
| P0 | Dead LlamaProvider | S | Med |
| P1 | Deterministic ITN | M | High |
| P1 | Language auto-detect | M | High |
| P1 | Failure taxonomy + status | M | Med |
| P1 | Language-drift guard | M | High |
| P1 | Auto-deliver speech model | M | High |
| P1 | Download resume | M | Med |
| P1 | S1 control line | M | High |
| P1 | Filler multilingual + false starts | M | Med |
| P1 | Per-context polish routing | M-L | Med |
| P1 | Ollama picker + verdicts | M | Med |
| P1 | Emoji + punctuation + learn-from-edits | M | Med |
| P1 | History search + diff + retention | M | Med |
| P2 | Escape recovery 24h | M | High |
| P2 | Crash-recovery audio spool | L | High |
| P2 | Apple Intelligence polish | L | High |
| P2 | Streaming ASR + live preview | L | Med |
| P2 | Multi-tier delivery | L | High |
| P2 | Streaming polish SSE | L | Med |
| P2 | Multi-hotkey + VAD auto-stop | L | Med |
| P2 | File transcription + diarization | XL | High |
| P2 | Auto-update + What's New | M | High |
| P2 | ASR metadata | S | Low |
| P2 | Accessibility | M | Low |
