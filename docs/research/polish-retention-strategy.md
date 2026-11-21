# Polish that people keep using

**A retention strategy for Teletype AI Polish, benchmarked against Wispr Flow**

Date: 2026-11-19
Scope: product strategy, prompt/pipeline design, settings, interface, learning loop, measurement
Method: repository audit of the current Polish implementation + public competitor teardown + synthesis of public user feedback. **No primary user interviews were run for this document**; §10 is the plan to run them.

> **Re-checked against `main @ e1fd01f` (2026-11-19).** Several items the first draft implied were missing are already partly shipped: the P6.3 protected-terms/hallucination guard (`crates/teletype-core/src/transforms/guard.rs`) is wired into the polish path; the severity-graded polish behavior gate (`evals/polish-adversarial/`, `scripts/eval/polish_behavior_gate.py`) runs in CI; Transforms and Styles scope their controls by model capability; and the pill surfaces skip/fallback reasons. Those are credited as shipped below; G9 records two defects found in the guard during this re-check. The items marked **open** are what the re-check still finds missing.

---

## 0. TL;DR

Teletype does not have a "polish quality" problem first. It has a **trust and predictability** problem, wrapped around an unmeasured quality problem.

The market leader, Wispr Flow, has already trained users to expect "speak naturally, send without fixing." Its own public claim is a **90% zero-edit rate** and a **p99 under 700 ms**. But one of its most repeated user complaints is the flip side of that promise: **it changes words the user did not want changed** — adds verbiage, drops "was," removes emphasis, "transforms" sentence structure, and sometimes changes meaning.

That is the opening. Teletype cannot beat Wispr by rewriting more aggressively. It can beat Wispr by being the polish users can **predict, verify, reverse, and teach** — while closing the baseline gap on zero-edit quality and latency.

**The thesis:**

> Polish should feel like a contract, not a slot machine.
> The user should always know what it is allowed to change, see what it changed, be able to undo it in one action, and watch it need fewer edits every week.

**Five highest-leverage moves (in order):**

1. **Diff card v2** — revert to raw, undo a single change, thumbs, "never change this," and "why." Today the card reports what happened but offers no recovery action or feedback (`ui/src/pill.tsx`, `PolishDiffCard`).
2. **Polish Profiles** — four named contracts (Clean / Natural / Structured / Transform) with per-app defaults. Collapse the current sprawl of knobs and four overlapping customization layers into one mental model.
3. **A deterministic trust layer** — backtrack/self-correction resolution and protected spans (names, numbers, URLs, code, quotes) before the model, plus a verifier after it. The shipped post-model guard is a start, but its removal and case-mismatch defects (G9) must be fixed before claiming "Polish cannot touch what must not change."
4. **Close the learning loop** — the 30-minute kept/undone signal already exists (`ui/src/screens/InsightsScreen.tsx`); today it feeds terminology promotion but never Polish behavior. Use it to adapt per-app profiles and visibly teach the product.
5. **Calibrate on day zero and prove value weekly** — a 60-second onboarding calibration on the user's own three sentences, and a weekly local "Polish report" with zero-edit rate.

**Positioning line:**

> *Polish that never surprises you — and needs fewer edits every week.*

---

## 1. What job is "polish" actually doing?

Dictation has two jobs, and they are often confused:

1. **Transcription** — turn sound into the words I said.
2. **Translation** — turn the words I said into the message I meant to send.

Polish is job 2. The user is not buying "clean text." The user is buying **the gap between thought and sendable message, closed.**

That gap has five components. Each is a lever, and each can lose the user:

| Component | User question | Failure mode | What it costs |
|---|---|---|---|
| **Fidelity** | "Did it keep what I meant?" | Changed meaning, dropped a fact, invented words | Trust; user proofreads everything |
| **Completeness** | "Do I still have to edit?" | Under-polished grammar, no punctuation, no structure | Effort; user reverts to typing |
| **Speed** | "How long until I can send?" | 2–3 s wait, spinner, server stall | Habit break; user types instead |
| **Control** | "Can I predict and fix it?" | Opaque changes, no undo, model roulette | Anxiety; user turns polish off |
| **Fit** | "Does it sound like me, here?" | Wrong tone in Slack, email framing in a chat | Embarrassment; user edits every time |

The current product conversation is mostly about **completeness** ("is the polish good enough?"). Users churn on **fidelity, control, and fit** first.

### The retention equation

Think of retention as:

```
Retention ≈ (Zero-edit quality × Trust × Speed × Context fit) / Effort
```

- **Zero-edit quality** is the entry ticket. If the default output needs more edits than typing, nothing else matters.
- **Trust** is the multiplier. One meaning-changing edit can undo weeks of goodwill.
- **Speed** is perceived quality. A 2.5 s wait feels worse than a 1.2 s wait even if the text is better.
- **Fit** is what makes it feel personal: Gmail vs Slack vs Cursor vs Terminal.
- **Effort** is the denominator: setup, knobs, proofreading, recovery.

Wispr Flow wins the numerator's quality and speed. It loses trust and fit at the margin. Teletype can win the denominator and the trust multiplier.

---

## 2. What the market taught us

### 2.1 Wispr Flow's actual promise

Wispr's public position (2026):

- **"Speak naturally. Send without fixing."**
- **90% zero-edit rate** across its benchmark; 3.4% WER for its Canto ASR model; **13% fewer dictations fixed** after Canto; **p99 under 700 ms** end-to-end.
- One tuned pipeline, no model picker, no prompt configuration. Cleanup is automatic: filler removal, self-correction, punctuation, formatting, lists, per-app style.
- Context awareness (app, screen text), personal dictionary that learns from edits, snippets, per-app styles, Command Mode.

The strategic insight is not the feature list. It is that Wispr picked the **right metric**: *how often the user sends without touching the text.* Everything in their product is optimized for that.

### 2.2 Where Wispr actually loses users

Public reviews and the r/WisprFlow community are unusually consistent. The complaints are not "the cleanup is too weak." They are:

| Complaint | Representative public evidence | What it tells us |
|---|---|---|
| **Adds words not said / changes meaning** | "It seems like this app has taken some very creative liberties... changing or adding words completely... what is being typed, I now have to completely proofread twice." (r/WisprFlow) | Over-editing is a trust breach, not a quality nit. |
| **Changes sentence structure even with transforms off** | "I voiced 'Six months ago it was like perfection.'... it's transformed it and taken the 'was' out." (r/WisprFlow) | Users notice grammar "improvements" that alter voice. |
| **Removes meaningful words** | "drops a leading 'so,' removes deliberate repetition or rewrites a phrase" — Wispr's own Smart Formatting help doc | The failure mode is documented by the vendor. |
| **Punctuation/capitalization regressions** | "all of a sudden, there's no punctuation anymore, and no capitalization of the names of people, words, or cities" (r/WisprFlow) | Basic deterministic fixes are still table stakes. |
| **Quality swings / latency spikes** | Multiple threads on degraded accuracy, 5 s waits, outages (2026) | Cloud dependency is a structural reliability risk. |
| **Per-app style reset / loss of auto-style** | "Flow used to be smart — more formal in emails... Now it just uses one default style everywhere... and it resets after 15 minutes" (r/WisprFlow) | Context fit is valued, and losing it is felt as a downgrade. |
| **Language auto-detect friction** | "In the past, it auto-recognized my language. Now I have to click a switch" (r/WisprFlow) | Multilingual users are a real segment; regressions hurt. |
| **Privacy / screen capture** | "cloud only", "context awareness screenshots your screen"; reviews flag this repeatedly | Teletype's local-first wedge is real and durable. |

