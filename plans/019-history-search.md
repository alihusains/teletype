# Plan 019: Add SQLite FTS5 full-text search to dictation history

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- crates/teletype-core/src/persist.rs crates/teletype-core/src/history.rs ui/src/screens/InsightsScreen.tsx`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MEDIUM
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

The `history` table stores every dictation (text, raw_text, context, model,
timestamp) but has no search capability. Users who dictate hundreds of
entries per week cannot find a specific dictation ("that email I wrote about
the Q3 budget") without scrolling. The Insights screen shows aggregate
stats but has no text search. Adding SQLite FTS5 (a virtual table indexed on
`text` + `raw_text`) gives instant full-text search with zero external
dependencies — SQLite has FTS5 built in, and the `rusqlite` dependency
already uses it (the `rusqlite` crate in `Cargo.toml` needs the
`bundled-full-feature` or `features = ["fts5"]` flag; verify which is
already present).

## Current state

- `crates/teletype-core/src/persist.rs:109-121` — `history` table:
  `id TEXT PRIMARY KEY, created_at INTEGER, text TEXT, context_json TEXT,
  duration_ms INTEGER, raw_text TEXT, edits_json TEXT, asr_model TEXT,
  polish_model TEXT, polish_ms INTEGER, word_count INTEGER, recording_path TEXT`
- No FTS5 virtual table exists. No `LIKE '%query%'` search in the codebase.
- `crates/teletype-core/src/history.rs` — history read/write API. Check for
  an existing `search` or `query` function.
- `ui/src/screens/InsightsScreen.tsx` — shows aggregate stats (word counts,
  per-app breakdown, latency percentiles). No text search input.
- `crates/teletype-core/Cargo.toml` — check the `rusqlite` feature flags.
  FTS5 requires `features = ["bundled", "fts5"]` or the system SQLite with
  FTS5 compiled in.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Build | `cargo build -p teletype-core` | exit 0 |
| Core tests | `cargo test -p teletype-core --lib` | all 1411+ pass |
| Typecheck | `cd ui && npx tsc --noEmit` | exit 0 |
| UI tests | `cd ui && npm test` | all pass |

## Scope

**In scope**:
- `crates/teletype-core/src/persist.rs` — add FTS5 virtual table + trigger
- `crates/teletype-core/src/history.rs` — add `search_history(query)` function
- `crates/teletype-core/Cargo.toml` — add `fts5` feature to `rusqlite` if not present
- `crates/teletype-desktop/src/commands/` — add Tauri command `search_history`
- `ui/src/screens/InsightsScreen.tsx` — add search input + results list
- `ui/src/` — add IPC binding for `search_history`

**Out of scope**:
- Any change to the dictation pipeline
- Any change to the learning store
- Any change to the audio or inference crates

## Git workflow

- Branch: `advisor/019-history-search`
- Commit message: `feat(history): add FTS5 full-text search to dictation history`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Verify rusqlite FTS5 support

Check `crates/teletype-core/Cargo.toml` for the `rusqlite` dependency. If
it does not have `features = ["fts5"]` (or `bundled-full-feature`), add it.
FTS5 is a SQLite extension that must be compiled into the SQLite library.

**Verify**: `grep "rusqlite" crates/teletype-core/Cargo.toml` shows the `fts5` feature (or `bundled-full-feature` which includes it).

### Step 2: Add the FTS5 virtual table to persist.rs

In `persist.rs`, after the `history` table creation (around line 121), add:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS history_fts USING fts5(
    text,
    raw_text,
    content='history',
    content_rowid='rowid'
);
```

Then add triggers to keep the FTS index in sync with the `history` table:

```sql
CREATE TRIGGER IF NOT EXISTS history_ai AFTER INSERT ON history BEGIN
    INSERT INTO history_fts(rowid, text, raw_text)
    VALUES (new.rowid, new.text, coalesce(new.raw_text, ''));
END;
CREATE TRIGGER IF NOT EXISTS history_ad AFTER DELETE ON history BEGIN
    INSERT INTO history_fts(history_fts, rowid, text, raw_text)
    VALUES ('delete', old.rowid, old.text, coalesce(old.raw_text, ''));
END;
CREATE TRIGGER IF NOT EXISTS history_au AFTER UPDATE ON history BEGIN
    INSERT INTO history_fts(history_fts, rowid, text, raw_text)
    VALUES ('delete', old.rowid, old.text, coalesce(old.raw_text, ''));
    INSERT INTO history_fts(rowid, text, raw_text)
    VALUES (new.rowid, new.text, coalesce(new.raw_text, ''));
END;
```

