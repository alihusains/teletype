# Teletype Progress

Last updated: 2026-10-11

## Where we are

Teletype is a working macOS dictation app (v0.2.6) with AI Polish, personalization,
Command Mode, and in-app updates. The core dictation pipeline is production-ready.
Command Mode is functional but architecturally ahead of the roadmap's intended
pipeline. The Needle/Cactus runtime is not bundled in dev builds, so the
model-backed command path (open arbitrary apps, open files/URLs via Needle)
requires the `cactus` feature to be compiled in.

---

## Roadmap phase status

### Phase 0: Architecture Refactor — DONE

| Task | Status |
|---|---|
| Replace `use_parakeet: bool` with engine enum | Done (`SpeechEngine` enum in `catalog.rs`) |
| Introduce `SpeechEngine` | Done (`Whisper`, `Parakeet`, `Whistle`) |
| Keep `SpeechProvider` as common STT abstraction | Done |
| Move provider-specific init behind providers | Done |
| Refactor `SpeechModelManager` | Done |
| Refactor model catalog | Done (with capabilities metadata) |
| Refactor app state for uniform speech providers | Done |
| Separate model metadata from runtime | Done (`ModelCapabilities` struct) |
| Add model capability metadata | Done (streaming, timestamps, language, keywords, embeddings, max_audio) |
| Add platform compatibility metadata | Done |
| Feature flags for experimental engines | Done (`cactus` feature gate) |

### Phase 1: Whistle Integration — PARTIAL

| Task | Status |
|---|---|
| Vendor Cactus native runtime | Local only (`third_party/needle/`, gitignored) |
| Add Rust FFI bindings | Done (`whistle.rs`, 44 `needle_*` calls) |
| Implement `WhistleProvider` | Done |
| Load `.cact` models | Done (model download + load) |
| Implement transcription | Done (30s window, chunking) |
| Implement language selection | Done |
| Automatic language detection | Done |
| Clean shutdown/unload | Done |
| Model download | Done (with size verification) |
| Checksum verification | Done |
| Model deletion | Done |
| Model status UI | Done (Models screen) |
| macOS ARM64 support | Done |
| Windows x86_64/ARM64 | Not started |
| **Cactus runtime in CI/build** | **Not done** — `libneedle.a`, `needle.h`, `whistle.cact` are gitignored; dev builds lack the `cactus` feature |

### Phase 2: Whistle Optimization — NOT STARTED

Benchmarks, latency tuning, vocabulary test sets.

### Phase 3: Whistle Advanced Features — NOT STARTED

Keyword biasing, word timestamps, speech embeddings.

### Phase 4: Command Mode Foundation — DONE (with divergence)

| Task | Status |
|---|---|
| Voice → command pipeline | Done (spoken + typed) |
| Command UI (Settings panel) | Done (`CommandModeSection.tsx`) |
| Local intent parser | Done (drafts, transforms, open app) |
| Open app (any installed app) | Done (dynamic discovery, `open -a`) |
| Open file/folder/URL | Done (`open_path`, LaunchServices) |
| Draft text (never sends) | Done |
| Transform (6 built-in) | Done |
| Dangerous verb refusals | Done (send, delete, remove, erase, trash, cancel, undo, quit, exit) |
| Shell metacharacter rejection | Done (all tool args) |
| Delivery gate (stale target) | Done |
| Cancel/stale-result guards | Done (op-id based) |
| 24 Command Mode tests | Done (all passing) |

**Divergence from roadmap:** The roadmap says Needle should be the primary command
engine (voice → Whistle → Needle → tool selection). The current implementation uses
a local string-matching parser first (fast, zero-model) and falls back to Needle
for unclassified speech. This is a pragmatic choice for latency but means the
model-backed path (open arbitrary apps, open files) only works when `cactus` is
compiled in.

### Phase 5: Needle 3 Integration — PARTIAL

| Task | Status |
|---|---|
| Add Needle native runtime | Local only (gitignored) |
| Create Rust FFI layer | Done (`whistle.rs`) |
| Load Needle model | Done (when `cactus` feature present) |
| Load tool definitions | Done (static JSON schema in `needle_tools_json()`) |
| Tool-call response parsing | Done (`parse_tool_call`) |
| Structured argument validation | Done (`validate_tool_call`) |
| Multi-step execution loop | **Not done** (single tool call only) |
| Maximum step count | Not done |
| Timeout | Partial (inference timeout via `scaled_timeout`) |
| Cancellation | Done (op-id based) |
| Confidence handling | Not done |
| Tool result injection (agent loop) | Not done |

### Phase 6: MCP Client — NOT STARTED

No MCP client, no stdio/HTTP transport, no tool discovery from external servers.

