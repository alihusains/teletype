# BUG-001 — Personalization loop records rewordings as mishearings

**Severity:** P0
**Area:** personalization / learn.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `learn.rs` + its test

---

## Description

The personalization loop's "judge" is not EW's judge. `terminology_swap` in
`learn.rs:242-268` fires on **any** mid-sentence word replacement where both
words are alphabetic, length ≥ 4, and differ by ≤ 3 characters. It does not
distinguish a *mishearing* (the user correcting a word the ASR got wrong) from
a *rewording* (the user choosing a different synonym).

EW's `RulesCorrectionJudge.swift` (559 lines) exists specifically to answer
"was this edit a mishearing or a stylistic change?" via 8 deterministic rules
(spelled-letters, identical-letters-never, numeric-rendering, residual-empty,
structure/permutation, closed-class grammar tables, Levenshtein+Dice+Soundex
residual similarity ≥ 0.55, name-shape ≥ 0.30). It **actively vetoes** grammar,
punctuation, and rewording edits. The Teletype port dropped the judge entirely.

### Concrete failure

User dictates: `"We value our clients."`
User edits to: `"We value our customers."`

Teletype records `term:clients→customers` as a learned preference
(`learn.rs` test `terminology_swap_is_a_signal` at `:329-334` codifies this as
correct behavior). The phrase `"use 'customers' instead of 'clients'"` is
injected into every future polish prompt for that app scope.

This is a **rewording, not a mishearing**. EW's judge would classify it as
such and never persist it.

### Why it compounds

- `apply_signals` (`learn.rs:123-150`) writes the preference at **count 1**.
- Confidence only gates *relevance* (`mod.rs:106-112`, thresholds 2/4/8);
  `relevant()` (`mod.rs:223-239`) does **not** filter below Weak.
- A single-observation learned preference is already injected into the prompt
  packet (`packet.rs:24-44`).

So one stylistic edit becomes a permanent rule that steers the user's AI
output toward their one-off choice, in every future dictation for that app.

### Secondary issue: the observation point is too narrow

`observe_pending_edit` (`dictation.rs:1288-1377`) reads the **currently
focused** field at the next dictation, not the field that received the previous
dictation. And `edit_watch.rs:170-192` (`diff`) only understands *appends*
after the inserted text. An **in-place edit inside the inserted text** (the most
common correction: fixing a misheard word) makes the diff `Unrecognisable`
and nothing is learned. So the loop that was "actually wired" (brain §1) only
learns appends — the least likely case to be a real correction.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-core/src/personalization/learn.rs` | 55-120 | `extract_signals` — the weak extractor |
| `crates/teletype-core/src/personalization/learn.rs` | 242-268 | `terminology_swap` — the specific bug |
| `crates/teletype-core/src/personalization/learn.rs` | 123-150 | `apply_signals` — writes at count 1 |
| `crates/teletype-core/src/personalization/mod.rs` | 106-112 | `from_count` — confidence thresholds |
| `crates/teletype-core/src/personalization/mod.rs` | 223-239 | `relevant()` — does not filter Weak |
| `crates/teletype-core/src/personalization/packet.rs` | 24-44 | injects prefs into prompt |
| `crates/teletype-desktop/src/dictation.rs` | 1288-1377 | `observe_pending_edit` — wrong field |
| `crates/teletype-desktop/src/edit_watch.rs` | 170-192 | `diff` — append-only |
| `ew/EnviousWispr/Sources/EnviousWisprPostProcessing/RulesCorrectionJudge.swift` | — | EW reference (559 lines) |

## Reproduction

1. Select a speech model and an LLM model (EG-1 or any local model).
2. Enable the personalization learning toggle (currently unreachable — see BUG-012).
3. Dictate into Gmail: `"We value our clients."`
4. Edit the inserted text to: `"We value our customers."`
5. Dictate again into Gmail: `"We value our clients."`
6. The personalization profile now contains: `Prefer 'customers' instead of 'clients'` (scope: Email).
7. On the next dictation that produces "clients" in an email, the polish prompt
   contains `"use 'customers' instead of 'clients'"` and the model rewrites it.

## Unit test cases (must pass after fix)

```rust
// 1. A synonym swap must NOT be recorded as a terminology signal.
#[test]
fn synonym_swap_is_not_a_mishearing() {
    let signals = extract_signals(
        "We value our clients.",
        "We value our customers.",
        &app(),
    );
    assert!(
        !signals.iter().any(|s| s.key.starts_with("term:")),
        "clients→customers is a rewording, not a mishearing"
    );
}

