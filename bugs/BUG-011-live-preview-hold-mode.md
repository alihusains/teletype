# BUG-011 — Live preview is held back from the default Hold mode, so 3 of 4 ported pills show no text

**Severity:** P1 (ported UI promises a feature the default mode doesn't deliver)
**Area:** desktop / dictation.rs + ui/pill.tsx
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `dictation.rs:560-566` and `pill.tsx`

---

## Description

The interim-transcript loop is spawned **only** in Toggle (push-to-talk) mode
(`dictation.rs:560-566`):

```rust
if self.recording_mode() == RecordingMode::Toggle {
    // spawn interim loop
} // "Hold is short; the final transcribe is fast enough that a preview would just flicker."
```

But the three ported pill styles — **Level Rail, Reading Well, Dot Matrix**
(`pill.tsx:497-501`, `540`, `719-722`) — all render `interimText`. So in the
**default Hold mode**, those pills show no live text for the entire take
(Reading Well's well shows "Listening…" permanently, `pill.tsx:511-549`).

The ported UI promises a feature the engine path doesn't feed in the default
mode.

### Note on streaming

The brain's "streaming ASR rejected" decision means the well will never get true
streaming words — only whole-take re-decodes (188–532 ms p50), and only in
Toggle mode. Hold mode gets none of it.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/dictation.rs` | 560-566 | interim loop only in Toggle mode |
| `ui/src/pill.tsx` | 497-501, 540, 719-722 | levelRail / well / dotGrid render `interimText` |
| `ui/src/pill.tsx` | 511-549 | ReadingWellPill well shows `interimText \|\| "Listening…"` |

## Reproduction

1. Set pill style to "Level Rail" (or Reading Well / Dot Matrix).
2. Use the default **Hold** recording mode.
3. Hold the hotkey and speak.
4. The pill shows no live words — just the recording animation.
5. Switch to Toggle mode and repeat — now words appear.

## Unit test cases (must pass after fix)

```rust
// Pin the intended behavior: does Hold mode feed interim text or not?
// Option A (feed Hold too):
#[test]
fn hold_mode_spawns_interim_loop() {
    let d = Dictation::new(RecordingMode::Hold);
    assert!(d.interim_loop_enabled(),
        "ported pill styles render interimText; Hold mode must feed it too");
}

// Option B (Hold must not preview): the interim-dependent pills must be gated.
#[test]
fn interim_only_in_toggle_mode_is_documented() {
    let d = Dictation::new(RecordingMode::Hold);
    if !d.interim_loop_enabled() {
        // Then the UI must not offer the interimText-dependent pills in Hold mode.
        assert!(pill_styles_for_mode(RecordingMode::Hold)
            .iter().all(|s| !s.depends_on_interim()),
            "Hold mode offers pills that promise live text it never delivers");
    }
}

// Regression: Toggle mode must keep feeding interim text.
#[test]
fn toggle_mode_still_feeds_interim() {
    let d = Dictation::new(RecordingMode::Toggle);
    assert!(d.interim_loop_enabled());
}
```

## Acceptance criteria

- [ ] **Either** the interim loop runs in Hold mode too (so the ported pills
      show text), **or** the three `interimText`-dependent pill styles are not
      offered in Hold mode / show an honest "no preview in Hold mode" state.
- [ ] The UI does not promise live text it doesn't deliver in the default mode.
- [ ] Toggle mode still shows live text (no regression).
- [ ] `cargo test -p teletype-desktop` and the UI tests pass.

## How to test (manual / smoke)

1. **Smoke (2 min):**
   - Pill style = Level Rail, recording mode = Hold (default).
   - Hold the hotkey and speak.
   - **Document the actual behavior:** does live text appear?
   - **Pass (Option A):** live words appear in the pill.
   - **Pass (Option B):** the pill is disabled for Hold or shows an honest
     "no preview in Hold mode" placeholder.
   - **Fail:** the pill silently shows only the recording animation while
     promising live text.

2. **Regression (1 min):**
   - Switch to Toggle mode, repeat.
   - **Pass:** live words still appear.

## Fix direction

Decide the product intent:

- **If Hold should preview:** spawn the interim loop in Hold mode too. The
  "flicker" concern in the comment is minor compared to a pill that looks
  broken for the entire take.
- **If Hold should not preview:** gate the three `interimText`-dependent pill
  styles (Level Rail, Reading Well, Dot Matrix) behind Toggle mode, or render a
  placeholder that explains why there's no preview.

The current state — ported UI promising a feature the default mode doesn't
feed — is the bug regardless of which direction is chosen.

## Related

- Brain note — "streaming ASR was measured and rejected"
- BUG-003 — model residency affects preview latency
