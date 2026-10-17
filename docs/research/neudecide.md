# NeuDecide — Audio-to-Tool-Call Model

## Overview

NeuDecide is a small ONNX model (~43 MB) that takes mono 16 kHz audio and
directly outputs a JSON tool call (tool name + enum arguments) in ~46 ms.
It is the hybrid decider for Command Mode: the model picks the tool and
enum args; the transcript fills free-form args.

- Source: https://huggingface.co/neuphonic/neudecide
- License: Apache-2.0
- Model size: ~43 MB (Q4 quantized)

## Architecture

1. **Audio encoder** — mono f32 16 kHz PCM (≤ 30 s) → frames at 12.5 Hz.
2. **Tool encoder** — cross-attention over frames + tokenized tool list
   (≤ 1536 tokens) → K/V context.
3. **Decoder (LM)** — greedy decode ≤ 128 tokens until EOS.
4. **Output** — a JSON list of tool calls:
   `[{"name": "open_app", "arguments": {"app": "terminal"}}]`.

The tool JSON is serialized with Python `json.dumps` defaults
(separators `", "` / `": "`, `ensure_ascii=True`) — the model was trained
on that exact format. The Rust crate reproduces it byte-for-byte
(`crates/teletype-decider/src/tool_json.rs`).

## v1 Tool Set (10 tools)

| Tool | Args |
|---|---|
| `open_app` | `app` (enum, injected) |
| `web_search` | `query` (string) |
| `transform_selection` | `transform` (enum), `language` (enum, optional) |
| `insert_autotext` | `snippet` (enum, injected) |
| `switch_style` | `style` (enum, injected) |
| `draft_message` | `recipient`, `body` (strings) |
| `media` | `action` (enum: play_pause, next, previous, volume_up, volume_down, mute) |
| `edit_text` | `action` (enum: undo_last_dictation, delete_last_sentence, new_line, new_paragraph, select_all) |
| `paste_last` | — |
| `no_action` | — |

## Rust Implementation Status

**Crate:** `crates/teletype-decider/` (workspace member).

| Component | Status |
|---|---|
| `tool_json.rs` — Python `json.dumps` compat serializer | ✅ Done, 20-fixture byte-identical compat test |
| `tools.rs` — v1 tool definitions as data | ✅ Done |
| `parse.rs` — `parse_tool_calls` (strict, no-panic, proptest-covered) | ✅ Done |
| `runtime.rs` — `NeuDecide`, `SessionRunner`, `run_inference` 7-step loop | ✅ Done (greedy loop unit-tested with fake runner) |
| `OrtRunner` (ONNX via `ort`) | ⚠️ Behind `onnx` feature (default OFF); tensor names/shapes marked `// UNVERIFIED` |
| Model files + SHA-256 | ⚠️ **Re-host pending (human step)** — upload 4 files to `teletype-models` release `neudecide-v1-q4`, record SHA-256 |
| `ModelKind::Decider` catalog entry | ⬜ Out of scope (needs re-hosted file + SHA) |
| Desktop wiring (`command_mode.rs`) | ⬜ Pending — A.3.3 unit (selection order + trace fields specified in ADR-015) |

### Unverified assumptions (model not yet obtained)

The Python package `neudecide` could not be downloaded (gated/offline),
so the following are assumed from the task spec and marked
`// UNVERIFIED:` in `runtime.rs`:

1. `config.json` key names: `audio_encoder`, `tool_encoder`, `decoder`,
   `tokenizer`, `bos_token_id`, `eos_token_id`, `sample_rate`, `frame_rate`.
2. ONNX input tensor names: `"input"` (audio encoder), `"frames"` /
   `"tool_tokens"` (tool encoder), `"input_ids"` / `"kv"` (decoder).
3. `bos_token_id = 0`, `eos_token_id = 2` (defaults).
4. Greedy decode returns argmax of the last position's logits.
5. `ort` 2.x API shape for `Session::builder().commit_from_file()`.

When the real package is obtained, read its model code + `config.json`
and reconcile these.

## Decision (A.5 verdict)

**D015 / ADR-015 — NeuDecide is the default Command Mode decider.** The
2026-10-10 provisional verdict ("do not integrate now": Python-only, alpha,
model unobtainable) no longer applies: the decoder now runs through the
committed Rust ONNX crate (`teletype-decider`, `onnx` feature, default OFF),
the tool JSON is byte-compatible with the Python reference, and the parse is
fuzz-covered. Selection order at runtime: NeuDecide (English + model present)
→ Needle (`cactus` feature) → LLM JSON fallback (`command_llm.rs`). NeuDecide
is skipped for non-English and never runs on dictation takes. Free-form args
(query, recipient, body) come from the transcript by rules first, LLM second.

**Still pending (Gate A):** gated-source re-host to the `teletype-models`
release `neudecide-v1-q4` with recorded SHA-256; reconciling the
`// UNVERIFIED` ONNX tensor names above against the real package; the
model-dependent A.4 rows (parity-with-python 50 WAVs, latency bench,
same-process ORT/sherpa) and the A.5 manual spoken-command battery, all
blocked on the re-host. The command-suite table in `evals/RESULTS.md` now
stands (2026-10-17, 185-clip run): Needle baseline 15/185 (8.1%), open_app
8/30 strict; the plan's ≥20-pt NeuDecide-vs-Needle comparison runs as soon as
the model is provisioned. Until the re-host lands, availability is `false`
and every command take falls through to the existing deciders — no
user-visible regression.