A widely shared public post in r/WisprFlow, consistent with Wispr's shipped behavior, argues for a deliberate product call worth considering: **they do not show a live raw transcript during dictation.** The reasoning: once words start appearing, most people switch into editing mode, pause, rephrase, and fix things mid-sentence — while the final pass is going to clean it up anyway. That is a useful counterweight to Teletype's live-preview default.

### 2.3 The strategic gap

Wispr is a **cloud product with one globally tuned cleanup pipeline** and a handful of preset styles. That is why it feels magical out of the box and why it occasionally betrays the user.

Teletype is a **local, configurable, privacy-first** product. That is why it can offer something Wispr structurally cannot:

1. **Proof** — the model and the diff are on the user's machine.
2. **A contract** — a bounded edit budget per context, not one global rewrite policy.
3. **Reversibility** — every change is visible and undoable.
4. **Compounding personalization** — local learning without sending data anywhere.
5. **Per-app fit that persists** — no 15-minute style reset.

But none of that matters if Teletype's default output is visibly worse than Wispr's. **Quality is table stakes; trust and fit are the differentiation.**

---

## 3. Audit: what Teletype has today

### 3.1 The good bones

The current implementation is more sophisticated than the UI lets on.

| Layer | What exists | Where |
|---|---|---|
| **Deterministic gate** | Skips polish for short/clean takes, with reasons: filler, self-correction, list cue, email context, low confidence, script needs LLM | `crates/teletype-core/src/transforms/gate.rs` |
| **Model-specific prompts** | S1-mini control line; Qwen 2B editor; Qwen 0.8B lean editor; SpeakoFlow fixed card; generic prompt path | `crates/teletype-core/src/transforms/prompt.rs` |
| **Fidelity guard** | Empty, prompt echo, instruction echo, preamble, markdown fence, massive expansion, truncation, language drift, off-topic, **too-much-change** (default 40%) | `crates/teletype-core/src/transforms/validator.rs` (`DEFAULT_MAX_CHANGE_PERCENT = 40`) |
| **Self-repair handling** | The gate detects self-correction cues; the filler corpus splices spoken repairs. There is no deterministic backtrack resolver before the model. | `crates/teletype-core/src/transforms/gate.rs`, `crates/teletype-core/src/fillers/corpus.rs`, `tiers.rs` |
| **Protected spans (post-model)** | P6.3 guard restores **changed** lexicon terms, numbers, URLs, emails, and code identifiers, but its case-sensitive word diff has two verified defects: a **removed** token is counted and not reinserted, and a case-only change can be mistaken for a substitution and displace the wrong word. Only >3 changed/removed tokens (or a hallucinated phrase) rejects the output. AutoText placeholders protect before the model; rescoring never replaces protected spans. | `crates/teletype-core/src/transforms/guard.rs`, `pipeline.rs:610`, `autotext/protect.rs`, `rescoring.rs` |
| **Personalization** | User profile, preferred terms, per-app style resolution, learned preferences — but the two shipped default models are cleanup-only and ignore the style layer. | `crates/teletype-core/src/personalization/`, `style.rs` |
| **Post-insert proof** | Diff card with change count and polish latency; History shows raw vs polished, model, ms. No one-click revert or per-span undo. | `ui/src/pill.tsx` (`PolishDiffCard`), `ui/src/components/dictation/`, `crates/teletype-core/src/history.rs` |
| **Settle signal** | "Corrections you kept" vs "undone" 30 minutes after insertion; displayed, and flags candidates for auto-apply, but no stage applies polish changes. | `ui/src/screens/InsightsScreen.tsx`, `learning/settle.rs` |
| **Local eval harness** | Replay/measure modes, weighted content-loss/hallucination rubric, WER, latency. The headline corpus is synthesised. | `crates/teletype-eval/`, `evals/report.md` |
| **Polish behavior gate** | 17 severity-graded adversarial cases (numbers, entities, fabrication, filler, tone, technical terms, AutoText) plus a satisfiability linter and a correct-output proof; the live judge runs when CI secrets are configured, and the model-free checks always run. | `scripts/eval/polish_behavior_gate.py`, `evals/polish-adversarial/`, `.github/workflows/ci.yml` |
| **No telemetry** | Everything local | repo-wide |

The existing "Beat Wispr Flow" plan (Phase 6) already scopes much of the right architecture: a richer gate, protected terms, context capture, latency targets, and a self-improving loop. Several pieces are shipped or in progress. The gap this document addresses is not the architecture — it is the **product layer, the trust contract, and the measurement** that turn those pieces into retention.

### 3.2 The gaps that matter

#### G1. There is no zero-edit metric

`evals/report.md` has a "mean polish score" (91.2) over **synthesised audio**, and the harness has no human "would I send this?" label. The production product has no measure of how often polish actually removes the need to edit. The closest signals are "edits per 100 words" and "corrections kept," both in Insights below the headline impact cards.

**Consequence:** prompt/model decisions are made on vibes and a synthetic score, not on the outcome that drives retention.

#### G2. The learning loop is open

The UI already says: *"Changes AI Polish made that were still in the text 30 minutes later... Nothing is applied automatically from this yet."* (`InsightsScreen.tsx`, QualityCard). That is accurate: the tally is displayed and can flag a candidate for auto-apply, but the store itself documents that nothing rewrites text from it yet. It does **not** feed back into Polish behavior — the prompt, profile, edit budget, or per-app choice. A kept/undone signal, per app, is exactly the training signal a polish system needs, and that part is discarded.

The edit-watch learning loop is dictionary/terminology-centric, fires on the *next* dictation, and promotes only after three keeps with no undos (`settle.rs`, `AUTO_APPLY_AFTER = 3`). It does not learn style, edit budget, protected spans, or per-app preference.

#### G3. Four overlapping customization layers, gated by an invisible capability matrix

A user who wants better polish meets a capability matrix, not one system:

| Model | Style channel | Tone / Structure / Context | Style phrases | Rules + instructions | Prompt override | Other presets |
|---|---|---|---|---|---|---|
| **SpeakoFlow Mini** (code default) | `FixedBehaviour` | no | no | no | no | no (`cleanup_only`) |
| **S1-mini** (onboarding lead) | `ControlLine` | yes | no | no | no | no (`cleanup_only`) |
| **Qwen 2B / 0.8B** | `ControlLine` | yes | no | no | yes | yes |
| **Generic / remote** | `StylePhrases` | no | yes | yes | yes | yes |

Source: `crates/teletype-core/src/style.rs` (`model_style`), `TransformsScreen.tsx` (`polishCustomizationFor`).

At HEAD, the Transforms screen scopes the rules, prompt, and custom-instruction controls by this matrix and blocks non-Polish presets on cleanup-only models; Styles disables the phrase list and explains why. That closes most of the "dead controls" trap. The remaining gap is **expectation**: both shipped first-run models (SpeakoFlow Mini as the code default, S1-mini as the onboarding lead) are cleanup-only, yet the AI Polish settings section and onboarding never say that the model cannot structure an email or tighten a rambling thought. A user can reasonably expect a capability the model does not have.

That is not a power-user feature. It is a trust trap: *"I set it up and nothing changed."*

#### G4. The post-insertion card has no recovery

`PolishDiffCard` shows the change count, latency, a diff, "Configure Polish," and "click to open History." It does **not** offer:

- revert to what I said,
- undo one change,
- "this was wrong,"
- "this was good,"
- "never change this word/name."

The raw words are not lost: History stores `raw_text` and renders the removed words struck through, and the card's hover title shows before/after. But the user's only *action* is ⌘Z or manual editing — and the card does not tell them that.

#### G5. The knobs speak engineer, not user

Current settings (`ui/src/settings/DictationPolishSections.tsx`):

- "Skip polish for short takes" + "Polish takes with at least N words"
- "Let Polish change up to X% of the words"
- "Polish whole text at the end"
- "Polish every dictation automatically"
- "Local provider" status chip