**Important**: the `history` table currently has no `rowid` column (it uses
`id TEXT PRIMARY KEY`). FTS5's `content_rowid` requires an integer rowid.
Either:
(a) Add a `rowid` alias: `ALTER TABLE history ADD COLUMN rowid INTEGER PRIMARY KEY AUTOINCREMENT` — but this breaks existing data.
(b) Use a separate `history_fts` table with its own rowid and a `history_id` column mapping back to `history.id`.

**Prefer option (b)** — it is safer for existing databases:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS history_fts USING fts5(
    text,
    raw_text,
    history_id UNINDEXED
);
```

And the triggers reference `history.id` as `history_id`.

**Verify**: `cargo build -p teletype-core` → exit 0.

### Step 3: Add a search function to history.rs

In `crates/teletype-core/src/history.rs`, add:

```rust
pub fn search_history(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<HistoryEntry>, String> {
    // Use FTS5 MATCH with the user's query.
    // Sanitize the query: wrap each token in double quotes to treat it
    // as a literal phrase (avoids FTS5 syntax errors from special chars).
    let safe_query: Vec<String> = query
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    let fts_query = safe_query.join(" ");

    let mut stmt = conn.prepare(
        "SELECT h.id, h.created_at, h.text, h.context_json, h.duration_ms,
                h.raw_text, h.asr_model, h.polish_model, h.polish_ms, h.word_count
         FROM history h
         JOIN history_fts f ON f.history_id = h.id
         WHERE history_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2"
    ).map_err(|e| e.to_string())?;

    let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
        // ... deserialize into HistoryEntry (match the existing struct) ...
    }).map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}
```

**Verify**: `cargo build -p teletype-core` → exit 0.

### Step 4: Add a unit test for search

In `history.rs` (or `persist.rs` tests), add a test that:
1. Creates a temp database.
2. Inserts 3 history entries with distinct text.
3. Calls `search_history("budget")` and asserts the matching entry is returned.
4. Calls `search_history("nonexistent")` and asserts an empty result.

**Verify**: `cargo test -p teletype-core --lib history` → all pass, including the new test.

### Step 5: Add the Tauri command

In `crates/teletype-desktop/src/commands/` (find the file that handles
history-related commands, likely `history.rs` or part of `settings.rs`),
add:

```rust
#[tauri::command]
pub async fn search_history(
    state: State<'_, AppState>,
    query: String,
) -> CommandResult<Vec<HistoryEntry>> {
    let db = state.db().await.map_err(CommandError::user)?;
    let results = teletype_core::history::search_history(&db, &query, 50)
        .map_err(CommandError::user)?;
    Ok(results)
}
```

Register it in the `generate_handler!` list in `lib.rs`.

**Verify**: `cargo build -p teletype-desktop` → exit 0.

### Step 6: Add the UI search input

In `ui/src/screens/InsightsScreen.tsx`, add a search input at the top of
the screen (or in a new "History" tab if the screen is too crowded). The
search should:
- Call `invoke("search_history", { query })` on submit (debounced, 300ms).
- Show results as a list of cards (text snippet, timestamp, word count).
- Show "No results" for empty queries.

**Verify**: `cd ui && npx tsc --noEmit` → exit 0. `cd ui && npm test` → all pass.

## Test plan

- Unit test: FTS5 search returns matching entries, empty for non-matches.
- Integration: the Tauri command returns results from the real database.
- UI: the search input renders, calls the command, and displays results.
- `cargo test -p teletype-core --lib` → all pass.
- `cd ui && npm test` → all pass.

## Done criteria

- [ ] `cargo build -p teletype-core` exits 0
- [ ] `cargo test -p teletype-core --lib` passes (including new FTS5 test)
- [ ] `cargo build -p teletype-desktop` exits 0
- [ ] `cd ui && npx tsc --noEmit` exits 0
- [ ] `cd ui && npm test` passes
- [ ] FTS5 virtual table + triggers are created on database init
- [ ] `search_history` Tauri command is registered
- [ ] UI search input renders and calls the command
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `rusqlite` does not support FTS5 with the current feature flags (check `cargo doc -p rusqlite` or the crate docs).
- The `history` table schema differs from the excerpt (columns may have been added since).
- The FTS5 virtual table creation fails on an existing database (migration issue).
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- FTS5 is a standard SQLite extension (available since SQLite 3.9.0). The `rusqlite` `bundled` feature compiles SQLite from source with FTS5 enabled.
- The FTS index grows with the history table. If the database gets very large (>10k entries), consider adding a `VACUUM history_fts` to a periodic maintenance task.
- If the user clears history, the FTS triggers handle deletion automatically.
