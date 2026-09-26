# Settings: Individual settings are half-wired (log-in vs. apply)

## Problem

Several settings are broken by design:

- **Individual settings** are not working as expected. The rule is:
  - read from the `Settings` struct at startup (log in)
  - apply side effect (no reposition, no live-update)

The Tauri `Settings` struct has `#[serde(rename_all = "camelCase")]` so the
`get_settings` / `save_settings` IPC carries camelCase keys (`removeFillerWords`,
`fillerWords`, `recordingMode`, `app_icon`, `inputDevice`, etc.).

But the UI Screens ( `SettingsScreen`, `ScratchpadScreen`, `TransformsScreen`,
`StylesScreen`, `OnboardingScreen`, `App.tsx`, `pill.tsx`) read/wrote the
**snake_case** names. On load every underscored field was `undefined`
(checkboxes showed unchecked, selects showed their first option by
coincidence); toggling "remove filler words" on then called
`settings.filler_words.map(...)` on `undefined`, which threw and unmounted the
whole React tree (no error boundary) -> blank window. `ModelsScreen` already
used camelCase correctly, which is why only some screens were broken.

## Solution

**Individual settings** are not working as expected. The rule is:
- **Individual settings**: read from the `Settings` struct at startup (log in)
- **Individual settings**: apply side effect (no reposition, no live-update)

This is a fundamental mismatch. The fix is to make the UI read the **camelCase**
field names that the `Settings` struct actually exposes, and then verify each
setting actually repositions the pill or live-updates the UI.

## Verification

After the fix, the following should be true for each setting:
- `settings.pillStyle` — change the pill's style (e.g., from "default" to "well")
- `settings.pillPosition` — change the pill's position (e.g., from "topLeft" to "bottomRight")
- `settings.removeFillerWords` — change the pill's style (e.g., from "default" to "well")
- `settings.app_icon` — change the app's icon (e.g., from "white" to "blue")
- `settings.typing_autotext_enabled` — enable AutoText expansion while typing
- `settings.vadAutoStop` — enable/disable VAD auto-stop
- `settings.vadSilenceMs` — set the VAD silence duration (e.g., 300-2000 ms)

The individual settings should be verified in the same way the "remove filler
words" pattern was: the UI reads the correct field name, and its change is
verified by actually repositioning the pill or live-updating the UI.
