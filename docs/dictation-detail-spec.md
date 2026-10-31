# Dictation detail cards: spec

Status: draft for review. Branch `feat/dictation-detail` from `release/v0.2.17` (4124f11).
Owner: Ali. Scope: the Dictation tab only, plus the data, audio and commands it needs.

## 0. What the user asked for

Reference screenshots (session images 2.png to 5.png):

- **2.png, collapsed card.** First line of the text. Below it a meta row: clock + time
  ("11:39 PM"), "T" + "3 words", stopwatch + "2s". Chevron down on the right. No icon
  column, no action buttons.
- **3.png, expanded card.** Meta row at the top with chevron up. Full text. An audio
  panel: round black play button, waveform bars (silence drawn as dots), "0:00 / 0:00".
  Actions: Copy (outlined), Show in Finder (outlined, folder icon), Delete (red text,
  trash icon). Right side: chip icon + speech model name ("Parakeet v3").
- **4.png, inline edits.** Removed filler words stay in the text, struck through and
  muted: "hey sam ~~um~~ quick update the".
- **5.png, current tab.** Emoji app icon column (the red arrow points at the 🎤), text,
  app chip ("Terax"), time, copy and delete icons.

The target is the 2/3 layout, plus what the current card shows (app name), plus what
the pipeline did: which fillers it removed and which words polish, the dictionary or
AutoText changed, all shown inline in the 4.png style.

## 1. Facts found in the code (release/v0.2.17)

Some of these differ from what the task brief assumed:

1. `history` in `crates/teletype-core/src/persist.rs:42-59` has five columns: `id`,
   `created_at`, `text`, `context_json`, `duration_ms`. **There is no `recording_path`
   column.** The unused audio column the brief means is `dictation.audio_path` in the
   *learning* store (`crates/teletype-core/src/learning/store.rs:40`). It is always written
   as `None` and only `learning/replay.rs:239` reads it. This spec does not touch it.
2. `persist.rs` has no column migration. It only runs `CREATE TABLE IF NOT EXISTS`. The
   pattern to copy is `column_exists` + guarded `ALTER TABLE ... ADD COLUMN` in
   `learning/store.rs:64-72` and `:96-123`.
3. Nothing in the code writes take audio to disk, except the debug env var
   `TELETYPE_DUMP_AUDIO` (`dictation.rs:4419-4433`). That path uses `dump_wav16`
   (`dictation.rs:7623`), which writes a 16 kHz mono 16-bit WAV. On Ali's Mac,
   `~/Library/Application Support/com.teletype.app/recordings/` already exists and is
   empty.
4. On macOS `config_dir` and `data_dir` both resolve to
   `~/Library/Application Support/com.teletype.app` (`lib.rs:565-566`). `teletype.db` is
   under `config_dir`.
5. Text stages run in `Pipeline::run` (`crates/teletype-core/src/pipeline.rs:196-716`) in
   this fixed order:
   control-char scrub (210), dictionary + pack correction (216-226), filler removal
   (237-261), ITN (266-270), intent routing (no text change), AutoText protect, polish /
   transform, AutoText restore or direct expansion (604-621), emoji restore (627-632),
   spoken emoji (637-641), screen-term snap (655-667).
   `polish_change` (693-701) is already the polish stage's own before/after pair.
6. `PipelinePayload.raw_text` and `PolishDiff {raw, polished, ms}`
   (`dictation.rs:1936-2011`) live only for the pill card. `DictationEntry`
   (`history.rs:16-36`) has no raw text, no model, and no edits.
7. The tauri version is 2.12.0. `tauri.conf.json` sets **no CSP** and does not enable the
   asset protocol (no `protocol-asset` feature in `crates/teletype-desktop/Cargo.toml:44`).
8. A UI word diff already exists (`ui/src/lib/wordDiff.ts`), used by the pill polish card.

## 2. Data

### 2.1 `DictationEntry` (crates/teletype-core/src/history.rs)

Add `Default` to the derive. Add these fields, each with `#[serde(default)]` so old JSON
and old rows still load:

| Field | Type | Meaning |
|---|---|---|
| `raw_text` | `Option<String>` | ASR output exactly as the engine returned it (`PipelineResult.raw_input`). |
| `edits` | `Option<Vec<TextSpan>>` | The full text as an ordered list of spans (see 2.3). `None` means "no diff available": legacy row, command take, or a stage chain that could not be composed. |
| `asr_model` | `Option<String>` | Speech model display name at the time of the take, for example "Parakeet v3". |
| `polish_model` | `Option<String>` | Polish model display name. `Some` only when polish actually ran and returned `transformed = true`. |
| `polish_ms` | `Option<u64>` | Polish wall time. Same formula as `PolishDiff.ms`. |
| `word_count` | `Option<u32>` | `text.split_whitespace().count()` of the final text. |
| `recording_path` | `Option<String>` | **File name only** (`<id>.wav`), relative to the recordings dir. Never an absolute path. |

The display names are stored, not the ids. A renamed or removed catalog entry must not
change what an old card says. Name lookups:

- ASR: `teletype_speech::catalog::find(id).map(|m| m.name)`, falling back to the id. The
  id is the session trace's `engine` (`dictation.rs:4161`), which records the model that
  served this take.
- Polish: a new helper `polish_model_label(&Settings) -> Option<String>`. It uses
  `teletype_inference::catalog::find(&selected_llm_model).map(|e| e.name)` for the
  `local-server` provider, and the configured model string for `openai-compat`. It
  returns `None` when no provider is set.

Every struct literal of `DictationEntry` outside tests gets `..Default::default()`:
`dictation.rs:5186`, `dictation.rs:5311`, `dictation_stages.rs:636`,
`commands/models.rs:2594`, `commands/system.rs:299`. Test literals are updated the same way.

### 2.2 SQLite (crates/teletype-core/src/persist.rs)

New nullable columns on `history`:

```
raw_text TEXT, edits_json TEXT, asr_model TEXT, polish_model TEXT,
polish_ms INTEGER, word_count INTEGER, recording_path TEXT
```

- Fresh databases: add the columns to the `CREATE TABLE` in `create_tables` (42-59).
- Existing databases: after the `CREATE`, copy `column_exists` from `learning/store.rs:64`
  and run a guarded `ALTER TABLE history ADD COLUMN <col> <type>;` for each column. This
  is idempotent and needs no `user_version` bump. The persist DB has never used one, and
  adding it now only adds a state to get wrong.
- Use one column list constant (`HISTORY_COLUMNS`) in every `SELECT`: `history_all`
  (211), `history_full` (231), `history_get` (250) and `read_history` (401-440). Extend
  `entry_from_row` (154) to match. Today the four queries each spell the list out by
  hand.
- `push_history` (180) and the legacy JSON import (97) share one `insert_history(conn,
  &entry)` function so the import cannot lag behind.
- `edits_json` is `serde_json` of `Vec<TextSpan>`. If the column holds JSON that does not
  parse, read it as `None`. Never fail the row.
- New writer op: `clear_recording_paths()`, which runs `UPDATE history SET recording_path
  = NULL WHERE recording_path IS NOT NULL`. The persist actor exposes it as
  `PersistMsg::HistoryClearRecordings` (`crates/teletype-desktop/src/persist.rs:66-90`,
  handled next to `HistoryRemove` at 192).
- New read-only helper `read_history_entry(dir, id)`, so a command can resolve an entry
  older than the 1000-row in-memory snapshot without loading the whole table.

### 2.3 Edits: capture per stage, compose once

Filler removals must never be inferred from a raw-to-final diff. They are captured where
they happen.

**Capture (teletype-core/src/pipeline.rs).** Add `pub stages: Vec<StageText>` to
`PipelineResult` (53-97):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditKind { Filler, Dictionary, Autotext, Polish, Format, Correction }