These are internally meaningful, but they ask the user to reason about word counts, replacement percentages, batching, and model plumbing. A user thinks: *"Will it sound like me in Gmail?"*

#### G6. Defaults optimize for safety, not for the magic moment

- Code default: SpeakoFlow Mini; onboarding lead: S1-mini. Both are **cleanup** models. They fix fillers, punctuation, and many self-corrections, but do not structure an email or tighten a rambling thought.
- Default `auto_apply_transform = true`, gate on, threshold 12 words, max change 40%, `polish_whole_text_at_end = false`.

For a first-time user dictating a rambling email, the result may be "clean but shapeless." That is a correct, safe behavior — and a weak first impression against Wispr's structured output.

#### G7. Latency is a product risk, not just an engineering metric

The gate helps short takes, but a long take still waits for a local LLM (0.5–3 s). There is no "instant text, then refine" path. Wispr's p99 target is sub-700 ms. On a CPU-only Mac with the 0.8B, Teletype's polish can feel like a pause.

#### G8. Evals are stronger than the report suggests, but still miss voice, repetition, and format

This is not a greenfield gap. The harness already has:

- **Weighted correctness checks** — `no_content_loss` (50) and `no_hallucination` (40) dominate the score (`crates/teletype-eval/src/polish.rs`).
- **A severity-graded adversarial corpus** — 17 behavior cases covering number preservation, entity preservation, no-fabrication, filler handling, tone/formality, technical terms, and AutoText placeholders (`evals/polish-adversarial/behavior-cases.jsonl`).
- **A real CI gate** — `polish-quality-gate` runs the corpus load check and the satisfiability/correct-output self-test on every push; the live model judge runs when the repo secrets are configured (`scripts/eval/polish_behavior_gate.py`, `.github/workflows/ci.yml`).
- **A CI failure on over-polishing** — the self-test runs the corpus against committed correct outputs (must pass) and an over-polished derivative (must fail).

What is still missing from the gate:

- Intentional **repetition/emphasis** preservation.
- **Voice drift** (contractions, sentence length, formality) as a scored signal.
- **Greeting/sign-off invention** and **per-app format contracts** (email vs Slack vs code).
- **Self-correction resolution** as its own behavior class, beyond "filler handling."
- The headline **91.2 mean polish score** is still from synthesised audio (`evals/report.md`); it validates plumbing, not sendability.

The remaining work is therefore to *extend* the gate, not invent one.

#### G9. The guard restores substitutions, not removals

`guard.rs` has two related defects, both verified with a temporary test:

1. **Removal is not restored.** A removed protected token is counted as a change but not reinserted; only more than three changed/removed tokens reject the output. Input `please ask Siobhan to review the plan` → `please ask to review the plan.` is accepted without `Siobhan`.
2. **The word diff is case-sensitive, so a case-only change can be mistaken for a substitution.** Input `ask Siobhan to review the plan` → `Ask to review the plan.` makes the unmatched `Ask` look like the replacement for the removed `Siobhan`; the guard returns `Siobhan to review the plan.`, dropping the verb.

The module comment says changed or removed tokens are "restored in place," so the code and its documentation disagree. The fix is small and testable: normalize case (or use a case-insensitive LCS), reinsert or reject when a protected token is deleted, and add golden cases for one removed name and one removed number. Until then, do not claim that Polish "cannot touch names or numbers"; it can drop or displace them.

---

## 4. Who the user is (and what each segment needs)

These are synthesized segments, not validated personas. §10 is how to validate them.

### Segment A — The privacy-sensitive professional

Lawyer, doctor, founder, journalist, finance. Dictates client notes, emails, memos.

- **Job:** get accurate, professional text without audio or text leaving the machine.
- **Fears:** a leaked recording; an altered fact; a compliance violation.
- **Wins:** "Clean" and "Structured" profiles, local proof, per-app formality, no surprises.
- **Churns if:** polish changes a name, number, or legal phrase once too often.
- **Retention hook:** "Polish cannot touch names, numbers, or quoted text" + visible diff + revert.

### Segment B — The developer / technical PM

Dictates prompts to Claude/Cursor, PR descriptions, Slack replies, commit-adjacent notes.

- **Job:** get long, technical thoughts out fast, with code tokens intact.
- **Fears:** camelCase mangled, file paths changed, bullets where code should be.
- **Wins:** "Clean"/"Off" in IDE and terminal, protected code spans, fast local latency, no email framing.
- **Churns if:** it "improves" a technical term or adds structure to a prompt.
- **Retention hook:** per-app profile + "never change code."

### Segment C — The email-heavy operator

Founder, sales, support, ops. Lives in Gmail/Outlook; writes 30–80 messages a day.

- **Job:** turn a rambling spoken update into a concise, structured email that is ready to send.
- **Fears:** sounding unprofessional; missing a fact; a greeting/sign-off that is wrong.
- **Wins:** "Structured" default for email, self-correction resolution, lists when enumerated, recipient-aware tone, no invented content.
- **Churns if:** it adds "Dear X" or "Best regards" when not dictated, or drops a date/time.
- **Retention hook:** zero-edit rate + weekly time saved.

### Segment D — The non-native English speaker

Writes in English daily; speaks with an accent; grammar is the main friction.

- **Job:** fix grammar and clarity without erasing voice or meaning.
- **Fears:** paraphrase, idiom replacement, translation, "sounding like a different person."
- **Wins:** "Natural" profile with explicit grammar-only mode, voice preservation, language lock, explainable changes.
- **Churns if:** it rewrites instead of correcting; it drifts language.
- **Retention hook:** "It fixes my grammar but still sounds like me."

### Segment E — The voice-first thinker

ADHD, accessibility, writing-heavy roles; thinks out loud, long takes.

- **Job:** capture thought before it disappears; get structure without losing content.
- **Fears:** truncation, lost ideas, a wall of text.
- **Wins:** long-take handling, "Structured" for notes, instant first text, no cap on length.
- **Churns if:** latency or a timeout kills the take.
- **Retention hook:** "It keeps up with me."

**Cross-segment truth:** every segment's top complaint is *not* "polish is too weak." It is "polish changed something I did not want changed." The conservative profile is not a compromise; it is the trust anchor.

---

## 5. The product: Polish Profiles

### 5.1 Four contracts, one mental model

Replace the model/tone/style/prompt maze with four named profiles. Each profile is a **contract** — what it may change, what it must preserve, how much it may rewrite, and which model runs it.

| Profile | Promise | Allowed | Forbidden | Edit budget | Default for |
|---|---|---|---|---|---|
| **Clean** | "Fix only what is wrong. Keep my words." | Fillers, false starts, retracted words, spelling, punctuation, capitalization, ITN, spacing | Rewording, reordering, restructuring, added words | ≤15% of words; deletions/fixes only | Code, terminal, quotes, legal, short transactional takes |
| **Natural** | "Make it correct and readable, still me." | Clean + grammar, light tightening, clarity, sentence-level fixes | New facts, new structure, tone change, added greetings | ≤30% | Slack, chat, notes, AI prompts |
| **Structured** | "Make it ready to send." | Natural + paragraphs, lists when enumerated, conciseness, app-appropriate format | New facts, invented greeting/sign-off, invented sections | ≤45% | Email, docs, long updates |
| **Transform** | "Rewrite it the way I asked." | Summarize, change tone, restructure, shorten/lengthen | Changing facts/names/numbers; answering questions | Explicit, user-requested | Command Mode, Rewriter, explicit transforms |

**Raw/Off** is the fifth state for code and terminals: no polish at all. It should be a first-class per-app choice, not a hidden model mismatch.

### 5.2 Why profiles beat knobs

