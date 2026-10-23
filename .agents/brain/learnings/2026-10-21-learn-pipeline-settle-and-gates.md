---
id: L-20261021-learn-pipeline-settle-and-gates
title: Where the post-dictation learning loop actually loses edits
status: verified
type: operational
area: personalization / learning pipeline
agent: general-purpose research agent (claims) + Qoder lead session (verification)
tool: Qoder CLI
date: 2026-10-21
---

# Where the post-dictation learning loop actually loses edits

## Problem

Ali asked how Teletype learns from post-dictation corrections, and how the
EnviousWispr reference (`ew/EnviousWispr`) does it better. The answer turned out
to be a set of specific places where a correction the user clearly made is never
learned. Each claim below was re-checked against the code by the lead session;
one research-agent claim was found to be wrong and is recorded as such.

## The pipeline, as it really runs

1. A take is pasted; `edit_watch` arms a `PendingEdit`
   (`crates/teletype-desktop/src/edit_watch.rs:67`).
2. `ObserverState` is a trigger state machine that decides *when* to read the
   field back (`crates/teletype-core/src/learning/observer.rs:246-256`).
3. The read is aligned to the inserted region by banded Levenshtein
   (`observer.rs:144`), diffed, and each changed span is judged
   (`learning/judge.rs`).
4. Only spans judged `Mishearing` reach `consider_observed_edit`
   (`learning/promote.rs:229`), which may write a lexicon row.
5. The lexicon feeds the next take's ASR boost list and the polish prompt.

## Finding 1: three of the six end-reasons are never dispatched in production

`ObserverEvent` declares `Paste, FocusLost, EnterPressed, Keystroke,
NextDictationStarted, Canceled`. Production feeds it only
`NextDictationStarted` (`dictation.rs:4910`) and `Canceled` (`dictation.rs:5015`),
plus the 20 s idle poll. `FocusLost`, `EnterPressed` and `Keystroke` appear
*only* in the unit tests at `edit_watch.rs:610-806`.

Consequence: the most common real flow, dictate, fix one word, press Enter or
Send, is not settled at the moment the user finishes. The observer waits for the
next dictation or the idle poll, and by then chat and mail clients have cleared
the field, so the anchored text is gone and the edit is dropped.

The reference implementation treats a send-shaped end as a settle point with a
closed set of thirteen end reasons
(`ew/EnviousWispr/.../PastedRegionObserver.swift:35-64`), which is exactly the
case Teletype loses.

**Partially fixed 2026-10-22:** `EditWatch` now keeps a memory-only
`LastGoodRead` (the newest field read that still located our paste and differed
from the inserted text), and `observe_decision` falls back to it when the field no
longer locates the paste, behind a 1 s quiet guard (`FLUSH_MIN_QUIET_MS`). Both
the `NextDictationStarted` trigger and the 20 s idle poll pass the cache, so a
message sent after a fix can still be learned. Remaining: the cache is refreshed
only by the 2 s prewarm tick (a fix typed and sent within one tick still misses;
the reference polls at 750 ms with accessibility change notifications), and the
preference/dictionary path in `observe_pending_edit` still re-reads the field
blind, as Finding 2 describes.

## Finding 2: promotion re-reads the field blind instead of reusing the anchored read

`dictation.rs:4533` calls `state.platform.focused_text()` again at promotion
time and returns early when the frontmost app changed or the field is
unreadable, logging "focus moved, skipping observation" / "field not readable,
skipping". They are logged (Info) but not **counted** and never surfaced, so the
loss rate is unknown: "learning is rare here" and "learning is broken here" look
identical from outside. The reference counts every drop with an explicit
`DropReason` (`ew/EnviousWispr/.../EditAlignment.swift:59`).

## Finding 3: only proper-noun-like terms can ever be learned

`consider_observed_edit` is called with `JudgeContext::default()`
(`dictation.rs:4590`), so `min_conf` is `None` (`judge.rs:60-63`). The
mishearing branch is `sim >= 0.6 && (low_conf || proper)` (`judge.rs:602`), and
with `min_conf: None`, `low_conf` is always false. Therefore `to` must be
`proper_noun_like`. Ordinary misheard vocabulary can never promote. This is
defensible as a design choice (learn names and jargon, not the dictionary) but
it is not documented anywhere in the UI, so the Personalization screen overstates
what the loop does.

