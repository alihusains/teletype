# Manual test: the 2026-09-28 bug-fix batch

These are the fixes I made that **cannot be verified by a code test**, because
they depend on a real microphone, a real clipboard, and another application
honestly accepting text. I need you to run these and tell me what you see.

Full bug list and reasoning: `docs/qa-audit-2026-09-27.md`.

## Before you start

```bash
cd /Users/a.sorathiya/Documents/Ali/teletype
cd ui && npm run build && cd ..
cargo tauri dev          # dev log: /tmp/teletype-dev.log
```

You need a dictation model. You already have Parakeet downloaded, so **Models
should show "Downloaded"** for Parakeet TDT v3.

Grant Microphone and Accessibility when prompted. Check the pill appears when
you hold the hotkey.

How to report: for each test write **PASS**, or **FAIL + what you actually saw
in quotes**. "It typed it twice" is gold. "Something felt off" is not.

**Reboot first.** The machine had 5.3 GB of 6 GB swap in use and 8 GB sitting in
the compressor while these fixes were verified, which is enough to make the
first local-model load behave oddly and look like a product bug.

**If a model is selected but nothing polishes**, read section 9 before
reporting it. There was a real bug with that exact symptom and it is fixed, but
it needed a specific fix to the build script to fix.

---

## 1. The double-paste fix (highest priority)

This was the worst user-facing bug: dictations typed **twice** into some apps.

Setup: open **TextEdit** (or Notes) in a blank document. Turn on
**Smart Quotes** if your app has it (TextEdit: Edit > Substitutions > Smart
Quotes).

1. Click at the start of an empty line.
2. Say: **"don't stop"**
3. Look at the line.

- **PASS**: the line reads `don't stop` — once.
- **FAIL, and the line reads `don't stop don't stop`**: tell me, and paste
  `/tmp/teletype-dev.log` around the injection. The app's smart-quote rewrite
  is what triggered it.

Repeat with a selection: select an existing word, dictate **"hello there"**.
The selection should be replaced by one copy.

Repeat 10 times in a row. The old bug was intermittent because it depended on
which apps normalise your text.

## 2. Your clipboard survives dictation, and pastes what you just said

Two bugs here. The first was that copying a file and then dictating destroyed
the copy. The second was that after dictating, Cmd+V pasted your *previous*
clipboard text instead of the dictation, and the clipboard grew by one entry per
dictation.

### Part A: your copy is not destroyed

1. In **Finder**, select any file and press Cmd+C.
2. Switch to **TextEdit**, click in a blank line.
3. Dictate: **"clipboard test one two three"**
4. Wait for the pill to go idle.
5. Go back to **Finder** and press Cmd+V somewhere harmless (a new Finder
   window, or a folder).

- **PASS**: the file pastes. Your clipboard was untouched.
- **FAIL**: nothing pastes, and Finder's Edit menu shows Paste greyed out. Your
  copy is gone. Say so loudly.

Repeat with:
- an **image** copied from Preview or a screenshot
- **text** copied from Safari (this one always worked, so it is a control: if
  text copy *also* breaks, something else is wrong)

### Part B: Cmd+V gives the dictation

This is the change in behaviour, so it is worth being explicit about what is now
expected.

1. Copy some text in Safari. Note what it is.
2. In **TextEdit**, dictate: **"the dictation should be here"**
3. Press **Cmd+V** in TextEdit.

- **PASS**: the dictation pastes, not the Safari text. It is the most recent
  thing, so it is what the clipboard gives.
- **FAIL**: the Safari text pastes. That is the reported bug and it is back.

### Part C: your copy is still reachable

Both cannot be item 0, so one item along is how the other survives.

1. Copy a file in Finder (Finder's Edit menu will show Paste, and it was
   enabled by step A).
2. Dictate something.
3. In Finder, press Cmd+V into a new window.

- **PASS**: the file still pastes. Dictation did not cost you the copy.
- **FAIL**: it does not. The copy is gone.

### Part D: the clipboard does not grow

