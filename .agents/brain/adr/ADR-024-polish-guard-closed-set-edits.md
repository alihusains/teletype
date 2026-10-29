# ADR-024 - The polish guard admits edits from a closed set of English grammar families

- Status: accepted
- Date: 2026-10-27
- Deciders: Ali (dispatch), `vocs-u4b-families-consumers` worker (implementation), lead (verification)
- Related: `.agents/brain/adr/ADR-023-settle-survival-measure-not-rewrite.md`,
  `.agents/brain/adr/ADR-020-polish-models-per-host.md`, Vocs `src-tauri/src/` (mechanism ported here)

## Context

The polish step sends user text to a general-purpose LLM with an instruction to fix
grammar. The only thing standing between that model's output and the user's document
was `validator.rs`, which rejected output when the *total* changed-word share exceeded
the user's `polish_max_change_percent` budget, when the output was off topic, or when it
looked truncated.

Two problems with that accounting:

1. A real grammar fix and a rewrite of the claim both cost the same. "their going to
   review it" -> "they're going to review it" (correct) and "their going to review it" ->
   "they will review it" (a different sentence) differ by one word each, and the ceiling
   could not tell them apart. Turning the ceiling up to let grammar through let rewrites
   through at the same time.
2. Language drift was unguarded. A model that answered a French take in Arabic, or an
   English take in Cyrillic, passed every existing check as long as it stayed short and
   shared enough words.

The old module doc claimed a "closed set of inflection families" that the code did not
enforce, and carried 69 word families accumulated for that claim, most of them content
words (`went`/`go`, `bought`/`buy`).

## Decision

Make the doc true: treat model output as a list of *proposed* edits and admit each one
only if it falls in a closed set.

1. `transforms/families.rs` holds the closed sets. `FAMILIES` is trimmed from 69 entries
   to 16, keeping only function-word families (`is/are/was/were`, `a/an`, `their/there`,
   `do/does/did`, `then/than`, ...). Content-word families are deleted: `is_grammar_edit`
   requires *both* sides to be in `FUNCTION_WORDS`, so a content family could never fire.
2. `is_grammar_edit(a, b, families)`: both sides function words AND (same family OR
   near-spelling). Anything else is a content edit.
3. `content_change_share` (validator.rs:238) replaces `change_share` inside
   `enforce_change_limit` (validator.rs:186). Two-pass multiset: pass 1 exact matches,
   pass 2 pairs the leftovers that are grammar edits. The user's ceiling is therefore
   only spent on content changes.
4. `Failure::LanguageDrift` (validator.rs:47) fires in
   `validate_with_off_topic_floor` (validator.rs:380) when the input is Latin-script and
   the output is not. The check is deliberately one-sided and script-based, not a
   language detector: `is_latin_script` is honest about that in its own doc comment, and
   a German take rewritten into German passes untouched.
5. Monotonicity is a tested invariant: `content_change_share(a, b) <= change_share(a, b)`
   for all inputs, so the change can only widen what the guard accepts, never silently
   narrow it.

## Consequences

- Grammar fixes no longer eat the change budget, so `polish_max_change_percent` can stay
  conservative while still letting real corrections through. The guard is reached in
  production on every polish take: `engine.rs:522` and `engine.rs:602` and
  `dictation.rs:262` all go through `validate_polish` -> `enforce_change_limit`.
- Dead weight removed: `Tok.lead`/`Tok.trail`/`Tok.norm`, `FamilyMap::in_any_family`,
  `MAX_CHANGED_SHARE`, `MIN_EDIT_ALLOWANCE`, `MAX_HUNK_WORDS`, and the false claim in the
  module doc.
- **Known over-conservatism (open):** `validator::words` tokenises by splitting on every
  non-alphanumeric character, so `don't` becomes `["don", "t"]` and `they're` becomes
  `["they", "re"]`. Any `FAMILIES` member containing an apostrophe is therefore
  unreachable through `content_change_share`: fixing `their` -> `they're` (with the
  apostrophe) still costs one content change, while `their` -> `theyre` is free. This
  fails *closed*, so it is not a safety hole, but it under-delivers on the contraction
  half of the guard. Loosening it means normalising apostrophes in the tokeniser, which
  widens what the guard accepts, and that needs measured evidence from the polish corpus
  before it should ship.
- The guard's closed set is English-only. On a non-English take, `is_grammar_edit` never
  fires, so accounting falls back to counting every edit as content, i.e. exactly the old
  behaviour. Non-English polish stays engine-bound (see the Vocs borrow note).

## Evidence

- `cargo test -p teletype-core --lib transforms` (re-run by the lead, not the worker's
  log): `173 passed; 0 failed; 747 filtered out`.
- `cargo test -p teletype-core --lib`: `912 passed; 8 failed`. All 8 failures are in
  `fillers::corpus`, `fillers::tiers` and `pipeline::tests::filler_removal_preserves_newlines`,
  which are the live `vocs-u5b` worker's files, not U4's.
- `cargo clippy -p teletype-core --all-targets` via `scripts/check-clippy-clean.py`: no
  lint originates in `transforms/**`.
- `families.rs` is 321 lines; the 16 retained families and `FUNCTION_WORDS` were read
  line by line against `is_grammar_edit`'s requirements, and `content_change_share` was
  re-derived for the monotonicity claim.