- **Predictability:** the user knows what to expect before speaking. That is the precondition for habit.
- **Risk mapping:** risk is a property of context, not a global slider. Email wants structure; code wants none.
- **Testability:** each profile has its own eval set, prompt, budget, and verifier. You can improve Clean without destabilizing Structured.
- **Onboarding:** "Pick how you want to sound" is a user question. "Set your max change percent" is not.
- **Differentiation:** Wispr ships one tuned cleanup pipeline with a handful of preset styles. Teletype can offer explicit edit contracts per context — and remember them.

### 5.3 Per-app defaults

Ship opinionated defaults, then let the user override per app:

| Context | Default | Rationale |
|---|---|---|
| Email (Gmail, Outlook, Mail) | **Structured** | The user's intent is a sendable message. |
| Slack / Teams / Discord | **Natural** | Conversational, short, no greeting. |
| IDE / Terminal | **Off** (or Clean if explicitly enabled) | Code tokens must never be "polished." |
| Docs / Notes | **Structured** for long, **Natural** for short | Long-form benefits from paragraphs. |
| AI chat (ChatGPT, Claude, Cursor chat) | **Natural** | Clarity helps; facts and code must survive. |
| Browser forms / unknown | **Natural** | Safe default. |
| Quoted text / selection transform | **Transform** (explicit) | The user asked for a change. |

Show the active profile somewhere subtle (pill tooltip, History entry, or a "Polish: Structured · Gmail" line in the diff card). Users should never wonder which contract ran.

### 5.4 What happens to the existing style system

Do not delete it; demote it.

- **Profiles** answer "what may change."
- **Voice** answers "how it sounds" (concise, warm, formal, direct). A profile can have a default voice, and the user can pick one per profile/app.
- **Tone / Structure / Context** remain as advanced controls for the models that honor them. They are no longer the primary UI.
- **Prompt instructions** become an Advanced "Polish instructions" list, shown only when the selected engine actually reads it, with a capability badge. Fix the inert-control trap.

This gives power users the same depth without making normal users learn the plumbing.

---

## 6. Prompts and pipeline: how to make polish actually better

Quality is table stakes. The goal is **zero-edit rate**, not "amount rewritten." The pipeline should be conservative by default and aggressive only when explicitly asked.

### 6.1 Three layers, not one model call

```
ASR
  → Layer 1: deterministic canonicalizer (0 ms)
  → Layer 2: small cleanup model (only if the gate says it helps)
  → Layer 3: editor model (only for Structured / Transform)
  → Verifier (deterministic + optional small model)
  → Insert
```

**Layer 1 — deterministic canonicalizer.** Always on, explainable, and free:

- filler removal (already exists) and self-repair splicing (extend)
- spoken punctuation, emoji, ITN (numbers, currency, dates, times, emails, URLs)
- spacing, doubled punctuation, capitalization of sentence starts
- list cue detection ("first... second...") → structured markers when the profile allows
- dictionary/AutoText protection and screen-term snap
- backtrack resolution (see 6.2)

This layer should handle the majority of cleanup with zero model risk. Every change it makes is explainable and testable.

**Layer 2 — cleanup model.** SpeakoFlow Mini or S1-mini, only when Layer 1 cannot decide (grammar, ambiguity, long sentence). Constrained to Clean/Natural. Fast, local, predictable.

**Layer 3 — editor model.** Qwen 2B (or the best available) for Structured/Transform. Longer budget, verifier, and optional second pass. Never used for Clean.

**Why this matters:** it reduces the model's opportunity to invent, cuts latency for the common case, and makes the quality story auditable. It also gives Teletype something Wispr cannot easily match: a transparent reason for every change.

### 6.2 The two highest-value deterministic passes

#### A. Backtrack / self-correction resolver

This is the single most visible polish behavior in every competitor demo: *"let's meet at 5... actually 6"* → *"let's meet at 6."* Wispr's own help doc lists the cue phrases and the ambiguity rules. Teletype has partial self-repair splicing in the filler corpus; make it a first-class, tested pass.

Cues: `no wait`, `wait no`, `scratch that`, `strike that`, `forget that`, `cancel that`, `never mind`, `actually`, `I mean`, `sorry`, `let me start over`, `correction`, `rather`.

Rules:

- Cue + replacement → remove the retracted span, keep the replacement.
- Cue + no replacement → keep the text (ambiguous; do not guess).
- `actually` used as emphasis, not correction → keep (heuristic: no contrasting replacement nearby).
- Never delete the first or last word of a take unless it is meaningless filler or explicitly retracted.
- If the retracted span contains a fact/number/name and the replacement is ambiguous, keep both (safe) and let the model resolve only when confident.

This pass should be evaluated on a dedicated corpus of 50+ real self-corrections. It is more reliable than asking a small model to infer intent.

#### B. Protected spans

**Shipped at HEAD (post-model):** `transforms/guard.rs` collects protected spans after the model and restores **changed** lexicon terms, numbers, URLs, emails, and code identifiers; more than three changed/removed tokens or a hallucinated phrase rejects the whole output. **Known holes:** a removed protected token is counted but not reinserted, so one dropped name or number can still pass; and the case-sensitive LCS can mistake a sentence-initial capitalization for a substitution, displacing a protected token over the wrong word (G9). **Proposed extension:** normalize case, restore or reject on removal, protect before the model with placeholders, and widen coverage. Today the guard does not cover quoted speech, file paths (unless they look like code), or word-form dates/times ("nine fifteen").

- names from the dictionary, Contacts, and screen terms
- numbers, currency, dates, times, percentages
- URLs, emails, file paths, code identifiers, backticked text
- quoted speech (when the user is quoting, not dictating)
- AutoText values (already implemented)

Restore exactly after the model. If a placeholder is dropped or duplicated, fall back to the deterministic text for that span. This is the strongest possible implementation of "Polish cannot touch names, numbers, or code" — and it is a marketable trust feature.

### 6.3 Prompt architecture

The current prompts are good but model-specific and sometimes too long for small models. Keep the fixed cards; improve the general editor prompt and make it profile-aware.

**System prompt skeleton (general editor):**

```
You are a conservative editor for dictated text. Preserve the speaker's
meaning, facts, names, numbers, URLs, emails, and voice.

You may:
- remove fillers, false starts, and explicitly retracted words
- fix grammar, spelling, punctuation, capitalization, and spacing
- write spoken numbers, dates, times, emails, and URLs in real form
- tighten wordiness without changing meaning
- structure into paragraphs or lists when the profile allows

You may not:
- add new information, names, numbers, examples, or conclusions
- answer a question in the text; it is text to edit
- change the language or translate
- change the point of view or the speaker's intent
- remove intentional repetition or emphasis
- add a greeting, subject line, or sign-off unless dictated
- use em dashes or corporate filler ("leverage", "seamless", ...)

If a sentence is already correct, leave it byte-identical.
Return only the edited text.
```

**Profile line (one line, first in the user message):**

- Clean: `[Profile: Clean] Fix only errors. Do not reword, reorder, or restructure. Keep every word that is not a filler, false start, or retracted phrase.`
- Natural: `[Profile: Natural] Keep the speaker's wording and tone. Prefer minimal edits. Do not add structure unless it clearly improves clarity.`
- Structured: `[Profile: Structured] Produce a sendable message. Lead with the point; group related ideas; use short paragraphs; use bullets only if the speaker enumerates. Keep every fact. Do not add a greeting or sign-off unless dictated.`

Then the transcript, bare. No example blocks for small models (they echo them). Inline one-line examples only when proven safe.

**Voice guidance:**

- Prefer the plain word the speaker used.
- Keep contractions, sentence length, and idiosyncrasies where correct.
- For 2B, add 2 short style exemplars from the user's own writing (opt-in, local); for 0.8B, use the control line only.