pub struct StageText { pub kind: EditKind, pub before: String, pub after: String }
```

`run` pushes a `StageText` only when `before != after`, at these points:

| Stage | Where | Kind | before -> after |
|---|---|---|---|
| Dictionary + vocabulary packs | 216-226 | `Dictionary` | scrubbed text -> corrected |
| Filler removal | 237-261 | `Filler` | corrected -> filler-free |
| ITN | 266-270 | `Format` | filler-free -> normalised |
| AutoText expansion | before polish: `protect::restore(&protected.text, &protected)`. Without polish: the direct expansion at 604-621 | `Autotext` | ITN output -> expanded input |
| Polish / transform | `polish_change` pair (693-701), which already restores AutoText on both sides | `Polish` | expanded input -> polished |
| Emoji restore, spoken emoji | 627-641 | `Format` | |
| Screen-term snap | 655-667 | `Correction` | |

The brief listed five kinds. `Format` is added because ITN ("six thirty" -> "6:30") and
emoji are neither polish nor corrections, and calling them polish would be wrong.
`Correction` is the on-screen-name snap. The control-char scrub is not a stage: it never
changes words.

**Compose (new module `crates/teletype-core/src/history_edits.rs`).**
`compose(raw: &str, stages: &[StageText]) -> Option<Vec<TextSpan>>`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    /// `same` for unchanged text, otherwise the stage that made the change.
    pub kind: SpanKind,          // Same | Filler | Dictionary | Autotext | Polish | Format | Correction
    pub from: String,            // original words ("" for a pure insertion)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,              // new words ("" for a removal; unused for Same)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub minor: bool,             // only case and/or punctuation differ
}
```

Algorithm:

1. Tokenise on whitespace, like `learning/diff.rs`, so punctuation stays on its word.
   Start with one `Same` span per raw token. The "visible" sequence is every span's
   output: `from` for `Same`, `to` otherwise.
2. For each stage in order, check that `stage.before` tokenises to the current visible
   sequence. If it does not, return `None`. This is the safety valve: no diff is better
   than a wrong diff. Otherwise run `diff_words(before, after)` (`learning/diff.rs:36`) and
   apply the ops to the visible tokens:
   - `Equal`: no change.
   - `Delete`: each deleted `Same` token becomes `{kind, from: tok, to: ""}`. If the
     deleted token came from an earlier change, remove it from that span's `to`, and the
     span takes the later stage's kind (last writer wins).
   - `Insert`: a new `{kind, from: "", to: words}` span after the previous visible token.
   - `Replace`: one span with the original `from` words and the new `to` words.
3. Merge adjacent spans that have the same kind. Set `minor = true` when `from` and `to`
   are equal after lower-casing and stripping ASCII punctuation.
4. If the raw text has more than 400 tokens (the same limit as `wordDiff.ts`), return
   `None`.

Composition runs on the bookkeeper thread, never on the release-to-paste path. The
worker passes `raw_input` and `stages` into the job (2.4).

**Filler counts.** The usage counter at `dictation.rs:5157-5167` re-counts fillers in
the raw text with `count_fillers`, which can disagree with the tiered pass. This spec
does not change that counter: Insights semantics are out of scope. The card counts its
fillers from the `Filler` spans, so the card always matches what the pass actually
removed.

### 2.4 Where each field is set (crates/teletype-desktop/src/dictation.rs)

| Field | Source | Line |
|---|---|---|
| `raw_text` | `result.raw_input` | built at 5186 |
| `edits` | bookkeeper: `history_edits::compose(raw, stages)` | job built at 5197 |
| `asr_model` | session trace `engine` -> catalog name | trace at 4161; pass the id into the worker with `language_for_worker` (4241) |
| `polish_model` | `polish_model_label(&settings)` when `result.transform.is_some_and(\|t\| t.transformed)` | 4977 |
| `polish_ms` | `trace_marks.t_polish_end - t_polish_start`, same as `polish_diff` (1991) | `trace_marks` built at 4992, before the entry |
| `word_count` | `result.final_text.split_whitespace().count()` | 5186 |
| `recording_path` | bookkeeper, after the WAV write succeeds | see 3 |

- **Command takes** (5311): `asr_model` and `word_count` only. No edits, no recording.
- **File transcription** (`commands/models.rs:2594`): `raw_text`, `asr_model`,
  `polish_model`, `word_count`, and `edits` composed inline (this is not a latency path).
  No `recording_path`: the user's source file stays where it is.
- **Staged path** (`dictation_stages.rs:636`): same fields as the live path, read from
  its input.
- **Recovery** (`commands/system.rs:299`): `asr_model` and `word_count` only.

`Bookkeeper::history` (`bookkeeper.rs:79`) and `Job::History` (32, 161) gain:
`raw: Option<String>`, `stages: Vec<StageText>`, and `audio: Option<Arc<[f32]>>`.

