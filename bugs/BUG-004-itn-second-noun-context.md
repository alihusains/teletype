# BUG-004 — ITN converts "second" (and other ordinals) to digits in noun contexts

**Severity:** P0 (high-frequency text corruption)
**Area:** transforms / itn.rs
**Status:** open
**Found:** 2026-09-28 QA audit, **verified by running `itn::normalize` directly**

---

## Description

The ordinal pass in `itn.rs:743-780` converts ordinal words to digits
("second" → "2nd") without reliably distinguishing the **ordinal sense**
(position: "the second floor") from the **noun/duration sense** ("a second of
silence", "wait a second", "the second meeting").

The article-guard at `:760-777` only fires when the word *following* the
ordinal is in a tiny 7-word whitelist:
`later | ago | before | after | more | please | and`. Any other following
word (a noun, a verb, a preposition) falls through to the conversion.

### Verified output (ran `itn::normalize` on the branch)

```
in a second i will be there              => in a 2nd i will be there     WRONG
a second of silence                      => a 2nd of silence             WRONG
the second floor                         => the 2nd floor                WRONG
a second attempt                         => a 2nd attempt                WRONG
the second one was better                => the 2nd one was better       WRONG
the second meeting is tomorrow           => the 2nd meeting is tomorrow  WRONG
the second tuesday                       => the 2nd tuesday              WRONG (comment says keep)
my second child                          => my 2nd child                 WRONG
the second time i tried                  => the 2nd time i tried         WRONG
wait a second                            => wait a second                OK (guard fires)
give me another second                   => give me another second       OK (guard fires)
i will be there in a second              => i will be there in a second  OK (guard fires)
```

The comment at `:760-762` says the guard should preserve "a second attempt" as
a genuine ordinal, but the guard only checks the *following* word against the
whitelist — "attempt" is not in the whitelist, so it converts.

### Why it matters

"second" is one of the most frequent words in spoken English. This is a
high-frequency, low-severity-per-instance corruption that compounds over a day
of dictation. Every "the second meeting", "my second child", "the second time"
comes out as a digit. The prior audit fixed 6 ITN bugs but this one survived
because the words are still present — only the form is wrong.

### Scope

The same shape likely affects "first", "third", etc. in noun contexts
("the first floor", "a first attempt"), but "second" is by far the most common
because it has the duration-noun sense that the others don't.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-core/src/itn.rs` | 743-780 | `ordinals()` — the buggy pass |
| `crates/teletype-core/src/itn.rs` | 760-777 | the article-guard (too narrow) |
| `crates/teletype-core/src/itn.rs` | 90-115 | `ordinal_word()` table |

## Reproduction

```bash
# Quick probe (temporary example, delete after):
# crates/teletype-core/examples/itn_probe.rs
fn main() {
    for s in [
        "in a second i will be there",
        "a second of silence",
        "the second floor",
        "a second attempt",
        "the second one was better",
        "the second meeting is tomorrow",
        "the second tuesday",
        "my second child",
        "the second time i tried",
    ] {
        println!("{s:40} => {}", teletype_core::itn::normalize(s));
    }
}
# cargo run -p teletype-core --example itn_probe
```

## Unit test cases (must pass after fix)

```rust
// Noun / duration senses must NOT convert.
#[test]
fn second_as_duration_is_not_converted() {
    assert_eq!(itn::normalize("in a second i will be there"),
               "in a second i will be there");
    assert_eq!(itn::normalize("a second of silence"), "a second of silence");
    assert_eq!(itn::normalize("wait a second"), "wait a second");
    assert_eq!(itn::normalize("give me another second"), "give me another second");
}

#[test]
fn second_as_count_noun_is_not_converted() {
    assert_eq!(itn::normalize("the second floor"), "the second floor");
    assert_eq!(itn::normalize("a second attempt"), "a second attempt");
    assert_eq!(itn::normalize("the second one was better"), "the second one was better");
    assert_eq!(itn::normalize("the second meeting is tomorrow"), "the second meeting is tomorrow");
    assert_eq!(itn::normalize("my second child"), "my second child");
    assert_eq!(itn::normalize("the second time i tried"), "the second time i tried");
}

// Genuine ordinals MUST still convert.
#[test]
fn genuine_ordinals_still_convert() {
    assert_eq!(itn::normalize("the second tuesday"), "the 2nd tuesday");
    assert_eq!(itn::normalize("the first floor"), "the 1st floor");
    assert_eq!(itn::normalize("the third attempt"), "the 3rd attempt");
    assert_eq!(itn::normalize("twenty second"), "22nd"); // compound cardinal
}

// The existing passing cases must not regress.
#[test]
fn existing_guard_cases_still_pass() {
    assert_eq!(itn::normalize("i will be there in a second"), "i will be there in a second");
}
```

## Acceptance criteria

- [ ] "second" in a duration/noun sense is NOT converted to "2nd".
- [ ] "second" as a genuine ordinal (position: "the second tuesday", "the
      second floor of the building" when it means position) IS converted.
- [ ] The existing guard cases (`wait a second`, `give me another second`,
      `in a second`) still pass.
- [ ] "first", "third", etc. in noun contexts are also protected (spot-check
      "the first floor", "a first attempt").
- [ ] `cargo test -p teletype-core` passes with the new tests.
- [ ] The eval harness (`crates/teletype-eval/`) corpus is extended with at
      least 5 "second-as-noun" cases so this is caught in CI measure mode.

## How to test (manual / smoke)

1. **Smoke (1 min):**
   - Dictate: "I will be there in a second."
   - **Pass:** text is "I will be there in a second." (not "2nd").
   - Dictate: "The second meeting is at noon."
   - **Pass:** text is "The second meeting is at noon." (not "2nd meeting").

2. **Genuine ordinal (1 min):**
   - Dictate: "See you on the second of the month."
   - **Pass:** text is "See you on the 2nd of the month."

3. **Regression (1 min):**
   - Dictate: "Wait a second, I need to think."
   - **Pass:** text is "Wait a second, I need to think."

## Fix direction

The current guard checks the *following* word against a 7-word whitelist. The
correct signal is the **preceding** word:

- If preceded by `a` / `an` / `another` / `one` / `two` / `three` (cardinal
  quantifier) → **noun/duration sense** → do NOT convert.
- If preceded by `the` / `this` / `that` / `my` / `your` / `his` / `her` /
  `its` / `our` / `their` (determiner/possessive) → **ordinal sense** →
  convert.
- If preceded by nothing (start of utterance) or a verb → ambiguous → do NOT
  convert (conservative).

This inverts the logic: instead of "convert unless the next word is in a
whitelist," it becomes "convert only if the previous word is a determiner."

A simpler minimum fix: expand the following-word whitelist to include common
nouns that follow "second" in the duration sense (`of`, `later`, `ago`,
`before`, `after`, `more`, `please`, `and`, `to`, `for`, `with`, `on`, `in`,
`at`, `by`) and common verbs (`will`, `can`, `would`, `could`, `should`,
`is`, `are`, `was`, `were`). But the preceding-word approach is cleaner and
covers more cases.

## Related

- BUG-005 (filler removal + ITN interaction on "first/second paragraph")
- Prior audit: `docs/qa-audit-2026-09-27.md` — the 6 ITN bugs fixed there were
  money/percent/digit-run, not ordinal noun-sense.
