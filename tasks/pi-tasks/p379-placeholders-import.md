# Task: P3.7+P3.9 — Snippet placeholders + custom words import/export

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Files you may edit
- `crates/teletype-core/src/autotext/mod.rs` (placeholder expansion) and any
  sibling file in `crates/teletype-core/src/autotext/`
- `crates/teletype-core/src/dictionary.rs` (import/export)
- `crates/teletype-desktop/src/commands.rs` (new commands)
- `crates/teletype-desktop/src/lib.rs` (register commands)

## Why
Roadmap Tier 3 quick wins:
1. **P3.7 snippet placeholders** — `{{date}}`, `{{time}}`, `{{clipboard}}` inside
   AutoText replacement values, expanded at injection time. Closed set only, no
   code execution (that is the hard rule from the roadmap).
2. **P3.9 custom words import/export** — users migrating from other apps (or
   backing up) need JSON import/export of the custom-words dictionary.

## Read first
1. `crates/teletype-core/src/autotext/` (whole directory): the `AutoTextStore`
   shape, how a replacement value is produced, and where `typing.rs` and the
   voice pipeline each consume it (search for `lookup` / `expand`).
2. `crates/teletype-core/src/dictionary.rs` (whole file): the custom-words store
   shape, its `Store` (save/load), and existing add/remove commands in
   `commands.rs` (search for `dictionary` or `custom_word`).
3. `crates/teletype-desktop/src/commands.rs`: the file-path command pattern if
   one exists (search for `open` / `dialog`); if there is no file dialog
   pattern, use `tauri_plugin_dialog` ONLY if it is already a dependency —
   otherwise take an explicit path parameter and say so in the report.

## Build
### Part A: placeholders
1. In the autotext expansion path (the single place where a replacement string
   becomes final text — find it; if there are two, voice and typed, factor a
   shared `expand_placeholders(&str) -> String` and call from both):
   - `{{date}}` → local date, format `YYYY-MM-DD` (use `chrono`, already in tree;
     verify with `grep chrono Cargo.toml`).
   - `{{time}}` → local time `HH:MM` (24h).
   - `{{clipboard}}` → current clipboard text via `arboard` (already a workspace
     dep). On error (clipboard unreadable), replace with empty string and log a
     warning; never fail the expansion.
   - Unknown `{{...}}` tokens are left verbatim (do not delete user text).
   - Expansion happens at EXPANSION time (each use gets a fresh value), not at
     save time.
2. Keep it synchronous and cheap; clipboard read is the only syscall.

### Part B: dictionary import/export
1. `export_custom_words(path: String)` — serialize the dictionary store to JSON
   (versioned envelope: `{"version": 1, "words": [...]}` following the envelope
   convention used by other stores in this crate; check `history.rs` /
   `storage.rs` for the house shape).
2. `import_custom_words(path: String)` — parse the same envelope, validate each
   entry, MERGE into the existing store (import wins on key conflict), return
   counts `{imported, updated, skipped}`. Reject anything that is not the
   envelope (wrong version or shape) with a clear error; never truncate the
   existing store on a bad file.
3. Register both commands in `lib.rs`.

## Out of scope
- No UI (buttons live in a later UI task).
- No other placeholder types (no `{{clipboard:line1}}`, no expressions).
- No changes to the typed-tap (`.m` file) or the watcher thread.

## Verification (paste real output)
1. `cargo test -p teletype-core --lib 2>&1 | tail -3`
2. `cargo test -p teletype-desktop --lib 2>&1 | tail -3`
3. `cargo build -p teletype-desktop 2>&1 | tail -3`
4. Unit tests:
   - `{{date}}`/`{{time}}` expand to the right format (freeze the clock or assert
     format shape with a regex if freezing is not available)
   - unknown `{{foo}}` stays verbatim
   - `{{clipboard}}` with an unreadable clipboard yields empty string, not an
     error (mock or accept the platform reality; document what you did)
   - export → import round-trip is lossless
   - import of a corrupt file leaves the existing store intact
   - import merge: new words added, conflicting key updated, counts correct
