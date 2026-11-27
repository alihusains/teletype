# Teletype v0.2.23 — safer dictation delivery and history reliability

## Fixed

- Failed speech-model warm-up now stops the active capture instead of leaving a hidden take running.
- Dictation checks that the original app and window are still the target immediately before inserting text.
- Clipboard restoration no longer overwrites a newer copy made while dictation is being delivered.
- History deletion waits for the database to confirm removal before deleting the associated recording.
- Concurrent settings changes no longer overwrite one another.
- Replacing a history entry no longer leaves stale full-text search results, and Windows clipboard initialization no longer risks replacing the clipboard with empty text.

## Release channel

This is an unsigned pre-release. macOS may show a Gatekeeper warning, and in-app updates are disabled. Download the installer from the GitHub release page.
