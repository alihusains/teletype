# Teletype v0.3.0 — polish learning, quieter screens, and Windows delivery

This is the first minor release after v0.2.23. It closes four items from the
consolidated roadmap: the polish learning loop, the dictation module split,
the UI active-screen gate, and the Windows platform-layer delivery gap.

## Added

- **Per-app polish learning in production.** Each take resolves the local
  bandit's profile choice for the current app and encodes it into the
  instruction channel; an explicit per-app override still wins. The pipeline
  no longer consults an empty bandit of its own. Regression tests pin the seam.
- **Windows UIA field reader.** Windows delivery now reads the focused control
  through UI Automation: password fields are refused without reading, writable
  focus is restricted to Edit controls (never browser Document/Text), and
  URL-bar detection uses a conservative token allowlist. Whole-field
  `ValuePattern.SetValue` is deliberately not used to replace a selection;
  delivery falls back to "hold for Paste last" instead of destroying the
  rest of the field. COM is initialized per worker thread.
- **Active-screen gate across the UI.** Ten screens now gate their background
  work. The shared runtime-status poll, its event-driven timer reset, and the
  readiness panel's one-second clock all stop while a screen is hidden.
  Mount-time fetches and user-action timers are untouched, so tab state is
  preserved.

## Changed

- `dictation.rs` was split from 16,461 to 3,738 lines: tests in
  `dictation/tests.rs` and six sibling test modules, the state machine in
  `dictation/session/`, the controller in `dictation/controller.rs`. Method
  bodies moved verbatim. The IPC contract test now scans the whole module tree.
- `.gitignore` no longer hides the tracked `ROADMAP.md` through a
  case-insensitive lowercase rule.

## Verification

- Workspace tests: 1,416 core, 661 desktop (3 ignored), all other targets green;
  IPC contract 11/11.
- UI: 61 files / 628 tests, `tsc --noEmit` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all -- --check` clean.
- Windows UIA code type-checks for `x86_64-pc-windows-msvc` and is
  `Send + Sync`. Real-Windows behaviour is still unverified (no hardware);
  the Windows test suite remains compile-only because of the upstream
  `needle-sys` LNK1143 prebuilt.

## Release channel

CI has no updater signing key, so this build is unsigned and in-app updates are
disabled. Unlike earlier unsigned releases, this one is published as a stable
release (not a pre-release) by an explicit one-off promotion after the
published assets were verified. macOS may still show a Gatekeeper warning on
first launch; download the installer from the GitHub release page.