## 3. Audio

**Source buffer.** Add `take_audio: Option<Arc<[f32]>>` to `Event::Transcribed`
(`dictation.rs:575-597`). It holds `asr_samples` (the 16 kHz mono buffer with preroll,
not VAD-trimmed, so playback sounds natural) and is set on both successful worker
branches: streaming (4391) and whole-take (4485). It stays `None` on the failure
branches (4258, 4271), when `save_recordings` is off (read once at worker spawn), and
for e2e file-fed takes. `pcm` is left alone because the command decider depends on its
current semantics. `transcribed()` (4505) moves `take_audio` into the pipeline worker
closure next to `pcm` (4679) and hands it to `bookkeeper.history`.

**Write.** On the bookkeeper thread (never the main thread, never the paste path):

1. `recordings_dir = state.data_dir.join("recordings")`. Create it if missing.
2. Write `<id>.wav.tmp`, then rename it to `<id>.wav`, so a crash never leaves a
   half-written file that a row points at. Move `dump_wav16` from `dictation.rs:7623` into
   a new `crates/teletype-desktop/src/recordings.rs` as `write_wav16(path, samples)` and
   point the debug dump at it.
3. Only on success, set `entry.recording_path = Some("<id>.wav")`, then push the entry.
   On failure, log a warning and push the entry without audio. The dictation is never
   lost because of audio.

**Size.** 16,000 samples/s x 2 bytes = 32,000 B/s, about **1.9 MB per minute** of
speech (115 MB per hour). A typical 10 s take is about 320 KB.

**Delete one.** `delete_dictation_entry` (`commands/history.rs:49`) resolves the entry
(in-memory snapshot first, then `read_history_entry`), removes the row as it does today,
then deletes the file through `recordings::resolve` (see 4). A missing file is not an
error.

**Delete all.** A new command, described in 4. It removes only files in the recordings
dir whose names match `^[0-9a-f-]{36}\.wav$` (plus stale `.wav.tmp`), and never recurses.
It clears `recording_path` in the in-memory snapshot and in SQLite
(`HistoryClearRecordings`).

**Setting.** `save_recordings: bool`, default `true`.

- `Settings` (`commands.rs:535`): `#[serde(default = "default_true")]` (helper at 1079).
  Set `true` in `Default` (1137). Add an `Option<bool>` to `SettingsPatch` (820) and
  `apply!(save_recordings)` (~909).
- UI: a new row in the **Audio** section (`ui/src/settings/DictationPolishSections.tsx:469`):
  - Toggle label: "Save recordings".
  - Explainer: "Keeps the audio of each dictation so you can play it back from the
    Dictation tab. Uses about 1.9 MB per minute of speech. Recordings stay on this Mac and
    are never uploaded." On Windows, "this PC".
  - Under it: a usage line ("42 recordings, 61 MB") and a secondary button "Delete all
    recordings", guarded by `confirmDialog("Delete all recordings? Your dictation text is
    kept.")`.
- Turning the toggle off stops new recordings. Existing recordings are kept until the
  user deletes them.
- Add `saveRecordings` to the TS `Settings` interface (`SettingsScreen.tsx`), to
  `BASE_SETTINGS` in `ui/src/dev/browserPreview.ts`, and to the four test fixtures that
  spell out settings (`settingsStates`, `a11y`, `learningSettings`, `polishChangeGuard`).
  `tsc` enforces this.

Old entries have no audio and render without a player.

## 4. Commands

All new commands live in `crates/teletype-desktop/src/commands/history.rs`, are
registered in `generate_handler!` (`lib.rs:1091`, next to `reveal_transcripts_dir` at
1228), and are invoked by the UI, so `contract_01` and `contract_02` cover them.

**`list_dictation_history`** (extended). `HistoryEntry` (15-22) gains:

```rust
pub raw_text: Option<String>,
pub edits: Option<Vec<teletype_core::history_edits::TextSpan>>,
pub asr_model: Option<String>,
pub polish_model: Option<String>,
pub polish_ms: Option<u64>,
pub duration_ms: Option<u64>,
pub word_count: u32,          // stored value, or computed from text for old rows
pub has_recording: bool,      // recording_path is Some AND the file exists
```

