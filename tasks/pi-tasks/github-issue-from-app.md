# Task: Create a GitHub issue from inside the app

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  any sibling reference tree.
- Files you may edit:
  - `crates/teletype-desktop/src/commands.rs` (add Rust command + Settings field)
  - `crates/teletype-desktop/src/lib.rs` (register the command in `generate_handler!`)
  - `ui/src/screens/SettingsScreen.tsx` (add a "Report an issue" section)
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
Internal testers need a way to file a bug report without leaving the app. The app should let a
user enter a GitHub token once (stored in the OS keychain, NOT settings.json), then create an
issue in the `alihusains/teletype` repo with a title + body. This unblocks internal testing.

## Read first
1. `crates/teletype-desktop/src/commands.rs`:
   - Lines 126-190: the `Settings` struct. It uses `#[serde(rename_all = "camelCase")]` — add
     your new fields with `#[serde(default)]` so old settings.json still parses.
   - Search for `set_llm_secret` and `read_api_key` / `secret_account_for_url` — this is the
     EXISTING pattern for storing an API key in the keychain via the `keyring` crate. Copy that
     pattern for a `github_token` secret. Do NOT store the token in the `Settings` struct.
   - Note the `CommandResult<T> = Result<T, String>` return convention and how commands take
     `State<'_, AppState>` where needed.
2. `crates/teletype-desktop/src/lib.rs` lines 382-476: the `generate_handler![ ... ]` list. Add
   your new command(s) here.
3. `crates/teletype-desktop/Cargo.toml` line 37: `reqwest` is already a dependency with
   `rustls-tls`, `blocking`, `json` features. Use it for the GitHub API call.
4. `ui/src/screens/SettingsScreen.tsx`:
   - The whole file. Note the `h3` section headers (uppercase, `var(--text-secondary)`) and the
     card/input styling. Add a new section following the same visual pattern.
   - Find how existing settings are read/written: search for `invoke(` and `get_settings` /
     `save_settings` to copy the IPC call pattern.

## What to build
1. **Rust command `create_github_issue(title: String, body: String) -> CommandResult<String>`**
   in `commands.rs`:
   - Read the GitHub token from the keychain (account id `github`, service `com.teletype.app`,
     mirroring the existing LLM-secret keyring usage). If absent, return `Err("No GitHub token
     set. Add one in Settings.")`.
   - POST to `https://api.github.com/repos/alihusains/teletype/issues` with
     `Authorization: Bearer <token>`, `Accept: application/vnd.github+json`, JSON body
     `{"title": ..., "body": ...}`. Use `reqwest::blocking` (the crate already enables it).
   - On 201, return `Ok(url)` where url is the `html_url` from the response. On error, return a
     clear `Err` including the status code and the API's `message` field (do NOT leak the token).
2. **Rust command `set_github_token(token: String) -> CommandResult<()>`** and
   **`clear_github_token() -> CommandResult<()>`** that write/clear the keychain entry,
   mirroring `set_llm_secret` / `clear_llm_secret`.
3. **Register all three** in `lib.rs` `generate_handler!`.
4. **Settings UI**: add a "Report an issue" section in `SettingsScreen.tsx` with:
   - A token input (password type) + "Save token" + "Clear" buttons calling the two token
     commands. Keep the token in local React state only; never persist to settings.json.
   - A title input, a body textarea, and a "Create issue" button that calls
     `create_github_issue`. On success show the returned issue URL (e.g. in a small status line
     and/or `window.open` it). On error show the message inline.
   - Match the existing card/input styling and CSS custom properties.

## Out of scope
- Do NOT add a new dependency. `reqwest` is enough.
- Do NOT store the token anywhere except the keychain.
- Do NOT touch the LLM-secret code paths, only mirror their pattern.

## Verification (paste REAL output)
1. `cargo build -p teletype-desktop 2>&1 | tail -5` — must compile.
2. `cargo clippy -p teletype-desktop 2>&1 | tail -15` — no new warnings.
3. `cd ui && npx tsc --noEmit 2>&1 | tail -10` — must be clean.
4. `git diff --stat` — show exactly which files changed.
