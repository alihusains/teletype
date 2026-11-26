# Plan 012: Stop polling macOS permissions every two seconds

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- ui/src/settings/PermissionsUpdatesLicenseSections.tsx crates/teletype-desktop/src/commands/settings.rs
> If either file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: performance
- **Planned at**: commit 8746c1c, 2026-11-25

## Why this matters

The Settings permissions section schedules a 2-second timer that calls get_permissions while the window is visible. On macOS that backend call spawns a thread for an AX round-trip and joins it, plus a TCC lookup — roughly 1,800 thread spawns and blocking permission probes per hour of an open screen. Permissions only change when the user goes to System Settings and comes back, and focus/visibility handlers already refresh at exactly that moment. Removing the timer costs no responsiveness; moving the backend probe off the async runtime also keeps a rare slow AX call from stalling other commands.

## Current state

- ui/src/settings/PermissionsUpdatesLicenseSections.tsx:63-67 — comment and constant: "A 2 s cadence is coarse enough to be free; the TCC lookups are cheap once the window is back in front." then const POLL_MS = 2000.
- :100-125 — useEffect: initial refresh(); tick() calls refresh() when !document.hidden and re-arms window.setTimeout(tick, POLL_MS); window focus, document visibilitychange, and getCurrentWebviewWindow().onFocusChanged all call refresh(); cleanup clears the timer and listeners.
- crates/teletype-desktop/src/platform/macos_impl.rs:23-32 — permissions() does catch_unwind(microphone_status), then std::thread::spawn(accessibility_trusted).join(), then returns the permission list. The comment says the AX round-trip can stall on a busy first launch.
- crates/teletype-desktop/src/commands/settings.rs:584-586 — pub async fn get_permissions(state: State<'_, AppState>) -> CommandResult<Vec<Permission>> { Ok(state.platform.permissions()) }. It is the only caller of platform.permissions() from that command; no Rust unit test calls it directly (verified by grep).
- ui/src/test/permissionsHotkey.test.tsx mounts PermissionsSection with mocked get_permissions and does not assert any timer behavior. Tauri mocks are in ui/src/test/tauriMock.ts.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| UI tests | cd ui && npm test | exit 0 |
| UI typecheck | cd ui && npm run typecheck | exit 0 |
| Desktop crate tests | cargo test -p teletype-desktop | exit 0 |
| Full suite | cargo test --workspace | exit 0 (run sequentially; timing tests are load-sensitive) |

## Scope

In scope:
- ui/src/settings/PermissionsUpdatesLicenseSections.tsx (the poll constant, the tick loop and the surrounding comment)
- ui/src/test/permissionsHotkey.test.tsx (one new test)
- crates/teletype-desktop/src/commands/settings.rs (get_permissions only)

Out of scope (do NOT touch):
- macOS permissions() itself, the AX/TCC implementation, any other settings command, or other UI sections.
- The focus/visibility/onFocusChanged refresh paths — they are the replacement and must stay.
- The onboarding permissions screen (different file).

## Git workflow

- Branch: advisor/012-permission-poll
- Commit message style: fix(settings): refresh permissions on focus instead of polling
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Remove the timer from the UI

In PermissionsUpdatesLicenseSections.tsx:
- Delete const POLL_MS = 2000 and its two-line comment; replace with a short comment saying permission state refreshes on window focus, visibility change, and Tauri focus events because grants change only in System Settings.
- In the useEffect, remove the tick function, the timer variable, both window.setTimeout(tick, POLL_MS) calls, and window.clearTimeout(timer) in cleanup.
- Keep the initial refresh(), the window focus listener, the document visibilitychange listener, and the onFocusChanged listener with their cleanup.

Verify: grep -n 'POLL_MS\|setTimeout\|clearTimeout' ui/src/settings/PermissionsUpdatesLicenseSections.tsx -> no matches; grep -n 'onFocusChanged\|visibilitychange\|addEventListener("focus"' -> all still present.

### Step 2: Move the backend probe off the async worker

In settings.rs, change get_permissions to inject AppHandle and run the synchronous probe in a blocking task:

    #[tauri::command]
    pub async fn get_permissions(app: tauri::AppHandle) -> CommandResult<Vec<Permission>> {
        let permissions = tauri::async_runtime::spawn_blocking(move || {
            app.state::<AppState>().platform.permissions()
        })
        .await
        .map_err(|e| e.to_string())?;
        Ok(permissions)
    }

Manager is already imported in settings.rs. No JS call site changes (Tauri injects AppHandle). Keep the function name and return type identical.

Verify: cargo check -p teletype-desktop -> exit 0.

### Step 3: Add a UI test proving refresh is event-driven, not timer-driven

In ui/src/test/permissionsHotkey.test.tsx, add a test permission_refresh_follows_focus_not_a_timer:
1. Use vi.useFakeTimers() before mount and vi.useRealTimers() in cleanup for this test.
2. Mount as the existing tests do, await the initial get_permissions call, and record invokeCalls().filter(c => c.cmd === "get_permissions").length.
3. Advance fake timers by 30 seconds (vi.advanceTimersByTime(30000)); assert the count is unchanged.
4. Dispatch a window focus event (act(() => window.dispatchEvent(new Event("focus")))); await waitFor the count to increase by one.

Model the test on the existing tests in that file and the Tauri mock helpers. If fake timers interact badly with waitFor, prefer real timers with a single explicit focus dispatch and assert the count increases — do not reintroduce polling to make the test easier.

Verify: cd ui && npm test -> all pass including the new test; cd ui && npm run typecheck -> exit 0.

## Test plan

- New test: permission_refresh_follows_focus_not_a_timer in permissionsHotkey.test.tsx.
- Existing tests: all of permissionsHotkey.test.tsx and the full Vitest suite.
- Backend: cargo test -p teletype-desktop -> all pass.

## Done criteria

- [ ] grep -n 'POLL_MS\|setTimeout\|clearTimeout' ui/src/settings/PermissionsUpdatesLicenseSections.tsx -> no matches.
- [ ] get_permissions in settings.rs uses spawn_blocking and no longer takes State directly.
- [ ] cd ui && npm test and npm run typecheck exit 0, with the new test listed.
- [ ] cargo test -p teletype-desktop and cargo test --workspace exit 0.
- [ ] Only the three in-scope files plus plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live excerpts at :63-67, :100-125 or settings.rs:584-586 do not match.
- spawn_blocking fails to compile because AppHandle is not Send (report the exact compiler error; do not revert to a blocking async command silently).
- The new UI test cannot be made deterministic with the existing mocks (report what you tried).
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- The focus/visibility handlers are the correctness backstop: a user who grants a permission in System Settings and returns to the app still sees the updated state immediately. If a future change removes those handlers, the timer removal becomes a regression.
- The AX round-trip still happens on explicit refresh; spawn_blocking keeps it off the async runtime but does not make it fast. Do not add a shorter poll to compensate for a slow AX call.
- If another screen starts polling get_permissions, revisit the backend cost — the command should stay cheap by construction.
