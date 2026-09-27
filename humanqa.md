# Teletype — Human QA Checklist

Manual test checklist for the **v0.1.0** release. Work through each section in
the running app. Mark each step [ ] pass / [x] fail and note what you saw.

Two platforms are covered. macOS notes use the .dmg/.app; Windows notes use the
NSIS .exe installer. Anything marked **(macOS only)** relies on macOS
permissions; anything marked **(Windows only)** is the Windows code path.

How to get the builds:
- macOS: https://github.com/alihusains/teletype/releases/tag/v0.1.0 → `Teletype_0.1.0_aarch64.dmg`
- Windows: same release page → `Teletype_0.1.0_x64-setup.exe` (NSIS installer)

---

## 0. Install & first launch

### macOS
- [ ] Open the .dmg, drag Teletype to Applications.
- [ ] Launch from Applications. First launch may be blocked by Gatekeeper
      (unsigned build): right-click the app → Open → Open.
- [ ] Grant **Microphone** permission when prompted (System Settings →
      Privacy & Security → Microphone → Teletype ON).
- [ ] Grant **Accessibility** permission (needed for text injection + typed
      AutoText): System Settings → Privacy & Security → Accessibility → add
      Teletype ON. If you skip this, dictation text and typed AutoText will not
      be inserted.
- [ ] Grant **Input Monitoring** if prompted (global hotkey + typing watcher).

### Windows
- [ ] Run the .exe installer, accept defaults, finish.
- [ ] Launch Teletype from the Start menu / desktop shortcut.
- [ ] Grant **Microphone** access (Windows Settings → Privacy → Microphone →
      allow desktop apps).
- [ ] The typing watcher uses a low-level keyboard hook; no extra permission
      prompt is expected, but confirm the tray icon appears.

### Both
- [ ] A tray icon appears (Settings → "Show tray icon" is on).
- [ ] The main window opens on the Home screen ("Welcome back").
- [ ] No crash, no error dialog, no console spam in the tray menu.

---

## 1. Core dictation (the main loop)

Setup: focus a text field (Notes, a browser box, or the app's own scratch
field). The default hotkey is **Fn** in **hold** mode.

- [ ] Hold Fn, speak a sentence, release Fn.
- [ ] **Expected:** the pill appears while recording; on release it shows a
      brief "processing" state, then the polished text is typed into the text
      field you were in.
- [ ] Say something with filler ("um, so, basically I think we should ship it")
      and confirm the output is cleaned up (filler removed, sentence
      capitalized/punctuated).
- [ ] Dictate a proper noun you expect the dictionary/vocab to fix, e.g.
      "cherrypick" misheard, or "IC Markets". **Expected:** corrected on first
      pass.
- [ ] Say a number/date in words ("three forty five", "the 25th of May").
      **Expected (ITN):** written form (3:45, May 25th) where supported.
- [ ] Cancel mid-recording: hold Fn, speak, then trigger Cancel (hover the pill
      and use the cancel control). **Expected:** nothing is inserted.
- [ ] Dictate into TWO different apps (e.g. Notes then a browser box) and
      confirm text lands in whichever app is frontmost when you release.

---

## 2. The floating pill (visual + per-style)

The pill is a small transparent window that never steals focus. Settings →
Pill style lets you switch between: default, classic, levelRail, well, dotGrid.

- [ ] Switch through each pill style. **Expected:** the pill window resizes to
      fit the style (no clipping, no big empty letterbox). The window should be
      exactly the size of the visible capsule.
- [ ] **Live transcript preview (T1.2):** while recording, in each of
      `levelRail`, `dotGrid`, and `well`, the words you are speaking appear as
      interim text, updating live.
      - `levelRail`: text appears to the right of the 24-bar meter.
      - `dotGrid`: text appears to the right of the dot matrix.
      - `well`: text appears in the reading-well area.
      - `classic`: intentionally shows NO live text (too narrow) — confirm it
        still shows the lips + clock and does not clip.
- [ ] The clock counts up while recording.
- [ ] The pill stays where you positioned it (Settings → pill position) and
      does not jump or get cut off at screen edges.
- [ ] The pill never takes keyboard focus: while it is showing, your text
      cursor stays in the app you were typing in.

---

## 3. Typed AutoText (NEW: on by default)

You have stored triggers. Current ones in this profile:
`/email` → a.sorathiya@ic.com, `/pemail` → personal email, `/Zoe` → Zowie,
`/ad` → AgentDesk.

- [ ] In any text field, type `/email` then a **space**.
      **Expected:** `/email ` is deleted and replaced with `a.sorathiya@ic.com`.
- [ ] Type `/Zoe` then Enter. **Expected:** replaced with `Zowie`.
- [ ] Type a trigger that is NOT stored (e.g. `/nope` + space). **Expected:**
      nothing happens, the text stays as you typed it.
- [ ] Settings → "Expand AutoText while typing" toggle: switch it OFF, type
      `/email` + space. **Expected:** no expansion. Switch back ON, confirm it
      works again.
