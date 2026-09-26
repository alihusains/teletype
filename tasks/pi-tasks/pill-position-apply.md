# Task: Pill position must change + emit settings-changed on save

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  the `enviouswispr/` reference tree or any sibling.
- Files you may edit: `crates/teletype-desktop/src/commands.rs`,
  `crates/teletype-desktop/src/dictation.rs`, and (only if needed)
  `crates/teletype-desktop/src/overlay.rs`. You own `save_settings` in commands.rs for this
  task; a separate task touches typing.rs/lib.rs but NOT save_settings.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why (two related gaps, both fixed here)
1. **Position doesn't change.** `dictation.rs:979` reads
   `parse_position(&settings.pill_position)` and passes it to `overlay::update` (dictation.rs:980).
   But `overlay::update` (overlay.rs:13-33) only repositions when the window is HIDDEN:

       // overlay.rs:27-30
       if !vis_before { place(app, position); let _ = window.show(); }

   So an already-visible pill (alwaysShowPill on, or mid-dictation) never moves when the user
   changes "Pill position" in Settings.

2. **No settings-changed signal to the pill webview.** `save_settings` (commands.rs:277-305)
   persists settings and applies icon/hotkey side effects, but emits NOTHING to the pill
   webview, so the pill UI (style, etc.) never live-updates. A separate UI task makes the pill
   listen for a `settings-changed` event; this task must emit it.

## Read first
1. `crates/teletype-desktop/src/overlay.rs` — whole file. `update` (13-33), `place` (43-68),
   `compute_origin` (74+). `place(app, position)` moves the window; it guards on the window
   existing so calling it on a hidden pill is a safe no-op.
2. `crates/teletype-desktop/src/dictation.rs` — `show` (977-985) and `parse_position` (996+).
3. `crates/teletype-desktop/src/commands.rs` — `save_settings` (277-305).
4. `crates/teletype-desktop/src/overlay.rs` — the `PILL_LABEL` constant (the pill window label,
   used in `app.emit_to(PILL_LABEL, "pill-state", &state)` at overlay.rs:21).

## What to build
1. **Reposition on position change.** In `save_settings`, before `state.replace_settings(...)`
   (which consumes `settings`), capture the diff:
       let position_changed = settings.pill_position != previous.pill_position;
       let new_position = settings.pill_position.clone();
   After `replace_settings`, if `position_changed`, reposition the pill immediately:
       crate::overlay::place(&app, parse_position(&new_position));
   `parse_position` is in `dictation.rs` (line 996). If it is private, either make it
   `pub(crate)`, or add a tiny `pub fn reposition(app: &AppHandle, position: PillPosition)`
   wrapper in `overlay.rs` and parse in commands.rs. Pick the least invasive option; note it
   in your report.
2. **Emit settings-changed.** In `save_settings`, after `replace_settings` succeeds, emit a
   `settings-changed` event to the pill webview so the pill UI can refresh (a companion UI
   task listens for it). Use the same pattern as overlay.rs:21:
       let _ = app.emit_to(PILL_LABEL, "settings-changed", &());
   (emit a unit/empty payload; the pill re-fetches via `get_settings`). Import `PILL_LABEL`
   from `crate::overlay` (check its visibility; make it `pub` if it is not already).

Keep both changes minimal and inside `save_settings`.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo build -p teletype-desktop 2>&1 | tail -20`
   (must compile, no errors).
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo test -p teletype-desktop 2>&1 | tail -30`
   (existing overlay position tests must still pass).
3. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff crates/teletype-desktop/src/`

## Out of scope (do NOT touch)
- `ui/src/pill.tsx` (a separate UI task adds the listener). Typing autotext, the Developer
  tab, or any other settings. The `enviouswispr/` reference tree.
