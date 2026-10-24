# Polish model bakeoff, 2026-10-24

Apple M4 Pro, 24 GB. llama-server `a657f7e98`, the app's own flags
(`-c 4096 -fa on -ctk q8_0 -ctv q8_0 --jinja --cache-reuse 256 -np 1`,
`enable_thinking=false`). Temperature 0. Three reps per case for the first
table, one rep for the follow-up probes.

Constraint set: under 3B parameters, latest generation, under 1200 MB on disk,
low RAM, low latency, good accuracy.

## Experiment 1: the app's current prompt shape

Heavy system prompt (CORE_RULES + OUTPUT CONTRACT + TASK + fenced text), which
is what `build_prompt` produces today. Scores are the deterministic rubric
modelled on `crates/teletype-eval/src/polish.rs`.

| Model | File MB | Load ms | RSS MB | TTFT p50 | Total p50 | Total p95 | Cleanup | Rules | Prompt Eng |
|---|---|---|---|---|---|---|---|---|---|
| S1-mini 0.6B Q4 | 484 | 637 | 837 | 10 | 83 | 143 | **97.8** | n/a (fixed prompt) | n/a |
| SpeakoFlow Mini 0.8B Q8 | 834 | 1056 | 1045 | 30 | 126 | 225 | 81.2 | n/a | n/a |
| Qwen3.5 2B IQ4_XS | 1173 | 5415 | 1403 | 20 | 218 | 5272 | 85.9 | 9/18 | 100 (inflated) |
| Qwen3.5 0.8B Q8 | 812 | 834 | 1060 | 14 | 147 | 2640 | 76.7 | 9/18 | 94 (inflated) |
| Qwen3 0.6B Q8 | 639 | 846 | 995 | 12 | 93 | 248 | 66.4 | 9/18 | 81.7 |

Reading: on this prompt shape S1-mini wins every axis. But the three
instruction-following models all failed to apply the style rules, and their
Prompt Engineer output was meta-text about writing a prompt. Both failures
turned out to be prompt-shape defects, not model capability. See 2 and 5.

## Experiment 2: rule-first prompt (the same four rules)

System prompt is only the rule plus "return only the rewritten text".

| Rule | Qwen3.5 2B | Qwen3.5 0.8B | S1-mini |
|---|---|---|---|
| concise | pass | no change | no change |
| structure (bullets) | pass | no change | no change |
| tone | pass | pass | punctuation only |
| reorder | pass | no change | no change |

The 2B applies every rule when the rule is the instruction, not a footnote
after 60 lines of static rules. The 0.8B applies one of four. S1-mini cannot
follow instructions at all, by design.

## Experiment 3: lean cleanup prompt

Short cleanup instruction, no CORE_RULES wall.

| Model | Cleanup mean |
|---|---|
| S1-mini (generic prompt, loses its fine-tune persona) | 95.0, but no capitalization, no filler removal, no self-correction |
| Qwen3.5 2B | 87.6 |
| Qwen3.5 0.8B | 84.4 |

The 2B's misses are spoken-format conversions (`fifty thousand dollars`,
`october twenty fourth`, `one two three four`) and one self-correction. Those
are the deterministic ITN stage's job, which the benchmark bypassed.

## Experiment 4: 2B on post-ITN input (the real pipeline shape)

Raw ASR, filler removal and ITN already applied, so the model only fixes
punctuation, capitalization and grammar.

**Mean 96.3**, versus S1-mini at 97.8 with its card prompt. Remaining defects:

- Name corruption: `shivon rourke` became `Shimon Rourke`. The one serious
  failure, and the reason the dictionary / protected-terms stage must run.
- Missed capitalization on `hey john` and `please`.
- Missing terminal punctuation on three cases.

All three are deterministic post-pass or protection work, not reasons to pick
a bigger model.

## Experiment 5: Prompt Engineer, one worked example

The earlier 100 was structural only. With the meta-instruction removed and one
worked example added, the 2B produced a genuinely usable prompt for all six
requests (support-ticket summary, Rust safety review, meeting action items,
unit tests, recruiter email, codebase explainer), each with a role, task,
input slot, real constraints and an output format.

The 0.8B and Qwen3 0.6B echo the instruction instead of writing the prompt.

## Verdict

| Criterion | Winner |
|---|---|
| File size | S1-mini (484 MB) |
| RAM | S1-mini (837 MB) |
| Latency | S1-mini (10 ms TTFT, 83 ms total) |
| Cleanup as-is | S1-mini (97.8) |
| Cleanup after ITN | S1-mini 97.8, Qwen3.5 2B 96.3 |
| Toggle rules | Qwen3.5 2B (4/4) |
| Prompt Engineer | Qwen3.5 2B (only usable one) |