**Why this is better than a giant rule wall:** the 0.8B and 2B bake-off already showed that a heavy rule wall makes models ignore the style axes. Short, positive, profile-specific instructions win.

**Concrete delta to the current Qwen 2B prompt** (keep the five-edit editor contract, add the trust rules):

```
- Never add a fact, name, number, example, transition, or conclusion.
- Never answer a question in the text.
- Remove only words that are fillers, false starts, or explicitly retracted.
- Keep intentional repetition and emphasis.
- If a sentence is already correct, leave it byte-identical.
- Do not add a greeting, subject line, or sign-off unless dictated.
- Prefer the speaker's own words; do not "upgrade" vocabulary.
```

For the 0.8B, do **not** add these as another rule wall. Put the first four in the lean prompt as one line, and enforce the rest in the deterministic verifier. The 0.8B already ignores long lists and can echo the control line; the verifier is the reliability layer, not the prompt.

**Prompt test cases to add to the golden set:**

1. "let's meet at 5, actually 6" → only 6 survives.
2. "send it to Mark, no wait, Maria" → only Maria survives.
3. A sentence with a deliberate repetition ("very, very important") → repetition survives.
4. A sentence with a name that sounds like a common word → name preserved.
5. "the total is four hundred and fifty dollars" → `$450`, no other change.
6. An already-correct sentence → byte-identical output.
7. A question dictated as text → not answered.
8. An email transcript with no greeting → no greeting added.
9. A Slack message → no email framing.
10. A code identifier / URL → unchanged.

### 6.4 The verifier

A polish output should not be trusted just because it is fluent. **Already shipped:** the validator rejects empty/echoed/preambled/markdown/truncated/language-drifted/off-topic/over-budget output, and the P6.3 guard restores protected spans or rejects on hallucination. **Proposed:** the full table below, including format contracts and backtrack consistency.

| Check | Rule | On failure |
|---|---|---|
| **Entity preservation** | Every name, number, date, time, URL, email, file path, and code token in the input appears in the output, unless the user explicitly corrected it | Snap back the original span, or fall back to deterministic text |
| **New-content check** | No new capitalized entity or number that was not in the input/dictionary | Reject the output; use deterministic text |
| **Change budget** | Profile budget (15/30/45/100%) | Reject or downgrade to Clean output |
| **Language/script** | Same language/script as input (exists) | Fall back |
| **Format contract** | Email: no invented greeting/sign-off; Slack: no greeting; lists only when enumerated | Strip the violation or fall back |
| **Backtrack consistency** | No retracted content survives; no replacement is lost | Deterministic fix |
| **Voice drift** | Contraction rate, sentence length, function-word distribution vs input (warn only) | Show a subtle "This sounds different from you" note? (Later) |

The verifier is the difference between "the model usually behaves" and "the product guarantees it." It also makes prompt changes safe: any regression is caught before the user sees it.

### 6.5 Model routing

Users should not pick a GGUF. They should pick an outcome.

| User choice | Engine | When |
|---|---|---|
| **Fast** | SpeakoFlow Mini / S1-mini + deterministic layer | Default on CPU-only Macs; Clean/Natural |
| **Balanced** | Qwen 0.8B + deterministic layer | Default on Apple Silicon; Natural |
| **Best** | Qwen 2B + verifier + second pass | Structured/Transform; GPU/Apple Silicon with RAM |
| **Auto** | Pick by host + profile + latency budget | Default for everyone |

Show the actual model only in Advanced. A "Polish engine: Fast / Balanced / Best" segmented control is understandable; "Qwen3.5-2B IQ4_XS" is not.

### 6.6 Latency architecture

Two targets:

- **Time to first usable text:** under 150 ms. Insert the deterministic-clean text immediately, even while the model is still working.
- **Time to final polish:** under 700 ms p95 on Apple Silicon, under 1.2 s p95 on CPU for Natural; up to 2 s for Structured.

**Instant insert + refine** is the killer feature:

1. On stop, run Layer 1 and insert the deterministic-clean text immediately.
2. Run Layer 2/3 in the background.
3. If the user has not typed and the polished text passes the verifier, replace the inserted text in place with a subtle highlight and a single undo group.
4. If the user has typed, do not replace; offer "Polish this take" in the diff card instead.

This removes the perceived wait entirely and is a genuine differentiator against a cloud round trip. It needs careful undo/selection handling, but it is the highest-value latency change available.

### 6.7 Prompt/eval program

You cannot tune what you do not measure. Build:

1. **A human-labeled golden set** — 200 real takes (consented, local), each with:
   - raw transcript
   - reference "sendable" rewrite (human)
   - label: sendable as-is? (yes/no)
   - edit distance from raw
   - failure tags: meaning change, invented fact, lost fact, wrong tone, format, over-edit, under-edit
2. **Per-profile tests** — Clean/Natural/Structured/Transform, per app type (email, chat, code, docs), per language (start with English + the top 3).
3. **Automatic checks** — entity preservation, new-content, budget, format, language.
4. **Human ratings** — 1–5 fidelity, voice, usefulness; pairwise vs Wispr where possible.
5. **Regression gate** — any prompt/model change must not reduce zero-edit rate or entity preservation on the golden set.

The current `evals/report.md` is a plumbing validation (synthesised audio). It is not a quality benchmark. Say that plainly in the report so nobody mistakes 91.2 for a product claim.

---

## 7. Settings: three controls, not twelve

### 7.1 The new primary controls

1. **Polish profile**
   - Clean / Natural / Structured / Transform
   - One-line description + a live example on the user's own text
   - Default: Natural; email defaults to Structured via per-app rules

2. **When to polish**
   - Always / When it helps / Never
   - Maps to the gate; "When it helps" is the default
   - Hide the word-count threshold in Advanced

3. **Per-app rules**
   - A simple table: app icon → Off / Clean / Natural / Structured
   - Prefill from the user's history ("Detected: Gmail, Slack, Cursor, Terminal")
   - "Reset to recommended"

Then, below the fold:

4. **Voice** (optional): Concise / Warm / Direct / Formal; previewed on the user's text.
5. **Never change**: chips for names, numbers, code, quoted text, plus learned protections.
6. **Learn from my edits**: on by default; "See what Polish learned" link; delete/export.
7. **Apply automatically**: on for Clean/Natural; for Transform, show a preview before inserting.

### 7.2 Mapping the old settings

| Today | Becomes | Notes |
|---|---|---|
| `polishGateEnabled` + `polishGateThresholdWords` | **When to polish** (Always / When it helps / Never) | Threshold moves to Advanced; the pipeline's internal `polish_skip_when_clean` is not a separate user control. |
| `polishMaxChangePercent` | **Editing freedom** (Light / Balanced / Bold) in Advanced | 15 / 30 / 45; raw % still available. |
| `polishWholeTextAtEnd` | Profile default | Structured: on; Natural: per-segment for chat, whole for long; Clean: on. |
| `autoApplyTransform` | **Apply automatically** | Default on for Clean/Natural; preview for Transform. |
| `selectedLlmModel` | **Polish engine** (Fast / Balanced / Best / Auto) | Model name in Advanced. |
| `polishRules` + `polishCustomInstructions` + `polishPromptOverride` | **Polish instructions** (one list) + Advanced addendum | Only show when the engine reads it; capability badge otherwise. |
| `activeStyleProfile` (Concise etc.) | **Voice** | Keep as a voice preset, not a separate style system. |
| Tone / Structure / Context | Advanced | Only for models that honor the control line. |
| Per-app language overrides | Keep | Add to the same per-app table. |

### 7.3 Settings anti-patterns to remove

