# Task: Research slashanyware.com and open-source alternatives for typing autotext expansion

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- This is a RESEARCH task. Do NOT write any code. Do NOT create any files except the
  research report at `tasks/pi-tasks/learnings/typing-autotext-research.md`.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
We need a working "expand autotext while typing" feature. The current implementation in
`crates/teletype-desktop/src/typing.rs` is a stub. A separate pi task is attempting a from-scratch
CGEventTap implementation, which is complex and error-prone. The user pointed to
slashanyware.com as a program that does this. We want to find open-source code we can adapt
or a library that does this, to avoid reinventing the wheel.

## What to research

### 1. slashanyware.com
- Visit https://slashanyware.com and understand what the product does.
- Search for their GitHub: `gh search repos slashanyware` and `gh search repos "slash anyware"`.
- Also try: `gh search repos "slash" "autotext"`, `gh search repos "slash" "text expansion"`.
- Check if they have a public GitHub org: `gh api orgs/slashanyware/repos` or
  `gh api users/slashanyware/repos`.
- If you find their repo, read the key files: how they implement the key-event hook,
  the trigger detection, and the text replacement. Note the language (Swift? Rust? Objective-C?),
  the license, and which files would be most reusable.

### 2. Open-source text expanders / autotext tools
Search GitHub for open-source text expansion tools, especially ones that:
- Run on macOS
- Use CGEventTap or NSEvent for key monitoring
- Support trigger-based expansion (type `/trigger` → expand)
- Are written in Swift, Objective-C, or Rust (easy to port to our Tauri/Rust codebase)

Useful searches:
- `gh search repos "text expander" --language swift`
- `gh search repos "text expansion" macos`
- `gh search repos "autotext" macos`
- `gh search repos "snippet" "text expansion" macos`
- `gh search repos "cgeventtap" "text"`
- `gh search repos "key monitoring" "text expansion"`
- `gh search repos "type substitution" macos`

Well-known candidates to check:
- **Espanso** (espanso/espanso) — open-source text expander, cross-platform. Check their
  macOS key-event implementation.
- **TextExpander** (not open source, but check if they have any public code)
- **aText** (not open source)
- **Pleak** (open-source, check GitHub)
- **FastScripts** (open-source)
- **Maccy** (clipboard, not text expansion, but check their key-event approach)

### 3. Rust crates for key-event monitoring
- Search crates.io for: `cgevent`, `key-event`, `keyboard-hook`, `global-hotkey`,
  `rdev`, `enigo` (we already have enigo 0.6).
- `rdev` is a popular crate for cross-platform key monitoring. Check if it supports
  macOS CGEventTap and if it can give us individual key-down events (not just hotkeys).
- Check if `enigo` 0.6 has any key-monitoring API (it's primarily for input simulation).

### 4. The enviouswispr reference
- Read `enviouswispr/EnviousWispr/Sources/EnviousWisprServices/` for any text expansion
  or typing-watcher code. Check `PasteService.swift`, `KeySymbols.swift`, and any
  service that monitors keystrokes.
- Note: enviouswispr is a Swift app, so the code won't port directly, but the architecture
  and approach (how they buffer keystrokes, detect triggers, and replace text) is valuable.

## Output
Write a research report to `tasks/pi-tasks/learnings/typing-autotext-research.md` with:

1. **slashanyware.com findings** — what it is, is it open source, what can we reuse.
2. **Best open-source candidates** (top 3) with:
   - Repo URL, license, language
   - How they implement key monitoring (CGEventTap? NSEvent? other?)
   - Which specific files/functions are most reusable
   - Effort estimate to port/adapt to our Rust/Tauri codebase
3. **Rust crate recommendations** — which crate(s) to use for key monitoring, with
   a short code sketch showing how to wire it into our `typing.rs`.
4. **Recommended approach** — the single best path forward: "use X from Y repo" or
   "use Z crate" with a 5-10 line sketch of how it plugs into our existing `typing.rs`.
5. **License compatibility** — confirm the chosen source's license is compatible with
   our project (check our LICENSE file if it exists).

## Verification (run these, paste real output)
1. `gh search repos "text expander" --language swift --limit 10`
2. `gh search repos "cgeventtap" text --limit 10`
3. `gh api repos/espanso/espanso --jq '.description, .stargazers_count, .license.spdx_id'`
4. `ls tasks/pi-tasks/learnings/typing-autotext-research.md` (confirm the report was written)
5. `wc -l tasks/pi-tasks/learnings/typing-autotext-research.md`

## Out of scope (do NOT touch)
- Any source code file (`.rs`, `.m`, `.tsx`, `.ts`).
- Any `Cargo.toml`.
- The `enviouswispr/` reference tree (read-only reference).

## UPDATED PRIORITY (user-provided leads, 2026-09-26)
The user confirmed slashanyware.com is NOT open source. Focus the research on these two repos:

1. **Espanso** — https://github.com/espanso/espanso
   - Cross-platform open-source text expander.
   - Check their macOS key-event implementation: `internal/provider/` or `internal/trigger/` dirs.
   - Note: Espanso is Go, not Rust/Swift, but the trigger-detection architecture
     (how they buffer keystrokes, match triggers, and replace text) is directly portable.
   - License: check `LICENSE` file.

2. **WayExpand** — https://github.com/cyberducttape/wayexpand
   - Check what this is, its language, license, and how it implements text expansion.
   - If it's Swift or Objective-C, the CGEventTap code may be directly reusable.

Prioritize these two over the general GitHub searches. If either has a reusable
key-event-monitoring component, document exactly which files to port and how they
would plug into our `typing.rs`.
