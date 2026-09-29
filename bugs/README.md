# Teletype QA Audit — Bug Tickets (2026-09-28)

A 5-agent sweep (text pipeline, injection/clipboard/AX, inference/lifecycle, UI/IPC, EW-parity) found 19 issues. Every claim was re-verified against the real code on branch `qa-audit-fixes` before being filed. None are in the prior audit's (2026-09-27/28) fixed set.

## The headline

The prior audit fixed OBVIOUS corruption (ITN money, "period"→".", the 64s lock). What survived is sneakier — bugs that keep the words present but corrupt meaning, AND a whole class of EW-adopted features that are advertised in the project brain but don't exist in the code. That second class is the biggest competitive risk vs Whisperflow.

## Severity table

| ID | Title | Severity | Area | One-line symptom |
|---|---|---|---|---|
| BUG-001 | Personalization loop records rewordings as mishearings | P0 | personalization/learn.rs | "clients"→"customers" persisted as a permanent rule; corrupts the user profile over time |
| BUG-002 | Spoken-emoji + spoken-punctuation toggles absent | P0 | settings/transforms | advertised in brain, zero code (grep 0 hits) |
| BUG-003 | "Unload model after" timer is a no-op | P0 | inference/speech lifecycle | no setting/UI/timer; unload() has zero callers |
| BUG-004 | ITN "second"→"2nd" in noun contexts | P0 | transforms/itn.rs | "the second meeting" → "the 2nd meeting" (verified by running itn::normalize) |
| BUG-005 | Splitter re-join duplicates text on partial echo | P0 | transforms/engine.rs | >500-word dictation can duplicate a section (narrow trigger) |
| BUG-006 | AX allowlist includes search fields + URL bars | P1 | desktop/ax_text.rs | dictation lands in a browser URL bar |
| BUG-007 | select_model spawns new server before killing old | P1 | desktop/commands.rs | doubled RAM; "Select did nothing" on a memory-strapped Mac |
| BUG-008 | Language picker: 8 languages behind a "99" label | P1 | desktop/commands.rs + ui | can't select it/nl/hi; cosmetic on Windows |
| BUG-009 | show_tray_icon toggle is a no-op until restart | P1 | desktop/commands.rs + lib.rs | toggle persists, tray stays |
| BUG-010 | typing_autotext_enabled toggle is dead | P1 | desktop/lib.rs + commands.rs | AutoText-while-typing keeps firing after toggle off |
| BUG-011 | Live preview held back from default Hold mode | P1 | desktop/dictation.rs + ui/pill.tsx | 3 of 4 ported pills show no text in Hold mode |
| BUG-012 | Personalization screen unreachable (no nav entry) | P1 | ui/App.tsx | screen rendered but nothing navigates to it |
| BUG-013 | Orphan llama-server on Exit (G008) | P2 | desktop/lib.rs + inference/server.rs | fire-and-forget shutdown; child outlives app |
| BUG-014 | chat_stream returns Ok(partial) if server dies mid-stream | P2 | inference/server.rs | silently truncated rewrite, no error |
| BUG-015 | ensure_local_provider races select_* provider | P2 | desktop/commands.rs | stale provider overwrites the user's choice |
| BUG-016 | Interrupted no-SHA download leaves corrupt model | P2 | inference/download.rs + catalog.rs | fast/quality models can pass size check corrupt |
| BUG-017 | fillerCounts object vs array; filler panel empty | P2 | desktop/commands.rs + ui/InsightsScreen.tsx | TypeError masked by .catch; panel permanently empty |
| BUG-018 | save_settings rewrites whole doc; RMW race | P2 | desktop/commands.rs + ui | concurrent save can revert a field |
| BUG-019 | No CI contract for settings/features/engine paths | P1 (structural) | CI/ipc_contract.rs | the pattern that produced a dozen of the above |

## Recommended fix order

### Fix now (this week) — trust & data-integrity

These are cheap and each is a reason a user uninstalls.

- [ ] BUG-004 — ITN second guard
- [ ] BUG-006 — AX allowlist
- [ ] BUG-001 — personalization judge + add its off-switch via BUG-012
- [ ] BUG-007 — select_model kill-before-spawn
- [ ] BUG-009 + BUG-010 — the two dead toggles

### Fix next sprint — the honesty block (EW-parity gaps Whisperflow will expose)

- [ ] BUG-002 + BUG-003 — build-or-remove the advertised features
- [ ] BUG-008 — language picker
- [ ] BUG-005 — splitter echo guard
- [ ] BUG-012 — make the personalization screen reachable

### Defer (track, don't block release)

- [ ] BUG-013, BUG-014, BUG-015, BUG-016 — bundle into one "lifecycle hardening" task
- [ ] BUG-017
- [ ] BUG-018

### Structural (do once, prevents recurrence)

- [ ] BUG-019 — extend ipc_contract.rs to assert every setting has a production caller and every advertised feature has an engine path

## Verification status

Every ticket's load-bearing claim was re-verified by reading the real code (and, for BUG-004, by running `itn::normalize` directly on the branch). The tickets are on branch `qa-audit-fixes`. The MD060/MD040 markdown advisories the linter emits on these files are empty-heading/code-fence lint noise, not real issues.

---

Generated 2026-09-28 from a 5-agent QA sweep. See each BUG-0NN file for full description, test cases, acceptance criteria, and how to test.