- **Inert controls.** If the selected engine ignores a setting, hide it or label it read-only with the reason. Never let a user tune a no-op.
- **Percentages as user language.** "Change up to 40% of words" is not a mental model. Use Light/Balanced/Bold.
- **Model IDs in the main flow.** Users do not know what S1-mini is. Put it in Advanced.
- **Four places to customize the same thing.** One "Polish instructions" surface, one "Voice," one "Profile."
- **Status chips as settings.** "Local provider: Loaded" belongs in a status panel, not a settings row with no action.

---

## 8. Interface: the five moments that decide retention

### Moment 1 — First run: prove the magic in 60 seconds

Current onboarding makes Polish an optional model download/choice. That is setup, not value.

**Replace it with a calibration:**

1. "Let's tune Polish to you." Three short cards:
   - a message to a teammate
   - an email to a client
   - a technical note/prompt
2. The user dictates (or picks a sample). Show Clean / Natural / Structured side by side on *their* text, with a diff.
3. Pick the default; set Gmail → Structured, Slack → Natural, Cursor → Clean as a suggested starting point.
4. "Teletype never invents words. You can always revert."

Outcome: a personalized default, a witnessed quality moment, and a trust promise before the first real dictation. This is more valuable than any model-download screen.

### Moment 2 — During: remove the polish gap

The 0.5–3 s after stop is the anxiety window. The pill already shows "Transforming…" with a sparkle. Improve it:

- **Instant text:** insert the deterministic-clean text immediately (see 6.6), then refine.
- **Never show a bare spinner for more than 400 ms.** If the model is slow, insert the deterministic text and let the refine land late.
- **Explain a skip:** "Inserted as dictated — short take, no polish needed" or "Polish is warming up." A silent no-op feels broken.

### Moment 3 — Landing: the trust check

This is the highest-leverage UI in the product. `PolishDiffCard` today is a report. Make it a **contract with recovery**:

```
Polished · 3 changes · 420 ms
[ Revert to my words ]  [ Keep ]  [ Less like this ]  [ More like this ]
------------------------------------------------------------
…the text with inline diff…
Footnote: click a change for why · "Never change 'Shivon'" · History
```

Behaviors:

- **Revert to my words** — replaces the inserted text with the raw transcript in one undo group.
- **Undo this change** — per-span action (context menu or click).
- **Why** — "Filler removed", "Grammar", "Structure", "Dictionary", "On-screen name."
- **Never change this** — select a word/phrase → add to protected spans.
- **No changes:** "Polish checked this — no edits needed. 120 ms." This is a positive signal, not a failure.
- **Fallback/skip:** "Inserted as dictated — polish skipped (server busy / short take)." Offer "Polish now."
- **Do not auto-dismiss while hovering;** 8 s default; remember "don't show this again."
- **Copy the raw** and **copy the polished** actions in the overflow.

This single card converts polish from an opaque model behavior into a controllable product. It is also the data collection surface for the learning loop.

### Moment 4 — Correction: the learning moment

When the user edits dictated text, detect the diff and offer a small, non-blocking toast:

> "Learn **Shivon** as a name?" [Keep] [Undo]
>
> "Prefer shorter sentences in Slack?" [Keep] [Undo]

Rules:

- Only ask when the edit is specific and learnable (name, term, casing, punctuation pattern, structure preference).
- Never ask twice for the same thing.
- One toast at a time; never block typing.
- Show the same items in "What Polish learned" with an undo.
- Do not learn from a single edit; require 2–3 consistent observations before it affects output, but show the candidate immediately.

This is the compounding-retention feature. Wispr learns a dictionary; Teletype can learn **how you want to be edited**, per app.

### Moment 5 — Weekly: proof of value

A local weekly recap (notification or Insights banner):

> "This week: 42 dictations, 81% sent without edits (+6), 18 edits saved, 12 minutes back. Gmail needed 2 edits; Slack needed 0."
> "Polish learned: 'Shivon', 'Q3 OKRs', shorter Slack sentences."

And a "Polish quality" card in Insights:

- **Zero-edit rate** (7/30 days, trend)
- **Edits per 100 words** (exists)
- **Corrections kept vs undone** (exists)
- **Top over-edit warnings** (e.g., "You reverted structure in Slack 4 times — lowered structure there")
- **Latency p50/p95**
- **Per-app breakdown**

This turns polish from an invisible feature into a visible habit with compounding returns.

### Secondary surfaces

- **History:** each entry shows raw vs polished, profile, model, latency, change count, and actions: *Re-run with profile…*, *Insert raw*, *Copy raw*. This makes history a recovery tool and a playground.
- **Styles screen** becomes "Compare profiles on my text" (already exists as a compare feature; retarget it to profiles, not just styles).
- **Pill:** a subtle "Structured · Gmail" tooltip; no model names in the main flow.
- **Command Mode:** use the same profile/verifier; do not create a second, inconsistent polish path.

---

## 9. The learning loop: turn edits into a moat

### 9.1 Signals already available locally

- raw transcript, polished text, final edited text (`history`, `edit_watch`)
- kept/undone after 30 minutes (learning store, `InsightsScreen`)
- app context, time, length, model, prompt/profile
- explicit thumbs (to add)
- per-span undo/protect (to add)

### 9.2 What to learn

Per `(user, app, profile)`:

| Learned thing | Signal | Effect |
|---|---|---|
| **Terminology** | repeated corrections of the same word | dictionary alias (exists) |
| **Casing/punctuation** | edits like lowercase names, Oxford comma, em-dash preference | deterministic post-pass |
| **Structure preference** | user removes bullets, splits paragraphs, shortens | lower structure budget for that app |
| **Length preference** | user consistently shortens/lengthens | tighten/loosen concision |
| **Tone** | contractions, formality edits | voice preset adjustment |
| **Protected patterns** | user reverts code/names/numbers | add to protected spans |
| **Profile choice** | kept rate per profile per app | bandit selects the best profile |

### 9.3 How to learn without being creepy or wrong

- **Local only.** No telemetry. The model of the user lives in the local SQLite store.
- **Require consistency.** 2–3 observations before affecting output; show the candidate immediately.
- **Cap drift.** Do not change more than one preference per app per week; never silently switch to Transform.
- **Explain and undo.** "Polish learned: shorter Slack sentences" with an Undo button.
- **Fail closed.** If the signal is ambiguous (focus moved, scratchpad, expired watch), learn nothing.
- **Never learn from a single word** that could be a one-off typo.

### 9.4 The bandit, explained simply

For each app, keep a small set of candidate profiles (e.g., Natural and Structured for email). Choose with a conservative bandit that maximizes kept-rate, but:

- start from the recommended default,
- require a minimum sample before switching,
- show the switch ("Gmail: Structured learned from 12 takes"),
- never explore Transform automatically.

This is the difference between "personalization" as a marketing word and a product that actually gets better.

---

## 10. Measurement and research plan

### 10.1 The metric that should run the roadmap

**Zero-Edit Rate (ZER):** the share of dictations that are sent or kept with no user edits after polish, within a defined window (30 minutes for the settle signal; immediately for "sent without edit" in email/chat).

Supporting metrics:

| Metric | Definition | Target |
|---|---|---|
| **ZER** | No edits after polish / all dictations | +10 pts vs baseline in 90 days |
| **Edit Distance Ratio** | Word edits / dictated words | Down 30% |
| **Fact fidelity** | Names/numbers/URLs/dates preserved exactly | 100% (guard-enforced) |
| **Over-edit rate** | Takes where the user reverts ≥1 polish span | <5% |
| **Meaning-change complaints** | User-marked "changed my meaning" | <0.5% |
| **Time to first usable text** | Deterministic insert | <150 ms p95 |
| **Time to final polish** | Last polished token | <700 ms p95 (Silicon), <1.2 s (CPU) |
| **D7 / D30 retention** | Active dictation days | +10% relative |
| **Dictations per active day** | Habit strength | Up |
| **PMF survey** | "Very disappointed if it disappeared" | >40% |

