# Task 5: NeuDecide page — investigation report (INVESTIGATION ONLY)

Worktree: `/Users/a.sorathiya/Documents/Ali/teletype-wt/t5-neudecide`, branch `t7/t5-neudecide`.
No source files were modified. All citations are `file:line` in that worktree.

## 1. What NeuDecide is

NeuDecide is Teletype's **Command Mode decider**: a 55.5M-parameter, Q4-quantized
ONNX model (Apache-2.0, by neuphonic) that takes the **raw audio of a command
take** and directly emits a **tool call** (tool name + enum arguments) from the
fixed 14-tool menu. It is not a text model and not a polish engine.

Key facts:

- Catalog row: `id: "neudecide-q4"`, `name: "NeuDecide Q4 (decider)"`,
  41 MB, `polish_capable: false`, `recommended: false`, `runs_on_cpu: true`
  (`crates/teletype-inference/src/catalog.rs:518-593`).
- It is a **hybrid decider**: the model supplies the tool name and enum
  arguments (e.g. `set_volume {level: "30"}`) straight from audio; free-form
  arguments (email body, message text, typed text) are filled from the ASR
  transcript by deterministic rules first, LLM fallback second
  (`crates/teletype-desktop/src/command_mode.rs:270-276`,
  `CommandResolver::fill_free_form_args` at `:237`, and the call site at
  `:502`).
- It is **language-gated**: it only runs when the take's language is English
  (`en` prefix, case-insensitive) or `"auto"`
  (`NeuDecideDecider::language_ok`, `command_mode.rs:349-352`; gate applied at
  selection in `select_decider`, `command_mode.rs:769-777`). It never runs on
  dictation takes, only on command takes.
- It has a **VAD speech-duration gate** (Addendum A.1, D019): takes with less
  than 300 ms of above-floor speech are rejected before the model loads
  (`MIN_SPEECH_MS = 300`, `command_mode.rs:367`; `vad_gate` at `:437`). This is
  the production noise protection, because the shipped q4 model itself
  false-triggers on ~97% of MUSAN noise clips including pure silence
  (`evals/RESULTS.md`, "Run 2026-10-18 c" section).
- Selection order per take: **1. NeuDecide** (model present, `neudecide`
  feature on, English/auto), **2. Needle** (transcript-based, TinyDecide +
  Needle 3), **3. LLM JSON-schema fallback**
  (`command_mode.rs:755-777`, brain D018 at
  `.agents/brain/PROJECT_BRAIN.md:1148`).
- Per-take trace fields record the path: `decider`, `decider_ms`, `tool`,
  `args_source` ("neudecide" / "transcript" / "llm" / "none")
  (`command_mode.rs:44,59,234`); the developer log line is
  `neudecide: {outcome} … tool={first_tool}` (`command_mode.rs:493`).

The tool menu is the 14-tool `COMMAND_TOOLS_JSON` from
`crates/teletype-inference/src/needle.rs` (open_app, open_url, open_file,
keyboard_shortcut, type_text, compose_email, send_message,
create_calendar_event, set_timer, set_volume, mute_speaker, set_brightness,
play_media, plus the `no_action` sentinel appended at
`command_mode.rs:313-321`).

## 2. How it is selected and where the model comes from

### Default vs picker

- The default decider model id is `"neudecide-q4"`
  (`default_command_decider_model`, `crates/teletype-desktop/src/commands.rs:1047-1049`;
  setting field `command_decider_model` / IPC `commandDeciderModel` at
  `commands.rs:634-635`, camelCase round-trip test at `:1861-1873`).
- The picker on the Command Mode screen lists rows from the Tauri command
  `list_command_deciders` (`commands.rs:1166-1171` →
  `command_decider_rows`, `commands.rs:1082+`): `neudecide-q4`,
  `qwen3-asr+llm` (available only when a Qwen ASR speech model AND a local
  polish LLM are installed), and `lfm3-omni` (always unavailable in this
  branch: needs llama-server b11960+, bundled is b11488 — `commands.rs:1110-1117`).
  The UI renders a plain `<select>`; unavailable rows are disabled with their
  reason (`ui/src/screens/CommandModeScreen.tsx:395-411`).
