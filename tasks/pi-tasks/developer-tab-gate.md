# Task: Gate the Developer nav tab behind enableDeveloperTab

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  the `enviouswispr/` reference tree or any sibling.
- Files you may edit: `ui/src/App.tsx` and `ui/src/screens/SettingsScreen.tsx`.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why (the real gap)
`enable_developer_tab` is a true placeholder. It is defined in
`crates/teletype-desktop/src/commands.rs` (`enable_developer_tab: bool`, default `false` at
~line 232) and serialized to the UI as `enableDeveloperTab`, but NOTHING reads it. The
Developer nav item is always present:

    // ui/src/App.tsx, NAV array (~line 46)
    { id: "developer", label: "Developer", icon: "terminal" },

is an unconditional array element, so the Developer tab always shows regardless of the setting.
And there is no Settings UI control to even toggle it.

Goal: the Developer nav item appears only when `enableDeveloperTab` is true, and there is a
Settings checkbox to enable it.

## Read first
1. `ui/src/App.tsx` — the `NAV` array (~line 40-50), the `screen` state and how screens are
   rendered (`{screen === "developer" && <DeveloperScreen />}` ~line 189), and how settings are
   loaded (the `get_settings` invoke ~line 73, and the `appIcon`/`hasCompletedOnboarding`
   state that already comes from settings).
2. `ui/src/screens/SettingsScreen.tsx` — the `Settings` interface (top of file) and the
   existing checkbox pattern (the "Behavior" section ~line 206-222 uses an array of
   `[key, label]` pairs to render checkboxes via `save({...settings, [key]: checked})`).
3. `crates/teletype-desktop/src/commands.rs` — confirm the field name and default
   (`enable_developer_tab`, camelCase `enableDeveloperTab`).

## What to build
1. In `App.tsx`: read `enableDeveloperTab` from the settings that are already fetched
   (the `get_settings` call near line 73). Build the `NAV` array so the `developer` entry is
   included only when `enableDeveloperTab` is true. Keep the existing nav items unchanged.
   If the current `screen` is `"developer"` but the tab is disabled, it is fine to leave it
   (the user can't reach it once hidden). Do not over-engineer a redirect.
2. In `SettingsScreen.tsx`: add `enableDeveloperTab: boolean` to the `Settings` interface, and
   add a checkbox to the "Behavior" section (or a new "Advanced" section) labeled
   "Show Developer tab". Use the exact same checkbox pattern the Behavior section already uses
   (`checked={settings.enableDeveloperTab}`, `onChange` -> `save({...settings,
   enableDeveloperTab: e.target.checked})`). If the Behavior section uses the `[key, label]`
   array trick, you can add `["enableDeveloperTab", "Show Developer tab"]` to that array
   instead — match whatever pattern is cleanest given the existing code.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype/ui && npx tsc --noEmit 2>&1 | tail -20`
   (must type-check, no errors).
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff ui/src/App.tsx ui/src/screens/SettingsScreen.tsx`
   (show the exact diff).

## Out of scope (do NOT touch)
- The Rust backend field (it already exists and is serialized correctly).
- Pill style/position, typing autotext, or any other settings.
- The `enviouswispr/` reference tree.
