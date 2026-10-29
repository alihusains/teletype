---
id: L084
title: A speech LLM's tool-calling demo is not command-mode evidence; LFM3 fires on non-commands and adds nothing over the existing text fallback
status: verified
type: measurement
area: command mode / eval / model selection
agent: main session (Qoder)
tool: llama.cpp b11960 llama-server (--jinja), scripts/eval/bench_lfm3_tools.py, s1-mini via the bundled b11488 server
date: 2026-10-28
---

# A speech LLM's tool-calling demo is not command-mode evidence; LFM3 fires on non-commands and adds nothing over the existing text fallback

## Problem

LFM3-Audio-2B's card advertises native tool calling whose "requests may be
spoken or written" (audio in, `<tool_call>` out). After the ASR benchmark
rejected it as a dictation engine (L083), the open question was whether that
tool-calling capability earns it a place in Command Mode, especially for
non-English takes where NeuDecide is skipped.

## Claims tested

- Audio-to-tool without an ASR step, in English and Polish.
- The card's 18-language coverage as a way to extend Command Mode beyond
  English.
- Adjacent features: speech translation, LiveASR streaming, TTS, audio
  understanding, interleaved voice chat.

## Investigation

1. Started `llama-server` b11960 with `--jinja` and the LFM3 Q4_K_M + mmproj
   pair; confirmed the GGUF chat template renders tools in the model's
   `<tool_call><function=...><parameter=...>` format and llama-server parses
   them into OpenAI `tool_calls`.
2. Added `scripts/eval/bench_lfm3_tools.py`: sends each clip as audio with the
   app's shared 13-tool schema (`COMMAND_TOOLS_JSON` from `needle.rs`) and
   scores with the Rust command-suite rules (tool name + normalized args; a
   null-expected clip is correct only when no call is produced).
3. Ran the frozen 185-clip command corpus in three configurations: the card's
   tool prompt; the card prompt plus a `none` escape tool; and the app's own
   `COMMAND_SYSTEM_PROMPT` plus `none`.
4. Generated eight Polish clips (`say -v Zosia`) covering open-app, open-URL,
   timer, mute, message, and two non-commands; also transcribed them with
   `Perform ASR in Polish.` to separate hearing errors from mapping errors.
5. Ran the app's actual last-resort fallback (s1-mini, `enable_thinking:false`,
   `json_schema` over the same 13 tools plus `none`) on the Polish command
   transcripts, via the bundled b11488 server.
6. Spot-checked speech translation on four `fleurs-pl` clips and text
   translation on the same references with s1-mini.

## Evidence

Command suite, best score per configuration (`strict / loose` out of 185):

| configuration | strict | loose (args case-insensitive) | open_app loose | non-commands with an action | p50 / p95 |
|---|---|---|---|---|---|
| card prompt | 36 | 57 | 21/30 | 47/50 | 547 / 679 ms |
| card + `none` tool | 23 | 46 | 23/30 | 47/50 | 568 / 722 ms |
| app prompt + `none` | 28 | 52 | 25/30 | 48/50 | 581 / 850 ms |
| NeuDecide (shipping English decider) | **61** | n/a | 26/30 effective (8 strict + 18 case-only) | protected by its own no-action output | **87.5 ms p95** |

- The `none` tool and the app's "If nothing fits, return no tool call" clause
  did not teach the model to decline: non-command clips still produced calls
  (open_app(calendar) for "the board meeting moved to the third floor",
  set_timer(2 weeks) for "the project would take a few more weeks").
- Latency is 6-10x NeuDecide; memory is 1.70 GB RSS versus NeuDecide's 43 MB
  model, on top of the ASR engine.
- Polish, 6 commands / 2 non-commands: open Safari, open Terminal, open
  github.com and "timer 5 minutes" mapped correctly; "wycisz dźwięk" (mute)
  produced open_app(windows) or type_text("Hello"); "napisz do Marka..."
  produced type_text("I am busy"), losing the recipient and the send action.
  Both non-commands produced actions. ASR on the same clips mangled 2/8
  ("Wycisz dźwięk" -> "Wyczyszdwięk"), so one-hop audio does not remove the
  ASR-error problem.
- Existing fallback baseline (s1-mini on the correct Polish transcripts):
  open_app x2, set_timer and open_url all mapped correctly; mute and message
  returned `none` instead of a wrong action; both non-commands returned
  `none`. This is the third decider path the app already runs for non-English
  takes, so LFM3 does not add command coverage over it and is less safe.
- Speech translation PL -> EN: 4/4 spot checks were accurate and fluent (one
  minor tense drift in fleurs-pl-0003). s1-mini, the resident recommended
  polish model, cannot translate: it echoes the Polish input even with an
  explicit translation system prompt.
- LiveASR is English-only and its dictation WER already loses to Ultra; TTS,
  audio understanding and voice chat have no surface in a push-to-talk
  dictation app.

## Root cause

LFM3's tool-calling training treats a tool list as a menu to choose from, not
as an optional escape: with tools present it almost always emits a call, and a
`none` tool in the schema does not change that. Its audio front end is also
weaker than the dedicated ASR, so "skip the ASR step" trades a known error
rate for a worse one. The app's text fallback has the two properties LFM3
lacks: a grammar-forced `none` branch and a 0.46 GB resident model that is
already loaded for polish.

## Verdict

No Command Mode integration and no catalog row. For English, NeuDecide wins on
accuracy, latency, and memory. For non-English, the existing s1-mini fallback
already covers the basic commands, declines the ones it cannot do, and is
already resident. The only genuine capability gap LFM3 fills is speech
translation (audio -> English text), which no current engine or resident model
provides, and which is not a product surface today (the transform prompts
explicitly say "never translate"). If translation becomes a feature, evaluate
it as its own opt-in path on a real translation corpus, not on four clips; the
same license ($10M revenue cap), llama.cpp pin bump (b11488 -> b11960+) and
1.70 GB RSS costs apply.

## Verification

Command-suite reports:
`evals/runs/command_suite_lfm3_2026-10-28/{report,report-none,report-none-appprompt}.json`.
Runner: `scripts/eval/bench_lfm3_tools.py`. Polish clips:
`.tmp/asrbench/pl-commands/` (regenerable with `say -v Zosia`). s1-mini
baseline: bundled `llama-server` b11488, `enable_thinking:false`,
`json_schema` over the 13 tools plus `none`, port 8195. Register row and run
log in `.agents/brain/asrs.md`.

## Reusable lesson

A vendor's tool-calling demo proves the format works, not that the model is a
usable decider. Measure three things before integrating: accuracy on real
command utterances, false triggers on non-commands (the safety property), and
the existing last-resort path's score on the same inputs. If the model cannot
decline, gate it behind an already-armed command take or an intent classifier,
never behind plain dictation. And check what the resident models already do:
a cheaper fallback that says "none" beats a bigger model that acts wrongly.
