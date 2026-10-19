# Teletype Windows Buildbot Session Brief

A self-contained brief for a coding agent (or human) running a **Windows buildbot
session**: bring Teletype to a real Windows build + test + runtime evidence, covering
the **eval harness** and the **e2e delivery matrix**. This is the Windows lane that
mirrors what `feat/beat-wispr` already proved on macOS.

Read this top to bottom before touching code. It states what exists, what is
portable, what is missing, and the exact acceptance bar.

---

## 1. What "done" means for this session

This is a **buildbot / verification session**, not a feature build. The goal is
**measured evidence on a real Windows machine**, recorded like the macOS evidence:

1. A native Windows **release build** that compiles and launches (the CI `ci.yml`
   already has a `windows-latest` `cargo check` gate — this session goes past
   `check` to a real `cargo build --release` + launch).
2. The **eval harness** (`teletype-eval`) runs on Windows and produces a
   `evals/RESULTS.md` row: WER, RTF, and latency for the Windows ASR engine
   (Whisper via whisper.cpp — CoreML/Parakeet is macOS-only).
3. The **e2e delivery matrix** runs on Windows against real target apps and produces
   a `MATRIX.md` (one PASS/FAIL per `target × forced-fallback` cell), proving
   "text lands in the focused app + clipboard restored" on Windows.

**Exit = a `docs/` evidence page (or `evals/runs/.../`) with the build, the eval
numbers, and the matrix PASS/FAIL table, all produced on Windows.** No claim is
"done" without the run output.

---

## 2. What already exists (do not rebuild)

| Thing | Where | Status on Windows |
|---|---|---|
| Windows keyboard hook | `crates/teletype-desktop/src/typing_windows.rs` (`WH_KEYBOARD_LL`) | Committed (`3c9783a`); verify, don't rewrite |
| Platform-unified key decode | `typing.rs::decode_key()` | Committed |
| Cross-platform speech resolve | `dictation.rs::resolve_speech_model` | Windows resolves to Whisper (whisper.cpp) |
| CI compile gate | `.github/workflows/ci.yml` `windows-latest` job | `cargo check --workspace` under MSVC only |
| Eval harness | `crates/teletype-eval/` + `scripts/eval/` | **Run it; it is the measurement tool** |
| macOS e2e matrix (reference) | `scripts/e2e/delivery_matrix_macos.sh`, `crates/teletype-desktop/src/e2e.rs` | **macOS-only — the template, not the deliverable** |
| Windows roadmap | `docs/roadmap-windows.md` | Phases W0–W4 + measured learnings |
| macOS matrix run (format reference) | `evals/runs/e2e_matrix_2026-10-17/MATRIX.md` | Copy its PASS/FAIL table shape |

---

## 3. The one hard gap: the e2e control channel is Unix-only

`crates/teletype-desktop/src/e2e.rs` is `#[cfg(unix)]`. It drives the app over a
**Unix domain socket** (`/tmp/teletype-e2e.sock`, overridable via `TELETYPE_E2E_SOCKET`)
with one JSON command per line:

- `{"cmd":"status"}` → app/pill/speech-model readiness.
- `{"cmd":"policy","app":"<id>","force":"clipboard|type|setvalue|hold"}` → pin a forced
  fallback for a target (the "repeat with each fallback forced" step).
- `{"cmd":"start","input":"<path.wav>","command":true|false}` → queue a fixture WAV as
  the take's audio source (file-input mode) and trigger the take on the production
  controller.
- `{"cmd":"stop"}`, `{"cmd":"result","id":N}` → control + read the take's outcome.

**On Windows this socket does not exist.** The `start` call site skips the harness.
So the Windows matrix cannot reuse the macOS driver as-is.

**Decision for this session (pick one, record which):**
- **(A) Named-pipe port (recommended):** implement a `#[cfg(windows)]` branch of
  `e2e.rs::start` on a Windows **named pipe** (`\\.\pipe\teletype-e2e`) speaking the
  same line-delimited JSON. The app still launches with `--e2e`. This is the smallest
  delta and keeps the harness + script logic identical across platforms.
- **(B) Reuse `TELETYPE_FORCE_APP`:** the `delivery.rs` policy override already reads a
  per-app forced mode; a Windows driver can set `TELETYPE_FORCE_APP` + the app's bundle
  equivalent and a separate process can trigger the take. Weaker (no live `result`),
  but zero new IPC.

Either way, the **production delivery cascade is untouched** — the e2e hook only
replaces the audio source and the trigger.

---

## 4. The Windows delivery matrix (the deliverable table)

Mirror the macOS matrix shape: **target app × forced delivery fallback**, each cell
asserts (1) the expected text lands in the focused field, (2) the clipboard is restored.

- **Targets (Windows analogues of the macOS 8):** Notepad, Microsoft Word, a browser
  (Edge or Chrome on a local test page), a terminal (Windows Terminal / cmd), a
  password/secure field (to prove the secure-field refusal). Pick 4–5 real apps that
  are installed on the buildbot; record which.
