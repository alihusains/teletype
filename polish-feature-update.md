# Polish feature update — plan of action

**Date:** 2026-11-20
**Baseline:** working tree at `e1fd01f` (main) **plus uncommitted work** — T0–T7 (`tasks/pi-reports/polish-t0…t7.md`) are present in the tree but not committed.
**Method:** read-only audit of the polish path (pipeline, guard, validator, profiles, learning stores, pill, Insights, evals) plus the existing strategy doc `docs/research/polish-retention-strategy.md`.
**Not done here:** no tests were run for this document, no live app session, no user interviews. Every claim below is from source reading; commands to confirm are in §9.

---

## 0. Read this first: the strategy doc is now out of date

`docs/research/polish-retention-strategy.md` (2026-11-19) lists nine "open" items. Six of them landed the next day as T0–T7 and are sitting uncommitted. Do not re-plan them.

| Strategy doc gap | Status in the working tree | Evidence |
|---|---|---|
| G9 guard drops removed tokens / case-sensitive diff | **Fixed** — case-insensitive, case-normalized diff; removal rejects; 4 new golden tests | `transforms/guard.rs` (`removed_name_rejects`, `removed_number_rejects`, `sentence_initial_case_plus_removed_name_does_not_displace`, `removed_name_after_insertion_rejects_instead_of_stealing_it`) |
| Profiles + per-app defaults | **Built** — Clean/Natural/Structured/Transform/Off, budgets 15/30/45, app table, capability fallback | `profile.rs` |
| Capability messaging in first-run surfaces | **Built** — onboarding/settings/Transforms copy + disabled states | `profileLabels.ts`, `ProfilePicker.tsx`, `DictationPolishSections.tsx` |
| Zero-edit metric | **Built** — 7-day rate + per-app top 3 | `insights.rs:417-456`, `InsightsScreen.tsx` |
| Deterministic backtrack resolver | **Built** — 160 unit tests, pipeline stage 0.55 | `transforms/backtrack.rs`, `pipeline.rs:386` |
| Diff card v2 | **Partial** — copy-raw, per-span inspect, thumbs, never-change, why-popover; **no live revert** | `pill.tsx:1854` |
| Learning loop v1 | **Built as a library, not wired in production** (see §2A) | `profile_bandit.rs`, `learning/feedback.rs` |
| First-run calibration | **Built** — 3 prompts × 3 profiles, applies profile + app defaults | `commands/calibration.rs:396` |
| Instant insert + refine | **Not started** | insertion still happens after the full pipeline |

The strategy doc's thesis still holds and this plan keeps it: polish should feel like a **contract, not a slot machine**. What changes is where the remaining work is.

---

## 1. What polish actually is today (verified)

```
ASR
 → scrub / dictionary / filler / spoken self-repair splicing
 → backtrack resolution            (new, deterministic)
 → ITN (numbers, times, emails, URLs)
 → intent routing (command/code takes can skip polish)
 → polish gate (short/clean takes skip)
 → profile resolution + bandit hook (bandit currently always default → inert)
 → model polish (model-specific prompt, profile line, edit budget)
 → validator (fidelity, off-topic, change ceiling)
 → guard (protected spans restored; >3 changes or hallucination → reject)
 → AutoText restore → emoji restore → screen-term snap
 → inject  →  history  →  settle (30 min)  →  Insights
```

Strong parts: the deterministic layer, the gate, the guard, the six-layer fallback discipline, and now explicit profiles. The model is not asked to be the product.

The weak part is everything **after** the text is inserted: nothing the user says about a take changes the next take in production, and the card cannot undo anything.

---

## 2. The five findings that matter (each with proof)

### A. The learning loop is write-only — the same failure as L057

`BanditStore` exists with `load`/`save` (`profile_bandit.rs:351-386`), but **no production code constructs it**:

- `pipeline.rs:305` calls `choose_take_profile(..., &BanditState::default(), ...)` — a fresh, empty state on every take.
- `insights.rs:536` does the same in `compute_learning`.
- `grep -rn "bandit" crates/teletype-desktop/src` → **zero hits**.
- `grep -rn "FeedbackStore" crates` → only its own module and the Tauri write command; **nothing reads `polish_feedback.jsonl`**.