The UI never receives a path. `has_recording` checks `exists()` on the resolved path, so
a file the user deleted in Finder drops the player instead of breaking it.

**`get_dictation_audio(id: String) -> tauri::ipc::Response`.** Resolve the entry by id,
resolve its file through `recordings::resolve`, read the bytes, and return
`tauri::ipc::Response::new(bytes)`. The JS `invoke` then resolves to an `ArrayBuffer`
(raw IPC payload, no JSON or base64). Refuse files over 64 MB with a user-facing error
("This recording is too long to play here. Use Show in Finder."). The UI builds a `Blob`
and an object URL for an `<audio>` element.

Why this and not the asset protocol: the asset protocol needs the `protocol-asset` cargo
feature, an `assetProtocol.scope` in `tauri.conf.json` that lets the webview read any
file in the recordings dir by path, and a path handed to the UI for `convertFileSrc`. That
contradicts "path resolved from the entry, never from the UI". The IPC route adds no
feature, no scope and no capability, and keeps every path decision in Rust. There is no
CSP today, so `blob:` media works. If a CSP is added later, it needs
`media-src 'self' blob:`; record that in `docs/architecture.md`.

**`reveal_dictation_recording(id: String)`.** Resolve the entry, then the path through
`recordings::resolve`:

- macOS: `open -R <file>`
- Windows: `explorer /select,<file>`
- other: `xdg-open <recordings dir>`

This uses `std::process::Command::spawn`, the same pattern as `reveal_transcripts_dir`
(70). It does not touch NSApp, so the main-thread rule does not apply. It returns an
error when the entry has no recording.

**`get_recordings_usage() -> RecordingsUsage { count: u32, bytes: u64 }`** and
**`delete_all_dictation_recordings() -> RecordingsUsage`**, for the settings row.

**`recordings::resolve(dir, stored: &str) -> Result<PathBuf, String>`**, the only way a
path is built:

1. `stored` must match `^[0-9a-f-]{36}\.wav$`. This rejects `..`, separators, absolute
   paths and drive letters.
2. Build `dir.join(stored)`, `canonicalize` both, and require
   `file.starts_with(canonical_dir)`. This rejects a symlink that points outside.
3. Error text names the problem without echoing the path.

**`delete_dictation_entry`** (existing): also deletes the recording, as described in 3.

## 5. UI (ui/src/screens/DictationScreen.tsx)

Split the card into `ui/src/components/dictation/`: `DictationCard.tsx`,
`InlineDiff.tsx`, `AudioPlayer.tsx`, `waveform.ts`. This keeps each file under 500 lines.
The screen keeps the header, file-transcribe row, search, day grouping and states.

### 5.1 Tokens and icons

Existing tokens only: `--surface`, `--surface-2`, `--border`, `--border-strong`,
`--text`, `--text-secondary`, `--text-tertiary`, `--accent`, `--accent-soft`,
`--danger`, `--success`, `--radius` (18px), `--radius-pill`.

New `Icon` names (24x24, 1.75 stroke, same style as `Icon.tsx`): `chevron-down`,
`pause`, `folder`, `type` (the "T"), `timer` (stopwatch), `cpu` (the model chip). The
existing `clock`, `play`, `copy` and `trash` are reused.

The emoji app-icon column (`APP_ICON`, 32-41) is removed, as 2.png shows. The app name
moves into the meta row.

### 5.2 Collapsed card (2.png)

```
+--------------------------------------------------------------------+
| Need option two.                                                 v |
| (clock) 11:39 PM   (T) 3 words   (timer) 2s   [Terax]              |
+--------------------------------------------------------------------+
```

- Container: `background: var(--surface)`, `border: 1px solid var(--border)`,
  `border-radius: var(--radius)`, padding `16px 20px`, 8px gap between cards (as today).
- The header is one `<button type="button" aria-expanded aria-controls={panelId}>` that
  fills the card width and wraps the text line and the meta row. Enter and Space toggle it
  natively. Visible focus: `outline: 2px solid var(--accent); outline-offset: 2px`.
- Text: 15px, `var(--text)`, one line, `text-overflow: ellipsis`. It shows the **final**
  text with no diff. `title` holds the full text.
