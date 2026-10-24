# L055 - Personalization is wired and gated, but its only accuracy instrument has no writer

- ID: L055
- Title: Personalization is wired and gated, but its only accuracy instrument has no writer
- Date: 2026-10-23
- Agent/tool: Qoder (read-only audit while answering a roadmap question)
- Project area: personalization / learning / eval
- Status: verified for reachability and a live dictionary write (L057, 2026-10-24); effectiveness still unmeasured

## What is true today

Three personalization paths exist and all are on by default
(`learn_from_edits`, `learn_app_specific`, `learn_terminology` are all `true`
in `crates/teletype-core/src/personalization/mod.rs:174`):

1. **Edit watch -> dictionary.** After a take, `dictation.rs` reads the focused
   field, diffs it against what was pasted, judges each swap
   (`is_learnable_terminology`), and writes an alias onto the D006
   `DictionaryWord` (`dictation.rs:4690-4720`). The Dictionary tab shows
   "N auto-learned" with provenance, and a "Learned: X" toast offers Undo.
2. **Dictionary -> text.** `correct_with_dictionary`
   (`crates/teletype-core/src/pipeline.rs:719`) rewrites exact/alias matches and
   distance-2 near misses for words flagged fuzzy.
3. **Preferences -> polish prompt.** Learned style preferences are injected as
   instructions before polishing.

## What is weaker than it looks

- **The lexicon path is inert.** `boost_capacity` returns 0 for every engine
  (`crates/teletype-desktop/src/dictation.rs:6606`, D029/ADR-018), so the
  learning store's lexicon never reaches a decode. Contacts import
  (`crates/teletype-desktop/src/contacts_cmd.rs:107`) writes *only* that
  lexicon, not the dictionary, so importing contacts currently changes nothing
  about dictation output.
- **No measured accuracy number.** `grep` over `evals/` finds no dictionary or
  personalization axis. `AccuracyPoint` and `LearningSummary::replay` exist
  (`crates/teletype-core/src/insights.rs:246`) and the Insights screen renders
  an "Error rate - check" card from them (`ui/src/screens/InsightsScreen.tsx:1162`),
  but nothing in `crates/` constructs or stores a replay run, so the card never
  appears. The measurement instrument is built and unwired.
- **Observation is narrow.** Learning needs: the same app still frontmost, a
  readable focused field, the whole inserted text still present verbatim, and a
  settle event (Enter/send or the 20 s poll, see L037). A user who fixes text
  later, or in a field the AX layer cannot read, teaches the app nothing.
- **The judge is deliberately conservative**: 1-2 word corrections, numbers,
  URLs, code and markdown excluded, number-word-to-digits excluded. A
  lexicon-only promotion still creates no undo offer (known gap in L033).

## Reusable lesson

Before calling personalization "working", separate the three claims: the loop is
*reachable* (yes, two bugs fixed in Oct 2026), the loop is *tested* (123 unit
tests across `learning/*`), and the loop is *effective* (unmeasured; the one
instrument is unwired). Roadmap or marketing statements should only use the
first two until the replay writer exists and a dictionary axis joins the eval
suite.

## Update 2026-10-24

`scripts/e2e/learn_from_edits_macos.sh` now proves the first claim on the real
accessibility path: a real take into TextEdit, a real keystroke correction, the
production watcher's AX read-back, and a learned dictionary alias with
`learnedFrom`. Getting there fixed four independent breaks that each made the
loop inert; see L057. This raises the loop from "wired" to "observably writes a
learned word", but it does not measure accuracy, and the lexicon boost
(`boost_capacity = 0`, D029) is still inert. The third claim is unchanged.

## Evidence

- `crates/teletype-desktop/src/{dictation.rs,contacts_cmd.rs,commands/personalization.rs}`
- `crates/teletype-core/src/{pipeline.rs,insights.rs,personalization/mod.rs}`
- `ui/src/screens/{DictionaryScreen.tsx,InsightsScreen.tsx}`
- `grep -rn "replay_runs\|replayRuns" crates/ ui/src/` -> no hits
- 123 `#[test]` functions across `crates/teletype-core/src/learning/*.rs`