### Phase 7: Native Teletype Tool Set — PARTIAL

| Tool | Status |
|---|---|
| `open_application` (any app) | Done |
| `open_file` / `open_path` | Done |
| `close_application` | Not done |
| `switch_application` | Not done |
| `get_active_application` | Partial (captured in `CommandContext`) |
| `type_text` | Not done as a tool (dictation does this) |
| `press_key` | Not done |
| `hotkey` | Not done |
| `clipboard_read` | Not done |
| `clipboard_write` | Not done |
| `create_file` / `create_folder` | Not done |
| `move_file` / `rename_file` / `copy_file` | Not done |
| Window management (focus, minimize, etc.) | Not done |

### Phase 8: Open-Source MCP Ecosystem — NOT STARTED

### Phase 9: Composio Integration — NOT STARTED

### Phase 10: Tool Registry — NOT STARTED

Tools are hardcoded in `needle_tools_json()`. No unified registry over
native/MCP/Composio tools. No per-tool metadata (risk level, confirmation,
enabled, authentication).

### Phase 11: Tool Retrieval — NOT STARTED

All tools are always advertised to Needle. No intent-based retrieval.

### Phase 12: Permissions and Safety — PARTIAL

| Task | Status |
|---|---|
| Risk model (READ/LOW_RISK/WRITE/DESTRUCTIVE/PRIVILEGED) | Not done (hardcoded refusals instead) |
| Dangerous verb refusals | Done (parser-level) |
| Shell metacharacter rejection | Done (validator-level) |
| Path/URL validation | Done |
| App name validation (installed apps only) | Done |
| Drafts never sent | Done (by design) |
| Delivery gate | Done |
| Permission engine (structured) | Not done |
| Sensitive action confirmation | Not done |

### Phase 13: Context System — PARTIAL

| Task | Status |
|---|---|
| Active app context | Done (`ApplicationContext` in `CommandContext`) |
| Selected text | Done |
| Caret surroundings | Done (AX read) |
| Record target (delivery gate) | Done |
| App-type-aware polish presets | Done |
| Deeper context (document, conversation) | Not done |

### Phase 14: Personal Memory — PARTIAL

| Task | Status |
|---|---|
| Personalization packet | Done (`packet.rs`) |
| Style profiles (global + per-app) | Done |
| Dictionary (user words) | Done |
| Polish candidates (accept/dismiss) | Done |
| Learned-by-Teletype provenance | Done |
| Insights (Polish counters) | Done |
| Aliases | Not done |
| Learned workflows | Not done |
| Context preferences | Not done |

### Phase 15: Agent Mode — NOT STARTED

No multi-step execution, planning, or agent loop.

### Phase 16: Escalation to Larger Models — NOT STARTED

### Phase 17: Three-Mode UX — PARTIAL

| Mode | Status |
|---|---|
| Dictate | Done (production) |
| Command | Done (functional, Needle-gated for full capability) |
| Agent | Not done |

### Phase 18: Tool Marketplace — NOT STARTED

### Phase 19: Security Architecture — PARTIAL

| Task | Status |
|---|---|
| Prompt injection defenses | Partial (tool validation, no model-side defense) |
| Permission engine | Not done |
| Sandboxing | Not done |
| Secret management | Done (`secrets.rs`, keychain-backed) |
| Audit logs | Partial (log entries, no structured audit) |

### Phase 20: Observability — PARTIAL

| Task | Status |
|---|---|
| Log entries | Done (`log_entry` with levels) |
| Insights screen | Done (activity, vocabulary, dictionary) |
| Usage stats | Done |
| Structured observability (metrics, tracing) | Not done |

### Phase 21: Testing Strategy — PARTIAL

| Task | Status |
|---|---|
| Unit tests (core) | 432 passing |
| Unit tests (desktop) | 121 passing |
| IPC contract tests | 11 passing |
| Journey smoke tests | Done |
| Text integrity tests | Done |
| ITN parity tests | Done |
| Clipboard safety tests | Done |
| Edit watch index tests | Done |
| File transcribe tests | Done |
| Eval harness | Partial (`evals/` directory exists) |
| CI (ci.yml) | Done (build + test) |
| CI (release.yml) | Done (macOS arm64/Intel + Windows, signing, notarization, updater manifest) |

---

## What is NOT in the roadmap but IS built

These are product features that went beyond the roadmap:

- **Quick Add** (dictionary word capture from selected text with confirmation dialog)
- **Polish candidate learning** (explicit accept/dismiss loop, tombstones, provenance)
- **In-app updater** (full Tauri updater lifecycle: check, download with progress, install, restart)
- **Release pipeline** (GitHub Actions: build, sign, notarize, publish, updater manifest)
- **Tray icon + pill overlay** (always-on-top recorder UI)
- **Scratchpad** (temporary text buffer)
- **AutoText** (expansion engine)
- **User packs** (shareable personalization bundles)
- **Edit watch** (track edits for personalization learning)
- **Recovery system** (crash recovery, 60-second bound)
- **Onboarding flow**
- **Keybinds screen** (custom hotkey capture)

---

## Known gaps and risks

1. **Cactus runtime not in CI:** `libneedle.a`, `needle.h`, `whistle.cact` are
   gitignored. Dev builds lack the `cactus` feature, so Needle tool-calling
   (open arbitrary apps, open files/URLs via model) does not work in dev.
   The local rule parser still handles well-known apps, drafts, and transforms.

2. **No MCP client:** The roadmap's core integration strategy (MCP-first) is not
   implemented. All tools are hardcoded in the Needle schema.

3. **No tool registry:** Tools are not in a unified registry with metadata
   (risk level, confirmation, enabled/disabled, source). The roadmap's
   `ToolDefinition` struct is not implemented.

4. **No permission engine:** Safety is enforced by hardcoded refusals and
   validator checks, not a structured permission layer with risk levels.

5. **No agent loop:** Single tool call only. No multi-step execution, no
   result injection back to Needle, no maximum step count.

6. **No confirmation UX:** Side effects (app launch, file open) execute
   immediately. The roadmap calls for confirmation on WRITE/DESTRUCTIVE actions.

7. **Windows support:** Build pipeline exists in CI but no local Windows
   development or testing has been done.

8. **Whistle benchmarks:** No performance data collected (Phase 2).

---

## Test counts (2026-10-11)

| Suite | Count | Status |
|---|---|---|
| `teletype-core --lib` | 432 | all passing |
| `teletype-desktop --lib` | 121 | all passing |
| `teletype-core --test ipc_contract` | 11 | all passing |
| `teletype-core --test journey_smoke` | passing | |
| `teletype-core --test text_integrity` | passing | |
| `teletype-core --test itn_parity` | passing | |
| `teletype-core --test clipboard_safety` | passing | |
| `teletype-desktop --test edit_watch_index` | passing | |
| `teletype-desktop --test file_transcribe` | passing | |
| UI (`tsc --noEmit`) | clean | |
| UI (`vite build`) | clean | |

---

## Architecture decisions (from project brain)

| ID | Title | Status |
|---|---|---|
| D001 | (see PROJECT_BRAIN.md) | accepted |
| D002 | | accepted |
| D003 | | accepted |
| D004 | | accepted |
| D005 | | accepted |
| D006 | | accepted |
| D007 | | accepted |
| D008 | | accepted |
| D009 | | accepted |
| D010 | | accepted |
| D011 | | accepted |
| D012 | | accepted |
| D013 | | accepted |
| D014 | Command Mode: validated Needle tool calling | production-hardened |
| D015 | Polish: explicit accept/dismiss candidate loop | accepted |

ADRs: ADR-010 (unsigned builds ship), ADR-011 (Command Mode on InferenceProvider,
superseded by D014).

---

## Suggested next steps (in priority order)

1. **Bundle Cactus runtime in CI** — get `libneedle.a` + `whistle.cact` into the
   release build so Needle tool-calling works in shipped builds. This unlocks
   the full Command Mode (open any app, open files/URLs via model).

2. **Add the missing native tools** (Tier 1-2 from the roadmap):
   `close_application`, `switch_application`, `type_text`, `press_key`, `hotkey`,
   `clipboard_read`, `clipboard_write`, `create_file`, `create_folder`.
   These go through the same validator + controller path.

3. **Build the Tool Registry** — a `ToolDefinition` struct with id, name,
   description, input_schema, source, risk_level, requires_confirmation, enabled.
   Populate it with the native tools. This is the foundation for MCP,
   permissions, and tool retrieval.

4. **Add the permission engine** — map each tool to a risk level. Auto-execute
   READ/LOW_RISK. Prompt confirmation for WRITE. Mandatory confirmation for
   DESTRUCTIVE. Disabled for PRIVILEGED.

5. **Agent loop** — after a tool executes, inject the result back into Needle
   for a second pass. Cap at 2 steps for Command Mode, 8 for Agent Mode.

6. **MCP client** — stdio transport, tool discovery, JSON schema handling.
   Start with one open-source MCP server (filesystem or git).

7. **Confirmation UX** — fast inline confirmation for WRITE/DESTRUCTIVE
   actions. The pill already has the surface; add a confirm/dismiss pair.

8. **Release gate** — run the full verification, package, smoke test, and
   create the GitHub release via `gh`.