// 2. A true mishearing (ASR error) MUST still be recorded.
#[test]
fn misheard_name_is_still_a_signal() {
    // "ic margets" is a known dictionary correction, not a rewording.
    let signals = extract_signals(
        "I work at ic margets.",
        "I work at IC Markets.",
        &app(),
    );
    assert!(
        signals.iter().any(|s| s.key.contains("margets")),
        "a dictionary-known correction must still be learned"
    );
}

// 3. A single-observation preference must NOT be injected into the prompt
//    packet until it reaches at least Weak confidence (count ≥ 2).
#[test]
fn single_observation_pref_is_not_injected() {
    let mut profile = UserProfile::default();
    let signals = extract_signals("Dear A,", "Hi A,", &app());
    apply_signals(&mut profile, &signals);
    let packet = build_packet(&profile, &ctx());
    assert!(
        !packet.phrases.iter().any(|p| p.contains("Hi")),
        "count-1 preference must not reach the prompt"
    );
}

// 4. The diff must detect an in-place edit inside the inserted text,
//    not only an append after it.
#[test]
fn in_place_edit_inside_insert_is_recognised() {
    let inserted = "We value our clients.";
    let current  = "We value our customers.";
    let d = diff(inserted, current);
    assert!(d.was_edited(), "in-place word swap inside the insert must be Edited, not Unrecognisable");
}
```

## Acceptance criteria

- [ ] `terminology_swap` does NOT fire when both words are common dictionary
      nouns (i.e., the swap is a synonym/rewording, not a mishearing).
- [ ] A swap where one word is a known dictionary entry (or an alias) and the
      other is not, IS still recorded (true ASR correction).
- [ ] A preference at count 1 is NOT injected into the prompt packet.
- [ ] `diff()` in `edit_watch.rs` recognises an in-place word replacement
      inside the inserted text as `Edited`, not `Unrecognisable`.
- [ ] The existing test `terminology_swap_is_a_signal` is **deleted or
      inverted** — it currently asserts the bug is correct.
- [ ] `cargo test -p teletype-core` passes with the new tests.

## How to test (manual / smoke)

1. **Smoke (2 min):**
   - Open the app, select EG-1, enable personalization learning.
   - Dictate "We value our clients." into Gmail.
   - Edit the inserted text to "We value our customers."
   - Dictate again into Gmail.
   - Open Personalization screen → Learned Preferences.
   - **Pass:** no `clients→customers` entry exists.
   - **Fail:** the entry exists.

2. **Regression (5 min):**
   - Dictate a sentence containing a known misheard term (e.g., "ic margets").
   - Edit to the correct "IC Markets".
   - Dictate again.
   - **Pass:** the correction IS learned (the loop still works for true
     mishearings).

3. **Prompt injection check (5 min):**
   - After any learned preference, inspect the last polish prompt (Developer →
     Show Last Prompt, or log).
   - **Pass:** count-1 preferences are absent from the prompt.

## Fix direction (not implementation)

Port the core of EW's `RulesCorrectionJudge` into a Rust `CorrectionJudge`
trait with at least:
- A Dice-coefficient residual-similarity gate (≥ 0.55) on the swapped pair.
- A closed-class veto: if both words are in the app's dictionary or in a
  common-noun stoplist, reject the signal.
- A structural gate: reject if the swap changes sentence structure
  (word count differs, or the swap is at a clause boundary).

Minimum viable fix (if porting the full judge is too large for this sprint):
- In `terminology_swap`, reject the signal when **both** words pass
  `dictionary.contains(a) && dictionary.contains(b)` (i.e., both are valid
  English words → it's a synonym swap, not a mishearing).
- Gate `apply_signals` on `count >= 2` before the phrase enters the packet.

## Related

- BUG-012 (personalization screen unreachable — the learning off-switch the
  user needs to undo a wrong correction is not navigable)
- EW reference: `RulesCorrectionJudge.swift`, `ObservedCorrectionWatcher.swift`
