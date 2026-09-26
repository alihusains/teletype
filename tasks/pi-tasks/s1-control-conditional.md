# Task: Hide S1 control box when S1-mini is not the active model

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  the `enviouswispr/` reference tree or any sibling.
- Files you may edit: `ui/src/screens/DeveloperScreen.tsx` only.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
The Developer tab shows an "S1 control" box with styling/structure/context wire tokens. This box
is only relevant when the active polish model is S1-mini (`s1-mini`). When the user has EG-1
selected (or any other model), the box is misleading noise: it implies S1 control values are being
sent to the active model, which they are not. The box should only render when `selectedLlmModel`
is `"s1-mini"`.

## Read first
1. `ui/src/screens/DeveloperScreen.tsx` — the whole file. Note:
   - Lines 42-56: component state, `refreshS1` which calls `get_profile`.
   - Lines 112-136: the S1 control box JSX (the `{s1 && (...)}` block).
   - The component does NOT currently read `selectedLlmModel` from settings.
2. `ui/src/screens/SettingsScreen.tsx` line 16: `selectedLlmModel: string` in the `Settings` interface.
3. `crates/teletype-desktop/src/commands.rs` line 136: `pub selected_llm_model: String` in the Settings struct.
   The `get_settings` IPC returns this as `selectedLlmModel` (camelCase).
4. `crates/teletype-inference/src/catalog.rs` line 172: `id: "s1-mini"` — the model ID string.

## What to build
1. Add a `useEffect` (or extend the existing `refreshS1`) in `DeveloperScreen` that calls
   `invoke<{ selectedLlmModel: string }>("get_settings")` and stores the result in a new
   `useState<string>("")` called `activeModel`.
2. Also listen for `settings-changed` (via `useTauriEvent`) to re-fetch `activeModel` when the
   user switches models in Settings. This is the same pattern already used in `ui/src/App.tsx`
   (line 90) and `ui/src/pill.tsx` (line 601).
3. Change the S1 control box condition from `{s1 && (` to `{s1 && activeModel === "s1-mini" && (`.
4. Do NOT change the `S1_WIRE_TOKENS`, `s1ValueFor`, or any other logic. Only gate the render.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype/ui && npx tsc --noEmit 2>&1 | tail -20`
   (must exit 0, no errors)
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff ui/src/screens/DeveloperScreen.tsx`

## Out of scope (do NOT touch)
- `ui/src/App.tsx`, `ui/src/pill.tsx`, any `.rs` file, `SettingsScreen.tsx`.
- The log view (a separate task handles that).
- The `enviouswispr/` reference tree.