- Meta row: 13px, `var(--text-secondary)`, 20px gap, 14px icons in
  `var(--text-tertiary)`, `font-variant-numeric: tabular-nums`.
  - Time: `timeLabel` (59).
  - Words: "1 word" or "N words".
  - Duration: `elapsedLabel` (70). Hidden when `durationMs` is null.
  - App chip: the existing chip style (11px, `--surface-2`, radius 6, `padding 1px 8px`),
    shown when `appName` is non-empty.
- Chevron: `chevron-down`, 18px, `--text-secondary`, right-aligned. It rotates 180deg
  when expanded (`transform`, 150ms). The rotation is off under reduced motion.
- The accessible name of the header comes from its content. Add a visually hidden
  "Show details" or "Hide details" after the text.

### 5.3 Expanded card (3.png + edits)

Top to bottom:

1. **Meta row** (same as collapsed) with `chevron-down` rotated up. This is the same
   header button. The one-line preview text is hidden when expanded.
2. **Full text**, 15px, line-height 1.6, `white-space: pre-wrap`, rendered by
   `InlineDiff`:
   - `same`: plain text.
   - Removal (`to == ""`, for example a filler): `<del>` with `text-decoration:
     line-through`, `color: var(--text-tertiary)`. This is the 4.png look.
   - Replacement: `<del>old</del>` followed by `<ins>new</ins>`. The `<ins>` has
     `text-decoration: none`, `background: var(--accent-soft)`, `color: var(--accent)`,
     `border-radius: 4px`, `padding: 0 2px`.
   - Insertion (`from == ""`): the `<ins>` alone.
   - `minor` spans (case or punctuation only): only the new word, undecorated, so
     capitalisation fixes do not cover the card in highlights.
   - Each changed span has a `title` and a visually hidden prefix for screen readers:
     "Filler removed: um", "Polish changed Tuesday to Thursday", "Dictionary fixed jon to
     John", "AutoText expanded", "Formatted", "Matched on-screen name".
   - With "Show changes" off, or when `edits` is null, show the final text with no
     markup.
3. **Change summary + toggle**, 12px `--text-secondary`, one line. Example: "2 fillers
   removed · 3 polish edits · 1 dictionary fix · polished in 0.4s". Only non-zero parts
   are shown, and minor spans are not counted. On the right is a switch "Show changes",
   default on, remembered in `localStorage` key `tt.dictation.showChanges` (read and
   write in try/catch). The row is hidden when `edits` is null or has no changed spans.
4. **Audio panel** (only when `hasRecording`):
   - Container: `background: var(--surface-2)`, `border: 1px solid var(--border)`,
     radius 14px, padding `12px 16px`, flex row, 16px gap.
   - Play/pause: a 36px circle, `background: var(--text)`, icon `var(--surface)`,
     `aria-label` "Play recording" or "Pause recording".
   - Waveform: 96 bars, flex 1, 2px wide with 2px gaps, radius 1px,
     `background: var(--border-strong)`. Played bars use `var(--text-secondary)`. Bars
     below 8% amplitude render as a 3px dot, as in 3.png. The bar strip has
     `role="slider"`, `aria-label="Seek"`, `aria-valuemin=0`,
     `aria-valuemax={durationSec}`, `aria-valuenow`, and `aria-valuetext="0:03 of 0:12"`.
     Clicking seeks. ArrowLeft and ArrowRight move 1s (Shift: 5s). Home and End go to
     the ends. `tabIndex=0`.
   - Time: "0:00 / 0:12", 13px tabular-nums, `--text-secondary`.
   - Loading: audio is fetched on the first expand, not on list load, with
     `get_dictation_audio`. While it loads, show the bars as flat dots and disable the
     play button. On error, a one-line `--danger` message: "Could not load this
     recording."
   - Only one card plays at a time. A module-level ref pauses the previous audio.
     Collapsing a card pauses its audio. Object URLs are revoked on collapse and unmount.
     A small LRU (10 entries) keeps decoded peaks so re-expanding does not refetch.
   - `waveform.ts`: parse the RIFF header (find the `data` chunk, do not assume 44 bytes),
     read Int16 samples, split them into 96 buckets, take the RMS of each, normalise to
     the take's maximum, and apply a sqrt curve so quiet speech is still visible. Total
     duration = samples / 16000. This is a pure function with its own unit test.