- [ ] Add a new AutoText entry (Settings → AutoText): trigger `/sig`,
      replacement "Cheers, Ali". Type `/sig` + space in a text field.
      **Expected:** expands to "Cheers, Ali".
- [ ] **(macOS only)** Confirm this still works when Accessibility is granted.
      If it does not expand, Accessibility/Input Monitoring is likely not
      granted.

---

## 4. Personalization ("it knows how I talk")

- [ ] Settings → profile: set your preferred sign-off / greeting / a term swap
      (e.g. prefer "customers" over "clients").
- [ ] Dictate a sentence that uses the old phrasing, let it polish, then EDIT
      the inserted text to your preferred phrasing.
- [ ] Dictain again in a similar context. **Expected (T2.1):** over time the
      polish starts using your preferred phrasing. (This is a learning loop;
      one edit may not be enough, but the preference should be recorded.)
- [ ] Settings → per-app language override: set a specific app (e.g. Gmail) to
      a language different from your global. Dictate into that app.
      **Expected (P3.3):** the output language matches the app override.
- [ ] **(T2.2, if shipped in this build)** Dictate into an email app vs a chat
      app with NO manual style override. **Expected:** email uses the
      professional style, chat uses casual, automatically.

---

## 5. Home screen & insights

- [ ] Home screen shows the **"Time saved vs typing"** card with a number and
      an "N× faster" badge (T3.1).
- [ ] The **"Your impact"** grid shows words, wpm, day streak, words/7d.
- [ ] The **privacy row** ("On-device · nothing uploaded") shows with a shield
      (T3.2); the dismiss (×) hides it and it stays hidden on reload.
- [ ] The **Weekly goal** bar reflects your last-7-days word count.
- [ ] Dictation history list is grouped by day (Today / Yesterday / date); each
      row can be copied and deleted.
- [ ] Copy a history row → **Expected:** the full text is on your clipboard.
- [ ] Delete a row → **Expected:** it disappears from the list.

---

## 6. Models & settings

- [ ] Settings → Models: confirm the speech model (Parakeet TDT 0.6b) and the
      LLM (EG-1) are listed and selected.
- [ ] Switch the LLM provider between local-server and an OpenAI-compatible
      endpoint (if you have a key): set base URL + key + model, confirm a test
      dictation polishes via that provider.
- [ ] Change the pill style and pill position; confirm both persist after
      quitting and relaunching.
- [ ] Change the app icon; confirm the tray + window icon update.
- [ ] Language setting: set a non-English language, dictate in that language,
      confirm ASR + polish follow it. Set language to **auto** and speak;
      confirm it detects and matches.

---

## 7. Reliability edge cases

- [ ] **Long dictation (T1.3 splitter):** hold Fn and speak for ~2 minutes
      (well over 500 words). **Expected:** it does not time out or error; the
      output is coherent (the transcript is split into <=500-word chunks for
      polish and re-joined). No words dropped.
- [ ] **Polish gate (short clean take, local model):** with a LOCAL model
      selected and the polish gate ON, dictate a very short clean phrase
      ("On my way."). **Expected:** it returns faster than a long take
      (the LLM polish pass is skipped for short clean input). If the gate is
      OFF in your settings, you will always see the full polish.
- [ ] **No network:** turn Wi-Fi off, use the local LLM. **Expected:**
      dictation + polish still work fully on-device.
- [ ] **Recovery spool:** if a dictation's paste fails (e.g. focus changed),
      check the recovery folder (Settings → reveal transcripts dir / recovery).
      **Expected:** the text is not lost; it can be recovered.
- [ ] **Quit and relaunch** several times. **Expected:** no crash on start,
      settings preserved, no duplicate tray icons.
- [ ] **Memory:** leave the app running 30+ minutes with several dictations.
      **Expected:** memory stays stable (no obvious growth), no leak.

---

## 8. Windows-specific checks

- [ ] The NSIS installer completes and the app launches from Start menu.
- [ ] Dictation inserts text into a Windows app (Notepad, browser).
- [ ] The typing watcher (low-level hook) expands typed AutoText triggers in a
      Windows app (type `/email` + space in Notepad).
- [ ] The tray icon shows the menu; the app can be quit from it.
- [ ] The pill window renders correctly (transparent, no focus steal) on the
      Windows desktop, including on a high-DPI / scaled display.

---

## 9. Regression guard (things that must NOT have changed)

- [ ] Existing dictations in history still load and display.
- [ ] Dictionary import/export still works (Settings → dictionary).
- [ ] Emoji restore: dictate "thumbs up" → expect a thumbs-up emoji in output
      (when language is a supported one and restore emoji is ON).
- [ ] VAD auto-stop (if enabled) still ends recording on silence.

---

## How to report a failure

For each [x] fail, capture:
1. Platform + build (macOS .dmg / Windows .exe) + exact steps.
2. What you expected vs what happened.
3. A screenshot or screen recording of the pill/main window.
4. The app log (macOS: `~/Library/Logs/teletype*` if present; or launch the
   app from a terminal to see stdout).