So: profiles are chosen only from static per-app defaults, thumbs go to a file nobody opens, and "Gmail: Structured learned from 12 takes" can never appear from real usage. This is structurally the same bug the brain already records for personalization (`L057`: the loop was implemented but had no production caller and had been inert for weeks). It is the highest-leverage fix in the plan because all the hard thinking is already done.

### B. "Never change this" does nothing

The card records `NeverChange` (`pill.tsx:1847`), the store persists it (`feedback.rs`), and then:

- `feedback_outcome(NeverChange) → None` (`profile_bandit.rs:394`) — deliberately ignored.
- `guard.rs:44 collect_protected` takes `terms` only from the dictionary/boost list; it never sees the learned terms.
- The UI even admits it: **"Saved locally — T5 will use it"** (`pill.tsx:1620`).

A user who clicks the most trust-building control in the product gets a promise the code does not keep. That is worse than not shipping the button.

### C. The card cannot revert; it asks the user to paste

`pill.tsx:1854` renders **"Copy what I said"**. There is no revert, no per-span undo. The strategy doc's Moment 3 ("Revert to my words" in one action) is not implemented — T1 documented this as "strict live undo was not available".

This is the single biggest gap between the promise ("Polish that never surprises you") and the product.

### D. Insertion waits for the model

The controller emits a single post-pipeline insertion event (`dictation.rs` — "Plain dictation: polish and inject", `ProcessingDone`/injection seam around `:728-750`). Every polish millisecond is felt before any text appears. On a CPU-only Mac with the 0.8B that is a visible pause; on Structured with the 2B it is longer. Wispr's whole pitch is speed; the local-first answer is to not wait at all.

### E. Fidelity still has categories it cannot protect, and the take does not remember its contract

`guard.rs:30-42` protects lexicon/boost terms, numbers, URLs, emails, and code identifiers. It does **not** protect:

- quoted speech the user is dictating as a quote,
- bare filesystem paths (`~/Documents/projects/teletype`, `src/lib.rs` is caught only by the `.rs` rule),
- word-form dates/times that ITN leaves alone (ITN covers the common cases before polish).

And the take does not record **which contract ran**: `DictationEntry` has `raw_text`, `polish_model`, `polish_ms` (`history.rs:18-56`) but no profile or fallback reason, and the card's Why popover prints `Profile: unknown` (`pill.tsx:1888`) even though Rust already resolved it (`PipelineOutput.polish_profile` / `polish_profile_reason`).

### F. Quality is still unmeasured on real speech

- `evals/polish-adversarial/behavior-cases.jsonl`: **25** cases, none of them profile-scoped (the word "profile" does not appear in the file); no repetition or per-app format cases.
- `evals/human-corpus/manifest.jsonl`: **0 bytes**. The headline quality number remains synthetic.
- Zero-edit rate now exists in Insights but is not a gate anywhere: a prompt change can lower it and CI stays green.

---

## 3. The wedge against Wispr Flow

Wispr's own documented failure modes (dropping a leading "so", removing emphasis, changing sentence structure with transforms off, punctuation regressions) are all **fidelity** failures. Its structural constraints are **cloud round-trip** and **one globally tuned pipeline**. Teletype cannot out-polish it on average text quality yet. It can win on the four things Wispr structurally cannot offer:

1. **A contract you can name before you speak.** "Structured · Gmail" — and it is the same contract every time. Wispr ships one cleanup policy plus styles that users report resetting.
2. **A guarantee that survives the model.** Names, numbers, URLs, code, quotes: restored or the take is rejected. Say it plainly, then test it per category.
3. **Reversibility on the machine.** Real revert and per-span undo, with the model and diff local.
4. **Proof that compounds.** Weekly zero-edit rate and what was learned, computed locally, no telemetry.

Positioning stays: **"Polish that never surprises you — and needs fewer edits every week."**
Copy rule: never claim parity on languages, platforms, or p99, and never claim "cannot touch names" until Phase B is done and tested.

---

## 4. The plan

Ordering is deliberate: **close loops that already exist first** (cheap, high trust), then make trust literal, then remove the wait, then prove value.