5. **Actions row**, flex with space-between:
   - Left: **Copy** (outlined secondary button: 1px `--border-strong`, radius 12, padding
     `8px 16px`, `copy` icon; becomes "Copied" with `check` in `--success` for 1.2s; on
     failure, "Copy failed" in `--danger`. This keeps the A-10 behaviour.) **Show in
     Finder** (same style, `folder` icon; "Show in Explorer" on Windows using the
     `/Windows/i.test(navigator.userAgent)` check from `OnboardingPermissionsStep.tsx:6`;
     hidden when `!hasRecording`). **Delete** (text button, `--danger`, `trash` icon;
     keeps `confirmDialog` and the `useWrites` per-row error and retry, 234-242 and
     537-546).
   - Right: `cpu` icon + `asrModel` (13px `--text-tertiary`). When `polishModel` is set:
     " · Polished by {polishModel}". Hidden when both are null.

The expanded content is the `aria-controls` target (`role="region"`, labelled by the
header). Under `prefers-reduced-motion: reduce`, there is no chevron rotation and no
expand fade. Otherwise the expand uses a 150ms opacity fade only. Height is not animated.

**Search** (245-257) matches the final text, `rawText` and `appName`, case-insensitive,
so a search for a word that polish removed still finds the take.

**Legacy entries** (no raw text, no edits, no audio, no model): the same card. Meta row
and text as usual, no summary row, no player, no "Show in Finder", nothing on the right.
No console errors.

### 5.4 Browser preview and screenshot harness

In `ui/src/dev/browserPreview.ts` (demo fixtures at 572), add:

- h1: with a recording, filler and polish edits, both models set.
- h2: legacy row (no new fields).
- h3: file transcription (edits, no audio).

`get_dictation_audio` returns a generated 3 s WAV (speech-like bursts and silence), so
the waveform shows both bars and dots. `get_recordings_usage` and the two new commands
return in-memory values.

## 6. Tests

### 6.1 Rust

1. `persist.rs`:
   - `old_schema_rows_load_after_migration`: create the 5-column table by hand, insert a
     row, `PersistStores::open`, assert that all 7 columns exist, that the row loads with
     the new fields `None`, that `read_history` works, and that a second open is a no-op.
   - `new_fields_round_trip`.
   - `clear_recording_paths_nulls_only_that_column`.
   - `bad_edits_json_reads_as_none`.
2. `history.rs`: `legacy_json_without_new_fields_deserialises`.
3. `pipeline.rs`, `stages_are_captured_per_stage`: one voice run with a dictionary word,
   a filler, an AutoText trigger and a mock `InferenceProvider` that changes one word.
   Assert the stage kinds in order and their before/after texts. Assert that the polish
   stage's `before` contains no filler (fillers can never be attributed to polish).
   Assert that `stages` is empty when nothing changed.
4. `history_edits.rs`:
   - "hey sam um quick update the" with a filler stage gives `[same "hey sam", filler
     "um"->"", same "quick update the"]` (4.png).
   - Dictionary then polish on the same word: last writer wins, `from` keeps the raw word.
   - A case-only polish change is `minor`.
   - A broken chain (stage `before` does not match) gives `None`.
   - More than 400 tokens gives `None`.
   - An insertion-only stage gives a span with an empty `from`.
5. `recordings.rs`:
   - `write_wav16` writes a valid RIFF header (16 kHz, mono, 16-bit) and exactly
     `44 + 2*n` bytes, through a tmp file and a rename.
   - Delete of a missing file is `Ok`.
   - `delete_all` leaves non-matching files and subdirectories alone.
   - Bookkeeper job with `save_recordings` off or a failed write: entry pushed,
     `recording_path` is `None`.
6. `recordings::resolve` containment: rejects `../x.wav`, `/etc/passwd`, `C:\x.wav`,
   `a/b.wav`, a non-uuid name, and a uuid-named symlink to a file outside the dir (unix
   test). Accepts a real `<uuid>.wav`.