- The UI default state is also `"neudecide-q4"`
  (`CommandModeScreen.tsx:143,156,180`).
- At startup and on settings change, `AppState::rebuild_command_decider`
  (`crates/teletype-desktop/src/lib.rs:490-551`) builds the decider slot:
  `"qwen3-asr+llm"` when both halves are installed, otherwise the NeuDecide
  decider from `models_dir/neudecide-q4` when its files exist, else `None`
  (takes fall through to the plain Needle path). It logs
  `command decider: neudecide` when built (`lib.rs:539`).
- Per-take, `select_decider` (`dictation.rs:6294-6298`) reads the slot and
  applies the language gate.
- NeuDecide availability = `config.json` + `tokenizer.json` exist in the model
  dir AND the `neudecide` cargo feature is on (it is a **default feature** of
  `teletype-desktop`: `crates/teletype-desktop/Cargo.toml:18,31`,
  `neudecide = ["teletype-decider/onnx"]`).

### Artifact and download source

- Artifact: the `neudecide-v1-q4` release of the `teletype-models` GitHub
  repo, 6 shards (config.json, audio_encoder_q4.onnx 22.5 MB,
  tool_encoder_q4.onnx 10.1 MB, decoder_step_q4.onnx 9.9 MB, tokenizer.json,
  tokenizer.model), each with a recorded SHA-256
  (`catalog.rs:546-583`). Backup URL: the upstream
  `huggingface.co/neuphonic/neudecide` repo (Apache-2.0).
- Downloaded to `~/Library/Application Support/com.teletype.app/models/neudecide-q4/`.

### Installed on this machine? **Yes.**

`~/Library/Application Support/com.teletype.app/models/neudecide-q4/` exists
with all six files (verified live with `ls -la`):

```
audio_encoder_q4.onnx  22519972
config.json               1002
decoder_step_q4.onnx    9951225
tokenizer.json          228594
tokenizer.model         124892
tool_encoder_q4.onnx   10090217
```

(Sizes match the catalog shard manifest exactly, `catalog.rs:547-583`.) So on
this machine `NeuDecideDecider::available` is true and the default decider is
active with no user action. (Note: the brain entry at
`PROJECT_BRAIN.md:197` mentions `~/Library/Caches/.../models` for the decider;
on this machine the decider lives in Application Support, the Caches dir holds
only speech models. The app's `models_dir` here is the Application Support
path.)

## 3. How a user works with it best

**Setup (3 steps):**

1. **Enable Command Mode** — Settings → Process → Command Mode tab, flip
   "Enable Command Mode". (The tab exists at
   `ui/src/screens/CommandModeScreen.tsx`; release note:
   `docs/release-notes-v0.2.17.md:68-76`.)
2. **Ensure the decider model is downloaded.** On this machine it already is.
   On a fresh machine, download it from the Models screen: the
   "Command Mode uses: NeuDecide Q4 (decider) · not downloaded" row with a
   Download button (`ui/src/screens/ModelsScreen.tsx:955-979`). It is a
   41 MB download; `polish_capable: false` keeps it out of the polish list and
   the backend refuses to select it as a polish model (brain L042,
   `PROJECT_BRAIN.md:2766`).
3. **Leave the decider on the default** (`neudecide-q4`). The picker on the
   Command Mode screen only matters if you want the `qwen3-asr+llm` hybrid
   (requires Qwen ASR + a local LLM installed); `lfm3-omni` is listed but
   permanently unavailable in this branch.

**Daily use:**

- Hold the Command Mode hotkey (default `Ctrl+Option+Space` on macOS,
  `Ctrl+Alt+Space` on Windows, `commands.rs:1269-1274`), speak a command,
  release. The take is transcribed for context but the **tool call comes from
  the audio**, not the transcript.