### 10.2 What to instrument locally (no telemetry)

- every take: raw, polished, final, profile, model, prompt version, latency, gate reason, fallback reason, change count
- every user action on the diff card: keep, revert, per-span undo, thumb, protect
- every edit: diff taxonomy (name, number, grammar, meaning, tone, format, length, other)
- weekly aggregates exported only by explicit user action ("Export my Polish report")

This is enough to run a real quality program without violating the local-first promise.

### 10.3 The research plan (run this next)

**Interviews — 10–12 users, 30 minutes each**

Screener: 4 privacy-sensitive professionals, 3 developers/PMs, 3 email-heavy operators, 2 non-native English speakers; current dictation users; mix of Wispr and non-Wispr.

Tasks:

1. Dictate a real email you would send today, then a Slack reply, then a prompt to an AI coding tool.
2. Think aloud: "What would you fix? What would you *not* want changed?"
3. Show Clean / Natural / Structured outputs on their own text; ask preference and why.
4. Show a deliberately over-edited output; ask for the trust reaction.
5. Ask: "What would make you stop using this?" and "What would make you recommend it?"

**Diary study — 8 users, 2 weeks**

After each dictation, one question: "Did you edit? Why?" with a 7-item taxonomy. This produces the failure distribution that should drive the roadmap.

**Corpus — 200 consented takes**

Human label: "sendable as-is?" plus a reference rewrite. Build the golden set described in 6.7.

**Competitive testing — same three tasks on Wispr Flow and Teletype**

Blind-rate fidelity, voice, structure, and "would send." Do not claim parity without this.

### 10.4 What we already know from public research (and its limits)

Public reviews are not a substitute for interviews, but they are strong directional evidence:

- The #1 complaint about the category leader is over-editing / meaning change, not weak cleanup.
- Users value out-of-box quality and speed, but punish unpredictability.
- Per-app style is valued; losing it is felt as a downgrade.
- Privacy is a real segment need, not just marketing.
- Users will tolerate a slightly less polished result if they trust it and can fix it.

These are hypotheses to validate, not settled facts.

---

## 11. Roadmap

### P0 — Trust and clarity (1–2 weeks)

| # | Change | Why | Effort |
|---|---|---|---|
| 1 | **Diff card v2**: revert, per-span undo, thumbs, "never change this," "why" | Converts polish from opaque to controllable; collects the learning signal | M |
| 1a | **Fix the guard word diff**: case-insensitive matching; reinsert or reject when a protected token is deleted, not just substituted; golden cases for one removed name/number and sentence-initial capitalization | A dropped name passes, and a case-only change can displace the wrong word (G9) | S |
| 2 | **Profiles + per-app defaults**, map existing budgets | Gives users a predictable contract; fixes the "one global personality" problem | M |
| 3 | **Extend the shipped capability gating** to AI Polish settings + onboarding; state the loaded model's contract (Cleanup vs Clean/Natural/Structured) | Transforms/Styles already scope controls; the first-run surfaces still don't say what the default cleanup model cannot do | S |
| 4 | **Zero-edit metric** from existing 30-min settle + final text; add to Insights | Makes quality visible and steerable | S |
| 5 | **Fix the default-model story**: auto engine, default Natural/Structured by app; do not ship a cleanup-only model as the only first impression | Closes the "clean but shapeless" first-run gap | S |

**P0 start-here map**

| # | Start here | Done means |
|---|---|---|
| 1 | `ui/src/pill.tsx` (`PolishDiffCard`), `ui/src/pillMachine.ts`, `crates/teletype-core/src/history.rs` | Revert restores the raw text for that insertion; per-span undo works; a thumb or "never change this" persists as `accepted/rejected`; the card renders without blocking the first keystroke. |
| 1a | `crates/teletype-core/src/transforms/guard.rs`, `pipeline.rs` | A single removed name, number, URL, or code identifier is reinserted or rejects the output; golden tests cover one removal of each category. |
| 2 | `ui/src/settings/DictationPolishSections.tsx`, `crates/teletype-desktop/src/commands.rs`, `crates/teletype-core/src/style.rs`, `transforms/engine.rs` | A user can pick Clean/Natural/Structured/Transform and a per-app default; the resulting prompt and edit budget are visible in one line; a Settings test shows the active contract. |
| 3 | `ui/src/settings/DictationPolishSections.tsx`, `ui/src/screens/OnboardingScreen.tsx`, `ui/src/screens/TransformsScreen.tsx` | The AI Polish section and onboarding say whether the loaded model is Cleanup-only or can do Structured; no control is shown that the active model ignores; the surfaces share one vocabulary. |
| 4 | `crates/teletype-core/src/insights.rs` (kept/undone), `crates/teletype-core/src/history.rs` (`raw_text`, final text), `ui/src/screens/InsightsScreen.tsx` | Insights shows "zero-edit rate this week", split by app, from local data; the number moves when the user reverts a polish; it can be exported/deleted. |
| 5 | `crates/teletype-desktop/src/commands.rs` defaults, `ui/src/screens/OnboardingScreen.tsx` | First-run dictation in Slack/email gets the profile and budget the user would choose; the default is no longer a cleanup-only model with no path to structure. |

### P1 — Quality and compounding (2–6 weeks)

| # | Change | Why | Effort |
|---|---|---|---|
| 6 | **Deterministic backtrack resolver + extend the shipped guard/verifier** (pre-model placeholders; quotes, paths, word-form dates) | Directly addresses the category's top trust failures; the post-model guard already protects names/numbers/code, so this closes the remaining categories and catches retractions before the model | L |
| 7 | **Onboarding calibration** on the user's own three sentences | The first magic moment; sets per-app defaults; builds trust | M |
| 8 | **Learning loop v1**: connect the existing settle/auto-apply plumbing to per-app profile choice; preference mining; "what Polish learned" | The signal and candidate flags exist; nothing feeds Polish behavior yet | L |
| 9 | **Instant insert + refine** | Removes perceived latency; differentiates vs cloud | L |
| 10 | **Extend the shipped adversarial gate** with a human golden set, per-profile prompts, and a zero-edit regression gate | The 17-case behavior gate and CI job exist; they need human sendability labels and profile coverage | L |
| 11 | **History re-polish / compare profiles** | Recovery + experimentation surface | S |
| 12 | **Per-app format contracts** (email, chat, code, docs) | Per-app style overrides exist; explicit format contracts do not | M |

### P2 — Depth and reach (6–12 weeks)

| # | Change | Why | Effort |
|---|---|---|---|
| 13 | Context-aware polish (thread/recipient/selected text) | Better fit with less user effort | L |
| 14 | Voice fingerprint + style exemplars | "Sounds like me" at scale | L |
| 15 | Second-pass verifier model for long takes | Higher zero-edit on Structured | M |
| 16 | Local experimentation framework (opt-in) | Continuous improvement without telemetry | M |
| 17 | Language expansion (only with per-language evals) | Closes a real gap vs Wispr; do not claim parity early | L |
| 18 | Command Mode parity with the same profiles/verifier | One consistent polish brain | M |

---

## 12. Risks, anti-goals, and open questions

### Risks

- **Over-editing by "improving" the prompt.** Every added capability increases the chance of meaning change. The verifier and edit budgets are not optional.
- **Latency regression.** A better model can be slower. Ship the deterministic path first; never let final polish block insertion.
- **Settings complexity returns.** Profiles can become another layer if Voice/Tone/Style/Instructions are not clearly subordinate.
- **Learning overfits.** Three edits in one app should not rewrite the user's style. Require consistency and cap drift.
- **Privacy theater.** "Local" must mean no hidden network calls, no telemetry, and a clear export/delete path.
- **Model availability.** Downloads, warm-up, RAM, and CPU-only Macs will produce inconsistent quality. Auto-routing and honest status matter.

