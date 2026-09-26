---
name: ali-app-dev-loop
description: Start/restart the Teletype Tauri dev app correctly (vite + binary + llama-server re-sign) and confirm the window is actually serving. Use whenever the app needs a live verification or comes up blank.
trigger: /ali-app-dev-loop
---

# ali-app-dev-loop — get the Teletype app running and verified

The app needs THREE things and a blank window means one is missing. Do not ask Ali to
restart — do it yourself.

## The three requirements
1. **vite dev server** on port 1420 (`ui/` → `npm run dev`). Missing = blank window.
2. **The compiled binary** (`target/debug/Teletype`) running.
3. **llama-server re-signed** (only if it was rebuilt) — L006.

## THE BLANK-WINDOW RULE (L009) — read this first

A plain `cargo build --release` / `cargo build -p teletype-desktop` produces a binary
that loads the frontend from `devUrl` (http://localhost:1420), **not** `frontendDist`.
Plain cargo does NOT set Tauri's production-mode flag. So that binary is blank unless
vite is running on 1420.

**The only two correct ways to run the app:**
- **Development:** `cargo tauri dev` — auto-starts vite and sets the dev flag. This is
  the default for live verification.
- **Release (packaged):** `cargo tauri build` — sets the production flag and embeds
  `frontendDist` into the binary; no vite needed.

**Never launch a plain-`cargo build` binary expecting the UI to appear** without vite
on 1420. If you must use a plain-built binary, start vite first (step 2).

Also: keep `tauri.conf.json` `frontendDist` as the portable relative path
`../../ui/dist`. An absolute machine path or a `dist` symlink is non-portable and was a
dead-end.

## Procedure

### 1. Check what's already up
```bash
lsof -iTCP:1420 -sTCP:LISTEN 2>/dev/null | head -3 || echo "vite NOT listening"
pgrep -f "target/debug/Teletype" | grep -v grep && echo "app running" || echo "app NOT running"
```

### 2. Start vite if missing
```bash
cd /Users/a.sorathiya/Documents/Ali/teletype/ui
npm run dev > /tmp/teletype-vite.log 2>&1 &
```
Wait until `curl -s -o /dev/null -w "%{http_code}" http://localhost:1420` returns 200.

### 3. Build + re-sign + launch the app (if the binary isn't running)
```bash
cd /Users/a.sorathiya/Documents/Ali/teletype
cargo build -p teletype-desktop 2>&1 | tail -3
# L006: the linker-signed ad-hoc signature is rejected by macOS → SIGKILL. Re-sign.
[ -f target/debug/llama-server ] && codesign --force --sign - target/debug/llama-server
cargo tauri dev > /tmp/teletype-dev.log 2>&1 &
```
Note: if vite is already on 1420, `cargo tauri dev`'s own vite attempt errors with
"Port 1420 in use" — that is HARMLESS; the app connects to the existing server.

### 4. Confirm it's actually serving
```bash
pgrep -f "target/debug/Teletype" | grep -v grep && echo "app up"
curl -s -o /dev/null -w "%{http_code}" http://localhost:1420   # must be 200
```
Both green = the window is live.

## Common failures
- **Blank window** → vite not on 1420. Step 2.
- **EG-1 / local model "Download failed: SIGKILL"** → unsigned llama-server. Step 3 re-sign.
- **Settings change not reflected** → the UI listens to the `settings-changed` Tauri
  event; if a screen reads a setting once on mount and never re-fetches, that's the bug
  (see L005). Add a `settings-changed` listener.

## IPC gotcha
Tauri settings are camelCase: `selectedLlmModel`, `enableDeveloperTab`, `pillStyle`
(not snake_case). The `Settings` struct uses `#[serde(rename_all = "camelCase")]`.
