# Research: typing autotext expansion — slashanyware.com and open-source alternatives

Date: 2026-09-26 — companion to `tasks/pi-tasks/typing-autotext-research.md`.
Context: `crates/teletype-desktop/src/typing.rs` has a working (not stub) Rust
watcher + `typing_tap.m` passive CGEventTap. The other pi task is attempting a
from-scratch CGEventTap implementation. This report finds reusable open-source
code to de-risk that work.

## 1. slashanyware.com findings

- `https://slashanyware.com` does not resolve (DNS `getaddrinfo ENOTFOUND`, both
  bare and `www.`). No GitHub org/user `slashanyware` exists
  (`gh api orgs/slashanyware/repos` and `gh api users/slashanyware/repos` both
  404). `gh search repos slashanyware` and `gh search repos "slash anyware"`
  return nothing.
- Web search finds no product by that exact name. Closest real products are
  **Slash Anywhere** (slashanywhere.com, browser `/`-command SaaS) and
  **Slashit** (slashit.app, browser snippet expander). Neither is a macOS
  global autotext app, neither is open source.
- **Conclusion:** treat slashanyware.com as a closed-source reference only.
  Nothing to reuse from it. The user-confirmed "not open source" is consistent
  with this: there is no public code at all.

## 2. Best open-source candidates (top 3)

### 2.1 SnipKey — dkdannyboy/snipkey  ⭐ best code reference

- Repo: https://github.com/dkdannyboy/snipkey — MIT, Swift, 7 stars, active
  (pushed 2026-09-25). "Free, open-source text expander for macOS — a drop-in
  replacement for TextExpander."
- Key monitoring: **passive CGEventTap**, `CGEvent.tapCreate(tap: .cgSessionEventTap,
  place: .tailAppendEventTap, options: .listenOnly, ...)` on the main run loop —
  exactly the same architecture our `typing_tap.m` already uses.
- Most reusable file: `Sources/SnipKey/ExpansionEngine.swift` (~590 lines,
  contains the whole pipeline). Key patterns:
  - `startIfPossible()` — tap creation; the C callback unretains the engine
    from `userInfo` (refcon) and forwards events.
  - **Tap re-enable on `tapDisabledByTimeout` / `tapDisabledByUserInput`** —
    macOS silently disables unresponsive taps; our `typing_tap.m` does NOT
    handle this. This is likely the #1 robustness fix to port.
  - **Self-event filtering**: `event.getIntegerValueField(.eventSourceUserData) ==
    TextInjector.magicUserData` — stamped on synthetic events at injection time
    so the tap ignores our own typed replacement. Our `typing.rs` has no such
    guard; enigo's backspaces/typed text re-enter the buffer.
  - **Quiescence guard** (`InputClock` + `arm()`): after a match, if ANY further
    user input (key OR click) arrives before injection, the expansion is
    cancelled instead of backspacing the wrong text. This solves the classic
    "user kept typing / clicked away, now our backspaces destroy real text"
    failure mode. Uses the *event* timestamp, not observation time.
  - Backspace-count handling, undo (next backspace reverts the expansion),
    case adaptation, IME/layout-aware matching (display buffer + physical
    buffer).
  - Injection via `TextInjector.expand(backspaces:text:...)` (clipboard-based,
    restores clipboard after a delay).
- Porting effort: **low-medium for concepts, none for code** (Swift → Rust
  rewrite of ~300 lines of logic). It is a design reference, not a drop-in.
  The tap re-enable, magic-userData filter, and quiescence guard are each
  ~10-30 lines in our `.m`/`.rs` and directly port the ideas.

### 2.2 Espanso — espanso/espanso  ⭐ best architecture reference

- Repo: https://github.com/espanso/espanso — **GPL-3.0**, **Rust** (rewritten
  from Go; the task's "internal/provider/" Go layout is stale), 14,540 stars,
  active. `gh api` output: `"A Privacy-first, Cross-platform Text Expander
  written in Rust"`, 14540, `GPL-3.0`.
- Key monitoring on macOS: **NOT CGEventTap** — `espanso-detect/src/mac/native.mm`
  uses `[NSEvent addGlobalMonitorForEventsMatchingMask:...]` (passive, needs
  Accessibility). It delivers `NSEvent.keyCode` + `event.characters` (the
  *typed character*, not just the key code) + modifier flags to a C callback.
  Hotkeys use Carbon `RegisterEventHotKey`.
- Injection: `espanso-inject/src/mac/native.mm` uses
  `CGEventCreateKeyboardEvent` + `CGEventKeyboardSetUnicodeString` (chunked
  for a Unicode-string length limit) posted to `kCGHIDEventTap`, and stamps
  `CGEventSetLocation(e, ESPANSO_POINT_MARKER)` — a far-off coordinate marker
  used by the detector to ignore its own injected events (`ESPANSO_EVENT_MARKER`
  check in the NSEvent handler). Same idea as SnipKey's magicUserData.
