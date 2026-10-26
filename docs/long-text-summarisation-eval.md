# Long-Text Summarisation Eval — Qwen3.5 2B

**Date:** 2026-10-25
**Branch:** `release/v0.2.7`
**Scope:** Evaluation only. No code changes, no builds, no model downloads, no llama-server runs. All claims below are grounded in `file:line` evidence from the current tree plus exact tokenizer arithmetic with the Qwen3 tokenizer (Qwen3.5 is the same Qwen3 family — same 151,669 vocab, same BPE; see caveat in §1).

**Verdict up front:**

> **A 2B model cannot do a real (aggressive) summarization of long text today.**
>
> It can do *light compression* (1.5-2×) of inputs up to **~370 words** per request
> on the 2B path — and the context preflight *rejects* any single chunk above that
> before the model even runs. Beyond ~370 words, every existing guard (context
> preflight, truncation floor, off-topic floor, change ceiling) is calibrated for
> "polish that stays close to the input" and independently kills the summarization
> use case before, during, or after generation.
>
> A real summarization feature needs **four things that do not exist in the tree**:
> (1) a purpose-built summarization prompt with an explicit output-length target,
> (2) a summarization-aware validator (drop the input/5 truncation floor, lower the
> off-topic floor, disable the change ceiling), (3) a deliberately *small* output
> budget (frees the window for ~3,200+ words of input), and (4) a map-then-reduce
> path for long selections. The 2B is *borderline* even with all four: it is
> demonstrated to do light compression but drops facts under aggressive compression
> (L059). This should be an explicit product decision.

---

## 1. Scope, constraints, and methodology

### Constraints honored

| Constraint | How it was honored |
|---|---|
| No code changes in `crates/` | None made. Only `docs/` and `evals/` files created/modified. |
| No builds | No `cargo build`/`test`/`clippy` run. |
| No downloads | No model weights downloaded. A ~11 MB public `tokenizer.json` (a tokenizer, not weights) fetched to `/tmp/qwen-tok/` for arithmetic. |
| No llama-server | Not spawned. Over-cap behavior traced from code, not observed. |
| `file:line` grounding | Every mechanism below cites the line it was read at. |

### Methodology

1. **Static read** of the full transform path: selection capture → transform engine →
   chunk splitter → prompt builder → inference provider → context preflight →
   validator → result/insertion.
