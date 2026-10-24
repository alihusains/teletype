---
id: L057
title: Learn-from-edits now passes a live AX-path check, after four independent breaks
status: verified
type: debugging
area: "personalization / edit watch / e2e harness"
agent: "qoder"
tool: "cargo test, cargo build, scripts/e2e/learn_from_edits_macos.sh, TextEdit + System Events"
date: 2026-10-24
---

# Learn-from-edits now passes a live AX-path check, after four independent breaks

## Problem

Question 3 of the personalization audit (L055) had no live proof: unit tests
covered the judge and the extractor, but nothing showed that a real take, a real
paste, a real keystroke correction and a real AX read-back end up as a learned
dictionary alias. The new harness
`scripts/e2e/learn_from_edits_macos.sh` is that proof. Getting it green exposed
four independent breaks, each sufficient on its own to make the loop inert.

## Symptoms

- The harness recorded a take, pasted it, typed the correction, and the
  learning store recorded `observed: "Shivon Rourke, ..."` with a replace op,
  but the dictionary stayed empty (`dictionary has the learned alias: 0`).
- Logs ended at `edit watch: field read (42 chars)` with no "learned" and no
  "no learnable edit" line. A `sample` of the process showed the
  `teletype-dictation` thread parked in `Receiver::recv`, so the handler had
  returned normally and something inside the learning path had filtered it out.

## Root causes, in the order they were found

1. **The e2e slot was looked up before AppState existed.**
   `Controller::spawn(app)` did `app.try_state::<AppState>()` to find the e2e
   slot, but AppState is managed only after `spawn` returns (it owns the
   controller). The lookup always found `None`, so the whole e2e hook was
   silently disabled and harness takes recorded from the real microphone.
   Fixed by passing the slot into `spawn` from the caller that has it.

2. **File input replaced only the streaming source.**
   The e2e feed pushed frames into the streaming committer, but the final
   decode read the real mic buffer, so every harness take transcribed room
   noise. Fixed by returning `Recording::synthetic()` for e2e takes and pushing
   fed frames into the release buffer as well.

3. **`go_idle` cleared the watch that the paste had just armed.**
   `go_idle` runs after a successful paste, and `cancel_learning_watch`
   dropped the pending edit every time, so the 15 s check always found
   `nothing pending to observe`. Fixed by not clearing the watch in `go_idle`;
   observation points consume or drop a stale watch instead.

4. **A shortened in-place correction was rebuilt as an append.**
   `observe_pending_edit` decided append-vs-in-place with
   `replacement.len() >= inserted.len()`. `Shavano` (7) to `Shivon` (6) is one
   character shorter, so the corrected take was concatenated with the old one.
   `terminology_swap` then saw a 6-word vs 12-word string and returned no
   signal, so nothing was written and nothing was logged. Fixed by making the
   diff state the case explicitly: `EditDiff::InPlace { current }` plus
   `EditDiff::edited_text(inserted)`. This is the class of every correction
   that removes a character, not just this fixture.

The first-word fix in `in_place_edit` (phonetic fallback at the judge's 0.6
band) is what lets a corrected first word be recognised at all. It is a fifth
fix, but it was already in place before the silent-stop debugging started.

## Fixture constraints worth keeping

`names.wav` says "Siobhan O'Rourke, Aoife, PyTorch, Kubernetes" and
parakeet-ultra transcribes it as the fixture text in the harness. Three
apparent corrections cannot prove the write path:

- `hello` is an everyday word, and the judge correctly refuses to learn it.
- `Siobhan` scores `phon_sim = 0.476` against `Shavano`, below the 0.6
  mishearing floor. The phonetic respelling `Shivon` scores 0.913.
- `O'Rourke` is skipped by the term extractor because `is_word` accepts only
  alphanumerics and `-`. TextEdit also converts `'` to the smart quote U+2019.

Reading TextEdit from System Events is also fragile: the AX text lives in a
nested scroll area, and a second document window can answer instead of the
front one. The harness reads `text of front document` through TextEdit's own
AppleScript model and closes extra documents before the take.

## Verification

```
scripts/e2e/learn_from_edits_macos.sh
take text:   Shavano Rourke, Epha, Py Torch, Kubernetes.
field after edit: Shivon Rourke, Epha, Py Torch, Kubernetes.
dictionary has the learned alias: 1
learning store recorded the observed edit: 1
PASS  learn-from-edits works on the real accessibility path ("Shavano" -> "Shivon")
```

Unit coverage added with the fix:

- `edit_watch::a_shorter_correction_is_the_learnable_text_not_a_concatenation`
- `edit_watch::an_append_edit_still_rebuilds_from_the_inserted_text`
- `commands::personalization::learn_terminology_accepts_a_shortened_name_correction`

`cargo test -p teletype-desktop --lib` is 330 green, `cargo test -p teletype-core`
green, `cargo fmt --all --check` clean.

## Reusable lesson

An observation loop can be "wired" and still inert at four different seams.
When a handler logs nothing after an input it clearly consumed, the silence is
the bug: enumerate every branch between the input and the write, and prefer an
explicit state (in-place vs append) over a heuristic inferred from lengths.
Harnesses also need a positive control that does not depend on the engine
guessing the fixture the way you expect: pick a correction that the judge's
phonetic band and the extractor both accept by construction.
