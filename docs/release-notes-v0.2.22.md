# Teletype v0.2.22 — learned-word pill, one shortcuts home, and polish fixes

## Why this patch exists

Three user-visible gaps were closed after v0.2.21:

1. Learning a word only produced a main-window toast and a tray item; there was no floating confirmation at the dictation pill with an immediate Undo.
2. Keyboard bindings were split between a partial Settings section, a separate Keybinds screen and per-transform editors.
3. Polish was told in its prompt to fix capitalization and grammar, but small models often ignored it, and card-locked cleanup models could not be reprompted at all.

## Added

- Learned-word pill: the canonical word with "Added to dictionary" or "Updated in dictionary", a filled Undo button, a quiet close button, a 12-second countdown rail that pauses on hover, and honest result states (Undone, word removed, Couldn't undo, Couldn't save word). A new setting controls the undo window (3-300 s, default 12 s) for both the pill and the tray.
- Dictionary: search, Auto-learned filter and a visible "Learned from" line. The Learn-from tab now hosts the self-learning toggle, status and a learning-memory Forget row with a blocked-pair count.
- One Shortcuts section inside Settings holds every binding (record, cancel, command, paste last, quick add and transform shortcuts) with conflict detection. Other screens show read-only bindings with deep links into it.

## Fixed

- Polish keeps formatting as a deterministic layer: sentence-start capitalization after .?! and ellipsis, newlines and quotes (protecting abbreviations, initials, decimals and domains), plus a bounded set of subject-verb agreement repairs. The 2B and 0.8B prompts now name tense and agreement explicitly. Fallback output is unchanged byte-for-byte.
- A failed dictionary save no longer produces a success toast or an undo offer.
- Undo is claimed atomically, so a pill click and a tray click cannot both apply the same offer.
- THIRD-PARTY-NOTICES.txt now lists the sentry direct dependency, restoring the CI notice gate.

## Verification

- cargo test -p teletype-core --lib: 1409 passed, 1 pre-existing fixture failure (snapshot_eval_corpus; the eval corpus manifest is absent from this tree).
- cargo test -p teletype-desktop --lib: 647 passed; IPC contract: 11 passed; new polish capitalization/agreement integration test: passed.
- UI: typecheck clean, 59 files / 621 vitest tests passed, production build clean.
- cargo clippy --workspace --all-targets and cargo fmt --all -- --check: clean.
- Release gates: scripts/check-notices.sh, scripts/check-artifact-pins.sh, scripts/check-updater-manifest.sh, scripts/check-brain-freshness.sh.

## Scope

Patch on top of v0.2.21. No model, engine or signing changes. This release is unsigned (no updater key in CI); ALLOW_NO_UPDATER=true keeps it a pre-release with in-app updates off.