1. Copy a single piece of text.
2. Dictate 10 times, in a row, without copying anything in between.
3. Open a clipboard manager (or paste into TextEdit repeatedly).

- **PASS**: the clipboard holds your original copy plus the latest dictation.
  It does not hold 10 dictations.
- **FAIL**: the list grows every time. This is the "again and again" half.

### Part E: the new setting

**Settings > Behavior > "Keep dictated text on the clipboard"**. With it on
(the default), after dictating the dictation is the clipboard. With it off, the
clipboard is exactly what you had before you started.

## 3. Numbers are no longer glued to words

Every price, percentage, time and date used to be corrupted. Say each of these
one at a time, in a fresh line, and check the result:

| you say | before (broken) | expected (fixed) |
|---|---|---|
| it will cost fifty dollars | `it will cost$50` | `it will cost $50` |
| that is one hundred percent right | `that is100% right` | `that is 100% right` |
| the meeting is at five pm | `the meeting is at5:00 PM` | `the meeting is at 5:00 PM` |
| the ratio is one point five to one | `the ratio is1.5 to one` | `the ratio is 1.5 to one` |
| lets meet on march twelfth two thousand seven | `lets meet onMarch 12, 2007` | `lets meet on March 12, 2007` |
| she is twenty one | `she is 2001` | `she is 21` |
| in a second | `in a 2nd` | `in a second` |
| call five five five zero one zero nine nine nine | `call 555-010-9999at` | `call 555-010-9999 at` |
| i visited example dot com slash about slash us | `example.com/aboutslashus` | `example.com/about/us` |

- **PASS**: every row matches the expected column.
- **FAIL**: tell me which row, and what you got instead. I care about the exact
  string.

## 4. No more wrong speech-speed numbers

The Insights screen used to invent a speed figure out of your word count.

1. Dictate about 300 words across several takes (any text will do).
2. Open **Insights**.

- **PASS**: it shows something like *"You speak at 150 wpm, measured over N
  dictations, against a measured 52 wpm average typing pace."* The number
  should be in a believable range, roughly 100-250.
- **FAIL, and it shows 250+ wpm**: tell me the number and your word count.
- **FAIL, and "Time saved" goes DOWN** when you dictate more: that was a real
  bug and it should be impossible now.

Also check the wording: with **no** dictations, or with history from before this
change, it should say *"Based on your word count against a 52 wpm average typing
pace"* and must **not** print a wpm number at all.

## 5. The knowledge of the model still comes through

Test 3 must not have broken the actual useful part. Check:

- Typing `/` in any app expands your AutoText snippets.
- A spoken snippet expands.
- If you have an LLM polish model loaded, the "Polish" transform still produces
  readable prose.

## 6. The app is not left stuck

This was the bug where the app froze on "Transcribing..." and needed a
force-quit, losing the dictation.

1. Dictate a normal sentence. Confirm it appears.
2. While it is still transcribing, **tap the hotkey again**.
3. Confirm the app recovers and you can dictate again immediately.

Repeat 5 times. The app must never be stuck showing "Transcribing..." or
"Transforming...".

---

## 7. Saying a word does not type a symbol

This was the bug where "the payment period ends in March" became "the payment.
ends in March". It is fixed, so **both halves matter**: the sentences that must
keep their words, and the requests that must still produce the character.

### Part A: ordinary sentences keep their words

Dictate each of these into Notes. **The words must come out exactly as you
said them.** If a `.`, `,`, `*`, `"` or `+` appears where a word should be, that
is a failure.

| say this | expected |
|---|---|
| the payment period ends in March | the payment period ends in March |
| use a comma to separate the fields | use a comma to separate the fields |
| the star of the show | the star of the show |
| the quote in the article was wrong | the quote in the article was wrong |
| that is a plus for the team | that is a plus for the team |
| start a new line for the address | start a new line for the address |
| please move this down to the next line of the poem | unchanged |
| there is a line break in the paragraph | unchanged |
| the plus sign shows the delta | unchanged |

### Part B: asking for the character still works

Each of these **must** produce the character. If a word survives where a symbol
should be, that is a failure.

