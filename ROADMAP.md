# Teletype — Roadmap & Plans

> **Single source of truth** for all roadmaps, plans, audits, and bug-fix records.
> Consolidated on 2026-11-25 from scattered root-level files (now deleted).
> The project brain (`.agents/brain/PROJECT_BRAIN.md`) remains the source for
> verified decisions and learnings; this file is the forward-looking plan.

---

## 1. Product Vision

Teletype is a local-first, privacy-first voice dictation app (Tauri v2 + Rust +
React). Hold a hotkey, speak, get text transcribed on-device and typed into the
focused app.

**Core product modes (target order):**

1. **Dictate** — voice → text → polish → type (shipped)
2. **Command** — voice → tool call → execute (shipped, v0.2.x)
3. **Agent** — voice → plan → multiple tool calls → result (planned)

**Positioning:** more accurate and faster than Wispr Flow, improves with use
(local learning, no cloud), with proof (benchmarks + evals). Do **not** claim
"at parity with Wispr Flow" — the honest position is measured per axis
(`usp.md` is the verified competitive sheet).

---

## 2. Beat Wispr Flow — Execution Plan (Phases 0–3)

> Source: `Teletype Agent Plan  Beat Wispr Flow.md` (14 Oct 2026, from a code
> review of branch `whistle-integration`). Progress was tracked in `PROGRESS.md`.

### Execution rules

1. Work phase by phase; every phase ends with a **Gate** that must pass before
   the next starts.
2. One branch, small commits (one commit per task ID). Every commit passes
   `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`,
   `cargo test --workspace`, `npm --prefix ui run typecheck`.
3. Measure before and after; speed/accuracy claims need before/after numbers
   in `evals/RESULTS.md`.
4. Never break delivery: whatever fails (polish, ITN, dictionary, learning,
   noise filter), the user still gets the raw transcript pasted.
5. Legal (hard): no EnviousWispr code (GPLv3); no EG-1 model (license bans
   competing products); models only from original publishers; add MIT `LICENSE`.

### Phase status (as of 2026-11-25)

| Phase | Scope | Status |
|---|---|---|
| 0 | Measurement, safety net, legal | ✅ Done (baseline measured 15 Oct; live p50/p95/p99 pending one real dictation session) |
| 1 | Speed: storage, prompt, SSE, ITN, flow, engine/pill/VAD | ✅ All six tasks done; live ≥400 ms p50 not yet measured |
| 2 | Command mode: state machine, hotkey/UI, LLM fallback, trace | ✅ Fully implemented (A–D) |
| 3 | Accuracy: bake-off, engines, personalization | ⚠️ Partial — bake-off complete (86-cell 5-engine matrix); live human gates open |

**Gate 0 baseline (15 Oct, 1306 cases, 14m13s):** verbatim micro WER 0.0789,
vocab recall 0.8277 / false-alarm 0.0000, entity 0.8097, harness ASR
p50/p95/p99 = 433/782/1010 ms. Full run: `evals/runs/baseline/`.

**Notable fixes from the plan:** take-kind retag bug (tag only on
`Action::Start`), LLM command extraction with per-tool JSON schema,
command-take trace lifecycle, wake phrase ("teletype" / "hey teletype").

### NeuDecide addendum (Command Mode)

Addendum A: `NeuDecide` for command mode — sessions + tokenizer, inserted
after Phase 2. Default decider is `neudecide` when `neudecide-v1` model is
present in `~/Library/Caches/com.teletype.app/models`. Gate A parity gaps
(0/50 direct, 45/50 conflate on TTS clips) are a known risk; the D019 VAD gate
protects takes shorter than 300 ms.

---

## 3. Refactoring & Feature Roadmap (static review @ v0.2.6)

> Source: `Teletype Refactoring and Feature Roadmap.md` (analyzed at commit
> `916f666`, 9 Oct 2026). Most structural items are done; remaining items
> are tracked in the project brain and `plans/`.

**Snapshot at review time:** ~47K lines Rust across 6 crates, ~13.3K lines
TS/React, ~705 Rust tests, 0 UI tests (now 622+), ~111 Tauri commands in one
file (since split into `commands/`), no coverage (now llvm-cov floor).

**Strong points:** clear crate boundaries, offline eval harness, regression
tests named by bug ID, IPC contract test, corrupt-file quarantine, brain
decision log.

**Key structural targets (status):**