- Trigger detection: `espanso-match/src/rolling/matcher.rs` + `tree.rs`
  (~900 lines) — a rolling stateful trie that matches across key events,
  supports case-insensitive chars, word separators, and multiple concurrent
  candidate paths. `espanso-engine/src/process/middleware/matcher.rs` wires
  events into it.
- **License blocker:** GPL-3.0. Our workspace is `license = "MIT"` (root
  `Cargo.toml`; no LICENSE file exists in the repo). Copying GPL code (even
  the `.mm`) into a distributed MIT app is incompatible; at best it can inform
  design. Do NOT copy files.
- Porting effort: **medium for the matcher design** (our `typing.rs` uses a
  simpler "buffer + delimiter" model that is fine for fixed triggers);
  the rolling trie only pays off if we later need multi-candidate/word-boundary
  matching.

### 2.3 WayExpand — cyberducttape/wayexpand  ⭐ cleanest Rust engine, but Linux-only

- Repo: https://github.com/cyberducttape/wayexpand — **MIT**, **Rust**, 1 star,
  pushed 2026-09-26. "Privacy-first Wayland text expander for Linux, written
  in Rust. Native GUI/CLI, snippet automation, Espanso import, and
  wlroots/libei/input-method backends."
- **Linux-only — no macOS backend at all.** Backends: `backend-evdev`,
  `backend-libei`, `backend-wlroots`, `backend-ibus`, `backend-input-method`,
  `backend-kwin-window` (window tracking). Zero `CGEvent`/`NSEvent` references.
  So its *key-event monitoring* is not reusable for macOS.
- What IS valuable: `crates/core/src/matcher.rs` (a **reversed-char-trie
  `Matcher`** with `find_suffix`/`can_continue`/`has_continuation` — ~115 lines,
  MIT, directly portable to our Rust codebase) and `crates/core/src/engine.rs`
  `ExpansionEngine::process(InputEvent)` — a clean event-driven state machine:
  `InputEvent::{Text, Key, Backspace, Delimiter, EndOfInput, Reset,
  FocusChanged{sensitive}, PauseChanged, WindowChanged}`, bounded buffer
  (`max_buffer_chars`, `pop_front`), backspace handling via `buffer.pop_back()`,
  deferred matches, undo chords, app filtering. This is a good template for
  restructuring our `typing.rs` loop into a testable pure engine.
- Porting effort: **low for `matcher.rs`** (copy the pattern, MIT); **medium**
  to restructure `typing.rs` around its `InputEvent` model.

### Honorable mentions (from the required searches)

- `Rezwanul-Haque/typely` — "Rust + Tauri" cross-platform text expander —
  closest stack match to us (Tauri), worth a look if we want a peer codebase.
- `brianyu28/streamline` — macOS text-expanding automation app (Swift).
- `jeffcaldwellca/macspanso` — macOS GUI for espanso (Swift, wrapper only).
- `Muminur/MuttonText` — cross-platform (Linux+macOS) open-source expander,
  Beeftext-compatible.
- TextExpander / aText: closed source, no public code. Maccy: clipboard
  manager, not relevant.

## 3. Rust crate recommendations (key monitoring)

| Crate | Verdict |
|---|---|
| `rdev` 0.5.3 | "Listen and send keyboard and mouse events on Windows, Linux and MacOS." **Yes, macOS uses CGEventTap internally**, and it delivers individual key-down/up events (not just hotkeys) — but only key *codes* and modifiers, **not the typed character** (no `characters`/Unicode resolution). Usable, but we'd keep our own keycode→char map and lose IME/composed text. |
| `keyboard-hook` | Windows-focused; not the right fit for macOS. |
| `enigo` 0.6.1 | "Cross-platform … library to **simulate** keyboard and mouse events." **No key-monitoring API.** Keep using it only for injection (backspaces + `enigo.text`), as `typing.rs` already does. |
| `cgevent` / `global-hotkey` | No maintained crate gives us what we need better than the 70-line Objective-C tap we already have. |

**Recommendation: keep the existing `typing_tap.m` CGEventTap** (it already
works and is the standard approach; SnipKey proves the pattern). No new crate
needed for listening. If we ever want a pure-Rust path, `rdev` is the option —
but its key-code-only events are a regression vs. reading `characters` in the
tap.

Sketch of wiring (concept only — no code changes made per task scope):

