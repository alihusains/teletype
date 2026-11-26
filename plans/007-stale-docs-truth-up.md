# Plan 007: Correct docs that contradict shipped code

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- README.md AGENTS.md docs/signing-and-updates.md docs/windows-from-mac.md docs/architecture.md
> If any file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S-M
- **Risk**: LOW
- **Depends on**: none
- **Category**: docs
- **Planned at**: commit 8746c1c, 2026-11-24

## Why this matters

Four onboarding/status documents actively mislead contributors and agents:
- docs/signing-and-updates.md opens by claiming signed/notarized builds and working self-updates, but releases are unsigned with an empty-platform manifest by design (release.yml:154, brain update 2026-10-24).
- README.md Status says the Windows platform layer is "still stubbed" with keyboard injection, hotkey and tray "not yet implemented", while the code has cross-platform enigo/arboard injection, a WH_KEYBOARD_LL hook, Tauri global shortcut and tray; docs/windows-from-mac.md:9-24 accurately describes this.
- docs/architecture.md lists only 4 workspace crates (there are 11), says history/statistics are deferred (Insights ships), names nonexistent platform files macos.rs/windows.rs, and claims Tailwind 4 (there is no Tailwind dependency; ui/src/pill.tsx says so explicitly).
- AGENTS.md:291 says decisions run D001-D044; the brain has D051.

These are documentation-only changes with concrete onboarding cost. No source code or behavior is touched.

## Current state

- docs/signing-and-updates.md:1-4 — "Teletype ships signed and notarized macOS builds and self-updating installs on both platforms." :98-100 uses git tag v0.1.1. :111-113 — "latest.json must list BOTH darwin-* and windows-x86_64."
  Actual posture: .github/workflows/release.yml:141-158 explicitly allows an unsigned pre-release (ALLOW_NO_UPDATER=true) with a zero-platform manifest; brain 2026-10-24 update: "latest.json ships unsigned with empty platforms ... auto-update stays disabled — the installers are downloadable from the release page."
- README.md:227-234 — Windows bullet claims stubbed platform layer. Actual: crates/teletype-desktop/src/platform/windows_impl.rs implements Platform (foreground app + permissions); teletype-core/src/injector.rs:43-44 uses enigo/arboard; crates/teletype-desktop/src/typing/typing_windows.rs has the hook; Tauri handles hotkey/tray. docs/windows-from-mac.md:9-24 lists these as implemented and says the real gap is missing real-Windows evidence. NOTE: that doc's path for the hook is wrong (says src/typing_windows.rs; actual src/typing/typing_windows.rs) and its Node 20+ / tauri-cli ^2 lines (:48) are stale — leave those version lines to separate work, but fix the path.
- docs/architecture.md:25-70 — workspace layout lists only teletype-core, teletype-speech, teletype-inference, teletype-desktop; "history/statistics screens (deferred)"; platform impls "macos.rs, windows.rs"; "React 19 + Tailwind 4 frontend" at :63. Actual Cargo.toml:3-15 lists 11 members: parakeet-sys, needle-sys, parakeet-cpp-sys, teletype-core, teletype-speech, teletype-fluid, teletype-apple, teletype-inference, teletype-desktop, teletype-eval, teletype-decider. Actual UI has no Tailwind; ui/index.html holds design tokens. Actual platform files are macos_impl.rs / windows_impl.rs.
- AGENTS.md:288-291 — "Decisions (D001–D044)"; PROJECT_BRAIN.md has D001-D051 (check-brain-freshness.sh:60 reports 51).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Decision count | grep -c '^### Decision D' .agents/brain/PROJECT_BRAIN.md | prints 51 |
| Crate list | grep -A14 '^members' Cargo.toml | shows 11 members |
| Stale-string check | grep -rn 'Tailwind|macos.rs, windows.rs|D001–D044' docs/architecture.md AGENTS.md | no matches after edits |
| Doc links | manual: every relative link written still resolves | no broken links |

## Scope

In scope:
- docs/signing-and-updates.md
- docs/windows-from-mac.md (status table typo/path and the evidence summary only)
- README.md (Status section only — plan 005 owns the Testing block)
- docs/architecture.md (workspace layout, stack claim, deferred-screens claim, platform filenames)
- AGENTS.md (one decision-range line)

