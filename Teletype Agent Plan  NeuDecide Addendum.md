# Addendum A: NeuDecide for Command Mode (insert after Phase 2, before Phase 3)

> For the AI agent. This extends `TELETYPE_AGENT_PLAN.md`; all of its rules (§0) apply.
> Do this **after Gate 2** (Command Mode trigger bugs fixed). It **replaces Whistle → Needle as the default Command Mode decider**. It does **not** touch dictation.

## A.0 Facts (verified 16 Oct 2026)

| Item | Value | Source |
|---|---|---|
| What it is | Audio + JSON tool list in, tool call out. **No transcript.** | [HF model card](https://huggingface.co/neuphonic/neudecide) |
| Size | 55.5M params, 43 MB. Three ONNX graphs: `audio_encoder_q4.onnx`, `tool_encoder_q4.onnx`, `decoder_step_q4.onnx`, plus `tokenizer.json` / `tokenizer.model` (8,192 SentencePiece BPE) and `config.json` | HF API file list |
| License | Apache-2.0 (code and weights) | [GitHub](https://github.com/neuphonic/neudecide) |
| Access | HF repo is **gated (auto-approve)**: a form asks for affiliation and role. The app can't download it anonymously. | HF API (`gated: auto`) |
| Limits | English only, 16 kHz, ≤ 30 s audio, ≤ 1,536 tool tokens, ≤ 128 output tokens, ≤ 10 tools recommended | model card |
| Accuracy (SLURP, 10 tools, vendor-run) | NeuDecide 75.5% tool / 37.5% exact; **Whistle → Needle 3: 25.6% / 7.1%** (what Teletype uses today) | model card |
| Weakness | Free-form arguments (names, queries) are weaker than ASR cascades; **fires on noise**, with no confidence score | model card |
| Speed | 46 ms per call on an M3 (q4), 159 ms load, peak RAM 146–174 MB | model card |
| Teletype's prior decision | `docs/research/neudecide.md` (10 Oct): "Do not integrate now" because it was Python-only and alpha | repo |

**Why integrate now:**
- Plan Phase 3 already adds ONNX Runtime to the app (sherpa-onnx), so the "native ONNX dependency" blocker in the 10 Oct note goes away.
- The ONNX graphs can be run directly from Rust. No Python is needed.

## A.1 Design: hybrid decider (tool from audio, arguments from transcript)

```
Command take audio ──┬──► NeuDecide (46 ms) ─► tool name + enum args ─┐
                     └──► Parakeet (same audio, parallel) ─► transcript ─┤
                                                                       ▼
                     CommandResolver: tool from NeuDecide; free-form args (query, recipient, body)
                     extracted from the transcript by rules → fallback local LLM (JSON schema)
                                                                       ▼
                                   existing safety checks (allowlist, no shell metachar, no send/delete)
```

**Rules:**
- **Command takes only.** Use it after the Command hotkey, the wake phrase or the tray item. **Never** use it to classify every dictation (it fires on noise and has no confidence score).
- **Always include a `no_action` tool.** If it's chosen, or `generate` returns `[]`, flash "Didn't catch a command" and do nothing.
- **Gate with Silero VAD first.** If speech is < 300 ms, skip.
- **Keep the tool list ≤ 10** and put fixed choices in `enum`, such as the app names from the installed-apps list (top 30 by usage), style names and transform names.

## A.2 Tool set (v1, ≤ 10)

| Tool | Args | Arg source |
|---|---|---|
| `open_app` | `app` (enum: top installed apps) | NeuDecide |
| `web_search` | `query` (string) | transcript (strip the trigger words) |
| `transform_selection` | `transform` (enum: shorten, expand, formal, casual, fix_grammar, translate, summarize, bullet_list) + `language` (enum, optional) | NeuDecide |
| `insert_autotext` | `snippet` (enum: user snippet names, top 30) | NeuDecide |
| `switch_style` | `style` (enum: user styles) | NeuDecide |
| `draft_message` | `recipient` (string), `body` (string) | transcript + LLM extraction |
| `edit_text` | `action` (enum: undo_last_dictation, delete_last_sentence, new_line, new_paragraph, select_all) | NeuDecide |
| `media` | `action` (enum: play_pause, next, previous, volume_up, volume_down, mute) | NeuDecide |
| `paste_last` | none | NeuDecide |
| `no_action` | none | NeuDecide |

Serialize the tools **exactly like Python's `json.dumps` default**: separators `", "` and `": "`, key order preserved, `ensure_ascii=True`. The model was trained on that exact format. Write a Rust serializer for it and test it byte-for-byte against fixtures produced by the Python package.

## A.3 Implementation

### A.3.1 Model delivery
- Because of the gating, **re-host** the four files in your own GitHub release (`teletype-models` repo, tag `neudecide-v1-q4`). Apache-2.0 allows redistribution. Include `LICENSE` and a NOTICE with attribution.
- Read the gating terms once while logged in and record them in `THIRD_PARTY_MODELS.md`. If they add any restriction on redistribution, stop and ask the owner.
- Add a catalog entry `neudecide-q4` (≈43 MB, SHA-256 pinned) under a new `ModelKind::Decider`. Download it on demand when the user enables Command Mode.

### A.3.2 Rust runtime: `crates/teletype-decider/`
- Dependencies:
  - [`ort`](https://crates.io/crates/ort) (ONNX Runtime bindings, MIT/Apache-2.0)
  - `tokenizers` (Apache-2.0) to load `tokenizer.json`
  - `ndarray`, `serde_json`
- **ONNX Runtime linking:** sherpa-onnx (Phase 3) also bundles ONNX Runtime, and two static copies will collide. Use **one shared ORT**:
  - Build `ort` with `load-dynamic`, pointing to the same `onnxruntime` dylib/dll that sherpa-onnx ships, **or** build sherpa-onnx with `shared` and point `ort` at it.
  - Add a CI test that loads both in one process.
- **Port the inference loop from the Python package** (Apache-2.0, so porting is allowed; keep attribution in the file header). Read `neudecide/` in the PyPI sdist and mirror it exactly:
  1. Audio: mono f32, 16 kHz, `[-1, 1]`, truncated to 30 s.
  2. `audio_encoder(waveform)` → frames at 12.5 Hz.
  3. Tokenize the serialized tool list (truncate at 1,536 tokens, with a warning).
  4. `tool_encoder(frames, tool_tokens)` → cross-attention K/V.
  5. Greedy loop over `decoder_step(prev_token, self_kv_cache, cross_kv)` up to 128 tokens, stopping at EOS.
  6. Detokenize and parse the JSON call list (same format as the Python output: `[{"name":…, "arguments":{…}}]`).
  7. Take tensor names, shapes, BOS/EOS ids and the prompt format from `config.json` and the Python code. **Don't guess.**
- Session options: `intra_op_num_threads = 1`, `GraphOptimizationLevel::Level3`. Create the sessions once and keep them warm while Command Mode is enabled.
- **API:**
  ```rust
  pub struct ToolCall { pub name: String, pub arguments: serde_json::Map<String, serde_json::Value> }
  pub struct NeuDecide { /* sessions + tokenizer */ }
  impl NeuDecide {
      pub fn load(dir: &Path) -> Result<Self>;
      pub fn generate(&self, pcm16k: &[f32], tools_json: &str) -> Result<Vec<ToolCall>>;
  }
  ```

### A.3.3 Wiring (`crates/teletype-desktop/src/command_mode.rs`)
- New `CommandDecider` trait with three implementations, chosen in this order:
  1. `NeuDecideDecider` (default when the model is installed and the language is English)
  2. existing Needle (if the `cactus` feature is on)
  3. the LLM JSON-schema fallback (plan P2.7)
- **Pipeline:**
  - Start NeuDecide and the final ASR **in parallel** on the command take.
  - The `CommandResolver` fills free-form args from the transcript:
    - `web_search.query`: strip leading trigger phrases ("search (the web) for", "google", "look up").
    - `draft_message`: rules first ("to X saying Y" / "email X that Y"), then the LLM fallback with a JSON schema.
- **Non-English UI language or detected language:** skip NeuDecide and use the fallback.
- Add trace fields: `decider`, `decider_ms`, `tool`, `args_source`.

## A.4 Tests

| Test | Type | Pass criteria |
|---|---|---|
| `json_dumps_compat` | unit | The Rust tool serializer is byte-identical to Python `json.dumps` on 20 fixture tool lists (generated once with the PyPI package, committed) |
| `parity_with_python` | integration (CI macOS + Windows) | On 50 fixture WAVs, Rust `generate` output is identical to the Python package's output (fixtures stored as JSON) |
| `noise_does_not_trigger` | integration | 20 noise/silence clips with VAD on give `no_action` or are skipped ≥ 95% of the time |
| `command_suite_accuracy` | eval | A new `evals/commands/` set of ≥ 150 clips (5+ voices) covering the tool set. Report tool accuracy, exact match and arg accuracy for NeuDecide-hybrid vs Needle vs the LLM fallback. **NeuDecide-hybrid must beat Needle on tool accuracy by ≥ 20 pts.** If not, keep Needle as the default and record why. |
| `latency` | bench | `decider_ms` p95 ≤ 80 ms on an M1, ≤ 150 ms on a 4-core x64 laptop; command end-to-end (release → action) p95 ≤ 400 ms |
| `ort_and_sherpa_same_process` | integration | Both load and run in one process with no symbol clash |
| `fuzz_decoder_output` | fuzz | The JSON parse of decoder output never panics on arbitrary token strings |

## A.5 Gate A
- [ ] All A.4 tests pass. `evals/RESULTS.md` has the command-suite table.
- [ ] Manual: "open Safari", "make this shorter" (with selection), "search for rust async traits", "new paragraph", "pause music" and "email Sara saying I'll be late" all work. Random noise with the Command hotkey held does nothing.
- [ ] Update `docs/research/neudecide.md` with a new verdict and `docs/decisions/` with a D-number for "NeuDecide default decider".

## A.6 What NeuDecide is NOT for (do not do these)
- Don't use it for transcription or dictation accuracy (it outputs no text).
- Don't use it as an always-on "is this a command?" classifier on every dictation (it fires on noise and has no confidence score).
- Don't use it for non-English commands.
- Don't confuse it with **TinyDecide** (`TheREZOR/TinyDecide`, already vendored), a 6 MB text classifier. Keep TinyDecide only for cheap text routing; an independent bench found it near chance on tasks needing inference ([lev](https://github.com/jlt-commons/lev)).