```rust
// typing.rs — keep the mpsc channel, but feed it (char, flags) instead of keycode
fn typing_loop(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<TypeEvent>();
    TAP_TX.set(tx).ok();
    #[cfg(target_os = "macos")]
    unsafe { teletype_typing_tap_start(on_key_down) }; // now passes chars + flags
    let mut engine = ExpansionEngine::new(&store);     // pure, unit-testable
    while WATCHER_ACTIVE.load(SeqCst) {
        while let Ok(ev) = rx.try_recv() {
            for m in engine.process(ev) {              // returns match + backspaces
                if quiescence_ok(&last_input_time) {   // SnipKey guard
                    expand(&m);                        // enigo backspaces + text
                }
            }
        }
        thread::sleep(Duration::from_millis(16));      // 60fps, not 50ms
    }
}
```

## 4. Recommended approach

**Keep our passive CGEventTap (`typing_tap.m`) as the input source; port three
robustness patterns from SnipKey (MIT) into the tap + `typing.rs`; borrow the
reversed-trie matcher shape from WayExpand (MIT) for the Rust side.**

Concretely, the single best path:

1. In `typing_tap.m`: handle `kCGEventTapDisabledByTimeout`/`...ByUserInput`
   by re-enabling the tap (SnipKey `handle(type:)` lines 154-160).
2. In `typing_tap.m`: also read `CGEventKeyboardGetUnicodeString` (typed char)
   and modifier flags, and pass them to Rust — stop mapping keycodes→chars in
   Rust (fixes shifted chars, non-US layouts). Stamp our synthetic events with
   a magic `kCGEventSourceUserData` value (or espanso's far-off-location marker)
   and ignore them in the tap.
3. In `typing.rs`: replace the 50ms polling buffer with a small pure
   `ExpansionEngine::process(InputEvent)` (WayExpand-style `InputEvent` enum:
   Text/Backspace/Delimiter/Reset) + the reversed-trie matcher; keep
   `enigo` for backspaces + `enigo.text` injection.
4. Add the quiescence guard (SnipKey): record the event timestamp at match
   time; if any further key/click arrives before injection, abort instead of
   backspacing.

Effort: ~2-4 days total (tap re-enable + chars: 0.5d; engine restructure:
1-2d; quiescence + self-filter: 0.5-1d). No new dependencies.

## 5. License compatibility

- Our project: `license = "MIT"` in root `Cargo.toml` (no LICENSE file present
  in the repo — worth adding one).
- **SnipKey: MIT — compatible.** We are porting *ideas* (and small patterns),
  not files; MIT would permit verbatim copies too.
- **WayExpand: MIT — compatible.** `matcher.rs` patterns can be reused freely.
- **Espanso: GPL-3.0 — INCOMPATIBLE with an MIT-licensed distributed app.**
  Use it strictly as a design reference; do not copy code or the `.mm` files
  into this repo.
- No crate recommendation above changes the license posture (we add none).

## Verification (real output)

### 1. `gh search repos "text expander" --language swift --limit 10`

```
brianyu28/streamline	A text-expanding automation app for macOS	public	2026-08-18T08:45:44Z
jeffcaldwellca/macspanso	macOS GUI for the espanso text expander	public	2026-09-13T03:19:00Z
y1lichen/Expander	text expander	public	2024-09-26T06:46:26Z
pchahal/TextXpander	Text expander productivity tool for Mac to Save you typing	public	2023-03-18T04:45:34Z
Tedixx/afkortext	Afkortext: a lightweight native macOS text expander	public	2026-08-14T12:02:25Z
uzairahmadxy/TypeAhead	Text Expander	public	2026-06-12T18:15:01Z
dkdannyboy/snipkey	Free, open-source text expander for macOS — a drop-in replacement for TextExpander. Native Swift, no subscription, no account, no network.	public	2026-09-25T03:17:44Z
leniejoicem/KeyExpander	macOS text expander app	public	2026-03-15T13:38:02Z
calebmpeterson/Expander	Simple text expander for macOS	public	2025-12-09T22:22:31Z
PrashantGaikwad-iOS/ExpandCellAnimation	Cell dynamic text expand animation	public	2018-11-21T11:32:54Z
```

### 2. `gh search repos "cgeventtap" text --limit 10`

```
(empty — no output, exit code 0)
```

### 3. `gh api repos/espanso/espanso --jq '.description, .stargazers_count, .license.spdx_id'`

```
A Privacy-first, Cross-platform Text Expander written in Rust
14540
GPL-3.0
```

### 4. `ls tasks/pi-tasks/learnings/typing-autotext-research.md`

```
tasks/pi-tasks/learnings/typing-autotext-research.md
```

### 5. `wc -l tasks/pi-tasks/learnings/typing-autotext-research.md`

```
     241 tasks/pi-tasks/learnings/typing-autotext-research.md
```
