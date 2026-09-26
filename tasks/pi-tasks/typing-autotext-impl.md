# Task: Implement typing autotext expansion (CGEventTap on macOS)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Files you may edit: `crates/teletype-desktop/src/typing.rs`,
  `crates/teletype-desktop/src/typing_tap.m` (new file),
  `crates/teletype-desktop/src/lib.rs` (only to register the new module if needed),
  `crates/teletype-desktop/Cargo.toml` (only if a new dependency is needed).
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
The "Expand AutoText while typing" setting is wired up (the toggle exists in Settings, the
`typing::set_enabled` call is in `lib.rs`), but `typing_loop` is a stub that just sleeps in a
loop. It does nothing. The user expects that when they type a trigger (e.g. `/addr`) in any
application, it gets expanded to the replacement text.

The reference app (enviouswispr) does this with a CGEventTap that watches for key-down events,
buffers the typed characters, and when a trigger + delimiter (space/enter) is detected, it
backspaces the trigger and types the replacement.

## Read first
1. `crates/teletype-desktop/src/typing.rs` — the whole file. Note the `WATCHER_ACTIVE` atomic,
   `start()`, `set_enabled()`, and the stub `typing_loop()`.
2. `crates/teletype-desktop/src/fn_tap.m` — the existing CGEventTap for the Fn key. This is the
   pattern to follow: a C function that creates a `CGEventTap`, a callback that fires on
   key-down events, and a Rust FFI wrapper.
3. `crates/teletype-desktop/src/fn_tap.rs` — the Rust FFI wrapper for the Fn tap. Note how it
   declares `extern "C"` functions and how the callback is wired up.
4. `crates/teletype-core/src/autotext/mod.rs` — `AutoTextStore`, `find_by_trigger`,
   `AutoTextEntry` (has `trigger` and `replacement` fields).
5. `crates/teletype-core/src/autotext/expand.rs` — the `expand()` function that the pipeline
   uses. This is the function that does the actual trigger→replacement substitution.
6. `crates/teletype-desktop/src/lib.rs` — the `AppState` struct (around line 200-260) to see
   how `autotext` is stored and accessed.
7. `crates/teletype-desktop/Cargo.toml` — check if `enigo` is already a dependency (it is,
   line 34). Also check if `core-graphics` or `core-foundation` are available for FFI.

## What to build

### 1. `typing_tap.m` — CGEventTap for key-down events
Create a new Objective-C file that:
- Creates a `CGEventTap` on `kCGEventTapOptionListenOnly` (we only observe, not modify).
- The callback fires on `kCGEventKeyDown` events.
- The callback extracts the `keyCode` from the event and calls a Rust callback
  (`extern "C" fn typing_tap_callback(key_code: i64)`).
- Exports `typing_tap_start()` and `typing_tap_stop()` functions.
- Follow the exact pattern of `fn_tap.m`: same structure, same error handling, same
  thread safety (the tap runs on the main run loop).

### 2. `typing.rs` — Implement the typing loop
Replace the stub `typing_loop` with a real implementation:
- On start, install the CGEventTap (call `typing_tap_start()`).
- Maintain a `String` buffer of recently-typed characters.
- The callback (called from the CGEventTap thread) pushes the character into the buffer.
  Use a `Mutex<String>` for thread safety.
- The main loop (every 50ms) checks if the buffer ends with a known trigger + a delimiter
  (space or enter, keycode 49 or 36).
- When a trigger is detected:
  1. Look up the trigger in the `AutoTextStore` (passed in or accessed via a shared reference).
  2. If found, use `enigo` to:
     a. Backspace the trigger length + 1 (for the delimiter).
     b. Type the replacement text.
  3. Clear the buffer.
- On stop, remove the CGEventTap (call `typing_tap_stop()`).

### 3. Wire up the FFI
- In `typing.rs`, declare the `extern "C"` functions for `typing_tap_start`, `typing_tap_stop`,
  and the callback.
- The callback needs access to the `Mutex<String>` buffer. Use a `static` or pass a pointer.
- Follow the `fn_tap.rs` pattern for the FFI declarations.

### 4. Access the AutoTextStore
The `typing_loop` needs access to the `AutoTextStore`. The cleanest way:
- Add a `fn set_autotext_store(store: Arc<RwLock<AutoTextStore>>)` to `typing.rs` that stores
  the store in a `static` or `OnceCell`.
- Call this from `lib.rs` after the `AppState` is created (where `typing::start` is called).
- The typing loop reads from this store to look up triggers.

### 5. Key code mapping
For the CGEventTap, you need to map macOS virtual key codes to characters. The simplest
approach: use `UCKeyTranslate` or a lookup table for the common ASCII keys (a-z, 0-9, `/`).
For V1, only support triggers that start with `/` (the standard AutoText trigger prefix).
You can use a simple lookup table for the 26 letters + `/` + space + enter.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo build -p teletype-desktop 2>&1 | tail -20`
   (must compile, no errors)
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo test -p teletype-desktop 2>&1 | tail -30`
   (existing tests must still pass)
3. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff crates/teletype-desktop/src/typing.rs`
4. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff crates/teletype-desktop/src/lib.rs`
5. `cat crates/teletype-desktop/src/typing_tap.m` (the new file)

## Out of scope (do NOT touch)
- `ui/src/` (any frontend file).
- `crates/teletype-core/` (the core autotext logic is already correct).
- `crates/teletype-inference/`.
- The `enviouswispr/` reference tree.
- Any other `.rs` file except `typing.rs`, `lib.rs` (wiring only), and the new `typing_tap.m`.