- Split `commands.rs` (4,755 lines) → ✅ done (`commands/` module)
- Add UI test runner → ✅ done (vitest + tauriMock, 622+ tests)
- Add coverage floor → ✅ done (llvm-cov 87.71% floor)
- Windows CI: `cargo check` only → ⚠️ partial (windows-test job added; full
  parity pending)
- Reduce largest files (`dictation.rs`, `pipeline.rs`, `vocab.rs`) → ✅
  `dictation.rs` split 2026-11-28 (§16 item 3); `pipeline.rs` and `vocab.rs`
  remain tracked in `plans/` and the ponytail audit (§6)

---

## 4. Voice, Command & Local Agent Roadmap (long-term)

> Source: `teletype_complete_roadmap.md` (30.6 KB). The full agent-mode
> roadmap.

**Three modes:** Dictate (shipped), Command (shipped), Agent (planned).

**Agent mode outline:**

- Voice → plan → multiple tool calls → result
- `SpeechEngine`, `ModelCapabilities`, `Transcript`, `ToolDefinition` types
  defined in the plan; implementation phased after Command mode stabilizes
- Local-first: agent runs on-device LLM (llama-server), no cloud dependency
- Safety: per-tool allowlist, no send/delete tools without explicit opt-in

**Status:** planning complete; implementation not started (blocked on
Command mode human-gate completion).

---

## 5. Mobile App Plan (decision document, v0.2.7, 2026-07-09)

> Source: `mobile-app-plan.md`. No code changed; decision document only.

**Conclusion:** a native mobile app is **not** the next step. The app is
deeply coupled to desktop input (hotkey, text injection, screen context).
A phone companion (view transcripts, manage models, sync settings) is the
right shape, not a full port.

**Deferred until:** desktop agent mode is stable and the eval harness
proves the model works well enough to justify the extra surface.

---

## 6. Optimization & Improvement Audit (ponytail, 2026-11-19)

> Source: `ponytail.md`. Baseline: ~150K lines Rust + ~39K lines TS, 10
> crates, v0.2.19. Workspace tests green. Only 2 TODOs, no dead code in
> production paths. The big problems are *shape*, not bugs.

**The three things that matter most:**

1. ~~**`dictation.rs` is too big** — the dictation controller, state machine,
   pill updates, and trace emission are all in one file. Split into
   `dictation/controller.rs`, `dictation/state.rs`, `dictation/trace.rs`.~~ —
   **DONE 2026-11-28.** `dictation.rs` is 3,738 lines; tests live in
   `dictation/tests.rs` (4,921), the state machine in
   `dictation/session/{state,hotkey,take,transcribe,learning,delivery,
   lifecycle}.rs`, the controller in `dictation/controller.rs`, and the trace
   machinery in the pre-existing `dictation_stages.rs`. Method bodies were
   moved verbatim; only visibility widened to `pub(in crate::dictation)`.
2. **`commands/` is flat** — 15+ files with no sub-grouping. Group by domain:
   `commands/settings/`, `commands/models/`, `commands/dictation/`.
3. ~~**UI screens stay mounted** (L063) — all 13 screens run their polling
   loops on every launch. Propagate the `active` gate (SettingsScreen
   precedent) to all screens.~~ — **DONE 2026-11-28.** Ten screens take
   `active`: the six with focus/visibility/interval work (Developer,
   Notetaker, Dictation, AutoText, Insights, Dictionary), plus Home, Models
   and Transforms, with Settings already gated. The shared
   `useRuntimeStatus(active)` and `ReadinessPanel active` stop the 5 s
   runtime poll and 1 s "checked Ns ago" tick, and `useTauriEvent` gained an
   `enabled` argument so hidden screens stop subscribing (its event path
   would otherwise restart a gated poll). Styles, Command Mode,
   Personalization and Keybinds only do mount-time fetches or user-driven
   timers, so they are deliberately left unchanged.

**Other items:** reduce `pipeline.rs` (2,039 lines) by extracting ITN and
filler-removal into their own modules; add a `teletype-core` public API doc
pass; Windows platform layer is still stubbed (UIA/OCR, injection, hotkey,
tray).

---

## 7. Polish Feature Update (plan of action, 2026-11-20)

> Source: `polish-feature-update.md`. Baseline: `e1fd01f` + uncommitted T0–T7.

**Key finding:** the strategy doc (`docs/research/polish-retention-strategy.md`)
is out of date. The polish path has been reworked (T0–T7) but the strategy
doc still describes the old behavior.

**Action items:**