- **Forced fallbacks (Windows subset of `clipboard|type|setvalue|hold`):**
  - `clipboard` → clipboard snapshot + `SendInput` paste (the Windows workhorse;
    roadmap Tier-2).
  - `type` → enigo `fast_text` typing fallback.
  - `setvalue` → UI Automation `TextPattern` (the higher tier; **coverage is
    per-app and UNMEASURED** — this is the biggest platform risk, roadmap §4).
  - `hold` → held-for-paste (secure-field path).
- **Fixture:** a WAV the ASR transcribes determinably enough for a substring assert
  (macOS uses `crates/teletype-speech/tests/fixtures/hello.wav` → assert contains
  "hello"). Use the same or a Windows-appropriate fixture; record the file.
- **Output:** `evals/runs/e2e_matrix_windows_<date>/MATRIX.md` with a PASS/FAIL/SKIP
  row per cell, plus the app pid, socket/pipe name, and per-case field read-back.
  Exit non-zero on any FAIL.

**Per-cell gotchas to expect (from roadmap §4, re-measure — do not trust):**
- Thread pinning: whisper.cpp must set `intra_op_num_threads` explicitly per hardware
  profile (E-core oversubscription made *more* threads 7–10x slower on the reference).
- Clipboard holds **one item**, not the macOS multi-item pasteboard; restore preserves
  all formats of the current item only. Text is lossless; record this as a known
  product limitation, not a bug.
- `SendInput` into an **elevated** window is blocked by UIPI with no prompt (silent
  failure) — detect + surface, and mark such cells SKIP-with-reason, not FAIL.
- RDP/console sessions don't draw; the buildbot must run in a **logged-in interactive
  desktop session**.
- Timing constants (50 ms poll, 1 s caps, 200 ms post-paste wait, 100 ms repair
  deadline) were tuned on Apple Silicon — **re-tune on Windows, do not port**.
- The `setvalue` (UIA) tier: the pattern is always "available" but the app may not
  implement it — read per-app UIA capability, not a global grant.

---

## 5. The eval harness on Windows

- Run `teletype-eval` against the **Windows ASR engine** (Whisper/whisper.cpp). Parakeet
  and CoreML are macOS-only; do not expect them here.
- Produce a `RESULTS.md` row: WER on the eval sets, RTF, and first/whole-clip latency,
  at the catalog's Windows default quantization. Record the model file + hash.
- If a GPU tier (CUDA) is in scope, that's a roadmap **open decision** (roadmap §6) —
  out of scope for this buildbot session unless explicitly requested.
- **Do not** treat the reference Windows port's numbers (roadmap §2: 0.32–0.35 s for a
  10 s clip tuned-CPU Parakeet) as Teletype evidence. They are a prior. Measure.

---

## 6. Buildbot environment checklist

Before the first run, confirm on the Windows host:

1. **Native MSVC toolchain** + a working C compiler (whisper.cpp/ring are C and
   **cannot** cross-compile from a macOS host).
2. An interactive **logged-in desktop session** (not a service/RDP session).
3. The target apps for §4 installed and launchable headlessly-enough for focus + read.
4. `teletype-speech` builds on Windows (Whisper path) and `resolve_speech_model`
   selects the on-disk model.
5. A way to read the focused field back per app (Windows UIA / `powershell` UIA
   automation is the analog of the macOS osascript/AX reads).
6. `llama-server` (s1-mini) present if polish is in the eval path; otherwise run the
   eval with polish off.

---

## 7. Definition of done (checklist)

- [ ] Native `cargo build --release` on Windows succeeds; the app launches in an
      interactive session (record the launch + a screenshot or log line).
- [ ] `teletype-eval` runs on Windows; a `RESULTS.md` row (WER, RTF, latency) is
      committed for the Windows engine.
- [ ] A Windows e2e driver exists (§3 decision A or B, recorded).
- [ ] `evals/runs/e2e_matrix_windows_<date>/MATRIX.md` has a PASS/FAIL/SKIP row per
      `target × forced-fallback` cell, non-zero exit on any FAIL.
- [ ] Every SKIP has a recorded reason (UIPI elevation, UIA unsupported, app missing).
- [ ] `docs/roadmap-windows.md` W0 (runtime proof) is ticked with the link to the
      evidence, or the gap is stated explicitly.
- [ ] No macOS-only assumption leaked into the Windows path (grep `coreml`,
      `cpal` CoreAudio constants, `osascript`, `AXValue` in the Windows driver).

---

## 8. What NOT to do

- Do not port CoreAudio timing constants or the macOS pasteboard multi-item behavior.
- Do not pull in GPL code (the reference port is GPLv3; Teletype is MIT, D001).
- Do not treat the reference Windows port's measurements as Teletype's.
- Do not ship a Windows build from a macOS cross-compile.
- Do not claim the UIA `setvalue` tier works without a per-app UIA read proving it.