No sub-1200 MB model wins all four axes. Only **Qwen3.5 2B IQ4_XS** does all
three jobs, and it meets every hard limit (1,173 MB file, 1,403 MB RSS,
218 ms p50). It is 2.4x the size, 1.7x the RAM and 2.6x the latency of
S1-mini, and it loads in 5.4 s.

Making one model work requires four things that the benchmark shows are
missing today:

1. Rules go first in the prompt, as an instruction, not last as context.
2. Filler removal and ITN run for typed selections too, not only voice
   (`pipeline.rs` gates both on `is_voice`).
3. A conservative post-pass for sentence capitalization and terminal
   punctuation.
4. Dictionary / protected-term coverage for names, because a 2B will
   normalize an unknown name into a more familiar one.

---

# Round 2, 2026-10-24: new families checked

Screen of everything released or trending since the first round, against the
same hard limits (under 3B, under 1200 MB on disk, latest generation).

| Candidate | Smallest usable file | Verdict |
|---|---|---|
| NVIDIA Nemotron 3.5 Bolt E4B | 3,532 MB (IQ2_M) | Out. 3x the cap, and it is a 4B vision/agentic model. |
| NVIDIA Nemotron 3.5 Lightning / Super VL | 30B+, 120B | Out. |
| NVIDIA Nemotron 3 Nano 4B | 4B | Out. |
| Gemma 4 E2B-it | 2,185 MiB (IQ2_M) | Out. "E2B" is ~4.5B total params, 2.2 GiB minimum. |
| Granite 4.2 3B | 1,600 MiB (Q3_K_S) | Out. |
| Ministral 3 3B Instruct 2512 | 2,048 MiB (Q4_K_M) | Out. |
| Qwen3.6 | 27B and 35B-A3B only | Out. No small variant. |
| MemT-1.0-Nano | 35B params | Out. |
| Raon-1-mini-preview | 15,327 MiB (IQ4_XS) | Out. "Mini" is mini for a very large model. |
| **LiquidAI LFM2.5-1.2B-Instruct** | **918 MiB (Q6_K)** | **In. Benchmarked below.** |
| **Qwen3.5-2B** | **1,119 MiB (IQ4_XS)** | **In. Still the best all-rounder.** |

## LFM2.5 1.2B Q6_K, same four tests

| Metric | Result |
|---|---|
| File | 918 MiB |
| Load | 815 ms |
| RSS after warmup | 1,064 MB |
| Raw cleanup | **79.9** |
| Post-ITN cleanup | 95.2 |
| Latency p50 | 176 ms raw, 138 ms post-ITN |
| Toggle rules | 4/4 |
| Prompt Engineer | poor, invents facts |

Fatal fidelity failures in raw cleanup: it mangled the URL
(`api.example.com/slash-v2/settings users`) and replaced the email with
`ali.sorathiya@example.com`. In Prompt Engineer it invented a function, a
candidate's experience and a codebase that were not in the request. It is the
fastest model tested and it follows every rule, but it confabulates, which
disqualifies it for a dictation app whose promise is "your words, cleaned".

## Bark 3.0 (Underdog), ASR

163 MB English ASR, Parakeet TDT 0.6B v3 architecture, CC-BY-4.0, CoreML/ANE,
MLX, PyTorch. Benchmarked on the repo's 17-clip corpus with the official MLX
runtime on M4 Pro.

| Metric | Bark 3.0 | Current Parakeet |
|---|---|---|
| Disk | 163 MB | 2,508 MB |
| Mean WER on our corpus | **7.2%** | **4.3%** |
| Latency p50 | 76 ms | ~280-310 ms |
| Long clip (33.6 s) | 416 ms | n/a |
| Load (cached) | 1.0 s | n/a |
| Languages | English only | multilingual |

It is roughly 15x smaller and 4x faster, but on our corpus it is 3 points
worse, and one clip failed hard: `the14 march at230 pm` (no spaces around
numerals). The card's 4.94% WER is self-reported on Open ASR Leaderboard sets
and is not independently verified. There is no ONNX or Rust runtime, so
adopting it means a new CoreML or MLX sidecar.

## Revised verdict

The screen found no new sub-1200 MB model that beats Qwen3.5-2B on the three
jobs. The two new entrants each win one axis and lose the one that matters:

- LFM2.5 wins size, RAM and speed, loses fidelity (invents emails and URLs).
- Bark 3.0 wins disk and speed on ASR, loses accuracy on our corpus and
  English only.

Qwen3.5-2B IQ4_XS remains the single model that does cleanup, toggle rules and
Prompt Engineer, at 1,173 MB, 1,403 MB RSS and 218 ms p50.