2. **Exact tokenizer arithmetic** with `@huggingface/tokenizers` (Node.js) + the
   Qwen3 `tokenizer.json`. The exact `QWEN_POLISH_SYSTEM_PROMPT` constant was
   extracted from `crates/teletype-core/src/transforms/prompt.rs` (1,820 chars) and
   tokenized with the real chat template (`
system
 /
user
 /
assistant
`).
3. **Model-capability judgment** against the 2B's measured behavior recorded in the
   project brain (L059, verified 2026-10-24: the 2B "follows five numbered edits"
   but "shortens by deleting facts", ignores capitalization advice in casual, and a
   0.34 OffTopic floor "rejected a legitimate aggressive rewrite that shared 0.21 to
   0.25 of content words").

### Tokenizer caveat

Qwen3.5-2B's tokenizer is assumed identical to Qwen3's (same Qwen3 family, same
vocab). Token counts are exact for the Qwen3 tokenizer; if Qwen3.5 diverges, expect
±5% drift. The *shape* of the budget (prompt ~480 tok, text ~1 tok/word, 8 chars/word)
is robust to that drift.

---

## 2. Current architecture (what a "summarize" request would actually hit)

There is **no summarize transform** in the tree. The built-in preset list in
`crates/teletype-core/src/transforms/mod.rs` is `builtin-polish`,
`builtin-professional`, `builtin-rewriter`, `builtin-prompt-engineer` — there is no
`builtin-summarize`. If a user asked for a summary today, the only routes are:

1. **A custom transform** (user-authored instruction) on the generic prompt path, or
2. **The Polish preset** on Qwen3.5-2B, which is a *card-faithful editor*, not a
   summarizer.

Both routes share the same engine, preflight, and validator. The trace:

```
UI shortcut / Tauri command
  └─ commands/transforms.rs:255  transform_selection()
       └─ text_selection.rs:9    selected_text()            // AX API, any length
       └─ pipeline.rs:458        engine::run_transform_blocking()
            ├─ splitter.rs:21    split_for_polish()         // chunks of <= 500 words
            ├─ engine.rs:445     context preflight (est. 4 chars/token)
            ├─ prompt.rs:165     build_qwen_polish_messages()
            ├─ llm.rs:142        polish_max_tokens()        // ceil(1.4x) + 24, cap 4096
            └─ validator.rs:143  validate_editor_polish()   // off-topic 0.15 + change %
```

---

## 3. Usable input budget (the arithmetic)

### 3.1 Fixed overhead (measured, Qwen3 tokenizer)

| Component | Tokens | Source |
|---|---|---|
| `QWEN_POLISH_SYSTEM_PROMPT` | **431** | tokenized exact constant, `prompt.rs` |
| Chat-template chrome (3 im_start/im_end blocks + roles) | **31** | `render(sys, "") - sysTok` |
| Control line `[Styling: ...] [Structure: ...] [Context: none]` + newline | **19** | tokenized |
| **Fixed total** | **~481** | |
| Context window | **4096** | `server.rs:160` (`-c 4096`) |
| Margin the preflight reserves | **256** | `engine.rs:456` (`+ 256`) |

### 3.2 Text density

A representative English word ("release", 6 letters) tokenizes as **1 token** in
Qwen3, and is ~8 chars including the trailing space. So:

- **1 word ≈ 1 token ≈ 8 chars**
- The model emits ~4 chars/token, so **output tokens ≈ output_chars / 4**.

### 3.3 The real constraint: output headroom

The 2B path (`is_card_cleanup`) reserves output headroom **equal to the input's char
count** (`engine.rs:451-452`):

```rust
let output_cap: u32 = if is_card_cleanup {
    (protected_input.chars().count() as u32).max(256)
} else {
    crate::llm::polish_max_tokens(protected_input)
};
```

For a 3,999-char chunk that is a **~4,000-token output budget** — the window is
assumed to hold *input + a full-length rewrite*. That is the single biggest waster
for summarization: a summary of a 500-word chunk is ~100-150 words, but the budget
reserves ~4,000 tokens for it. The preflight (`engine.rs:459`) then checks
`est_prompt + output_cap + 256 <= 4096`, and with a char-based cap, **this fails for
any chunk above ~374 words**.

### 3.4 Where the budget actually runs out (measured)

Solved exactly with the real tokenizer and the code's *own* preflight formula:

| Path | Output cap formula | Max words where preflight passes |
|---|---|---|
| **2B card-cleanup** (the active path for Qwen3.5-2B Polish) | `chars.max(256)` | **~374 words** |
| Generic (1.4×) path | `min(max(ceil(chars/4×1.4)+24, 128), 4096)` | **~774 words** |
| True summarization (small output) | 128 tokens | **~3,250 words** (real fit) |

Measured table (real input tokens measured; 2B preflight sum = `round((chars+400)/4) + chars + 256`):

| Words | Chars | Real input tok | 2B cap (chars) | 2B preflight sum | 2B preflight |
|---|---|---|---|---|---|
| 100 | 799 | 562 | 799 | 1,355 | ✅ |
| 200 | 1,599 | 662 | 1,599 | 2,355 | ✅ |
| 300 | 2,399 | 762 | 2,399 | 3,355 | ✅ |
| 374 | ~2,991 | ~836 | 2,991 | ~4,032 | ✅ (edge) |
| **375** | ~2,999 | ~837 | 2,999 | ~4,040 | ❌ |
| 400 | 3,199 | 862 | 3,199 | 4,355 | ❌ |
| 500 | 3,999 | 962 | 3,999 | 5,355 | ❌ |
| 1,000 | 7,999 | 1,462 | 7,999 | 10,355 | ❌ |
| 2,000 | 15,999 | 2,462 | 15,999 | 20,355 | ❌ |
| 3,000 | 23,999 | 3,462 | 23,999 | 30,355 | ❌ |
| 3,250 | ~25,999 | ~3,712 | — (128 floor) | ~4,096 | ✅ (summarize only) |

**Key finding:** on the active 2B path, the context preflight rejects any chunk above
~374 words. The splitter's `MAX_CHUNK_WORDS = 500` means the standard 500-word chunks
*always* fail the 2B preflight and return raw text via `ContextOverflow`.

### 3.5 Why "practical" is even lower than 374

Even where the preflight passes (≤374 words), the **validator** rejects aggressive
summarization:

- Truncation floor (`validator.rs:287`, input/5): a 5× compression *is* input/5 —
  rejected as `Truncated` by construction.
- Off-topic floor (0.15, `validator.rs:339`): a 5× summary shares <15% of words —
  rejected as `OffTopic`.
- Change ceiling (40% default, `validator.rs:123`): a 5× summary has ~80% change —
  rejected as `TooMuchChange`.

So the *effective* usable budget for a real summary — the range where a compression
survives **all** guards — is **~200-300 words** with a modest 1.5-2× compression.
A 300-word input compressed 2× to 150 words: output ~150 tokens, input ~760 tokens;
truncation floor = 760/5 = 152 tokens; 150 < 152 → *just* trips the floor. A 1.5×
compression (300 → 200 words) produces ~200 tokens > 152 → passes. So the practical
light-compression budget is **~200-300 words at 1.5×** before any guard fires.

---

## 4. What happens above the cap (code path)

### 4.1 Chunking happens *before* the window check

`run_transform_blocking` (`engine.rs:338`) calls `split_for_polish` first. Inputs
over 500 words are split into sentence-aligned chunks of at most
`MAX_CHUNK_WORDS = 500` (`splitter.rs:12`), each chunk transformed independently,
then re-joined with a single space (`engine.rs:371`).

**Consequence for summarization:** a "summary" of a 3,000-word selection becomes
6 independent ~500-word "summaries" joined with spaces. But each 500-word chunk
*fails the 2B preflight* (§3.4) and returns raw text. So a 3,000-word selection on
the 2B path today returns **the original 3,000 words unchanged** — the chunker
splits, the preflight rejects each chunk, and the rejoin concatenates the raw chunks.
The user asked for a summary and gets the original back, with no error message (the
`ContextOverflow` skip reason is logged but the text is the input).

### 4.2 Per-chunk context preflight (the binding constraint)

`engine.rs:445-473`. For the 2B path the output cap is `chars.max(256)`. The
preflight computes `est_prompt = (chars + 400) / 4` and checks
`est_prompt + output_cap + 256 <= 4096`. With `output_cap = chars`, this is
`chars/4 + 100 + chars + 256 <= 4096`, i.e. `1.25 × chars <= 3740`, i.e.
**`chars <= ~2,992`**, i.e. **~374 words** at 8 chars/word.

This is the binding constraint for any long-selection feature on the 2B. The
char-based cap was designed for polish (output ≈ input length) and is *wrong* for
summarization (output ≪ input), where a 128-token output budget would let the
preflight pass for ~3,250 words.

### 4.3 If a request *did* exceed 4096

Two independent backstops:

1. **Client-side preflight** (`engine.rs:459-472`): refuses to send, returns the
   raw input with `SkipReason::ContextOverflow` → user sees
   `"Input too large for the model. Using raw text"` (`state.rs:232`).
2. **Server-side** (llama-server): with `--jinja` and a 4096 ctx, an over-window
   chat is truncated to the window (prompt-truncation); the model emits a
   truncated fragment, which the `Truncated` validator floor (`validator.rs:287`)
   then rejects → raw-text fallback.

### 4.4 The partial-echo guard (BUG-005)

`engine.rs:373-391`: when a chunk's output is notably longer than the chunk and
>70% overlaps it, the chunk is treated as a "re-stated chunk" and the **input** is
used instead. For summarization this is harmless (a summary is *shorter*), but it
confirms the rejoin logic is built for near-1:1 polish, not for compression.

---

## 5. Guard conflicts (why summarization gets rejected)

The validator (`validator.rs`) is tuned so a Polish output *stays close to* the
input. Every floor is a problem for a summary.

### 5.1 Off-topic floor — `validate_editor` uses 0.15

`validator.rs:339`: `OFF_TOPIC_MIN_SHARED_EDITOR = 0.15`. A summary must share at
least 15% of its words (case/punctuation-insensitive multiset) with the input.
L059 (verified) already measured that the 2B's *legitimate aggressive rewrite*
shared only **0.21-0.25** — a true summary of a long doc sharing <15% would be
rejected as `OffTopic`. A 5× summary of a 500-word chunk to 100 words will almost
always share <15% of the original's words.

### 5.2 Truncation floor — input/5

`validator.rs:287` (`MIN_INPUT_FOR_TRUNCATION_CHECK = 80`): a substantial input
that comes back **shorter than input/5** is treated as `Truncated`. A 5×
summarization is *exactly* input/5 — it trips the floor by construction. The floor
exists to catch mid-sentence cutoffs, but it cannot distinguish "cut off by the
window" from "successfully compressed 5×".

### 5.3 Change ceiling — default 40%

`validator.rs:123`: `DEFAULT_MAX_CHANGE_PERCENT = 40`, applied by
`enforce_change_limit` (`validator.rs:156`). `change_share` measures the share of
the *larger* text that did not survive. A 5× summary has change_share ≈ 80%
(only ~20% of the original words survive) → rejected as `TooMuchChange`. The user
*can* raise the ceiling in Settings (100 disables it), but the default is set for
polish, not summary.

### 5.4 MassivelyExpanded / Preamble / PromptEcho

- `MAX_GROWTH = 4.0` (`validator.rs:71`): a summary is shorter, so this never fires
  — no conflict, but it shows the validator's mental model is "output ≈ input
  length".
- `PREAMBLES` list (`validator.rs:79-93`) and `PromptEcho`
  (`MIN_INPUT_LEN_TO_CHECK_ECHO = 24`): a 2B model asked to "summarize" often
  prefaces with "Here's a summary:" — caught and rejected. The lean
  `QWEN_POLISH_SYSTEM_PROMPT` avoids this for polish, but a summarization prompt
  would need the same "Return only the text. No commentary." discipline.

### 5.5 Summary of conflicts

| Guard | Threshold | Summary behavior | Verdict |
|---|---|---|---|
| Off-topic floor (editor) | ≥15% shared words | 5× summary shares <15% | **Rejects** |
| Truncation floor | < input/5 = truncated | 5× summary = input/5 | **Rejects** (by construction) |
| Change ceiling (default) | >40% change | 5× summary ≈ 80% change | **Rejects** at default |
| MassivelyExpanded | >4× input | summary is shorter | OK |
| Preamble / echo | list / 24-char | model may preface | **Risk** |

**Three of five guards independently reject a real summary.** The validator was
designed for polish (output ≈ input); a summary (output ≪ input) is outside its
operating envelope on three separate axes.

---

## 6. Can the 2B do it at all? (capability judgment)

L059 (verified, 2026-10-24, `2026-10-24-qwen35-editor-prompt.md`) is the direct
evidence:

- The 2B **does** follow five numbered edits (concise, reword, reorder, structure,
  tone) at temperature 0.
- But it **"shortens by deleting facts"** — it optimizes the shorter output and
  keeps only the facts it considers central. The prompt had to be rewritten to name
  "fact, name, date, time, source or recipient" explicitly to stop it dropping
  `Tuesday`, `2 pm`, and `readme`.
- It **ignores capitalization advice** in the casual register (a tone instruction
  overrides a capitalization rule).
- A 0.34 OffTopic floor **"rejected a legitimate aggressive rewrite"** (0.21-0.25
  shared).

Interpretation for summarization:

- **Light compression (1.5-2×)** is within the 2B's demonstrated ability. L059's
  winning prompt produced concise output that kept every named fact.
- **Aggressive summarization (3-5×, cross-paragraph, cross-chunk)** is *not*
  demonstrated. The 2B's conciseness is achieved by *deleting*, not by
  *synthesizing*. A summary that must compress 500 words into 100 while keeping the
  *right* 100 words is a ranking/synthesis task the 2B has not shown it can do
  reliably. L059's fact-dropping is exactly the failure mode a summarizer must
  avoid.
- **Cross-chunk coherence** is impossible with the current splitter (6 chunks, no
  shared state, re-joined with spaces). A "summary" of a long selection would be a
  list of local summaries, not a coherent one.

**Capability verdict:** the 2B can do *light, local, fact-preserving compression*.
It cannot be trusted with *aggressive, global summarization* in the current
architecture. This is a model-capability limit, not just a window limit — raising
the context window alone would not fix the validator rejections (§5) or the
fact-dropping.

---

## 7. Recommendation

### 7.1 For a real "Summarize" feature (not polish)

A summarization transform is a **different product surface** from Polish. It needs,
in dependency order:

1. **A summarization prompt** with an explicit output-length target ("Summarize in
   at most N words / M% of the original") and the same "return only the text, no
   preamble" discipline as `QWEN_POLISH_SYSTEM_PROMPT`.
2. **A summarization-aware validator**: a *separate* floor set from polish.
   - Drop the `input/5` truncation floor (a summary is *supposed* to be short).
   - Lower or drop the off-topic floor (a summary legitimately shares few exact
     words; use an embedding/semantic overlap check instead of word multiset).
   - Set the change ceiling to 100% (disable) by default for summaries.
   - Keep the preamble/echo guards.
3. **A deliberately small output budget**: `max_tokens = target_summary_words × 4`,
   not `input_chars`. This frees the window: the §3.4 table shows **~3,250 words**
   fit with a 128-token output budget, vs **~374 words** today on the 2B path.
   That is an **8.7× increase** in usable input for the same 4096 window.
4. **Map-then-reduce for long selections** (replaces the 500-word splitter for
   this path): summarize each chunk to a local summary, then feed the local
   summaries (which are small) to a second pass for a global summary. This fits
   the 4096 window because the reduce pass sees only the compressed intermediates.
5. **Model fit:** the 2B is *borderline*. For light (1.5-2×) summarization it is
   viable with the above changes. For aggressive (3-5×) summarization, the 2B's
   demonstrated fact-dropping (L059) makes it risky; a 4B+ model or a
   longer-window model is the safer choice. This should be an explicit product
   decision, not an accident of which model is loaded.

### 7.2 For the *existing* Polish path (no new feature)

If the ask is simply "can I select 2,000 words and Polish them today?" — **no,
not on the 2B path**:

- The splitter chunks into ~4 × 500-word pieces.
- Each 500-word chunk **fails the 2B preflight** (§4.2) and returns raw text.
- The rejoin concatenates the raw chunks → the user gets the original 2,000 words
  back, unchanged, with no error.
- This is a **silent no-op** for any selection over ~374 words on the 2B path. The
  user asked for polish and got their input back, with a `ContextOverflow` skip
  reason logged but no visible error.

On the *generic* path (custom transforms, non-card-cleanup models), the 1.4× cap
lets ~774 words pass preflight, so a 500-word chunk polishes fine. The 2B-specific
char-based cap is what breaks the 500-word chunks.

### 7.3 For UX when a selection exceeds the budget

Recommend a **two-tier behavior**, not a hard failure:

1. **Under ~300 words** (single request, all guards satisfiable): run as one
   transform. For Polish, this is the happy path. For a "Summarize" label, show a
   subtle "light compression" affordance, not "summary".
2. **300-3,500 words** (summarizable with a small output budget, after §7.1 ships):
   - If the user asked for **Polish**: on the 2B path, the preflight rejects the
     chunks → **surface a message**, not a silent raw-text fallback. "This
     selection is too long to Polish with the current model. Shorten it to ~370
     words, or switch to a larger model."
   - If the user asked for **Summarize**: run map-then-reduce (§7.1 item 4).
3. **Over 3,500 words**: do *not* silently fall back to raw text. Instead:
   - **Option A (recommended):** "Selection too long for a summary. Shorten it to
     ~3,000 words, or choose a section." No raw-text fallback — that is worse than
     no result.
   - **Option B (best-effort):** run map-then-reduce and label the result
     "Partial summary of N sections". Never present the raw input as the summary.
4. **Never** present the raw input as the summary or polish result without telling
   the user. The current `ContextOverflow` → raw-text path (`state.rs:232`) is
   acceptable for Polish on short clean text but is a *silent no-op* for long
   selections on the 2B path — the user gets their input back thinking it was
   polished. This is the worst UX outcome and the most likely to go unnoticed.

### 7.4 Concrete guard thresholds if summarization ships

| Guard | Polish (keep) | Summarize (propose) |
|---|---|---|
| Off-topic floor | 0.15 (editor) | 0.05 or semantic check |
| Truncation floor | input/5 | **off** (summary is short by design) |
| Change ceiling | 40% default | 100% (disabled) default |
| Max growth | 4× | 4× (keep) |
| Output budget | `chars.max(256)` | `target_words × 4` (small) |
| Preflight output cap | `chars.max(256)` | `target_words × 4` (frees 8.7× input) |

---

## 8. Verification (compliance)

### 8.1 No code changes

```
$ git status --porcelain | grep -E 'long-text|evals/long-text'
?? docs/long-text-summarisation-eval.md
```

This eval created only `docs/long-text-summarisation-eval.md`.

The working tree has **pre-existing** modifications in `crates/` from prior sessions
(L060, L061, and other concurrent work) — confirmed by mtime: the most recently
modified `crates/` file (`engine.rs`) was last touched at 11:52, ~21 minutes *before*
this eval's doc was written at 12:13. No `crates/` file was created or modified by
this eval. The pre-existing changes were *read* (not edited) to ground the analysis.

### 8.2 No builds

No `cargo build`, `cargo test`, or `cargo clippy` was run. Tokenizer arithmetic
used the Node.js `@huggingface/tokenizers` package in `/tmp/qwen-tok/` (outside
the repo).

### 8.3 No downloads / no server

No model weights downloaded. No `llama-server` spawned. The only artifact fetched
was the public Qwen3 `tokenizer.json` (~11 MB) to `/tmp/` for exact token counts.

### 8.4 Disk

Free space checked at start (~9 GB) and at end — unchanged by this eval (only
small doc/eval files written; tokenizer in `/tmp`).

### 8.5 Tokenizer arithmetic repro

```
$ cd /tmp/qwen-tok && node bounds.js
{
 "preflight_2B_words": 374,
 "preflight_generic_words": 774,
 "real_5x_summary_words": 3250,
 "real_self5x_words": 2738
}
[
 { "w": 100, "chars": 799,   "inputTok": 562,  "cap2B": 799,   "sum2B": 1355,  "passes2B": true },
 { "w": 200, "chars": 1599,  "inputTok": 662,  "cap2B": 1599,  "sum2B": 2355,  "passes2B": true },
 { "w": 300, "chars": 2399,  "inputTok": 762,  "cap2B": 2399,  "sum2B": 3355,  "passes2B": true },
 { "w": 374, "chars": 2991,  "inputTok": 836,  "cap2B": 2991,  "sum2B": 4032,  "passes2B": true },
 { "w": 400, "chars": 3199,  "inputTok": 862,  "cap2B": 3199,  "sum2B": 4355,  "passes2B": false },
 { "w": 500, "chars": 3999,  "inputTok": 962,  "cap2B": 3999,  "sum2B": 5355,  "passes2B": false },
 { "w": 1000,"chars": 7999,  "inputTok": 1462, "cap2B": 7999,  "sum2B": 10355, "passes2B": false },
 { "w": 2000,"chars": 15999, "inputTok": 2462, "cap2B": 15999, "sum2B": 20355, "passes2B": false },
 { "w": 3000,"chars": 23999, "inputTok": 3462, "cap2B": 23999, "sum2B": 30355, "passes2B": false },
 { "w": 3491,"chars": 27927, "inputTok": 3953, "cap2B": 27927, "sum2B": 35265, "passes2B": false }
]
```

---

## Appendix A — Key file:line references

| Claim | Location |
|---|---|
| 4096-token context | `crates/teletype-inference/src/server.rs:160` (`-c 4096`) |
| `context_tokens()` | `server.rs:310` |
| Chunking, `MAX_CHUNK_WORDS = 500` | `crates/teletype-core/src/transforms/splitter.rs:12,21` |
| Rejoin with space | `crates/teletype-core/src/transforms/engine.rs:371` |
| Partial-echo guard (BUG-005) | `engine.rs:373-391` |
| Context preflight (4 chars/token, +256 margin) | `engine.rs:445-473` |
| Card-cleanup output cap = input chars | `engine.rs:451-452` |
| `QWEN_POLISH_SYSTEM_PROMPT` | `crates/teletype-core/src/transforms/prompt.rs` (1,820 chars, 431 tok) |
| `build_qwen_polish_messages` | `prompt.rs:165` |
| `polish_max_tokens` = ceil(1.4×)+24, cap 4096 | `crates/teletype-core/src/llm.rs:142` |
| `DEFAULT_MAX_CHANGE_PERCENT = 40` | `crates/teletype-core/src/transforms/validator.rs:123` |
| Off-topic floor 0.15 (editor) | `validator.rs:339` |
| Off-topic floor 0.34 (default) | `validator.rs:333` |
| Truncation floor input/5 | `validator.rs:287` (`MIN_INPUT_FOR_TRUNCATION_CHECK = 80`) |
| `change_share` / `enforce_change_limit` | `validator.rs:156,173` |
| `validate_editor_polish` | `validator.rs:143` |
| `SkipReason::ContextOverflow` → raw text | `engine.rs:468`; message `crates/teletype-core/src/state.rs:232` |
| Selection capture (AX) | `crates/teletype-desktop/src/text_selection.rs:9` |
| `transform_selection` command | `crates/teletype-desktop/src/commands/transforms.rs:255` |
| `run_transform_blocking` from pipeline | `crates/teletype-core/src/pipeline.rs:458` |
| 2B capability (L059) | `.agents/brain/learnings/2026-10-24-qwen35-editor-prompt.md` |
| Qwen3.5-2B catalog entry | `crates/teletype-inference/src/catalog.rs` (id `qwen3.5-2b`, ~1173 MB, IQ4_XS) |

## Appendix B — Budget table (measured, Qwen3 tokenizer)

| Words | Chars | Real input tok | 2B cap (chars) | 2B preflight sum | 2B preflight |
|---|---|---|---|---|---|
| 100 | 799 | 562 | 799 | 1,355 | ✅ |
| 200 | 1,599 | 662 | 1,599 | 2,355 | ✅ |
| 300 | 2,399 | 762 | 2,399 | 3,355 | ✅ |
| 374 | 2,991 | 836 | 2,991 | 4,032 | ✅ (edge) |
| 375 | 2,999 | 837 | 2,999 | 4,040 | ❌ |
| 400 | 3,199 | 862 | 3,199 | 4,355 | ❌ |
| 500 | 3,999 | 962 | 3,999 | 5,355 | ❌ |
| 774 | 6,191 | 1,236 | 6,191 | 8,095 | ❌ (generic edge: 774) |
| 1,000 | 7,999 | 1,462 | 7,999 | 10,355 | ❌ |
| 2,000 | 15,999 | 2,462 | 15,999 | 20,355 | ❌ |
| 3,000 | 23,999 | 3,462 | 23,999 | 30,355 | ❌ |
| 3,250 | ~26,000 | ~3,712 | — (128 floor) | ~4,096 | ✅ (summarize only) |

*Real input tok = system(431) + chrome(31) + control(19) + words(≈1 tok each),
measured with the Qwen3 tokenizer. Output cap for the 2B path = `chars.max(256)`
(char count, not tokens) — the code reserves *char-count* tokens of headroom, which
over-reserves in English (~4 chars/token) and is the binding constraint the
preflight enforces. The "summarize only" row assumes a 128-token output budget
(requires §7.1 item 3).*