- Speak clearly and fully: takes with **less than ~300 ms of speech are
  dropped** by the VAD gate and delivered as plain dictation
  (`command_mode.rs:367,437`).
- **English only.** If the take's detected language is not `en*`/`auto`,
  NeuDecide is skipped and the take falls to the Needle/text path
  (`command_mode.rs:771-776`).
- Use the exact phrasings the menu expects (the screen's "What it can do"
  table lists an example phrase per tool, `CommandModeScreen.tsx:44-107`):
  "open Safari", "set volume to 30 percent", "mute the speaker",
  "email bob at example dot com that lunch is at six",
  "text Sam that I am running late", "meeting with Sam tomorrow at three".
- Know what each tool does before speaking: the same table states the
  outcome truthfully — `send_message` **sends immediately with no
  confirmation**, `compose_email`/`create_calendar_event` open drafts and
  never send/save, `keyboard_shortcut` has no secure-field check,
  `type_text` refuses secure fields (`CommandModeScreen.tsx:44-107`;
  release-notes caveats at `docs/release-notes-v0.2.17.md:516-528`).
- Verify in the **Live Pipeline** panel on the same screen: each take shows
  the COMMAND/DICTATION intent chip, the executed tool + args, and total
  pipeline ms (`CommandModeScreen.tsx:421-470`). The Developer log carries
  the per-take `neudecide: … tool=…` line with the decider's own latency
  (`command_mode.rs:493`).

**Measured accuracy/latency (from the brain and `evals/RESULTS.md`):**

- Gate A (frozen 185-clip TTS corpus, run `command_suite_2026-10-18_c`,
  post tokenizer fix): NeuDecide **61/185 (33.0%)** vs Needle **15/185
  (8.1%)** = **+24.9 pts**, beating the +20-pt gate. open_app 17/30 (56.7%),
  itn_sensitive 24/31 (77.4%); the negative (no-call) column is weak at
  16/50 — the model has a noise prior and fires junk calls on some
  non-command audio (`evals/RESULTS.md`, "Run 2026-10-18 c").
- Parity vs the Python reference: **46/50 (92%)** exact tool calls on TTS
  clips, gate ≥90% met (`evals/RESULTS.md`; also
  `PROJECT_BRAIN.md:199`).
- Latency: **mean 79.2 ms, p50 78.6 ms, p95 87.5 ms** (2 s silence, release,
  intra-op threads=4) under the user-approved **≤ 200 ms** budget
  (recalibrated 2026-10-18; the original 80 ms bound was retired). The brain
  also cites "~46 ms" for the model-side decision in the summary at
  `PROJECT_BRAIN.md:1148`.
- Decision D018 (accepted): NeuDecide is the default decider
  (`PROJECT_BRAIN.md:1137-1156`; ADR at
  `.agents/brain/adr/ADR-015-neudecide-default-decider.md`).

**Caveats a user should know:**

- **Noise false-triggers:** the q4 model fires on ~97% of MUSAN noise clips
  (e.g. `open_app {"app":"text"}` on pure silence). The VAD 300 ms gate is
  the production protection, but a short confident non-command utterance can
  still produce a junk call. The Gate-A human spoken battery is still
  outstanding (`PROJECT_BRAIN.md:1145`).
- **Gate-A risk note (superseded but worth knowing):** an earlier 2026-10-18
  run showed 45/50 parity FAIL and p95 531 ms; root cause was a Rust
  tokenizer transport bug (missing `tools_token_id` prefix, learning L034,
  `.agents/brain/learnings/2026-10-17-neudecide-tools-token-id-transport-bug.md`),
  fixed in `79d89dc` and re-verified (`PROJECT_BRAIN.md:1143,1145,199`).
- **LFM3 finding (L084):** a speech LLM with "native tool calling" (LFM3
  Audio 2B) was benchmarked and rejected for Command Mode: it fires on
  non-commands and adds nothing over the existing fallback; "For English,
  NeuDecide wins on accuracy, latency, and memory"
  (`.agents/brain/learnings/2026-10-28-lfm3-tool-calling-not-command-mode-evidence.md:87-96`).
