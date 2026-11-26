# Plan 016: Gate the devtools_toggle command at compile time (debug-only)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- crates/teletype-desktop/src/commands/system.rs crates/teletype-desktop/Cargo.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

The `devtools_toggle` Tauri command has a **runtime** check that returns an
error in release builds. This means the devtools-open code is compiled into
every shipped binary — it is just forbidden at runtime. If a future change
removes or bypasses the runtime check (a one-line deletion), the release
binary would expose a working DevTools panel with no additional work.

Converting the runtime `if` to a `#[cfg]` compile-time gate means the
devtools-open code is **not compiled** in release builds. Even if someone
later removes the gate, there is no devtools-open code to invoke.

**Known limitation** (documented, not a STOP): the tauri `devtools` *feature*
stays in `Cargo.toml` for all builds because `cargo tauri build` does not
support `--features` (verified: the flag is absent from `cargo tauri build
--help`, and `release.yml` never passes one). So the devtools *protocol* code
is still linked into release binaries. Removing that requires a CI change
(out of scope for this S-effort plan; see Maintenance notes). The command
gate is the security boundary: no release-build command can open DevTools.

## Current state

- `crates/teletype-desktop/src/commands/system.rs:86-106` — `devtools_toggle`
  command. Read the exact current code before editing; the excerpt below is
  the expected shape:

```rust
#[tauri::command]
pub fn devtools_toggle(app: tauri::AppHandle) -> CommandResult<()> {
    if !cfg!(debug_assertions) {
        return Err(CommandError::user("devtools is only available in debug builds"));
    }
    // ... devtools open logic (webview.open_devtools() etc.) ...
    Ok(())
}
```

- `crates/teletype-desktop/Cargo.toml:49` —
  `tauri = { version = "2", features = ["tray-icon", "image-png", "macos-private-api", "devtools"] }`
  — **do NOT change this file** (see Known limitation above).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Build (debug) | `cargo build -p teletype-desktop` | exit 0 |
| Build (release) | `cargo build --release -p teletype-desktop` | exit 0 |
| Desktop tests | `cargo test -p teletype-desktop --lib` | all pass |

## Scope

**In scope** (the only file you should modify):
- `crates/teletype-desktop/src/commands/system.rs`

**Out of scope** (do NOT touch):
- `crates/teletype-desktop/Cargo.toml` — the `devtools` feature stays (see Known limitation)
- Any other Rust or TypeScript file

## Git workflow

- Branch: `advisor/016-devtools-gate`
- Commit message: `fix(security): compile-time gate on devtools_toggle (debug-only)`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Read the current devtools_toggle code

Open `crates/teletype-desktop/src/commands/system.rs` and read the
`devtools_toggle` function in full. Identify:
- The runtime `if !cfg!(debug_assertions)` check (or equivalent).
- The devtools-open logic (which lines call `open_devtools()` or similar).
- Any variables or borrows that the open logic uses.

**Verify**: you can identify the runtime check and the open-logic block.

### Step 2: Convert the runtime check to cfg gates

Replace the runtime `if` with `#[cfg]` attribute gates so the devtools-open
code is not compiled in release builds:

```rust
#[tauri::command]
pub fn devtools_toggle(app: tauri::AppHandle) -> CommandResult<()> {
    #[cfg(debug_assertions)]
    {
        // ... existing devtools open logic, unchanged ...
        Ok(())
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = &app; // silence unused-variable warning in release
        Err(CommandError::user("devtools is only available in debug builds"))
    }
}
```

Key rules:
- The devtools-open logic moves **inside** the `#[cfg(debug_assertions)]` block, unchanged.
- The release block returns the same error message (users see identical behavior).
- If the open logic uses `app`, the release block needs `let _ = &app;` to avoid an unused-variable warning.
- If the open logic uses other local variables, either move their declaration into the debug block or add `let _ = ...;` in the release block.

**Verify**: `cargo build -p teletype-desktop` → exit 0 (debug build compiles with devtools logic).

### Step 3: Verify the release build compiles without the devtools-open code

**Verify**: `cargo build --release -p teletype-desktop` → exit 0 (release build compiles; the devtools-open code is not compiled).

### Step 4: Run the desktop test suite

**Verify**: `cargo test -p teletype-desktop --lib` → all pass. (If there is a test that calls `devtools_toggle` in a release-mode context, it should now get the error — confirm the test still passes or adjust it to expect the error.)

## Test plan

- Debug build: `devtools_toggle` opens DevTools (existing behavior, unchanged).
- Release build: `devtools_toggle` returns the error (existing behavior, now compile-time).
- `cargo test -p teletype-desktop --lib` → all pass.

## Done criteria

- [ ] `cargo build -p teletype-desktop` exits 0
- [ ] `cargo build --release -p teletype-desktop` exits 0
- [ ] `devtools_toggle` in `commands/system.rs` uses `#[cfg(debug_assertions)]` / `#[cfg(not(debug_assertions))]` gates (not a runtime `if`)
- [ ] The devtools-open code is inside the `#[cfg(debug_assertions)]` block
- [ ] The release block returns the same error message
- [ ] `cargo test -p teletype-desktop --lib` passes
- [ ] `crates/teletype-desktop/Cargo.toml` is NOT modified
- [ ] No files outside the in-scope list are modified (`git diff --name-only`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The `devtools_toggle` function structure differs from the expected shape (the runtime check may have been refactored — re-read and adapt the gate placement, but do not change the error message).
- Converting to `#[cfg]` gates introduces a compile error that cannot be fixed by moving variable declarations into the debug block (e.g. the open logic borrows a variable that is also used in the release block).
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- The tauri `devtools` feature stays in `Cargo.toml` for all builds. A follow-up plan (M effort, touches `release.yml`) could remove the protocol code from release binaries by passing feature flags through a custom build step. Until then, the compile-time gate on `devtools_toggle` is the security boundary.
- If a future change adds a new debug-only command, apply the same `#[cfg]` gate pattern.
- The error message "devtools is only available in debug builds" is user-visible — do not change it without updating any UI code that displays it.
