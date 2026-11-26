# Plan 011: Replace the predictable temp file used for hotkey capture

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- crates/teletype-desktop/src/commands/settings.rs crates/teletype-desktop/src/hotkey_capture.m
> If either file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S-M
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit 8746c1c, 2026-11-25

## Why this matters

The native hotkey panel and the Rust command currently hand off the captured combination through a fixed, world-writable path: /tmp/teletype_hotkey_result.txt. Any local process can write a chosen binding there, and get_captured_hotkey returns it with no ownership or freshness check; the app then registers it as a global hotkey. start_hotkey_capture also deletes that predictable path before capture. Replacing the file with an in-process callback removes the injection primitive entirely, with no new dependency.

## Current state

- crates/teletype-desktop/src/commands/settings.rs:17-44 — start_hotkey_capture() is macOS-only inside cfg(target_os = "macos"), declares extern C teletype_start_hotkey_capture() and teletype_hotkey_capture_has_app(), removes the fixed file at :35, parks the input engine, then calls the native start.
- :84-97 — get_captured_hotkey() reads std::fs::read_to_string("/tmp/teletype_hotkey_result.txt"), trims it, removes the file and returns Some(String); any read failure returns None.
- crates/teletype-desktop/src/hotkey_capture.m:144, :163, :227 — three writes of [g_currentHotkey writeToFile:@"/tmp/teletype_hotkey_result.txt" atomically:YES encoding:NSUTF8StringEncoding error:nil] (Enter confirm, normal key combination, bare-modifier confirm).
- :241 — void teletype_start_hotkey_capture(void) is the native entry point; build.rs:38 compiles this .m file, so signature changes need no build.rs edit.
- :873-917 — test hotkey_capture_commands_are_available_on_every_platform exercises the non-macOS error path; keep it passing.
- teletype-desktop/Cargo.toml already depends on uuid and dirs, but this plan needs neither — the callback approach removes the file path entirely.
- Repo convention: FFI declarations live inline in the cfg(macos) blocks of settings.rs; native globals are static at the top of hotkey_capture.m.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Desktop crate tests | cargo test -p teletype-desktop | exit 0 |
| Full suite | cargo test --workspace | exit 0 (run sequentially; timing tests are load-sensitive) |
| Format check | cargo fmt --all -- --check | exit 0 |
| Stale path check | grep -rn 'teletype_hotkey_result.txt' crates/ | no matches |

## Scope

In scope:
- crates/teletype-desktop/src/commands/settings.rs (hotkey-capture section only)
- crates/teletype-desktop/src/hotkey_capture.m

Out of scope (do NOT touch):
- build.rs, Cargo.toml, any UI code, or any other command in settings.rs.
- stop_hotkey_capture, hotkey_status, reregister_hotkey, and the input-engine parking logic.
- Any file-based settings persistence.

## Git workflow

- Branch: advisor/011-hotkey-capture-channel
- Commit message style: fix(hotkey): hand the captured binding to Rust in-process
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add the in-process result slot and callback in Rust

Near the hotkey-capture section of settings.rs, add:

    static HOTKEY_RESULT: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
        std::sync::OnceLock::new();

    fn hotkey_result_slot() -> &'static std::sync::Mutex<Option<String>> {
        HOTKEY_RESULT.get_or_init(|| std::sync::Mutex::new(None))
    }

    #[no_mangle]
    pub extern "C" fn teletype_hotkey_captured_result(ptr: *const std::os::raw::c_char) {
        if ptr.is_null() { return; }
        let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_string_lossy().trim().to_string();
        if !text.is_empty() {
            *hotkey_result_slot().lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(text);
        }
    }

Define this unconditionally so unit tests run on every platform; only the extern declaration and native call are cfg(macos).

Verify: cargo check -p teletype-desktop -> exit 0.

### Step 2: Change the native side to call the callback

In hotkey_capture.m:
1. Add a file-level static: static void (*g_resultHandler)(const char *) = NULL;
2. Change the entry point to void teletype_start_hotkey_capture(void (*handler)(const char *)) and set g_resultHandler = handler; at its start.
3. Replace each of the three writeToFile blocks (:144, :163, :227) with a single call: if (g_resultHandler) g_resultHandler([g_currentHotkey UTF8String]); Keep the surrounding control flow (Enter confirm, key-confirm-then-stop, bare-modifier-confirm-then-stop) exactly as it is.
4. Do NOT clear g_resultHandler in teletype_stop_hotkey_capture — the native code calls stop right after confirming, and clearing first would drop the result.

Verify: grep -n 'writeToFile\|teletype_hotkey_result' crates/teletype-desktop/src/hotkey_capture.m -> no matches; grep -n 'g_resultHandler' -> at least four occurrences (declaration, assignment, three calls). macOS build check via cargo check -p teletype-desktop on this host -> exit 0.

### Step 3: Rewire the Rust commands and add tests

In start_hotkey_capture(), inside cfg(macos):
- declare fn teletype_start_hotkey_capture(handler: extern "C" fn(*const std::os::raw::c_char));
- clear the slot before starting: *hotkey_result_slot().lock().unwrap_or_else(...) = None;
- replace the remove_file line with that clear, and call teletype_start_hotkey_capture(teletype_hotkey_captured_result).

In get_captured_hotkey(), replace the whole file read with:

    hotkey_result_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()

Add two tests in the existing tests module: stored_hotkey_is_returned_once (feed a CString through the callback, assert Some binding then None) and empty_hotkey_result_is_ignored (feed an empty CString, assert None). Use std::ffi::CString::new(...).unwrap().as_ptr().

Verify: cargo test -p teletype-desktop hotkey -> all pass including the two new tests; grep -rn 'teletype_hotkey_result.txt' crates/ -> no matches.

## Test plan

- New tests: stored_hotkey_is_returned_once, empty_hotkey_result_is_ignored in settings.rs.
- Existing hotkey_capture_commands_are_available_on_every_platform must still pass.
- Verification: cargo test -p teletype-desktop, cargo test --workspace, cargo fmt --all -- --check.

## Done criteria

- [ ] grep -rn 'teletype_hotkey_result.txt' crates/ returns nothing.
- [ ] The native side has no writeToFile call; the three confirm paths call g_resultHandler.
- [ ] cargo test -p teletype-desktop -> all pass including the two new tests by name.
- [ ] cargo test --workspace -> exit 0.
- [ ] Only settings.rs, hotkey_capture.m and plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live excerpts at settings.rs:35, :88-96 or hotkey_capture.m:144/163/227/241 do not match.
- The callback symbol fails to link on macOS (report the linker error; do not revert to the file).
- The existing every-platform test starts failing for a reason other than your intended changes.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- The callback is a plain extern C function pointer; the native side keeps it for the lifetime of the process. Replacing it on each start is safe because only one capture panel can be open at a time.
- No ownership or mode checks are needed any more: there is no shared filesystem object. Do not reintroduce a temp file as a "simpler" fallback.
- The result slot is process-global; if the product ever supports two simultaneous capture windows, it needs a per-session identifier instead.