**Fixed 2026-10-22:** the production call site now passes
`JudgeContext { allow_lowercase_terms: true, ..Default::default() }`
(`dictation.rs:4590`), and the mishearing branch also accepts
`ctx.allow_lowercase_terms && to_words.len() <= 2` (`judge.rs:609-611`). Every
other call site keeps the default, so tests and `promote` behaviour are
unchanged. Everyday words are still rejected by `word_list_skip`. Known edge: the
2-word cap applies only to the correction (`to`) side, so a 3-word misheard
phrase corrected to a 1-2 word lowercase term can also promote.

## Finding 4 (fixed): `undo` cleared the wrong candidate key

`promote::undo` tombstoned the lexicon row and then called
`clear_learn_candidate("mishearing", "", term, "")`, while candidates are written
keyed by alias at `promote.rs:159` and cleared correctly by alias at `promote.rs:192`.
`clear_learn_candidate` matches `a = ?2` exactly with no wildcard
(`learning/store.rs:608-620`), so the undo path deleted nothing. The existing
`tombstone_is_permanent` test could not catch it because the candidate it asserts
on had already been cleared by promotion.

Fixed by reading the term's alias list from the lexicon row before tombstoning
and clearing each one, with a regression test
(`undo_clears_a_candidate_written_under_a_stale_observation`) that fails against
the old code. Severity was low (stale rows are inert behind the tombstone); the
doc comment claiming "clears every candidate row for that term" was the lie.

## Finding 5: the lexicon only reaches the ASR on engines that can boost

`build_boost_terms` (`dictation.rs:4728`) reads every live lexicon row and hands
the list to `provider.set_boost_terms` (`dictation.rs:2851`), so the loop is
genuinely wired, not aspirational. The boundary is `boost_capacity(kind)`
(`dictation.rs:6426`): Fluid 100, Sherpa 50, Whistle 50, Whisper 30, and
**`Parakeet | ParakeetCpp => 0`** — an early `return Vec::new()` at `:4738`.

That is not a hole on either recommended default: `Engine::ParakeetCoreml` maps
to `SpeechEngineKind::Fluid` (`dictation.rs:6018`), so Parakeet Ultra on Apple
Silicon boosts at the largest capacity in the table, and the Windows default
(sherpa int8) boosts at 50. It is a hole on the ggml Parakeet v3 fallback that
older/Intel Macs get.

The in-code comment says "post-ASR rescoring covers it", and
`rescore_words` does run (`dictation.rs:5649`) — but it is built from
`dictionary.words` + vocab packs, **not** from the lexicon. The two stores only
meet on the personalization path: a learned *preference* is mirrored into the
dictionary by `dict.insert_learned` (`dictation.rs:4666`, gated by
`is_learnable_terminology`), whereas a pure lexicon *promotion*
(`consider_observed_edit` → `out.created_terms`) never touches the dictionary.
So a lexicon-only promotion is inert on ggml Parakeet. Severity is low because
that engine is the fallback, not the default; the fix, if it is wanted, is to
union the live lexicon into the rescore term list rather than to teach ggml
Parakeet to boost.

## A research claim that was wrong

The background agent reported "Non-macOS `read_focused_field` returns
`Unreadable`: no learning on Windows at all", citing `dictation.rs:5962/5970`.
That is incorrect: `dictation.rs:5961-5966` is the `#[cfg(target_os = "windows")]`
function and it reads through `uia::focused_control_text()`. Only the
`#[cfg(not(any(target_os = "macos", target_os = "windows")))]` fallback returns
`Unreadable`. Lesson: an agent's line-number citation is only evidence once the
lines have been read.

## Verification

`cargo test -p teletype-core --lib learning::promote` passed 14 tests including
the new one. Findings 1-3 were confirmed by reading the cited lines directly
(`grep` for `ObserverEvent::` across `crates/teletype-desktop/src`,
`sed` on `dictation.rs:4525-4600`, `judge.rs:590-612`).

## Reusable lesson

The learning loop has two independent chokepoints that no test covers: *when* it
reads the field back (only three of six end-reasons are wired) and *what* it is
allowed to learn (only proper-noun-like spans). A complaint that "it never
learned my fix" is far more likely to be one of those than a bad judge score.
Measure the drop count before tuning the thresholds.

Related: L033 (the Phase 7 reachability gotcha this extends),
`learnings/2026-10-21-agent-wrote-after-failure-signal.md`.