- Update `docs/research/polish-retention-strategy.md` to match T0–T7 behavior
- ~~Wire the polish learning loop into production (learning loop has no
  production caller — see L-2026-11-20)~~ — **DONE 2026-11-27:** the desktop
  layer resolves the bandit choice per take
  (`crates/teletype-desktop/src/dictation.rs`, `choose_take_profile_for_app`),
  encodes it via `teletype_core::profile::encode_instruction`, and the settle
  sweep in `crates/teletype-desktop/src/edit_watch.rs` promotes changes into
  the bandit. Regression tests pin the seam; `pipeline.rs` no longer
  consults an empty bandit.
- Add eval cases for the new polish profiles
- Document the model-scoped polish controls (D044)

**Status:** T0–T7 implemented; learning loop wiring done 2026-11-27.

---

## 8. QA & Production Readiness Audits

### 8.1 Strict QA Code Review (2026-10-05)

> Source: `agvity.md`.

**Verdict:** production-ready for macOS with caveats. Windows is stubbed.
Key findings:

- Crash reporting (Sentry) added 2026-11-22 (L-2026-11-22); was absent at audit
- Metal static destructor crash on quit (L015/L038) — fixed
- NSPasteboard concurrent access abort (L086) — fixed
- Controller wedge on spool join (L-2026-10-19) — fixed
- Windows platform layer: UIA/OCR, injection, hotkey, tray still stubbed

### 8.2 Human QA Checklist (v0.1.0)

> Source: `humanqa.md`. Manual test checklist for macOS and Windows.
> Superseded by `docs/manual-test-2026-09-28.md` and the per-version
> release notes in `docs/release-notes-v0.2.*.md`.

### 8.3 UI/UX Technical Audit (design-changes)

> Source: `design-changes.md`. Findings from `/impeccable audit`:

- StylesScreen IPC storm (L062) — fixed
- All screens stay mounted (L063) — design choice, `active` gate on
  SettingsScreen is the precedent
- Pill rendering broken after rphonetic panic (L082) — fixed
- Quick-tap paste misread as dictation (L084) — fixed

---

## 9. Windows Testing Guide

> Source: `windows-testing.md`. For whoever is testing the Windows build.
> Click-and-observe, no developer knowledge needed.

**What to test and why:**

- Hotkey (Ctrl+Alt+Space) — does it start/stop recording?
- Text injection — does typed text land in the focused field?
- Tray icon — does it show/hide the window?
- Model download — does it complete without error?
- App quit — does it exit cleanly (no crash report)?

**Known Windows gaps:** UIA/OCR, injection, hotkey, tray are stubbed.
The llama-server prebuilt guard is in place but full Windows parity is
not yet achieved.

---

## 10. Voice Command Testing Library

> Source: `voice-command-testing-library.md`. Complete reference for voice
> input, intent routing, and command execution.

**Pipeline:** Parakeet STT → dictionary correction → filler removal → ITN →
`TinyDecide.classify_intent()` → `Needle 3.extract_command()` →
`CommandExecutor`.

**Use:** test dataset specification for developers; command reference for
users. Covers all supported command types, edge cases, and expected outputs.

---

## 11. Bug Fix Records (BUG-001 – BUG-020)

> Source: `bugs/` directory (20 files). These are **resolved** bug records.
> The project brain (`.agents/brain/learnings/`) is the canonical record for
> verified lessons; these files are the original bug reports with fix details.

| ID | Title | Status |
|---|---|---|
| BUG-001 | Personalization records wrong corrections | ✅ Fixed |
| BUG-002 | Spoken emoji/punctuation toggles absent | ✅ Fixed |
| BUG-003 | Unload model timer noop | ✅ Fixed (replaced by D052 no-unload rule) |
| BUG-004 | ITN second noun context | ✅ Fixed |
| BUG-005 | Splitter partial echo duplication | ✅ Fixed |
| BUG-006 | AX allowlist search/URL fields | ✅ Fixed |
| BUG-007 | Select model spawns before kill | ✅ Fixed |
| BUG-008 | Language picker 8 vs 99 | ✅ Fixed |
| BUG-009 | Tray icon toggle noop | ✅ Fixed |
| BUG-010 | Typing/autotext toggle dead | ✅ Fixed |
| BUG-011 | Live preview hold mode | ✅ Fixed |
| BUG-012 | Personalization screen unreachable | ✅ Fixed |
| BUG-013 | Orphan llama-server on exit | ✅ Fixed (L038) |
| BUG-014 | Chat stream partial on server death | ✅ Fixed |
| BUG-015 | Provider race ensure vs select | ✅ Fixed |
| BUG-016 | Download no SHA corrupt resume | ✅ Fixed |
| BUG-017 | FillerCounts object vs array | ✅ Fixed |
| BUG-018 | Save settings race | ✅ Fixed |
| BUG-019 | No CI contract for features/settings | ✅ Fixed |
| BUG-020 | Stale sherpa SHA pins Windows download | ✅ Fixed |

