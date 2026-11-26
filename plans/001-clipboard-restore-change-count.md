# Plan 001: Restore the user's clipboard after every macOS dictation

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- crates/teletype-core/src/delivery.rs
> If that file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit 8746c1c, 2026-11-24

## Why this matters

Teletype promises in crates/teletype-core/src/delivery.rs:10-11 that "the user's clipboard survives every path." On macOS it does not: the restore guard compares the live clipboard change count against the count captured before our own set_text bumped it, so the guard always concludes the clipboard moved and skips the restore. After every dictation the user's previous copy is silently replaced by the dictated text. The current tests pass only because the test clipboard never increments its count, so the production behavior is untested.

## Current state

- crates/teletype-core/src/delivery.rs — platform-independent delivery cascade. The clipboard contract is at :254-279:

  pub trait ClipboardIo: Send + Sync {
      fn set_text(&self, text: &str) -> Result<(), String>;
      fn snapshot(&self) -> Option<ClipboardSnapshot>;
      fn restore(&self, snapshot: &ClipboardSnapshot);
      fn change_count(&self) -> Option<u64> { None }
  }

- :600 — the snapshot is taken before the write: let snapshot: Option<ClipboardSnapshot> = io.clipboard.snapshot().filter(...).
- :612 — let outcome = match paste_and_verify(text, io) { ... }.
- :632-641 — the broken guard:

  let still_ours = match snap.change_count {
      Some(captured) => io.clipboard.change_count().is_none_or(|now| now == captured),
      None => true,
  };
  if still_ours { io.clipboard.restore(snap); }

- :752-806 — fn paste_and_verify(text: &str, io: &DeliveryIo) -> Option<DeliveryOutcome> has exactly four return shapes: return None at :757 (set_text failed) and :765 (paste key failed), return Some(DeliveryOutcome::PastedVerified) at :772, and a final three-branch if/else expression at :803-806.
- :1090-1114 — TestClipboard has a fixed change_count: Option<u64> field; set_text only increments set_calls, never the count. That is why the F-18 tests at :1973-2048 pass despite the production bug.
- crates/teletype-core/src/injector.rs:276-290 — ClipboardSnapshot { parts, items, change_count: Option<u64> }. Its doc comment states the count is re-checked before restore.
- crates/teletype-desktop/src/delivery_io.rs:288-292 — production change_count() re-reads the live NSPasteboard changeCount; crates/teletype-desktop/src/clipboard.rs:64 confirms the OS count increments on every write.

Repo conventions to match:
- Error handling: Result<_, String> in the delivery traits; no panics on clipboard failure.
- Logging inside this file uses tracing::warn!/tracing::debug! (see :606, :644).
- Tests are inline #[cfg(test)] mod tests in delivery.rs, using make_io(...), snap_with_text(...) and atomic counters. Model new tests on clipboard_restore_skipped_when_change_count_moved (:1996) and clipboard_restore_runs_when_change_count_unchanged (:2023).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Core tests | cargo test -p teletype-core | exit 0, 0 failures |
| Full suite | cargo test --workspace | exit 0, 0 failures |
| Format check | cargo fmt --all -- --check | exit 0 |

## Scope

In scope (only files you may modify):
- crates/teletype-core/src/delivery.rs

Out of scope (do NOT touch, even though related):
- crates/teletype-desktop/src/delivery_io.rs and clipboard.rs — the platform adapters already report the live count correctly; the bug is in the cascade.
- crates/teletype-core/src/injector.rs — ClipboardSnapshot is the right shape; do not add fields.
- Any change to DeliveryMode, DeliveryOutcome, or the paste/verify decision logic. The fix must not alter which delivery path runs.

## Git workflow

- Branch: advisor/001-clipboard-restore
- Commit message style (from git log): fix(delivery): restore clipboard using the change count after our own write
- Do NOT push or open a PR unless the operator instructs it.

## Steps

### Step 1: Return the post-write change count from paste_and_verify

