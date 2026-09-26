# Task: P3.2+P3.3 — Emoji restore + language detection (per-app language)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Files you may edit
- `crates/teletype-core/src/emoji.rs` (NEW) + module declaration in `lib.rs`
- `crates/teletype-core/src/emoji-dictionary.json` (NEW data file, see below)
- `crates/teletype-core/src/pipeline.rs` (post-process hook after the transform)
- `crates/teletype-core/src/itn.rs` (READ for the house style of a pure
  post-processor; do not edit it)
- `crates/teletype-core/src/context/` (per-app language map) +
  `crates/teletype-desktop/src/commands.rs` (get/set command)

## Why
Roadmap T3.4 (Tier 3): polished output should feel professional. Two cheap wins:
1. **Emoji restore** — ASR transcribes "thumbs up" / "smiley face" as words.
   Restoring a small curated set of emoji after the transform makes output feel
   human. (Roadmap P3.2. Do NOT copy EnviousWispr's 127 KB dictionary; license
   unclear. Ship our own ~100-150 entry list, Apache-2.0, written by us.)
2. **Language detection + per-app language** — we hardcode `auto` → `en` today.
   Parakeet already lists 29 languages. A per-app language override
   (e.g. Gmail → en, a German-notes app → de) is the cheap half of P3.3; full
   statistical LID is NOT in scope.

## Read first
1. `crates/teletype-core/src/itn.rs` (first 80 lines): the house pattern for a
   pure post-processor (`normalize(input) -> String`, table-driven tests in
   `crates/teletype-core/tests/`).
2. `crates/teletype-core/src/pipeline.rs` line ~137: where `itn::normalize` is
   applied (`if is_voice && itn::should_run(&self.profile.language)`) — the emoji
   restore hooks in the same place, AFTER the LLM transform (emoji words survive
   polishing as words).
3. `crates/teletype-core/src/context/`: `ApplicationContext` fields.
4. `crates/teletype-desktop/src/commands.rs`: settings get/set patterns (search
   for `language`).

## Build
### Part A: emoji restore
1. `crates/teletype-core/src/emoji-dictionary.json`: our own curated list,
   format `[{"trigger": "thumbs up", "emoji": "👍"}, ...]`. 100-150 entries,
   common ones first (thumbs up/down, heart, smiley, laughing, fire, clap, eye,
   wave, muscle, thinking face, crying, party, check mark, x, star, sparkles,
   ...). Each trigger is the phrase ASR most often produces. Include both the
   bare phrase and "the emoji for X" variants only where unambiguous.
2. `crates/teletype-core/src/emoji.rs`:
   - Load the JSON at compile time with `include_str!` + serde (no IO at runtime).
   - `pub fn restore(text: &str) -> String`: word-boundary, case-insensitive
     replacement; longest trigger first; never replace inside a longer word
     (e.g. "heart" alone must NOT become ❤️ — require the trigger to be
     "heart emoji" or "heart emoticon", or the bare word only for unambiguous
     entries; document the rule in the file header).
   - Idempotent: running it twice changes nothing.
3. `pipeline.rs`: apply `emoji::restore` after the transform step when the input
   language is a Latin script (skip CJK) and the setting is on.

### Part B: per-app language
1. Add `app_language_overrides: BTreeMap<String, String>` (follow
   whatever container the styles store uses) mapping normalized app key →
   language code.
2. Where the pipeline resolves the ASR language (search for where `profile.language`
   or `auto` is resolved to a concrete code), consult the override for the
   frontmost app first, then the global setting.
3. Commands: `set_app_language_override(app_key, lang)` (empty removes) +
   `get_app_language_overrides()`, registered in `lib.rs`.

## Out of scope
- No statistical/ML language detection (that is the larger P3.3).
- No UI (data layer + commands only).
- Do not copy EnviousWispr's emoji dictionary or any of their data files.

## Verification (paste real output)
1. `cargo test -p teletype-core --lib 2>&1 | tail -3`
2. `cargo test -p teletype-core --test itn_parity 2>&1 | tail -3` (must not regress)
3. `cargo test -p teletype-desktop --lib 2>&1 | tail -3`
4. `cargo build -p teletype-desktop 2>&1 | tail -3`
5. Unit tests in `emoji.rs`:
   - "thanks, thumbs up!" → "thanks, 👍!"
   - "my heart" unchanged (bare "heart" is ambiguous)
   - "heart emoji" → "❤️"
   - idempotency: `restore(restore(x)) == restore(x)`
   - CJK text unchanged
   - per-app language: override for app A wins over global; unknown app falls
     back; round-trips through the store