| say this | expected |
|---|---|
| comma | `,` |
| period | `.` |
| semicolon | `;` |
| insert a comma here | `insert a, here` |
| type a period | `type a.` |
| the symbol for a period | `the symbol for a.` |
| give me a colon | `give me a:` |
| i need a comma here | `i need a, here` |
| email at sign example dot com | `email @ example dot com` |
| the end full stop | `the end.` |

Part B is the one to watch. "email at sign example dot com" is the case that
decided the design: an address spelled aloud, where the symbol sits in the
middle of a long sentence, must still produce `@`.

### Part C: the one thing that is now stricter

"two plus two equals four" now types the **words** `plus` and `=`, where it used
to type `+`. That is intentional: "plus" between two words is as likely to be
prose as arithmetic. To get the symbol, say **"two plus sign two equals four"**,
or add a cue: "type two plus sign two equals four".

If you dictate arithmetic often and this gets annoying, that is worth telling
me, because it would change the design.

### Part D: your own custom AutoText is never affected

If you have ever created a custom snippet, it is exempt from all of the above,
because you typed the trigger in on purpose. Create a custom snippet with the
trigger `comma` and expansion `[X]`, then dictate "a comma b". You should get
`a [X] b` even though Part A says the word must survive.

---

## 8. Two things I need your call on

Not bugs. Decisions I did not want to make for you.

**1. Should the polish capitalise and punctuate the model's own bullet points?**
EG-1 returns, for *"First apple. Second jala. Third mango."*:

```
Hi George, I want you to bring three things from the market.
- apple
- jala
- mango
```

The structure is right, and that is what you asked for. The bullets are
lower-case and unpunctuated. Teletype's own list pass fixes this when the words
are *spoken* ("first apple second jala"), but it deliberately leaves a model's
formatting alone, so here they pass through as the model wrote them. If you want
`- Apple.` `- Jala.` `- Mango.`, say so and I will add it.

**2. Does "two plus two equals four" annoy you?**
It now types the words `plus` and `=` where it used to type `+`. That is
intentional: "plus" between two words is as likely to be prose as arithmetic, so
the fix for *"that is a plus for the team"* stopped it. To get the symbol, say
**"two plus sign two equals four"**. If you dictate arithmetic often, tell me
and I will reconsider the rule.

---

## 9. If a model is selected but nothing polishes

This is section 9 because it is the symptom two bugs had, one of which is fixed
and one of which is not. Work down it in order.

1. **Is the model actually downloaded?** **Models** screen. EG-1 is eight
   shards; if any is missing it will not run, and the screen will not say why
   beyond "not downloaded".
2. **Check the Developer tab** in Settings, or `/tmp/teletype-dev.log`. You are
   looking for one of:
   - `llama-server exited early with signal: 9` and a **0-byte** log. That was
     the build-script bug: the binary had been overwritten while an older
     `llama-server` was running from it, which macOS then refuses to execute. To
     fix: `pkill -fl llama-server` and re-run `./scripts/build-llama-server.sh`.
     The script now does this for you by installing with `mv` and then execing
     what it installed, so you should not hit it again.
   - `llama-server not ready after 120s`. The model is too big for the memory
     available. Close apps, or pick S1-mini (0.6B) instead of EG-1 (4B).
   - anything else: paste it here, the message names the cause now.
3. **Is the paste the right text?** With EG-1 loaded, dictate *"first apple
   second jala third mango"* and check it becomes three `- ` lines. If the lines
   are lower-case, see section 8 question 1, that is expected.

---

## What is still broken (so you do not report it as new)

One known issue is **not fixed yet**:

1. **Some UI needs a restart.** If you change Theme, Reduce Motion, or the
   Developer tab in Settings, nothing happens until you restart the app. Same
   for a few other toggles. Known, not fixed.

The symbol bug that used to be on this list is fixed; see section 7.

## If something crashes

`/tmp/teletype-dev.log` has the log. Look for lines starting with `PANIC:` and
send me the whole file if you can. Also, the app records errors in the
**Developer** tab inside the app; enable it in Settings, restart, and the tab
appears.
