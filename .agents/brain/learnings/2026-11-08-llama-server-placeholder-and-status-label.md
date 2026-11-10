---
id: L018
title: 0-byte llama-server placeholder makes every local polish model unusable; "Local transform" status label hid the real model state
status: verified
type: debugging
area: models / inference / readiness
agent: pi
tool: code inspection + build + live process check
date: 2026-11-08
---

# llama-server placeholder + misleading "Local transform" status

## Problem

User reported three things at once: (1) Models/Command Mode/Keybinds tabs
were narrower than AutoText; (2) every downloaded local polish model showed
"llama-server binary not found"; (3) the dashboard showed a green check next
to "Local transform" while pasting still said "No polish model loaded".

## Symptoms

- Models tab: red error under every downloaded local model.
- Home status card: green check, label "Local transform", while no engine
  was actually running.
- Insights: "No polish model loaded. Dictations are not being rewritten."

## Investigation

1. `target/debug/llama-server` and `crates/teletype-desktop/binaries/llama-server`
   were both 0-byte files. `find_llama_server_bin` (teletype-inference/src/server.rs)
   skips non-executable/empty files, so every local model load failed with
   "llama-server binary not found". The user had never run
   `scripts/build-llama-server.sh` on this machine.
2. `ensure_local_provider` (lib.rs warmup thread) already loads the selected
   local model at startup — the code was right, it just failed on the missing
   binary. After building the binary, `llama-server -m s1-mini.gguf` was
   running and `/health` returned ok with no user action.
3. `llm_runtime` (commands/models.rs) labelled every non-remote, non-loaded
   state "Local transform" — a category name, not the model. A green check
   next to it read as "transform is working" when nothing was loaded.

## Fix

- Built the real binary: `./scripts/build-llama-server.sh`
  (installs to both `crates/teletype-desktop/binaries/llama-server` and
  `target/debug/llama-server`).
- `llm_runtime` now: names the selected model (catalog `name`) in the label;
  reports `error` when `find_llama_server_bin()` fails (with the rebuild
  command in the detail); reports `ready` with an accurate "on disk, loads
  automatically before your next dictation" sentence otherwise.
- Tab widths: removed the `maxWidth: 860` caps on Models, Command Mode and
  Keybinds screens so all tabs share the app's 1200px container like AutoText.

## Verification

- `llama-server --version` runs from both install paths.
- Live dev app: `pgrep -fl llama-server` shows s1-mini loaded;
  `curl /health` → `{"status":"ok"}`.
- `cargo check`, `cargo clippy` (teletype-desktop, teletype-inference) clean;
  `cargo test -p teletype-desktop --lib` runtime/llm tests pass; `tsc` clean;
  vitest failures are pre-existing (DictionaryScreen matchMedia, untouched).

## Reusable lesson

- A green check must never be rendered for a state the backend cannot
  actually serve. Derive the readiness label from the resolved model name
  and from `find_llama_server_bin()` success, not from "a selection exists".
- The 0-byte placeholder in `crates/teletype-desktop/binaries/llama-server`
  is required for `cargo tauri build` to pass; on a fresh dev machine the
  first thing to run is `scripts/build-llama-server.sh`, or every local
  polish model is dead and the UI says something unhelpful.
- When several tabs "look narrower", check for per-screen `maxWidth` caps —
  the app container is 1200px; screens that add `maxWidth: 860` break the
  right-edge alignment.
