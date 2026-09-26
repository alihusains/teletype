# Task: P2.3 — Escape Recovery spool (survive a crash mid-dictation)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Files you may edit
- `crates/teletype-desktop/src/recovery.rs` (NEW)
- `crates/teletype-desktop/src/lib.rs` (module + startup recovery check)
- `crates/teletype-desktop/src/dictation.rs` (spool write/teardown hooks ONLY —
  minimal, see below)
- `crates/teletype-desktop/src/commands.rs` (recover/discard commands)
- `crates/teletype-core/src/scratchpad.rs` (READ for the storage house pattern;
  edit only if the recovered dictation must land in scratchpad/history)

## Why
Roadmap P2.3 (Tier 4 reliability): if the app crashes (or the Mac sleeps badly)
while recording, the user loses the entire utterance. A bounded PCM spool
written during recording lets the next launch offer "recover last dictation".
EnviousWispr ships exactly this (`RecoverySpool`); we do not.

## Read first
1. `crates/teletype-desktop/src/dictation.rs`: the recording lifecycle — where
   audio frames are captured (search for the audio callback / `start` and
   `stop`), where the PCM buffer accumulates, and where a clean stop flushes to
   ASR. The spool hooks in at exactly these two seams.
2. `crates/teletype-desktop/src/lib.rs`: app startup (where `AppState` is built)
   — the recovery check runs here, before the tray, non-blocking.
3. `crates/teletype-core/src/scratchpad.rs` or `history.rs`: how a finished
   dictation is persisted (the recovered audio should be transcribed on demand,
   and the transcript lands where normal dictations land).
4. Where the app data dir comes from (`app.path()` / `config_dir` in lib.rs) —
   the spool lives under `<app-data>/recovery/`.

## Build
1. `recovery.rs`:
   - `Spool::begin(app_data_dir) -> Result<Spool>`: creates
     `recovery/spool-<timestamp>.pcm` with 0600 perms (macOS) — plain PCM
     (no encryption in V1; the file is 0600 and deleted on clean stop; document
     this choice in the module header).
   - `Spool::write(&mut self, frame: &[f32])` (or whatever the frame type is in
     dictation.rs): append, converting to the container format the ASR provider
     expects (match the existing buffer's sample rate/channels/endianness —
     read how the in-memory buffer is fed to whisper/parakeet and mirror it
     exactly).
   - BOUND: max 60 s of audio. When exceeded, drop the OLDEST data (implement
     as: stop writing and mark `truncated: true`, or a ring of 60 s chunks —
     pick the simpler correct one and document it).
   - `Spool::complete(self)` (clean stop): delete the file.
   - `Spool::abandoned()`: leave the file in place (crash path = no cleanup).
   - Startup scan: `pub fn find_pending(app_data_dir) -> Option<PendingRecovery>`
     — any `spool-*.pcm` older than 10 minutes and > 100 KB is a candidate
     (small files are noise from instant stop). Delete candidates older than
     7 days unconditionally.
2. `dictation.rs` (minimal hooks):
   - On record start: `Spool::begin`, store in the dictation session state.
   - In the audio callback: `spool.write(frame)` — MUST be allocation-cheap and
     non-blocking (file write on the audio thread is acceptable at 16 kHz mono
     f32 = 64 KB/s; if the existing callback is on a real-time thread, write
     via a bounded mpsc to a writer thread and document it).
   - On clean stop/flush: `spool.complete()`.
3. Startup (lib.rs): `find_pending` → if found, emit a Tauri event
   `recovery-available { path, seconds }` (the UI toast is a follow-up task; the
   event + commands are this task).
4. Commands:
   - `recover_last_dictation()` — transcribe the pending spool through the
     normal ASR path (reuse the existing transcribe function; pass the file),
     insert/record the result like a normal dictation, then delete the spool.
   - `discard_recovery()` — delete the spool.
   - `recovery_status()` — returns pending seconds or null.
   Register all three in `lib.rs`.

## Hard rules
- The spool must NEVER block or slow the audio callback: if a write would block
  (disk full), drop the frame and log once, keep recording.
- Never let a corrupt/partial spool crash startup: all file IO in recovery.rs
  is fallible and logged, never panics.
- Disk bound is hard: 60 s max, 7-day expiry. Test both.
- No UI in this task (event + commands only).

## Verification (paste real output)
1. `cargo test -p teletype-desktop --lib 2>&1 | tail -3`
2. `cargo test -p teletype-core --lib 2>&1 | tail -3`
3. `cargo build -p teletype-desktop 2>&1 | tail -3`
4. Unit tests in `recovery.rs` (use a tempdir, not the real app data dir):
   - begin → write frames → complete leaves no file
   - begin → write frames → abandoned leaves the file with the right size
   - 60 s bound: writing 70 s of frames results in ≤ 60 s stored (or the
     documented truncation behavior)
   - find_pending ignores files < 100 KB and < 10 minutes old; finds a valid
     one; deletes a 8-day-old one
   - corrupt file (truncated mid-frame): recover returns a clear error, does
     not panic
5. In the report, state the measured write cost (bytes/s at 16 kHz mono f32)
   and which thread writes.