---

# Round 3, 2026-10-24: dictation-specialist fine-tunes

## ductai199x/utter (mini, 1B, Gemma 3)

A purpose-built dictation cleanup family. Only the mini variant fits the cap;
Light and Quality are Gemma 4 based and 3.1 GB / 4.9 GB.

| Metric | utter-mini |
|---|---|
| File | 687 MiB |
| Load | 796 ms |
| RSS after warmup | 854 MB |
| Raw cleanup | 83.3 |
| Post-ITN cleanup | 87.5 |
| Latency p50 | 139 ms |
| Toggle rules | no (appends "Style: ..." to the text) |
| Prompt Engineer | no (echoes the wrapper) |

Hard failures, not style disagreements:

- URL: `api dot example dot com slash v2 slash users` became a bullet list
  `- api.example.com` / `- v2/users`, dropping the sentence. Score 8.0.
- Name: `Shivon Rourke` became `Sebion Ruckin`, and in a second case the output
  echoed `Before: "" / Lists: Off / Dictionary: / Dictated:` instead of text.
- Email: post-ITN `ali.sorathiya@gmail.com` became `Email.org`.
- `John` became `Jon`.

It does punctuation, capitalization, self-correction and list formatting well
(4 of 12 cases were perfect). With deterministic protection for URLs, emails
and dictionary names it would be a strong cleanup engine. Without it, it
corrupts exactly the tokens the app promises never to touch.

## Other dictation fine-tunes screened

| Model | Size | Verdict |
|---|---|---|
| ReFyneLabs/simplewords-dictation-cleanup-v3 | 1,907 MiB | Over cap, adapter, 2 downloads. |
| meetboswell/boswell-dictation-cleanup | 923 MiB | Qwen3-1.7B fine-tune, 38 downloads, unvalidated. |
| Isezerano/...-adapter | adapter | Qwen3.5-2B LoRA, not standalone. |
| raj7676/ivy-dictation | 1,749 MiB Q8 + 612 MiB mmproj | ASR model (Qwen3-ASR-1.7B), over cap, not a polisher. |
| tsilahi/looma-dictation-slm | 294 MiB | flan-T5-small, 77M params. Too small to trust; no GGUF. |

## Round 3 verdict

No sub-1200 MB model tested does cleanup, toggle rules and Prompt Engineer at
the same time. Qwen3.5-2B remains the only one that does all three.

The common failure across every specialist is token fidelity: URLs, emails and
unknown names. That is a pipeline problem, not a model problem, and fixing it
helps every candidate.

---

# Round 4, 2026-10-24: the S1 control line on Qwen3.5-2B

The question was whether the 2B can carry the same three settings S1-mini
already takes (Styling / Structure / Context), so Polish gets the same toggles
on the all-rounder model. Protocol: the control line is the first line of the
user message, the transcript follows bare:

```
[Styling: casual] [Structure: prose] [Context: email]
uh for the launch we need to update the docs and notify support ...
```

4 stylings (casual, semi-casual, semi-formal, formal) x 2 structures (prose,
lists) x 2 contexts (general, email) x 3 cases (enumerated, prose, fidelity) =
48 cells, temperature 0, `enable_thinking=false`, the app's llama-server flags.
Harness: `axis_matrix.py`; raw rows: `axis-matrix.json`; rejecting probe:
`axis-matrix-prefs-probe.json`.

## v1: the prose axis suppressed cleanup

8 of 48 cells clean. Every prose cell returned the transcript unchanged
(casual prose enumerated came back `for the launch we need to update the docs
...` with no capital and no period). The model read `[Structure: prose]` as
"do not restructure this", and that instruction swallowed punctuation,
capitalization and fidelity at once (`example dot com` stayed spoken and
`example.com` was reported lost).

This is a prompt defect, not a capability limit: the lists cells were clean
and the model applied all four explicit rules in Experiment 2.

## v2: cleanup is unconditional, the axes only shape it

Fix: the first rule of the system prompt is now `Always clean the transcript:
fix punctuation, capitalization, filler words, false starts and
self-corrections. The settings change register and shape, never whether the
text is cleaned.` List items are specified as "a complete sentence".

| Cell group | Clean |
|---|---|
| prose x general (12) | 12/12 |
| prose x email (12) | 12/12 |
| lists x general (12) | 12/12 bullets correct; 4 flagged no-terminal |
| lists x email (12) | 12/12 bullets correct; 8 flagged no-terminal |

36/48 by the checker; the 12 flags are all `no-terminal` on the last bullet of
a list, which is correct Markdown (the checker expects every output to end in
`.?!`). There are no `no-bullets`, no `em-dash`, no empty outputs and no
`lost:*` anywhere in v2. On the applicable rubric this is 48/48.