Out of scope (do NOT touch):
- Any Rust/TS source, CI, or script.
- README.md Testing block (plan 005), Requirements section (separate finding #10), and Models section.
- docs/windows-from-mac.md:48 Node 20+ and tauri-cli ^2 lines — known stale but tracked as separate work; do not silently fix here.
- No credential values, secret names beyond what is already in prose, or DSNs in any doc.

## Git workflow

- Branch: advisor/007-stale-docs
- Commit message style: docs: align status docs with shipped code
- Do NOT push or open a PR unless instructed.
- Coordinate with plan 005 on README.md: disjoint sections, but do not run both executors against the file simultaneously.

## Steps

### Step 1: Correct the signing/updates doc to the current posture

Rewrite docs/signing-and-updates.md:1-4 to state the truth: macOS builds currently ship unsigned; the updater manifest is empty-platform and in-app updates are off until a signing key is configured; installers are downloadable from the release page. Keep the existing explanation of why signing/notarization matters, but label it as what will be true once credentials exist.

In the verification section around :95-113: replace the v0.1.1 tag example with the current release line v0.2.22, and replace the "must list BOTH platforms" claim with the current rule: an unsigned release intentionally writes an empty-platform manifest, and the doc should explain how to verify that state plus what changes when signing is enabled. Do not include any key material.

Verify: sed -n '1,8p;95,118p' docs/signing-and-updates.md contains no "ships signed" claim and names v0.2.22.

### Step 2: Correct the Windows status contradiction

In README.md Status (:231-233), rewrite the Windows bullet to: the workspace compiles on a real Windows host (CI gate) and the shipped exe passes --self-test; text injection (enigo/arboard), the typed-AutoText WH_KEYBOARD_LL hook, global hotkey and tray exist; UIA/OCR context reading is the remaining platform gap; no human has run a real dictation on a Windows desktop yet. Link docs/windows-from-mac.md for the evidence checklist. Keep the macOS and Linux bullets unchanged.

In docs/windows-from-mac.md:16, fix the hook path from crates/teletype-desktop/src/typing_windows.rs to crates/teletype-desktop/src/typing/typing_windows.rs. Add one sentence at :24-26 noting that README.md Status now reflects this table and that the real gap is hardware evidence, not implementation.

Verify: grep -n 'not yet implemented' README.md -> no matches in the Status section; grep -n 'typing/typing_windows.rs' docs/windows-from-mac.md -> found.

### Step 3: Refresh the architecture workspace map

In docs/architecture.md:
- Replace the 4-crate workspace tree at :31-70 with all 11 Cargo.toml members, grouped as: sys/build crates (parakeet-sys, needle-sys, parakeet-cpp-sys), core logic (teletype-core), speech/inference (teletype-speech, teletype-inference, teletype-fluid, teletype-apple, teletype-decider), app (teletype-desktop), eval (teletype-eval). Use Cargo.toml:3-15 as the source of truth.
- Change "history/statistics screens (deferred)" to shipped (History + Insights).
- Change platform impl filenames to macos_impl.rs / windows_impl.rs.
- Remove the Tailwind 4 claim; state React 19 + Vite 6 with CSS design tokens in ui/index.html.

Verify: grep -rn 'Tailwind|macos.rs, windows.rs|screens (deferred)' docs/architecture.md -> no matches; diff the crate names against Cargo.toml members.

### Step 4: Correct the decision range

In AGENTS.md:291, change "Decisions (D001–D044)" to "Decisions (D001–D051)" (or remove the explicit upper bound).

Verify: grep -n 'Decisions (D001' AGENTS.md shows D051; grep -c '^### Decision D' .agents/brain/PROJECT_BRAIN.md prints 51.

## Test plan

- No production code changes: no Rust/TS tests required.
- Verification is grep-based for each stale string plus a manual link check of any relative path written.
- Optional: run scripts/check-brain-freshness.sh after editing AGENTS.md to confirm no new warning (it does not check AGENTS.md, so this is a sanity check only).

## Done criteria

- [ ] grep -rn 'ships signed and notarized|git tag v0.1.1|must list **both**' docs/signing-and-updates.md -> no matches.
- [ ] README.md Status no longer says Windows keyboard injection/hotkey/tray are unimplemented.
- [ ] grep -rn 'Tailwind|macos.rs, windows.rs|D001–D044' docs/architecture.md AGENTS.md -> no matches.
- [ ] docs/architecture.md lists all 11 Cargo.toml workspace members.
- [ ] No credential values or DSNs appear in any edited doc.
- [ ] Only the five in-scope docs plus plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live doc excerpts no longer match (especially if another agent already fixed them).
- A README Status edit would require changing the Requirements section (finding #10) — leave that for separate work and report.
- Any edited doc would need a secret value to be accurate — stop and report instead of writing it.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- These docs drifted because status facts changed in code and releases without a doc owner. A reviewer should check that the corrected claims cite code paths (windows_impl.rs, injector.rs, typing_windows.rs, Cargo.toml) so the next drift is detectable.
- The signing doc should be revisited when credentials actually exist; the plan deliberately leaves the future-state instructions in place, clearly labeled.
- README.md has three independent edit owners across plans 005 (Testing), 007 (Status) and deferred finding #10 (Requirements). Keep changes in disjoint sections to avoid conflicts.