- **Populated-enum path:** the shipped in-app tool list is the fixed
  14-tool menu; the brain notes the populated app-name enum path (as used in
  one eval run) "has not been verified live end-to-end"
  (`PROJECT_BRAIN.md:1145` point 2).
- **No per-tool confirmation** for anything that "Acts immediately"
  (`docs/release-notes-v0.2.17.md:516`).

## 4. Gaps / recommendations (for the lead, no code changed)

1. **The Command Mode screen shows no accuracy or latency for the decider.**
   The picker rows carry only `installed`/`available`/`reason`
   (`commands.rs:1058-1071`). The measured numbers (33% vs 8.1%, p95 87.5 ms,
   92% parity) live only in `evals/RESULTS.md`. A one-line "measured" note per
   row (the speech catalog already has a `specialty.measured` pattern) would
   let users trust the default.
2. **No "download decider" shortcut from the Command Mode screen.** If the
   model is missing, the picker row just reads "(not installed)"
   (`CommandModeScreen.tsx:407-409`); the only download path is the
   Models screen's "Command Mode uses: …" row
   (`ModelsScreen.tsx:955-979`). A download button on the disabled row would
   close the loop.
3. **The screen's top-level heading is an `<h1>` with hard-coded
   `fontSize: 22` / `fontWeight: 650`** (`CommandModeScreen.tsx:245-252`),
   which violates this repo's stated convention that a tab screen's heading is
   `<h2 className="tt-screen-title">` with no hard-coded font metrics. Flagged
   only; not touched (out of scope).
4. **Language gate is invisible in the UI.** Nothing tells a user that
   commands in a non-English take silently skip NeuDecide and use the weaker
   text path. A hint on the screen ("English commands only for the audio
   decider") would prevent confusion.
5. **`verdict_note` is stale:** the catalog says "not measured on our corpus
   yet" (`catalog.rs:543`) although the corpus runs landed 2026-10-18 and the
   model is the default. Copy fix only.
6. **The LFM3 picker row is dead weight** (always unavailable,
   `commands.rs:1110-1117`). Either remove it or wire it to the real
   llama-server requirement; as-is it teaches users a third option that can
   never work in this branch.
7. **Per-take decider latency is logged (`decider_ms`, `command_mode.rs:498`)
   but the Live Pipeline card only shows total `pipeline_ms`**
   (`CommandModeScreen.tsx:444-449`). Showing the decider's own ~80 ms share
   would make the "fast path" a visible, checkable property.

## 5. Verification (real output)

`git -C /Users/a.sorathiya/Documents/Ali/teletype-wt/t5-neudecide status --short`:

```
?? ui/node_modules
```

exit code 0. The only untracked entry is `ui/node_modules` (pre-existing,
gitignored tooling deps, not a source edit). No tracked file was modified;
`git diff` is empty.

`ls -la /Users/a.sorathiya/Documents/Ali/teletype/tasks/pi-reports/t5-neudecide.md`:

```
-rw-r--r--  1 a.sorathiya  ICM\Domain Users  14587 Nov  8 22:36 /Users/a.sorathiya/Documents/Ali/teletype/tasks/pi-reports/t5-neudecide.md
```

exit code 0.

## 6. Files needed but not owned

None. Everything needed was read-only and in-scope.

## 7. Decisions made

- None affecting code. I treated `evals/RESULTS.md` run
  `command_suite_2026-10-18_c` + brain D018/L042/L084 as the current
  authoritative numbers (they are the latest dated evidence; the 2026-10-18
  pre-fix run is explicitly marked superseded).

## 8. Could not do

- Could not run the live app to watch a real command take (investigation-only
  task; no dev loop required). The "installed on this machine" claim is from
  the on-disk model files matching the catalog manifest, not from a running
  app log.