Styling is visible and ordered. `can you ...` comes back as `Can you send the
report to Shivon at example.com ...` at casual and semi-casual, and `Please
send ...` at semi-formal and formal. Fidelity held in prose: Shivon,
example.com, Q3, README, Tuesday and 2 PM all survived every styling.

## Rejected: a preferences line

Adding `Preferences: warm, concise, no jargon` to the control line made
casual + prose + fidelity echo the transcript verbatim in 2 of 8 probed cells
and lose `example.com`. Preferences were not added to the 2B polish prompt.
The Styles Compare view now says the 2B ignores learned phrases, so the two
panes do not promise a difference the model will not produce.

## Round 4 verdict

Qwen3.5-2B can take the control line on the Polish path. It is the only
catalog row that does cleanup, the three Polish settings and Prompt Engineer
in one 1,173 MB file. The prompt must state that cleanup is unconditional;
without that rule the prose setting turns the model into an echo.

## Round 5: a stronger editor prompt

The user asked for the 2B Polish prompt to do more than S1-mini: make the
text more concise, reword for clarity, reorder for readability, add
structure, and keep the selected tone. The shipped prompt is now an editor,
not a normalizer. It states five numbered edits and applies all of them on
every take, even when the transcript is already clean.

Harness: `editor_matrix.py`. 6 cases (rambling, tangled invoice, casual
register, fidelity, steps, dictated question) x 4 tones, with prose for
everything except `steps` under lists, so 24 cells. Temperature 0,
`enable_thinking=false`, the app's llama-server flags. The checker mirrors
the app pipeline: prose bullet folding, first-word capitalization, the
editor OffTopic floor (0.15), and number normalization (`2:00 PM` folds to
`2 pm`, `four thousand two hundred` counts as 4200). Raw rows:
`editor-matrix.json`.

| Prompt revision | Clean |
|---|---|
| V3 numbered five edits, "keep every fact" | 19/24 |
| V3 + names dates, times, sources, file names | 21/24 |
| Final: concise rule says "never delete a fact, name, date, time, source or recipient" | 24/24 |
| Rejected: V4 "final check" wall of rules | 15/24 |

The failures were real, not checker noise. The first version dropped
`readme`, `Tuesday` and `2 pm` while shortening the fidelity case under
casual, semi-casual and formal. Naming those categories in the concise rule
fixed all four tones. A separate clause, "end every sentence, including each
bullet, with a period, question mark or exclamation mark", fixed missing
terminal punctuation on clean casual input and casual bullets. The model
would not capitalize a lowercase "hey" under any prompt wording, so the app
enforces that deterministically.

Evidence from the final 24/24 run:

- Rambling, 38 words in: 24 to 28 words out depending on tone, every filler
  removed, every fact kept.
- Fidelity: `Shivon`, `example.com`, `Q3`, `readme`, `Tuesday` and `2 pm` all
  survive at all four tones.
- Tone: casual and semi-casual keep `hey` and contractions; semi-formal and
  formal return `I am writing to request your review ...`.
- Structure: prose has no `- ` lines (bullets are folded if the model returns
  them); lists return one complete sentence per bullet.
- A dictated question stays a question and is never answered.
- No em dash in any cell.

### App support for the prompt

Three pipeline changes make the prompt's promises deterministic:

1. `validate_editor` in `transforms/validator.rs` uses a 0.15 OffTopic floor
   for the 2B Polish path only. Aggressive conciseness can share 21 to 25
   percent of content words with the input; an unrelated answer still scores
   near zero and is rejected. Test:
   `editor_floor_accepts_aggressive_compression_but_still_rejects_off_topic`.
2. `prose_from_bullets` in `transforms/engine.rs` folds any bullet list the
   model returns on the prose path into short paragraphs. Test:
   `prose_mode_folds_returned_bullets_into_a_paragraph`.
3. `capitalize_leads` capitalizes the first word of the text and of each
   bullet, while leaving URLs, paths and identifiers alone. Test:
   `capitalize_leads_touches_words_but_not_urls_or_identifiers` and
   `qwen_polish_capitalizes_a_lowercase_greeting`.

Verification: `cargo test -p teletype-core -p teletype-inference` (790 core
tests, all green), `cargo fmt --check`, `tsc --noEmit`, 194 UI tests, and a
live read-back of the Styles and Models copy in the `?demo&ready&qwen35`
preview.

Limits: this run bypasses ITN, so a spoken amount can stay spoken
(`four thousand two hundred dollars`) and the checker accepts that form. No
fresh S1-mini head-to-head was possible because the S1-mini GGUF is no longer
on disk.
