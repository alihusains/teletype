# Plan 008: Correct the toolchain versions in setup docs

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- README.md docs/windows-from-mac.md .github/workflows/release.yml ui/package.json
> If any file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none (README.md sections are owned by different plans — see Dependency notes in plans/README.md)
- **Category**: docs / dx
- **Planned at**: commit 8746c1c, 2026-11-25

## Why this matters

A contributor following the README installs Rust 1.88 and Node 18, and the build fails before any test runs: the workspace declares rust-version 1.90, CI pins Rust 1.90 and Node 24, and the locked Vitest 5 / jsdom 30 test stack requires Node 22.22.2+ or 24.15+. The README also says cargo install tauri-cli --version ^2, while release.yml pins exactly 2.12.0 to match the tauri crate in Cargo.lock — the repo already had an incident where CLI/crate skew produced divergent Windows and macOS builds. These are small doc edits plus an additive package.json engines field; no code behavior changes.

## Current state

- README.md:122 — "Rust 1.88+"; :123 — "Node.js 18+ (for the UI build)"; :131 — cargo install tauri-cli --version "^2".
- Cargo.toml:24 — rust-version = "1.90". .github/workflows/ci.yml:74 (and every toolchain step) — toolchain "1.90"; :98, :943, :986 — node-version 24.
- .github/workflows/release.yml:306 — comment "Pin to a tauri-cli that builds on the pinned Rust 1.88 toolchain."; :307 — "tauri-cli must match the tauri crate version in Cargo.lock."; :310 — cargo install tauri-cli --version "2.12.0" --locked. Cargo.lock:6479-6480 — tauri 2.12.0. :802-807 records the prior 2.9.3 Windows CLI skew incident.
- docs/windows-from-mac.md:48 — Node 20+ and cargo install tauri-cli --version ^2.
- ui/package.json — no engines field. ui/package-lock.json:3183 — vitest 5.0.5 requires ^22.12.0 || ^24.0.0 || >=26.0.0; :56 — jsdom 30.1.2 requires ^22.22.2 || ^24.15.0 || >=26.0.0; :1789 — @testing-library/jest-dom requires >=22.
- README.md Requirements also claims the macOS 15.0 floor is "set ... in Cargo.toml" (:106-109). That is a separate stale-doc finding and is OUT of scope here — do not edit it, but note it in your report if it blocks understanding.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| UI deps still install | cd ui && npm ci | exit 0 |
| UI tests | cd ui && npm test | exit 0 |
| UI typecheck | cd ui && npm run typecheck | exit 0 |
| Engine field | cd ui && npm pkg get engines.node | prints the configured range |

## Scope

In scope:
- README.md — Requirements bullets (:122-123) and the Tauri CLI install command (:131) ONLY.
- docs/windows-from-mac.md — line 48 tool versions ONLY.
- .github/workflows/release.yml — the comment at :306 ONLY.
- ui/package.json — add an engines.node field.

Out of scope (do NOT touch):
- README.md Testing block (plan 005), Status section (plan 007), and the macOS deployment-target sentence (separate docs finding).
- Cargo.toml, Cargo.lock, ci.yml, ui/package-lock.json, any Rust/TS source.
- docs/windows-from-mac.md other lines (including its status table, corrected by plan 007).

## Git workflow

- Branch: advisor/008-onboarding-versions
- Commit message style: docs: state the real Rust, Node and tauri-cli versions
- Do NOT push or open a PR unless instructed.
- README.md is also edited by plans 005 and 007 in other sections; keep your change confined to Requirements and the CLI command, and make a separate commit.

## Steps

### Step 1: Correct Rust and Node in the README

At README.md:122, change "Rust 1.88+" to "Rust 1.90+" (or "Rust 1.90 or later"). At :123, change "Node.js 18+ (for the UI build)" to "Node.js 24 (CI's pin; at least 22.22.2 or 24.15+ also works)". Keep the surrounding bullet style and the CMake bullet unchanged.

Verify: sed -n '120,125p' README.md shows 1.90 and 24.

### Step 2: Pin the Tauri CLI install command and fix the stale CI comment

At README.md:131, replace cargo install tauri-cli --version "^2" with:

    cargo install tauri-cli --version "2.12.0" --locked

Add a short sentence after the build block explaining that 2.12.0 matches the tauri crate in Cargo.lock and must be bumped with it. At docs/windows-from-mac.md:48, replace "Node 20+ and cargo install tauri-cli --version ^2" with "Node 24 and cargo install tauri-cli --version 2.12.0 --locked". At release.yml:306, change "pinned Rust 1.88 toolchain" to "pinned Rust 1.90 toolchain".

Verify: grep -rn '1.88|Node 20+|tauri-cli --version "^2"' README.md docs/windows-from-mac.md .github/workflows/release.yml -> no matches; grep -n '2.12.0' README.md docs/windows-from-mac.md -> both present.

### Step 3: Add an engines guard to the UI package

Add to ui/package.json after the "type" line:

    "engines": { "node": "^22.22.2 || ^24.15.0 || >=26.0.0" },

This is additive: it does not change any dependency or script. Keep JSON formatting consistent with the existing file (2-space indent).

Verify: cd ui && npm pkg get engines.node prints the range; cd ui && npm ci exits 0; cd ui && npm test exits 0; cd ui && npm run typecheck exits 0.

## Test plan

- No source code changes; verification is grep-based plus an actual npm ci/test/typecheck cycle to prove the new engines field does not break the locked install.
- Verification: all commands in the table pass.

## Done criteria

- [ ] grep -rn 'Rust 1.88|Node.js 18+|Node 20+|Rust 1.88 toolchain' README.md docs/windows-from-mac.md .github/workflows/release.yml -> no matches.
- [ ] grep -n 'rust-version' Cargo.toml still shows 1.90 (unchanged).
- [ ] cd ui && npm pkg get engines.node shows the range; npm ci, npm test, npm run typecheck exit 0.
- [ ] Only the four in-scope files plus plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live excerpts at README.md:122-123, :131, docs/windows-from-mac.md:48 or release.yml:306 do not match.
- npm ci fails after adding the engines field (report the error; do not remove the field silently).
- Another plan has already edited the same README lines.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- The tauri-cli version must stay in sync with the tauri crate in Cargo.lock; the README sentence and release.yml comment now both say so. When bumping, update all three places plus the docs.
- Node's supported range comes from transitive dev deps (vitest/jsdom), not a product decision. If those deps change, re-derive the range from package-lock.json rather than copying this value blindly.
- The macOS 15.0 / Cargo.toml claim is deliberately left for a separate docs plan; do not fix it here.
