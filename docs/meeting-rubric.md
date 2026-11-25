# Meeting feature — design & quality rubric

The bar for the Meeting experience (screen, island, report). A change to the
meeting feature is done when every row below passes; a row that fails is a
defect, not a preference.

## 1. Behavior

| # | Criterion | Pass when |
|---|-----------|-----------|
| B1 | Island appears on start | The floating island is visible on the primary screen within ~1 s of `meeting_start`, on all workspaces, without stealing keyboard focus. |
| B2 | Island shows recording state | While recording: pulsing indicator, live elapsed timer (±1 s), live word count from committed segments. |
| B3 | Stop from the island | The island's Stop button ends the meeting and starts finalization; the Meeting screen reflects it without a reload. |
| B4 | Island lifecycle | Finalizing → "building report" state; Done → "report ready" with Open action, auto-hides after ~8 s; Cancel/failure → hides immediately. |
| B5 | Open report while recording | From the island or the recording view, `report.html` opens in a new browser tab **while the meeting is still recording** and refreshes itself (no JS) as new segments commit. |
| B6 | Real-time transcript | Words appear in the UI within a few seconds of being spoken (P4 streaming commits); the live report file updates on each commit. |
| B7 | Same-page report | After stop, the Meeting screen renders the full report (overview, action items, stats, transcript) without leaving the app. |
| B8 | Dictation gate | Dictation start is refused while a meeting is recording, with a clear reason. |
| B9 | Durability | The meeting WAV + folder are written crash-safe (tmp+rename, owner-only on Unix); a crash mid-meeting never leaves a torn artifact. |
| B10 | Island minimize | The recording state has a minimize button that collapses the island to a compact dot+timer pill; clicking the compact pill expands it. Minimize never stops or affects the meeting. |
| B11 | Island drag | The island can be dragged by its body (never by its buttons) and the drop position persists across meetings (restored on next show, with a top-center fallback when off-screen). |
| B12 | Rubric select | The Meeting screen offers the built-in rubrics (General, Sales call, Customer support, Performance review) plus "No rubric"; the choice is passed to `meeting_start` and unknown ids degrade to no rubric, never a failed start. |

## 2. Design (Letters system — see `ui/index.html` tokens)

| # | Criterion | Pass when |
|---|-----------|-----------|
| D1 | Monochrome actions | Primary actions use `--action` (obsidian `#070709`), never a chromatic CTA background. Brand purple is functional punctuation only. |
| D2 | Shape language | Cards 18px radii, pills/buttons fully rounded, inputs 12px. |
| D3 | Type | Display font (Open Runde) for headings/timer, Inter for body; tabular numerals for timer/stats. |
| D4 | Island look | A compact dark pill (obsidian fill, white text, soft shadow) that reads as a "Dynamic Island": one line of state + at most two actions. |
| D5 | Screen hierarchy | One clear primary action per state; stats are chips or a grid, not prose; the live transcript is the visual center of the recording state. |
| D6 | Report look | The HTML report looks like a finished document: gradient hero title block with meta chips, clear section order (Overview → Score → Details → Transcript), speaker avatars + colors, stat tiles, readable measure (≤ ~70ch), professional spacing. |
| D7 | Self-contained | Report: single file, inline CSS, no JS, no external refs. Opens offline, prints cleanly (print stylesheet). |
| D8 | Score look | The Score section shows a large colored overall number (green ≥ 7, amber ≥ 4, red below), per-criterion bars in the same color scale with short notes, and Strengths/Improvements lists. Unscored rubrics list their criteria plainly. |

## 3. Report content

| # | Criterion | Pass when |
|---|-----------|-----------|
| C1 | Overview | "What this meeting was about": the LLM summary when a provider ran; a mechanical duration/words/segments line otherwise — never an empty section. |
| C2 | Action items | Only explicit commitments from the transcript; none invented. |
| C3 | Details | Started, duration, words, WPM, segments, speakers, language, ASR engine — all computed from real data. |
| C4 | Transcript | Combined lines (never one word per line), timestamps, speaker labels (single-mic baseline: `Speaker 1`). |
| C5 | Honesty | No-speech and no-model states are stated plainly, not faked. |
| C6 | Rubric score | When a rubric is selected and a local model ran: overall 0–10 + per-criterion 0–10 with notes + strengths/improvements, all from the LLM pass (schema-constrained, clamped). The durable `rubric.json` artifact is written next to the transcript. No model → the rubric is listed unscored, never faked. |
| C7 | Live report | The live (recording) report shows the rubric's Score section unscored from the first second, so the document shape is stable while it updates. |

## 4. Verification checklist (per change)

- [ ] `cargo test -p teletype-core --lib meeting` green (includes rubric + report score tests)
- [ ] `tsc --noEmit` green
- [ ] Desktop build green (`build-windows.bat` on Windows)
- [ ] Live run: start → island visible → live text → (open live report mid-recording) → stop from island → same-page report → new-tab report
- [ ] Island: minimize → compact pill → expand; drag → position persists across meetings
- [ ] Rubric: select a rubric at start → Score section present in live + final report (unscored without a model; scored with llama-server running)
- [ ] Report HTML: self-contained (no `<script>`, no http refs), all sections present, prints cleanly