Effort: **S** ≤ half a day, **M** 1–3 days, **L** 4+ days (one engineer, this repo's conventions).

### Phase A — Make the learning real (week 1)

The whole phase is wiring plus one policy decision. No new algorithms.

#### PF-01 — Load, use, and persist the bandit (M)

- **Why:** Finding A. Without this, profiles are static and the "learned" UI is fiction.
- **Change:** one `BanditStore` owned by `AppState` (data dir `polish_bandit.json`), loaded at startup and after Settings changes; pass the live `&BanditState` into `PipelineConfig` and `compute_learning`; save after settle, feedback, and undo. `BanditState` is already serde.
- **Where:** `profile_bandit.rs:351` (store exists), `crates/teletype-desktop/src/lib.rs` (state), `pipeline.rs:261-310`, `insights.rs:520-600`, `commands.rs` (settle path).
- **Done when:** a persisted state with a recorded `BanditChange` makes `pipeline.produce` choose the learned profile after restart; a settle outcome is visible in the file; Insights lists the change with Undo; the Undo writes the per-app override back.
- **Tests:** unit — `choose_take_profile` with a non-default state returns the learned adjacent profile; integration — `record_polish_feedback`/settle writes and reloads the same tally.

#### PF-02 — Read `polish_feedback.jsonl` at settle and promotion time (S)

- **Why:** Finding A/B. The file exists; nothing consumes it.
- **Change:** one loader in the learning store path that feeds thumbs into `feedback_outcome` and the existing promotion thresholds (`>=12 settled`, `<=1 negative`, `>= incumbent + 0.15`, one change per app per 7 days). `CopyRaw`/`NeverChange` stay non-votes.
- **Where:** `learning/feedback.rs`, `learning/promote.rs`, `learning/settle.rs`.
- **Done when:** a `thumb_down` row reduces the incumbent's score in the next promotion decision; a duplicate row from a double click does not (dedupe already implemented).
- **Tests:** throwaway JSONL fixture → promotion refuses/allows exactly per the threshold table.

#### PF-03 — Make "Never change this" enforceable (M)

- **Why:** Finding B. This is a shipped control that currently does nothing, and it is the clearest expression of the trust wedge.
- **Change (v1, no new model):** turn `NeverChange` rows into a local protected-term list, and merge it into the `terms` list the guard already accepts (`collect_protected(input, terms, boost_terms)`), i.e. the same path dictionary terms use. Protection is post-model restore-or-reject, which is already correct. Pre-model placeholders can follow in PF-08.
- **Where:** `guard.rs:44`, the guard call site in `pipeline.rs` (~`:700`), `learning/feedback.rs` reader, `commands/polish_feedback.rs`.
- **Done when:** saying "never change Siobhan", then dictating a take containing "Siobhan", prevents the model from altering **or removing** it — guard restores the token, or the take is rejected and the deterministic text is used.
- **Copy:** replace "Saved locally — T5 will use it" with "Added to protected terms".
- **Tests:** guard test with a never-change term that the model changes; guard test with one that the model removes (must reject/restore, not pass).

#### PF-04 — Tell the truth in the card and in History (S)

- **Why:** Finding E. Rust knows the contract; the UI says "unknown".
- **Change:** carry `polish_profile` + `polish_profile_reason` into the result payload the pill receives, render `Structured · Gmail` in the card header and in the Why popover; add `polish_profile` (and gate/fallback reason) to `DictationEntry` so History shows it per take. Serde-default the new fields.
- **Where:** `pipeline.rs:145-151`, `dictation.rs` (result delivery), `history.rs:18-56`, `ui/src/pill.tsx:1888`, `ui/src/screens/DictationScreen.tsx`, `ui/src/components/dictation/DictationCard.tsx`.
- **Done when:** a take in Gmail reads `Structured · Gmail` on the card and in History; a cleanup-only fallback reads `Clean · model is cleanup-only`; no surface says "unknown".
- **Tests:** ipc_contract snapshot for the new fields; one UI test asserting the label renders from a fixture.

#### PF-05 — Real revert, via the AX tier (L)

- **Why:** Findings C and D both need an "replace this text in the focused app" primitive. Building it once unlocks both.
- **Change:** reuse the shipped direct-AX injection tier (`crates/teletype-desktop/src/ax_text.rs`) which already writes at the caret and reads back. Persist, per insertion, the target element/range + inserted length + raw text. "Revert to my words" replaces exactly that range with `raw_text`, verifies by read-back, and removes the card. If AX is unavailable, focus moved, or the range no longer matches, **fall back honestly** to the current copy-raw behavior with the existing copy ("Copied what you said — paste to restore").
- **Where:** `ax_text.rs`, `dictation.rs` (store the insertion receipt), `ui/src/pill.tsx`, new command e.g. `revert_polish_take`.
- **Done when:** in TextEdit, Notes, Safari and Chrome (the apps the E2E matrix already covers) revert restores the raw text in one click and one undo group; in an app where the range cannot be proven, the card says so and offers copy instead. Nothing is replaced on a focus mismatch.
- **Tests:** command-level test with a mocked platform; the existing `scripts/e2e/` harness extended with one revert case (do not claim success from unit tests alone).

#### PF-06 — Per-span undo (M, depends on PF-05)

- **Why:** the strategy doc's Moment 3; a single revert is all-or-nothing, but users usually resent one change.
- **Change:** the card already groups changed spans. Add "undo this change" per group: replace the span's replacement with its original in the same AX range, recompute the remaining groups, keep the rest. Refuse when the range cannot be verified.
- **Where:** `pill.tsx` (`groups`), the PF-05 primitive, `history_edits` span mapping.
- **Done when:** undoing one span leaves the other polish changes intact and the text reads correctly; the card updates its count.

### Phase B — Make trust literal (week 2)

#### PF-07 — Extend protected spans (M)

- **Why:** Finding E. Quotes and paths are the remaining "it changed something I must not lose" categories.
- **Change:** add to `collect_protected`: paired quoted spans the user is dictating as a quote (open+close within the take), filesystem paths (`~/…`, `/Users/…`, `a/b/c` with an extension), and word-form dates/times that ITN did not normalize. Keep the existing rule that a capitalization-only change is not a protected change.
- **Where:** `guard.rs:30-180`, table tests at `guard.rs:659+`.
- **Done when:** one golden case per new category (a quoted phrase, a path, a word-form date) is restored or rejects; the existing 50 benign-paraphrase cases still pass — this is the guard's own false-positive floor.

#### PF-08 — Protect before the model, not only after (M)

- **Why:** post-model restore is a safety net; the model still *saw* the value and may have restructured around it, which is how dropped facts happen.
- **Change:** reuse the AutoText placeholder pattern (`autotext/protect.rs`): replace protected spans with stable placeholders before the model, restore after, and fail closed per span to the deterministic text if a placeholder is dropped or duplicated.
- **Where:** `pipeline.rs` (before the model call), `autotext/protect.rs` pattern, `guard.rs`.
- **Done when:** a take with 5 protected values round-trips byte-identically with a stub model that scrambles them; the placeholder never leaks to output.

#### PF-09 — Format contract checks in the validator (M)

- **Why:** the two most cited Wispr complaints after meaning change are invented greeting/sign-off and structure changes. `validator.rs` checks fidelity, not format.
- **Change:** add three deterministic checks with profile awareness:
  - email: no greeting/sign-off unless one existed in the input (the personalization packet already models greet/sign-off preferences, so reuse that vocabulary rather than inventing a second one);
  - chat/Slack: no "Dear …", no sign-off;
  - repetition: if the input repeats a word or short phrase deliberately (same token 2+ times in a clause) and the output drops it, treat as a fidelity failure, not a cleanup.
  On failure: strip the invented framing or fall back to the deterministic text — never block insertion.
- **Where:** `transforms/validator.rs`, `profile.rs` (format axis per profile), `pipeline.rs`.
- **Done when:** "send it to Bob, thanks" in Slack cannot become "Dear Bob, … Best regards"; "very very important" survives both Clean and Natural.

#### PF-10 — Instant insert + refine (L, depends on PF-05)

- **Why:** Finding D. This is the one latency change that removes the perceived wait instead of shaving it, and it is only possible locally.
- **Change:** on stop, run the deterministic layers, insert that text immediately, then run the model in the background. If the user has not typed and focus is unchanged, replace the inserted range in place with the polished text, one undo group, subtle highlight. If the user typed or focus moved, do **not** replace; offer "Polish this take" on the card.
- **Where:** `dictation.rs` (two-phase completion), `ax_text.rs` (replace range), `pill.tsx` (refining state + offer), a setting `polish_instant_insert` default **on** for Clean/Natural and off for Transform.
- **Done when:** time-to-first-usable-text p95 is under 150 ms on a 20-take local run; a take typed over mid-refine is never overwritten; with the model deliberately delayed 3 s the behavior is still correct.
- **Guard rails:** no replacement on any doubt; every replacement verifiable by read-back; if anything fails, the text stays as inserted.

### Phase C — Prove it (weeks 3–4)

#### PF-11 — Per-profile eval coverage and a zero-edit regression gate (M)

- **Why:** Finding F. Profiles are unmeasured; a prompt tweak can silently lower the thing that matters.
- **Change:** extend `behavior-cases.jsonl` from 25 to ~60 cases with a `profile` field: per-profile tone/structure expectations, repetition preservation, greeting/sign-off, format-per-app, plus one case per new protected category from PF-07. Add a replay-mode test that asserts each profile's output stays inside its budget (15/30/45) and that protected spans survive. Wire it into the existing `polish-quality-gate` CI job, which already runs model-free checks.
- **Where:** `evals/polish-adversarial/`, `scripts/eval/polish_behavior_gate.py`, `.github/workflows/ci.yml:573`.
- **Done when:** a deliberately over-editing fixture still fails, a correct fixture passes, and each profile has at least 8 cases; `--self-test` and `--list-corpus` stay green.

#### PF-12 — Weekly polish report (M)

- **Why:** the strategy doc's Moment 5; all the data already exists (zero-edit 7d, per-app, kept/undone, profile learnings, polish ms).
- **Change:** a local recap card at the top of Insights: zero-edit rate this week vs last, takes settled, top app by reverts, "Polish learned: X" from PF-01's `profileLearnings`, estimated minutes saved from word count. No notification in v1 (a system notification is a separate decision).
- **Where:** `insights.rs` (comparison + `prev_7d` fields), `InsightsScreen.tsx`.
- **Done when:** the card renders for a fresh install (honest empty state) and for a seeded week; numbers move when a take is reverted; nothing renders a rate with a zero denominator.

#### PF-13 — Over-edit signal that lowers ambition (M, depends on PF-01/05)

- **Why:** the bandit currently learns *profile choice* but not *how much to change*; the top public complaint is over-editing, so the product should notice it.
- **Change:** count reverts and `thumb_down`s per `(app, profile)`; when a take is reverted after a Structured polish, record a negative for that profile **and** propose the adjacent more conservative profile (Structured→Natural→Clean), subject to the existing sample minimum and once-per-7-days cap. Show it as a normal learned row with Undo.
- **Where:** `profile_bandit.rs` (`Outcome`, `decide`), `promote.rs` thresholds, `insights.rs`.
- **Done when:** 6 reverted Structured takes in Gmail propose Natural, with a visible row and working Undo; it never proposes Transform or Off.

#### PF-14 — Engine routing by outcome, not model name (optional, decision-gated)

- **Change:** a Fast / Balanced / Best / Auto selector that maps to the existing catalog and profile budgets; model IDs move to Advanced. Auto picks by host and profile.
- **Gate:** needs the open question in §8 #2 answered first (whether shipping a 2B default is acceptable). Do not start before that.

---

## 5. Sequence and dependencies

| Order | Task | Effort | Depends on |
|---|---|---|---|
| 1 | PF-01 bandit wired | M | — |
| 2 | PF-02 feedback read | S | PF-01 |
| 3 | PF-03 never-change enforced | M | PF-02 |
| 4 | PF-04 contract visible | S | — |
| 5 | PF-05 revert via AX | L | — |
| 6 | PF-06 per-span undo | M | PF-05 |
| 7 | PF-07 more protected spans | M | — |
| 8 | PF-08 protect before model | M | PF-07 |
| 9 | PF-09 format contracts | M | — |
| 10 | PF-10 instant insert + refine | L | PF-05 |
| 11 | PF-11 eval coverage | M | PF-07/09 |
| 12 | PF-12 weekly report | M | PF-01 |
| 13 | PF-13 over-edit signal | M | PF-01/05 |
| 14 | PF-14 engine routing | M | open decision |

Week 1: 1–4 (all wiring, no model risk). Week 2: 5–9. Weeks 3–4: 10–13.

Two things can run in parallel: the AX primitive (PF-05/06/10) and the guard/verifier track (PF-07/08/09). Keep them on disjoint files as `AGENTS.md` requires.

---

## 6. Metrics that decide whether this worked

Local only, no telemetry. All of these are computable from stores that already exist.

| Metric | Now | Target | Where it comes from |
|---|---|---|---|
| Zero-edit rate (7d) | measured, ungated | +10 pts in 90 days | `insights.rs:417` |
| Reverted takes | not counted | < 5% of polished takes | PF-05/13 |
| Fact fidelity | guard-enforced for 5 categories | 100% across 8 categories | PF-07 tests + gate |
| Time to first usable text | = time to polish | < 150 ms p95 | PF-10 |
| Time to final polish | unmeasured in Insights | < 700 ms p95 (Silicon), < 1.2 s (CPU) | `polish_ms` exists |
| Learned-profile changes | 0 in production (inert) | > 0 per active app after 2 weeks | PF-01 |

The gate for any prompt/model change becomes: **zero-edit rate on the human corpus must not drop, and fact fidelity must not drop.** Until the human corpus has takes, say so plainly instead of quoting a synthetic score.

---

## 7. Non-goals

- **No new prompt walls.** The 0.8B ignores long rule lists and can echo them (`t8`). New rules go in the verifier; the lean prompt gains at most one line.
- **No telemetry.** Not even "just crashes". The privacy stance is the product.
- **No competing on rewrite aggressiveness.** That is Wispr's game and its weakness.
- **No percentage sliders in the main flow.** Budgets are profile properties.
- **No second polish brain for Command Mode.** It uses the same profile, verifier, and guard.
- **No auto-switch to Transform/Off ever.**

---

## 8. Open decisions needed from Ali

1. **Default profile.** Ship `Natural` globally, or `Structured` for email apps on day one? Calibration already writes `sensible_app_defaults()`; the question is whether that is aggressive enough for the first impression.
2. **Default engine.** A cleanup-only model cannot honor Structured. Do we accept a ~1.1 GB 2B download as the recommended first-run engine, or keep the 0.8B and treat Structured as an opt-in?
3. **Instant insert + refine** needs in-place replacement after insertion. Is that acceptable when the user has not typed? (PF-10 assumes yes, with an undo group and a setting.)
4. **Never-change scope.** Per-term forever, or per-term per-app? v1 in PF-03 assumes global; per-app is more correct but needs a store change.
5. **History retention** for the AX insertion receipt (used by revert). One take, one day, or until the next dictation? Longer retention means better revert, more memory of what was on screen.

---

## 9. How to verify this plan's claims

```bash
# A. The loop is inert (expect zero production callers)
grep -rn "BanditStore\|FeedbackStore" --include=*.rs crates | grep -v "profile_bandit.rs\|feedback.rs\|polish_feedback.rs"
grep -n "BanditState::default()" crates/teletype-core/src/pipeline.rs crates/teletype-core/src/insights.rs

# B. The guard's current coverage and its removal tests
sed -n 30,60p crates/teletype-core/src/transforms/guard.rs
cargo test -p teletype-core transforms::guard

# C. The card has no revert
grep -n "Copy what I said\|Revert\|Profile:" ui/src/pill.tsx

# D. The take does not record its profile
grep -n "pub raw_text\|pub polish_model\|pub polish_ms" crates/teletype-core/src/history.rs

# E. Eval corpus size
wc -l evals/polish-adversarial/behavior-cases.jsonl && python3 scripts/eval/polish_behavior_gate.py --list-corpus

# After each task
cargo test -p teletype-core -p teletype-desktop
cd ui && npm run typecheck && npm test
```

Uncommitted state note: T0–T7 add ~2,700 changed lines plus 4,500 new lines across 26 files. Land or at least commit them before starting Phase A, otherwise the diff above keeps growing and every later change gets harder to review.

---

## 10. One paragraph

Polish already has the right bones: a deterministic layer, a gate, a model-free fidelity guard, explicit contracts, a backtrack resolver, a zero-edit metric, and a calibration flow. What it lacks is **connection and consequence**: the bandit is never loaded, feedback is never read, "never change this" protects nothing, the card cannot undo, and insertion waits for the model. Fix those five in that order — they are wiring plus one AX primitive, not new research — then extend the guard to quotes and paths, add format contracts, and let the evals prove it. That turns polish from a feature users hope behaves into a contract users can predict, verify, reverse, and teach — which is the one thing Wispr Flow structurally cannot offer.