---

## 12. Style References (design)

> Source: `Super-wispr-DESIGN.md`, `LettersDESIGN.md`,
> `ticket/amie-style-reference.md`.

These are UI style references used during the pill and settings screen
redesign. They describe color palettes, typography, and layout patterns
inspired by Superwhisper, Letters, and Amie. They are **reference material**,
not active plans. The current UI design is documented in `DESIGN.md`.

---

## 13. Cross-Worktree Progress (BRIDGE-PROGRESS)

> Source: `BRIDGE-PROGRESS.md`. Tracker for cross-worktree PI units
> (PI-BE-*), created 2026-10-17.

**PI-BE-005** (sr plugin + bake-off + delivery/learning): complete.
86-cell 5-engine matrix + CV set, decision regenerated.

This file is **historical** — the work it tracked is merged into `main`.

---

## 14. Checkpoint (2026-09-20)

> Source: `checkpoint.md`. Last updated 2026-09-20 (Insights Added).

**Status at checkpoint:** Full Wispr Flow roadmap implemented. Both
previously-blocking issues (stuck pill, language detection) fixed.

**Insights Added (2026-09-20):**

- Dashboard + Insights merged into single metric-first Insights screen
- Impact metrics: speaking WPM, "times faster" vs 40 wpm, time saved
- Personal records, milestones, 12-week trend
- `get_insights` returns 14-day activity series, words today/last-7, avg/day

This file is **historical** — the features it describes are shipped.

---

## 15. Active Plans (plans/ directory)

The `plans/` directory (001–013) contains the **improve-skill audit** plans.
All 13 are implemented (batch 1: 001–007 on `advisor/*` branches; batch 2:
008–013 applied to `main` on 2026-11-25). See `plans/README.md` for the
index. These are **completed** and kept for reference.

---

## 16. What to do next

**Open items (priority order):**

1. **Live E2E test for Dock-click fix** (L100) — hide window, let hotkey die,
   click Dock icon, confirm window raises + hotkey re-registers
2. ~~**Wire polish learning loop** into production (no production caller)~~ —
   **DONE 2026-11-27.** Desktop layer resolves the bandit choice per take
   (`dictation.rs`) and the settle sweep (`edit_watch.rs`) promotes changes;
   regression tests pin the seam. Remaining: add eval cases and refresh the
   strategy doc's status notes.
3. ~~**Split `dictation.rs`** — controller, state, trace into sub-modules~~ —
   **DONE 2026-11-28.** See §6: 16,461 → 3,738-line root, tests in
   `dictation/tests.rs`, state in `dictation/session/`, controller in
   `dictation/controller.rs`; trace was already in `dictation_stages.rs`.
4. ~~**Propagate `active` gate** to all UI screens (L063)~~ — **DONE
   2026-11-28.** Six screens gate their own intervals/listeners; the shared
   `useRuntimeStatus(active)` (Home, Models, Transforms, Settings) and
   `ReadinessPanel active` stop the 5 s runtime poll and 1 s tick, and
   `useTauriEvent` takes `enabled` so hidden screens unsubscribe. 628 UI
   tests pass, typecheck clean.
5. **Windows platform layer** — UIA/OCR, injection, hotkey, tray
   - **UIA `FieldReader` — LANDED 2026-11-27** (ROADMAP item 5): `uia.rs`
     now provides secure-field, writable-focus, and URL-bar reads; the
     delivery cascade uses `WindowsFieldReader` (selection readback,
     conservative Edit-only writability, allowlisted URL-bar guard, no
     ValuePattern whole-field write). Still open: **runtime validation on
     real Windows hardware** (a Mac and CI can only prove compilation —
     see `docs/windows-from-mac.md`), and **Windows tests are still
     compile-only / blocked by needle-sys LNK1143**.
   - Remaining: OCR, hotkey, tray, and the rest of the layer.
6. **Agent mode** — voice → plan → multiple tool calls (blocked on Command
   mode human-gate completion)
7. **Mobile companion** — view transcripts, manage models, sync settings
   (deferred until agent mode is stable)

**Verification commands:**

```bash
cargo test --workspace
cd ui && npm test && npx tsc --noEmit
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
bash scripts/check-brain-freshness.sh
```