Change the signature at delivery.rs:752 to:

  fn paste_and_verify(text: &str, io: &DeliveryIo) -> (Option<DeliveryOutcome>, Option<u64>) {

Immediately after the successful io.clipboard.set_text(text) check, capture the count that represents our write:

  let count_after_our_write = io.clipboard.change_count();

Wrap all four existing return shapes so the second tuple element is count_after_our_write: (None, count_after_our_write) at the two early returns, (Some(DeliveryOutcome::PastedVerified), count_after_our_write) at :772, and (Some(...), count_after_our_write) for each of the three final branches. The set_text-failure return at :757 must be (None, None) because no count was captured.

Verify: cargo check -p teletype-core -> exit 0. (There is exactly one caller, at :612, so the compiler will point directly at it if a return shape was missed.)

### Step 2: Compare against our write, not the pre-write snapshot

At :612, destructure the tuple:

  let (outcome, count_after_our_write) = match paste_and_verify(text, io) { ... };

Change the guard at :632-641 so the captured value is count_after_our_write rather than snap.change_count:

  let still_ours = match count_after_our_write {
      Some(ours) => io.clipboard.change_count().is_none_or(|now| now == ours),
      None => true,
  };

Keep the None => true behavior (platforms without a change count still restore unconditionally) and keep the existing tracing::debug! line. Update the F-18 comment block at :622-631 to describe the new comparison point — the comment is the contract future readers rely on.

Verify: cargo test -p teletype-core -> exit 0 (the old fixed-count tests may still pass; step 3 makes them meaningful).

### Step 3: Give the tests a clipboard that actually increments

In the inline tests module, add a small CountingClipboard implementing ClipboardIo that owns an Arc<AtomicU64> count. set_text fetches-adds 1; change_count returns the current value; snapshot returns a ClipboardSnapshot whose change_count is the initial value (to model the real pre-write capture); restore increments a restore counter.

Build a DeliveryIo around it using the same pattern as make_io ( :1140-1180 ), with a paste_key closure that may optionally bump the shared count (simulating a user copy after our write). Then write these two tests:

1. clipboard_restore_runs_when_only_our_write_moved_the_count — initial count 41; set_text makes it 42; no other write; assert restore called once and the outcome is PastedVerified.
2. clipboard_restore_skipped_when_user_copies_after_our_write — initial 41; have the paste_key closure bump the count to 43; assert restore called zero times.

Update the two pre-existing tests clipboard_restore_skipped_when_change_count_moved (:1996) and clipboard_restore_runs_when_change_count_unchanged (:2023) to the new semantics, or delete them if the two new tests cover them exactly. Do not leave a test asserting the old snap.change_count comparison. clipboard_restore_called_once_on_success (:1973) may stay as-is (its snapshot has change_count: None, the unconditional path).

Verify: cargo test -p teletype-core delivery::tests -> exit 0, including the two new tests by name. Run cargo test -p teletype-core -- --list | grep clipboard_restore and confirm both new test names appear.

## Test plan

- New tests: the two named above, in the inline tests module of delivery.rs.
- Structural pattern: make_io (:1140) and snap_with_text (:1118); the atomic counters in TestClipboard (:1082-1114).
- Edge cases covered: count moves only due to our write (restore), count moves again before the guard (skip), platform with no count (unchanged unconditional restore).
- Verification: cargo test -p teletype-core -> all pass, including 2 new tests.

## Done criteria

- [ ] grep -n 'snap.change_count' crates/teletype-core/src/delivery.rs returns no matches inside the F-18 guard (remaining occurrences may be in comments/tests describing the snapshot field).
- [ ] cargo test -p teletype-core exits 0; the two new test names appear in --list.
- [ ] cargo test --workspace exits 0 with zero failures.
- [ ] cargo fmt --all -- --check exits 0.
- [ ] Only crates/teletype-core/src/delivery.rs and plans/README.md are modified (git status --short).

## STOP conditions

Stop and report if:
- The live code at :600, :612, :632-641 or :752-806 does not match these excerpts.
- paste_and_verify has a fifth return site not listed here.
- A correction appears to require changing DeliveryMode, DeliveryOutcome, or the platform clipboard adapters.
- Any test's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- This fix depends on ClipboardIo::change_count() being the live OS count. If a future platform adapter returns a cached value, the guard is meaningless again — review that adapter when adding a platform.
- The platform restore() implementations also re-check the snapshot's change_count internally (see delivery_io.rs:263-282). That check uses the snapshot's pre-write count and is currently harmless because the cascade now gates the call; if it is ever made authoritative, revisit it alongside this fix.
- A reviewer should scrutinize the two new tests for a fake that increments on set_text — a non-incrementing fake is exactly what hid this bug.
