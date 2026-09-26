# Task: Wire the typingAutotextEnabled toggle to the typing watcher

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  the `enviouswispr/` reference tree or any sibling.
- Files you may edit: `crates/teletype-desktop/src/typing.rs` and
  `crates/teletype-desktop/src/lib.rs`.
- **Do NOT edit `save_settings` in commands.rs** — a different task owns that function and is
  editing it concurrently. If you need a command registration, add it in lib.rs's
  `generate_handler!` list (a different file region), not inside save_settings.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why (the real gap)
`typing_autotext_enabled` is a half-wired placeholder:
- `crates/teletype-desktop/src/typing.rs:35` defines `pub fn set_enabled(app, enabled)` which
  starts the `typing_loop` thread when toggled on. But NOTHING calls `set_enabled` (grep
  returns zero call sites). So the "Expand AutoText while typing" checkbox
  (SettingsScreen.tsx:211) does nothing.
- `typing_loop` (typing.rs:47-67) is an explicit V1 placeholder: the loop body just sleeps;
  the comment says "The actual key-event hook would go here."
- `typing::start` (typing.rs:27-31) is called at startup (lib.rs:287) but only sets
  `WATCHER_ACTIVE` to false.

Goal: make the setting demonstrably start/stop the typing watcher. The minimum real wiring:
on app startup, if the persisted `typing_autotext_enabled` is true, start the watcher. The
actual low-level key-hook inside `typing_loop` is a larger V2 effort and is OUT of scope.

## Read first
1. `crates/teletype-desktop/src/typing.rs` — whole file. `WATCHER_ACTIVE`, `start`,
   `set_enabled`, `typing_loop`.
2. `crates/teletype-desktop/src/lib.rs` — the startup section (~250-300). It loads `settings`
   and calls `typing::start(app.handle().clone())` at line 287. See how `settings` is in
   scope there.

## What to build
1. In `lib.rs` startup, immediately after `typing::start(app.handle().clone());` (line 287),
   reflect the persisted setting:
       if settings.typing_autotext_enabled {
           typing::set_enabled(&app.handle(), true);
       }
   (match however `app` / the handle is referenced in that scope — read the surrounding lines
   to use the correct receiver; `typing::start` uses `app.handle().clone()`).
2. In `typing.rs`, remove the now-stale `#[allow(dead_code)]` on `set_enabled` (it has a real
   caller now). Leave `typing_loop`'s placeholder body as-is (out of scope), but you may
   tighten its comment to note the key-hook is a V2 task.

Do NOT add a new Tauri command or touch commands.rs. Startup reflection is the minimal,
dependency-free wiring that makes the setting real without racing the other task.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo build -p teletype-desktop 2>&1 | tail -20`
   (must compile, no errors).
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo test -p teletype-desktop 2>&1 | tail -30`
3. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff crates/teletype-desktop/src/typing.rs crates/teletype-desktop/src/lib.rs`

## Out of scope (do NOT touch)
- `save_settings` in commands.rs (owned by another task). Implementing the actual key-event
  hook inside `typing_loop`. Pill style/position, the Developer tab, or any other settings.
  The `enviouswispr/` reference tree.