7. `ipc_contract.rs`:
   - Add `TextSpan` (`crates/teletype-core/src/history_edits.rs` to
     `ui/src/components/dictation/types.ts`) and `RecordingsUsage` to `WIRE_TYPES` (1169
     area). Point `HistoryEntry` at the new types file if the interface moves.
   - Add `HistoryEntry.edits` to `WIRE_TYPES_NESTED` (1236).
   - `contract_04`, `contract_06` and `contract_07` pass because `saveRecordings` has a UI
     control and `dictation.rs` reads it.

### 6.2 Vitest (new `ui/src/test/dictationCard.test.tsx`; update `dictationStates.test.tsx`)

- Collapsed: first line, "3 words", "2s", app chip, `aria-expanded="false"`, no player,
  no actions.
- Click, Enter and Space each toggle `aria-expanded` and reveal the full text and actions.
- Diff: a filler span renders `<del>um</del>`. A polish replacement renders `del` + `ins`.
  A `minor` span renders undecorated. Turning off "Show changes" removes every `del` and
  `ins`.
- No-audio entry: no player, no "Show in Finder", no errors. Null `edits`: plain text and
  no summary row.
- Audio: mocked `get_dictation_audio` returns a synthetic WAV `ArrayBuffer`.
  `URL.createObjectURL` and `HTMLMediaElement.play` are stubbed. The play button's name
  flips. The slider has the right `aria-valuetext`. 96 bars render. The ArrowRight key
  advances `currentTime`.
- "Show in Finder" calls `reveal_dictation_recording` with `{ id }`. With a Windows
  userAgent the label is "Show in Explorer".
- Delete still goes through `confirmDialog` + `delete_dictation_entry`. Update the A-1 and
  A-10 tests to expand the card first and to use the new accessible names "Copy" and
  "Delete".
- Search matches `rawText` and `appName`.
- `waveform.ts`: bucket count, normalisation, and a header with an extra chunk before
  `data`.
- a11y: `vitest-axe` on the collapsed and expanded states (`a11y.test.tsx` pattern).
- Reduced motion: with `matchMedia` mocked to `reduce`, the chevron has no transition.

### 6.3 Screenshot check

Run the `screenshot-verify-ui` pass on the Vite preview (`?demo=1`, Dictation tab) at
900px (the window's `minWidth`) and 1100px, in light and dark. Capture h1 collapsed, h1
expanded, h1 with "Show changes" off, and h2 expanded. Compare against 2.png and 3.png
by eye. Then do one live take in `cargo tauri dev`, expand it, play it, and use Show in
Finder.

### 6.4 Commands to run

```
export CARGO_TARGET_DIR=/Users/a.sorathiya/Documents/Ali/teletype-wt/integration/target
cargo test -p teletype-core history persist pipeline history_edits --lib
cargo test -p teletype-core --test ipc_contract
cargo test -p teletype-desktop recordings commands::history
export NODE_OPTIONS=--use-system-ca
cd ui && npx tsc --noEmit && npx vitest run src/test/dictationCard.test.tsx src/test/dictationStates.test.tsx src/test/a11y.test.tsx
```

## 7. Out of scope

- Writing `audio_path` into the learning store (it would switch on `learning/replay.rs`
  over real user audio, which is a behaviour change).
- Changing the Insights filler counter.
- Re-transcribe from a recording, export, or share.
- Compressing audio (Opus or AAC).
- Recordings for command takes and file transcriptions.

## 8. Decisions for Ali

1. **Retention.** Saving is on by default. At 1.9 MB per minute, heavy use is about
   100 MB per day, and this Mac's disk is nearly full. Options: (a) keep forever, with
   "Delete all" only (this spec as written); (b) add "Keep recordings for 30 days", which
   prunes on startup. Recommendation: (b), about a half-day of extra work.
2. **Quick actions when collapsed.** 2.png shows no Copy or Delete on the collapsed card.
   Today's card has one-click copy. Follow 2.png exactly, or keep a small copy icon next
   to the chevron?
3. **SpeakType recordings.** The 55 WAVs in
   `~/Library/Application Support/com.2048labs.speaktype.desktop/recordings` are not part
   of this feature. Should they be imported as history entries (each one transcribed
   again), or used only as a private test and eval corpus? Either is a separate task.
4. **Emoji icon column.** Removed, as 2.png shows. Confirm that 5.png's arrow meant
   "remove this".