### Anti-goals

- Do **not** compete on "most aggressive rewrite." That is Wispr's game and its weakness.
- Do **not** expose model names, prompt walls, or percentages in the main flow.
- Do **not** show a live raw transcript by default if it triggers editing anxiety; keep it optional.
- Do **not** claim parity on languages, platforms, or p99 latency without measurement.
- Do **not** add telemetry to chase metrics. Measure locally, with consent.

### Open questions for the team

1. **Primary segment:** privacy-sensitive professionals or developers? The packaging (defaults, copy, onboarding) differs.
2. **Default engine:** is Teletype willing to default to a 2B model (1.1 GB) for Structured, or keep the 0.8B cleanup model and accept weaker first-run structure?
3. **Instant insert + refine:** is in-place replacement acceptable if the user has not typed? It needs a clear undo contract.
4. **Style exemplars:** is the team comfortable using the user's own sent texts (local only) as style examples? What consent copy is required?
5. **Transform preview:** should Transform always show a preview before replacing selection/text, or stay one-shot?
6. **Pricing:** the repo's position is no subscription gate on core features. If polish is the retention engine, what is the sustainable model?

---

## 13. The one-page version

**Problem:** Polish is a trust product disguised as a text feature. The category leader wins on zero-edit quality and speed, and loses users when it changes words they meant to keep.

**Opportunity:** Be the polish that never surprises you. Local, bounded, reversible, explainable, and teachable.

**Do this:**

1. Give the user a **contract**: Clean / Natural / Structured / Transform, per app.
2. **Extend the shipped guard**: protect names, numbers, URLs, code, quotes, and dates deterministically, before and after the model.
3. **Resolve backtracks** deterministically before the model.
4. **Extend the shipped verifier**; fall back to what the user said when in doubt.
5. **Show the diff** and let the user revert, undo one change, or say "never change this."
6. **Learn** from kept/undone and corrections, per app, visibly.
7. **Prove value** weekly with zero-edit rate and time saved.
8. **Insert instantly**, refine in the background.
9. **Calibrate on day zero** with the user's own three sentences.

**Already shipped at HEAD:** the post-model protected-span + hallucination guard (changed tokens only — removals and case-sensitive mis-substitution are open), the weighted content-loss/hallucination rubric, the 17-case CI behavior gate, capability-scoped Transforms/Styles controls, and skip/fallback reasons in the pill. **Still open:** zero-edit measurement, diff-card recovery, the guard's word diff, profiles/per-app contracts, backtrack resolution, the polish learning loop, and first-run calibration.

**Measure:** zero-edit rate, fact fidelity, over-edit rate, time to first usable text, D7/D30.

**Positioning:** *Polish that never surprises you — and needs fewer edits every week.*

---

## Appendix A — Source notes

Repository evidence (verified 2026-11-19):

- `crates/teletype-core/src/transforms/gate.rs` — deterministic gate, reasons, email/list/self-correction cues, 12-word default.
- `crates/teletype-core/src/transforms/prompt.rs` — model-specific prompts, control line, prompt-info/editability, override placement.
- `crates/teletype-core/src/transforms/validator.rs` — fidelity failures, `DEFAULT_MAX_CHANGE_PERCENT = 40`.
- `crates/teletype-core/src/transforms/guard.rs` — P6.3 protected-span restore/reject, >3-change ceiling, hallucination guard; wired at `pipeline.rs:610`.
- `crates/teletype-core/src/transforms/engine.rs` — routing: card-cleanup models ignore the transform instruction; Qwen control-line paths; chunking.
- `crates/teletype-core/src/style.rs` — `StyleChannel`, `cleanup_only`, `polishes_from_card`; SpeakoFlow is fixed behaviour; S1 is control-line but cleanup-only.
- `crates/teletype-core/src/autotext/protect.rs` — placeholder protection/restore pattern.
- `crates/teletype-core/src/rescoring.rs` — dictionary rescoring never replaces protected spans.
- `crates/teletype-core/src/fillers/corpus.rs`, `tiers.rs` — existing self-repair splicing.
- `scripts/eval/polish_behavior_gate.py`, `evals/polish-adversarial/`, `.github/workflows/ci.yml` (`polish-quality-gate`) — severity-graded behavior gate and its CI wiring.
- `ui/src/screens/TransformsScreen.tsx` (`polishCustomizationFor`), `ui/src/screens/StylesScreen.tsx` — model-capability scoping.
- `crates/teletype-desktop/src/commands.rs` — defaults: SpeakoFlow Mini, auto-apply on, gate 12 words, 40% max change, whole-text-at-end off.
- `ui/src/settings/DictationPolishSections.tsx` — current AI Polish settings UI.
- `ui/src/pill.tsx` (`PolishDiffCard`) — post-insert card; no revert/feedback.
- `ui/src/screens/InsightsScreen.tsx` — kept/undone 30-min signal; "nothing applied automatically from this yet."
- `ui/src/screens/TransformsScreen.tsx`, `ui/src/components/PolishPlayground.tsx`, `ui/src/screens/StylesScreen.tsx` — customization/compare surfaces.
- `evals/report.md` — synthetic corpus, mean polish score 91.2, content-loss/bullets failures.
- `.agents/brain/PROJECT_BRAIN.md` — D012/D013 polish customization history; human corpus still needed.

Public competitor sources (accessed 2026-11-19):

- Wispr Flow, "Why Flow" — 90% zero-edit claim, "Speak naturally, send without fixing": https://wisprflow.ai/why-flow
- Wispr Flow, "Speech to text: how AI dictation apps work" — Canto 3.4% WER, 13% fewer fixed dictations, sub-700 ms target: https://wisprflow.ai/dictation-blogs/speech-to-text-how-it-works
- Wispr Flow, "Features" — cleanup, styles, languages: https://wisprflow.ai/features
- Wispr Flow Help, "Why Wispr Flow Sometimes Removes or Changes Words on Android (Smart Formatting)" — documented over-edit modes: https://docs.wisprflow.ai/articles/6568835559-why-wispr-flow-sometimes-removes-or-changes-words-on-android-smart-formatting
- Baseten customer story — p99 under 700 ms, Llama-based transcript enhancement: https://www.baseten.co/resources/customers/wispr-flow/
- AI Birdview review — "occasional over-editing" and cloud caveats: https://aibirdview.com/blog/wispr-flow-review
- r/WisprFlow — adds verbiage / must proofread twice: https://www.reddit.com/r/WisprFlow/comments/1pzyvk5/
- r/WisprFlow — quality decline, sentence structure changed with transforms off: https://www.reddit.com/r/WisprFlow/comments/1u0egnx/
- r/WisprFlow — live transcription deliberately not shown; users edit a rough draft: https://www.reddit.com/r/WisprFlow/comments/1rpqk2b/
- r/WisprFlow — per-app styles reset / removed: https://www.reddit.com/r/WisprFlow/comments/1s7yrch/
- r/WisprFlow — punctuation/capitalization/language regressions: https://www.reddit.com/r/WisprFlow/comments/1tzfwqz/
- r/WisprFlow — "removes emphasis" / pronoun changes: https://www.reddit.com/r/WisprFlow/comments/1qrn8pe/
- r/WisprFlow — "100% local" question and cloud privacy discussion: https://www.reddit.com/r/WisprFlow/comments/1nzo3cw/
- Software Lifecycle review — cloud-only, quality swings, privacy controls: https://software-lifecycle.com/software/wispr-flow
- eesel review — cloud processing, privacy history: https://www.eesel.ai/blog/wispr-flow-review

These public sources are directional. They are not a controlled benchmark, and the vendor claims are the vendor's own.
