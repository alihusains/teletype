#!/usr/bin/env python3
"""R1: restore lost dictation history from the day-wise transcript files.

Background
----------
A bug fixed on this branch (F-01) meant live dictations were written only to
`history.json` while launch loaded history from SQLite, so each restart
rewrote `history.json` from SQLite and the takes in between were lost from the
database. Every take is *also* appended to a day-wise Markdown archive, which
still covers the gap. This script rebuilds the missing `history` rows from
that archive.

Formats, as derived from the code (not guessed)
----------------------------------------------
* Transcript file format — `crates/teletype-core/src/history.rs`,
  `append_transcript_file` (plus `day_stamp_ms` / `time_stamp_ms`):
    - one file per day, `YYYY-MM-DD.md`, where the day boundary is **UTC**
      (`day_stamp_ms` divides the UTC-ms timestamp by 86_400_000);
    - a new file starts with the title line `# Teletype transcripts — <day>`
      followed by a blank line;
    - each entry is `## HH:MM` (`time_stamp_ms`, 24h, **UTC**), followed by
      ` · <app name>` when the app name is non-empty, then the entry text on
      the following lines and one blank line (`writeln!(f, "{text}\\n")`);
    - multi-line text is written verbatim, so internal newlines survive and
      need no escaping.
* History row — `crates/teletype-core/src/persist.rs` (`HISTORY_COLUMNS`,
  `create_tables`, `write_history`): `id, created_at, text, context_json,
  duration_ms, raw_text, edits_json, asr_model, polish_model, polish_ms,
  word_count, recording_path`. `created_at` is UTC milliseconds,
  `context_json` is the serde JSON of `ApplicationContext`
  (`crates/teletype-core/src/context/mod.rs`, camelCase, `applicationType`
  serialised as `"unknown"`), and the detail columns are nullable.
* Id format — the app builds every new entry with
  `uuid::Uuid::new_v4().to_string()` (e.g.
  `crates/teletype-desktop/src/dictation_stages.rs`), i.e. a lowercase,
  hyphenated, 36-character UUIDv4. Restored ids are the same shape.

Privacy
-------
This tool exists because the data is a real person's dictation. It never
prints, logs or summarises transcript text: output is counts, dates, times and
application names only. Text is compared in memory for de-duplication and
written straight to the database.

Behaviour
---------
* Dry run by default: no writes at all (the write lock is taken and rolled
  back so a locked database is detected without touching data).
* `--apply` inserts only the missing rows, in one transaction, and never
  UPDATEs or DELETEs anything.
* Idempotent: a second `--apply` inserts nothing.
* An entry is "already exists" when a row carries the same text after
  whitespace normalisation and either (a) a `created_at` within plus or minus
  2000 ms, or (b) a `created_at` in the same UTC minute. (b) is required
  because the archive stores minute-granular UTC times while SQLite stores
  milliseconds: without it, a row the app itself re-imported from
  `history.json` would be duplicated. Both are documented in the report.

Exit codes: 0 success, 1 usage/IO error, 2 refused (app running with a
non-empty WAL, or the database is locked).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sqlite3
import subprocess
import sys
import uuid
from pathlib import Path

# ── formats taken from the Rust sources ────────────────────────────────
# `HISTORY_COLUMNS` in crates/teletype-core/src/persist.rs, in the app's
# own order.
HISTORY_COLUMNS = (
    "id",
    "created_at",
    "text",
    "context_json",
    "duration_ms",
    "raw_text",
    "edits_json",
    "asr_model",
    "polish_model",
    "polish_ms",
    "word_count",
    "recording_path",
)

# `## HH:MM` with the optional ` · <app>` suffix from `append_transcript_file`.
TRANSCRIPT_HEADER = re.compile(r"^## ([0-9]{2}):([0-9]{2})(?: · (.*))?$")

# The tolerance the task specifies for "same row".
DEDUPE_TOLERANCE_MS = 2_000

# "Older than 2026-10-31" (local midnight) bounds the search for the gap
# start: the newest DB row below it is the last dictation that survived.
GAP_CUTOFF_LOCAL = dt.datetime(2026, 10, 31, 0, 0)

# How the running app is recognised (spec: pgrep -f ...).
APP_PATTERNS = ("target/debug/teletype", "Teletype.app")

REFUSED = 2


# ── small helpers ──────────────────────────────────────────────────────
def _now_ms() -> int:
    """Current time in UTC milliseconds."""
    return int(dt.datetime.now(tz=dt.timezone.utc).timestamp() * 1000)


def _utc_text(ms: int) -> str:
    """`YYYY-MM-DD HH:MM:SS.mmm UTC` for a UTC-ms instant."""
    stamp = dt.datetime.fromtimestamp(ms / 1000, tz=dt.timezone.utc)
    return f"{stamp:%Y-%m-%d %H:%M:%S}.{ms % 1000:03d} UTC"


def normalize_whitespace(text: str) -> str:
    """Collapse runs of whitespace so a re-wrapped row still matches."""
    return re.sub(r"\s+", " ", text).strip()


def parse_iso_local(value: str) -> dt.datetime:
    """Parse an ISO-8601 instant; a naive value is read as local time."""
    stamp = dt.datetime.fromisoformat(value.strip())
    if stamp.tzinfo is None:
        stamp = stamp.astimezone()
    return stamp


def _ms_of(stamp: dt.datetime) -> int:
    return int(stamp.timestamp() * 1000)


def _is_lock_error(exc: BaseException) -> bool:
    message = str(exc).lower()
    return "locked" in message or "busy" in message


def _app_running() -> bool:
    """True when a Teletype build appears to be running.

    Patched in the tests so the refusal path can be exercised without an
    actual app process.
    """
    for pattern in APP_PATTERNS:
        try:
            result = subprocess.run(
                ["pgrep", "-f", pattern],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
        except OSError:
            return False
        if result.returncode == 0:
            return True
    return False


def _context_json(app_name: str) -> str:
    """`ApplicationContext` as `serde_json::to_string` would write it.

    The application name comes from the transcript header; every other field
    is the serde default (`AppType::Unknown`, no window title, confidence 0).
    """
    return json.dumps(
        {
            "applicationId": "",
            "applicationName": app_name,
            "applicationType": "unknown",
            "windowTitle": None,
            "confidence": 0.0,
        },
        separators=(",", ":"),
        ensure_ascii=False,
    )


# ── transcript parsing ─────────────────────────────────────────────────
def parse_transcript_file(path: Path) -> list[dict]:
    """Parse one `YYYY-MM-DD.md` archive file into candidate entries.

    Returns dicts of `created_at` (UTC ms, minute-granular), `app` and `text`.
    Text is kept verbatim (internal newlines included) and never printed.
    """
    try:
        year, month, day = (int(part) for part in path.stem.split("-"))
    except ValueError:
        return []

    lines = path.read_text(encoding="utf-8").split("\n")
    headers = [i for i, line in enumerate(lines) if TRANSCRIPT_HEADER.match(line)]

    out: list[dict] = []
    for position, index in enumerate(headers):
        end = headers[position + 1] if position + 1 < len(headers) else len(lines)
        match = TRANSCRIPT_HEADER.match(lines[index])
        hour, minute = int(match.group(1)), int(match.group(2))
        if hour > 23 or minute > 59:
            continue
        app = (match.group(3) or "").strip()

        try:
            created_at = _ms_of(
                dt.datetime(year, month, day, hour, minute, tzinfo=dt.timezone.utc)
            )
        except ValueError:
            # A file whose name is not a real date cannot be placed in time.
            continue

        body = lines[index + 1 : end]
        while body and body[-1].strip() == "":
            body.pop()
        text = "\n".join(body)

        out.append({"created_at": created_at, "app": app, "text": text})
    return out


def parse_transcripts(directory: Path) -> tuple[list[dict], int]:
    """Parse every archive file in `directory`, oldest day first."""
    entries: list[dict] = []
    files = sorted(directory.glob("*.md"))
    for path in files:
        entries.extend(parse_transcript_file(path))
    return entries, len(files)


# ── database ───────────────────────────────────────────────────────────
def _history_columns(conn: sqlite3.Connection) -> list[str]:
    return [row[1] for row in conn.execute("PRAGMA table_info(history)")]


def _gap_start_ms(created_ats: list[int]) -> int | None:
    """Newest `created_at` older than the local 2026-10-31 cutoff."""
    cutoff = _ms_of(GAP_CUTOFF_LOCAL)
    older = [value for value in created_ats if value < cutoff]
    return max(older) if older else None


def _already_exists(created_at_ms: int, normalized: str, by_text: dict) -> bool:
    """Same text and either within +/-2000 ms or in the same UTC minute."""
    for other in by_text.get(normalized, ()):
        if abs(other - created_at_ms) <= DEDUPE_TOLERANCE_MS:
            return True
        if other // 60_000 == created_at_ms // 60_000:
            return True
    return False


def load_json_overlap(data_dir: Path) -> tuple[dict, int, str]:
    """Index the retired `history.json` by text, for duplicate avoidance only.

    `persist.rs::reconcile_history_json` imports every entry of that file the
    database lacks, on the next launch, then renames it. A take in that file
    is therefore already spoken for: re-inserting it from the transcript would
    leave two cards for one dictation. Nothing here is ever written back.

    Returns (index, status note).
    """
    path = data_dir / "history.json"
    if not path.is_file():
        return {}, "absent (already reconciled by the app)"
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
        entries = document.get("entries", [])
    except (OSError, ValueError, AttributeError):
        return {}, "unreadable (ignored)"
    index: dict[str, list[int]] = {}
    for entry in entries:
        try:
            created_at = int(entry["createdAt"])
            text = str(entry["text"])
            identifier = str(entry["id"])
        except (KeyError, TypeError, ValueError):
            continue
        if not identifier:
            continue
        index.setdefault(normalize_whitespace(text), []).append(created_at)
    return index, f"present ({len(entries)} entry(ies))"


# ── the run ────────────────────────────────────────────────────────────
def run(
    data_dir,
    window_from_ms: int | None = None,
    window_to_ms: int | None = None,
    apply: bool = False,
    out=None,
) -> int:
    """Report (and with `apply`, perform) the restore. Returns an exit code."""
    if out is None:
        out = sys.stdout
    data_dir = Path(data_dir)
    db_path = data_dir / "teletype.db"
    transcripts_dir = data_dir / "transcripts"

    def say(line: str = "") -> None:
        print(line, file=out)

    if not db_path.is_file():
        say(f"error: no database at {db_path}")
        return 1
    if not transcripts_dir.is_dir():
        say(f"error: no transcript directory at {transcripts_dir}")
        return 1

    wal_path = db_path.with_name(db_path.name + "-wal")
    wal_bytes = wal_path.stat().st_size if wal_path.exists() else 0
    app_running = _app_running()

    if app_running and wal_bytes > 0:
        say(
            "refused: the app appears to be running "
            f"({', '.join(APP_PATTERNS)}) and {wal_path.name} holds "
            f"{wal_bytes} bytes of unmerged writes."
        )
        say("         quit Teletype and run this again.")
        return REFUSED

    try:
        conn = sqlite3.connect(
            str(db_path), timeout=0.25, isolation_level=None
        )
    except sqlite3.Error as exc:
        say(f"refused: cannot open {db_path}: {exc}")
        return REFUSED

    try:
        try:
            # Takes the write lock and proves nobody else holds it, without
            # writing: rolled back on every path except a committed apply.
            conn.execute("BEGIN IMMEDIATE")
        except sqlite3.OperationalError as exc:
            if _is_lock_error(exc):
                say(
                    f"refused: {db_path.name} is locked by another process "
                    f"({exc}). Quit Teletype and run this again."
                )
                return REFUSED
            say(f"error: cannot lock {db_path.name}: {exc}")
            return 1

        table_columns = _history_columns(conn)
        if not table_columns:
            say("error: this database has no `history` table")
            return 1

        rows = conn.execute(
            "SELECT id, created_at, text FROM history"
        ).fetchall()
        existing_ids = {row[0] for row in rows}
        created_ats = [int(row[1]) for row in rows]
        by_text: dict[str, list[int]] = {}
        for row in rows:
            by_text.setdefault(normalize_whitespace(row[2] or ""), []).append(
                int(row[1])
            )

        gap_start = _gap_start_ms(created_ats)
        if window_from_ms is None:
            window_from_ms = gap_start
            window_origin = "gap start = newest DB row older than 2026-10-31 local"
        else:
            window_origin = "--from"
        if window_to_ms is None:
            window_to_ms = _now_ms()
            window_to_note = "now"
        else:
            window_to_note = "--to"

        json_by_text, json_note = load_json_overlap(data_dir)
        entries, file_count = parse_transcripts(transcripts_dir)

        per_day: dict[str, dict[str, int]] = {}
        to_insert: list[dict] = []
        totals = {
            "parsed": 0,
            "in_window": 0,
            "existing": 0,
            "existing_json": 0,
            "empty": 0,
            "outside": 0,
            "insert": 0,
        }

        for entry in entries:
            day = dt.datetime.fromtimestamp(
                entry["created_at"] / 1000, tz=dt.timezone.utc
            ).strftime("%Y-%m-%d")
            bucket = per_day.setdefault(
                day,
                {
                    "parsed": 0,
                    "in_window": 0,
                    "existing": 0,
                    "empty": 0,
                    "insert": 0,
                },
            )
            bucket["parsed"] += 1
            totals["parsed"] += 1

            if window_from_ms is not None and entry["created_at"] <= window_from_ms:
                totals["outside"] += 1
                continue
            if entry["created_at"] > window_to_ms:
                totals["outside"] += 1
                continue

            bucket["in_window"] += 1
            totals["in_window"] += 1

            if entry["text"].strip() == "":
                bucket["empty"] += 1
                totals["empty"] += 1
                continue

            if _already_exists(
                entry["created_at"],
                normalize_whitespace(entry["text"]),
                by_text,
            ):
                bucket["existing"] += 1
                totals["existing"] += 1
                continue

            if json_by_text and _already_exists(
                entry["created_at"],
                normalize_whitespace(entry["text"]),
                json_by_text,
            ):
                # The app imports this take from history.json itself on the
                # next launch; restoring it here would duplicate it.
                bucket["existing"] += 1
                totals["existing"] += 1
                totals["existing_json"] += 1
                continue

            bucket["insert"] += 1
            totals["insert"] += 1
            to_insert.append(entry)

        # ── report ─────────────────────────────────────────────────────
        say("Restore dictation history from the day-wise transcript archive")
        say(f"data dir    : {data_dir}")
        say(f"database    : {db_path.name} ({len(rows)} history row(s))")
        say(f"transcripts : {file_count} file(s) under {transcripts_dir.name}/")
        say(f"history.json: {json_note}")
        if window_from_ms is None:
            say(f"window      : (beginning of time, {_utc_text(window_to_ms)}]  [{window_origin}]")
        else:
            say(
                f"window      : ({_utc_text(window_from_ms)}, "
                f"{_utc_text(window_to_ms)}]  [{window_origin}]"
            )
        if window_from_ms is not None and window_from_ms == gap_start:
            say(
                "note        : the window starts strictly after the newest row "
                "older than 2026-10-31 local, so nothing surviving in the DB "
                "is re-inserted."
            )
        say(f"to window   : {window_to_note}")
        missing_details = [
            column for column in HISTORY_COLUMNS if column not in table_columns
        ]
        if missing_details:
            say(
                "note        : this history table predates the detail columns "
                f"({', '.join(missing_details)}); they stay absent and the app "
                "adds them on its next launch."
            )
        say(f"mode        : {'APPLY' if apply else 'DRY RUN (no writes)'}")
        say("")
        say(
            f"{'day (UTC)':12} {'parsed':>7} {'in-window':>10} "
            f"{'existing':>9} {'insert':>7} {'empty':>6}"
        )
        for day in sorted(per_day):
            bucket = per_day[day]
            say(
                f"{day:12} {bucket['parsed']:7d} {bucket['in_window']:10d} "
                f"{bucket['existing']:9d} {bucket['insert']:7d} "
                f"{bucket['empty']:6d}"
            )
        say("")

        inserted = 0
        if apply and to_insert:
            insert_columns = [
                column for column in HISTORY_COLUMNS if column in table_columns
            ]
            placeholders = ", ".join("?" for _ in insert_columns)
            statement = (
                f"INSERT INTO history ({', '.join(insert_columns)}) "
                f"VALUES ({placeholders})"
            )
            for entry in to_insert:
                new_id = str(uuid.uuid4())
                while new_id in existing_ids:
                    new_id = str(uuid.uuid4())
                existing_ids.add(new_id)
                values = {
                    "id": new_id,
                    "created_at": entry["created_at"],
                    "text": entry["text"],
                    "context_json": _context_json(entry["app"]),
                    "duration_ms": None,
                    "raw_text": None,
                    "edits_json": None,
                    "asr_model": None,
                    "polish_model": None,
                    "polish_ms": None,
                    "word_count": None,
                    "recording_path": None,
                }
                conn.execute(statement, [values[column] for column in insert_columns])
                inserted += 1

        if apply:
            conn.execute("COMMIT")
        else:
            conn.execute("ROLLBACK")

        say(f"outside window: {totals['outside']} entry(ies) not considered")
        if totals["existing_json"]:
            say(
                f"already in history.json: {totals['existing_json']} entry(ies) "
                "the app will import itself"
            )
        if not apply:
            say(f"would insert: {totals['insert']} row(s)")
        else:
            after = conn.execute("SELECT COUNT(*) FROM history").fetchone()[0]
            say(f"history rows now: {after}")
        say(
            f"inserted {inserted} rows, skipped {totals['existing']} existing, "
            f"ignored {totals['empty']} empty"
        )
        return 0
    except sqlite3.Error as exc:
        try:
            conn.execute("ROLLBACK")
        except sqlite3.Error:
            pass
        say(f"error: database error: {exc}")
        return 1
    finally:
        conn.close()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Restore missing dictation-history rows from the day-wise "
            "transcript files. Dry run unless --apply is given."
        )
    )
    parser.add_argument(
        "--data-dir",
        required=True,
        help="directory holding teletype.db and transcripts/ (work on a copy)",
    )
    parser.add_argument(
        "--from",
        dest="window_from",
        default=None,
        metavar="ISO",
        help="exclusive lower bound on created_at (ISO-8601; naive = local)",
    )
    parser.add_argument(
        "--to",
        dest="window_to",
        default=None,
        metavar="ISO",
        help="inclusive upper bound on created_at (ISO-8601; naive = local)",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="insert the missing rows (default: dry run, no writes)",
    )
    args = parser.parse_args(argv)

    try:
        window_from = (
            _ms_of(parse_iso_local(args.window_from))
            if args.window_from is not None
            else None
        )
        window_to = (
            _ms_of(parse_iso_local(args.window_to))
            if args.window_to is not None
            else None
        )
    except ValueError as exc:
        print(f"error: bad --from/--to value: {exc}", file=sys.stderr)
        return 1

    return run(
        data_dir=args.data_dir,
        window_from_ms=window_from,
        window_to_ms=window_to,
        apply=args.apply,
    )


if __name__ == "__main__":
    raise SystemExit(main())
