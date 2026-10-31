#!/usr/bin/env python3
"""Tests for `restore_history_from_transcripts.py`.

Everything here is synthetic. No real transcript text is read, quoted or
asserted on: the fixtures are made-up lines, and the assertions are about
counts, ids, column values and timestamps only.
"""

from __future__ import annotations

import contextlib
import datetime as dt
import io
import json
import re
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import restore_history_from_transcripts as restore  # noqa: E402

# The real `history` table, exactly as `persist.rs::create_tables` creates it.
FULL_SCHEMA = """
CREATE TABLE history (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    text TEXT NOT NULL,
    context_json TEXT,
    duration_ms INTEGER,
    raw_text TEXT,
    edits_json TEXT,
    asr_model TEXT,
    polish_model TEXT,
    polish_ms INTEGER,
    word_count INTEGER,
    recording_path TEXT
);
"""

# A database created before the detail columns existed. It is still valid
# input: the app adds the missing columns on its next launch.
OLD_SCHEMA = """
CREATE TABLE history (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    text TEXT NOT NULL,
    context_json TEXT,
    duration_ms INTEGER
);
"""


def utc_ms(year: int, month: int, day: int, hour: int, minute: int, second: int = 0) -> int:
    stamp = dt.datetime(
        year, month, day, hour, minute, second, tzinfo=dt.timezone.utc
    )
    return int(stamp.timestamp() * 1000)


class Fixture:
    """A temporary data dir laid out like the app's config dir."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self.transcripts = root / "transcripts"
        self.transcripts.mkdir()

    @property
    def db_path(self) -> Path:
        return self.root / "teletype.db"

    def create_db(self, rows=(), schema: str = FULL_SCHEMA) -> None:
        conn = sqlite3.connect(str(self.db_path))
        try:
            conn.executescript(schema)
            conn.executemany(
                "INSERT INTO history (id, created_at, text, context_json, duration_ms)"
                " VALUES (?, ?, ?, ?, ?)",
                rows,
            )
            conn.commit()
        finally:
            conn.close()

    def write_transcript(
        self, day: str, hour: int, minute: int, text: str, app: str = ""
    ) -> None:
        """Mirror `history.rs::append_transcript_file` byte for byte."""
        path = self.transcripts / f"{day}.md"
        is_new = not path.exists()
        with path.open("a", encoding="utf-8", newline="\n") as handle:
            if is_new:
                handle.write(f"# Teletype transcripts — {day}\n\n")
            suffix = f" · {app}" if app else ""
            handle.write(f"## {hour:02d}:{minute:02d}{suffix}\n{text}\n\n")

    def rows(self):
        conn = sqlite3.connect(str(self.db_path))
        try:
            # `*` so the same helper works against the old (narrow) schema,
            # where the app's detail columns do not exist yet.
            return conn.execute(
                "SELECT * FROM history ORDER BY created_at, id"
            ).fetchall()
        finally:
            conn.close()

    def count(self) -> int:
        conn = sqlite3.connect(str(self.db_path))
        try:
            return conn.execute("SELECT COUNT(*) FROM history").fetchone()[0]
        finally:
            conn.close()

    def texts(self) -> list[str]:
        return [row[2] for row in self.rows()]


class RestoreTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.fixture = Fixture(Path(self._tmp.name))
        self._original_app_running = restore._app_running
        restore._app_running = lambda: False

    def tearDown(self) -> None:
        restore._app_running = self._original_app_running
        self._tmp.cleanup()

    def run_tool(self, **kwargs):
        buffer = io.StringIO()
        code = restore.run(self.fixture.root, out=buffer, **kwargs)
        return code, buffer.getvalue()


class DryRunTests(RestoreTestCase):
    def test_dry_run_writes_nothing(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 29, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-29", 9, 0, "fixture beta line")
        self.fixture.write_transcript("2026-10-29", 10, 0, "fixture gamma line")
        before = self.fixture.rows()

        code, output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 29, 7, 0),
            window_to_ms=utc_ms(2026, 10, 29, 23, 0),
        )

        self.assertEqual(code, 0)
        self.assertEqual(self.fixture.rows(), before)
        self.assertEqual(self.fixture.count(), 1)
        self.assertIn("DRY RUN", output)
        self.assertIn("would insert: 2 row(s)", output)
        self.assertIn("inserted 0 rows", output)

    def test_dry_run_reports_the_window_it_used(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 28, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-28", 9, 0, "fixture beta line")
        code, output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 28, 7, 0),
            window_to_ms=utc_ms(2026, 10, 28, 23, 0),
        )
        self.assertEqual(code, 0)
        self.assertIn("window      : (2026-10-28 07:00:00.000 UTC", output)
        self.assertIn("2026-10-28 23:00:00.000 UTC]", output)


class ApplyTests(RestoreTestCase):
    def test_apply_inserts_only_the_missing_rows(self) -> None:
        existing_at = utc_ms(2026, 10, 30, 9, 0)
        near_at = utc_ms(2026, 10, 30, 9, 59, 58)
        self.fixture.create_db(
            [
                ("existing-id", existing_at, "fixture existing line", None, None),
                ("near-id", near_at, "fixture near line", None, None),
            ]
        )
        # Same text at the same minute, same text 2000 ms away in the
        # previous minute, and one genuinely missing entry.
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture existing line", "TextEdit")
        self.fixture.write_transcript("2026-10-30", 10, 0, "fixture near line")
        self.fixture.write_transcript("2026-10-30", 10, 30, "fixture missing line", "Slack")

        code, output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        self.assertEqual(self.fixture.count(), 3)
        self.assertIn("inserted 1 rows, skipped 2 existing, ignored 0 empty", output)
        self.assertEqual(self.fixture.texts().count("fixture missing line"), 1)

    def test_existing_rows_are_not_touched(self) -> None:
        existing_at = utc_ms(2026, 10, 30, 9, 0)
        self.fixture.create_db(
            [
                (
                    "existing-id",
                    existing_at,
                    "fixture existing line",
                    '{"applicationId":"","applicationName":"Notes",'
                    '"applicationType":"unknown","windowTitle":null,"confidence":0.0}',
                    1234,
                )
            ]
        )
        before = self.fixture.rows()
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture existing line")
        self.fixture.write_transcript("2026-10-30", 10, 0, "fixture missing line")

        code, _ = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        after = self.fixture.rows()
        self.assertIn(before[0], after)
        self.assertEqual(len(after), 2)

    def test_second_apply_inserts_nothing(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture beta line")
        self.fixture.write_transcript("2026-10-30", 10, 0, "fixture gamma line")
        window = dict(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
        )

        first_code, first_output = self.run_tool(apply=True, **window)
        rows_after_first = self.fixture.rows()
        second_code, second_output = self.run_tool(apply=True, **window)

        self.assertEqual(first_code, 0)
        self.assertEqual(second_code, 0)
        self.assertIn("inserted 2 rows", first_output)
        self.assertIn("inserted 0 rows, skipped 2 existing, ignored 0 empty", second_output)
        self.assertEqual(self.fixture.rows(), rows_after_first)

    def test_empty_entries_are_ignored(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "")
        self.fixture.write_transcript("2026-10-30", 10, 0, "   \n  ")

        code, output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        self.assertEqual(self.fixture.count(), 1)
        self.assertIn("inserted 0 rows, skipped 0 existing, ignored 2 empty", output)

    def test_multi_line_text_round_trips(self) -> None:
        body = "fixture first line\n\nfixture second paragraph\n- fixture bullet"
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, body)

        code, _ = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        self.assertIn(body, self.fixture.texts())

    def test_inserted_row_uses_the_apps_column_list_and_shapes(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture beta line", "FixtureApp")

        code, _ = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )
        self.assertEqual(code, 0)

        inserted = [row for row in self.fixture.rows() if row[2] == "fixture beta line"]
        self.assertEqual(len(inserted), 1)
        (
            row_id,
            created_at,
            _text,
            context_json,
            duration_ms,
            raw_text,
            edits_json,
            asr_model,
            polish_model,
            polish_ms,
            word_count,
            recording_path,
        ) = inserted[0]
        self.assertEqual(created_at, utc_ms(2026, 10, 30, 9, 0))
        self.assertEqual(
            context_json,
            '{"applicationId":"","applicationName":"FixtureApp",'
            '"applicationType":"unknown","windowTitle":null,"confidence":0.0}',
        )
        for detail in (
            duration_ms,
            raw_text,
            edits_json,
            asr_model,
            polish_model,
            polish_ms,
            word_count,
            recording_path,
        ):
            self.assertIsNone(detail)
        self.assertTrue(re.fullmatch(r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}", row_id))

    def test_old_schema_database_still_receives_rows(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)],
            schema=OLD_SCHEMA,
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture beta line")

        code, output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        self.assertIn("inserted 1 rows", output)
        self.assertIn("fixture beta line", self.fixture.texts())


class TimestampTests(RestoreTestCase):
    def test_header_time_is_utc_as_the_app_writes_it(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 7, 23, "fixture utc line")

        entries, files = restore.parse_transcripts(self.fixture.transcripts)

        self.assertEqual(files, 1)
        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0]["created_at"], utc_ms(2026, 10, 30, 7, 23))
        # Not the local-time reading of the same clock face.
        self.assertNotEqual(entries[0]["created_at"], utc_ms(2026, 10, 30, 3, 23))

    def test_stored_created_at_matches_the_parsed_utc_ms(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 23, 5, "fixture late line")

        code, _ = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 31, 12, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        inserted = [row for row in self.fixture.rows() if row[2] == "fixture late line"]
        self.assertEqual(inserted[0][1], utc_ms(2026, 10, 30, 23, 5))


class WindowTests(RestoreTestCase):
    def test_default_window_starts_at_the_gap_start(self) -> None:
        # Both rows predate the 2026-10-31 cutoff, so the newest one is the
        # gap start and everything at or before it is out of scope.
        self.fixture.create_db(
            [
                ("seed-id", utc_ms(2026, 10, 28, 8, 0), "fixture seed line", None, None),
                ("last-id", utc_ms(2026, 10, 30, 9, 0), "fixture last line", None, None),
            ]
        )
        self.fixture.write_transcript("2026-10-28", 9, 0, "fixture before gap line")
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture last line")
        self.fixture.write_transcript("2026-10-30", 10, 0, "fixture after gap line")

        code, output = self.run_tool(apply=True)

        self.assertEqual(code, 0)
        self.assertIn("gap start = newest DB row older than 2026-10-31 local", output)
        self.assertIn("outside window: 2 entry(ies)", output)
        self.assertIn("inserted 1 rows", output)
        self.assertIn("fixture after gap line", self.fixture.texts())
        self.assertNotIn("fixture before gap line", self.fixture.texts())

    def test_explicit_window_bounds_are_respected(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 27, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-28", 10, 0, "fixture before line")
        self.fixture.write_transcript("2026-10-29", 10, 0, "fixture inside line")
        self.fixture.write_transcript("2026-10-30", 10, 0, "fixture after line")

        code, _ = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 28, 23, 0),
            window_to_ms=utc_ms(2026, 10, 30, 0, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        texts = self.fixture.texts()
        self.assertIn("fixture inside line", texts)
        self.assertNotIn("fixture before line", texts)
        self.assertNotIn("fixture after line", texts)

    def test_cli_from_and_to_are_parsed(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 27, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-28", 10, 0, "fixture before line")
        self.fixture.write_transcript("2026-10-29", 10, 0, "fixture inside line")

        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = restore.main(
                [
                    "--data-dir",
                    str(self.fixture.root),
                    "--from",
                    "2026-10-28T23:00:00+00:00",
                    "--to",
                    "2026-10-30T00:00:00+00:00",
                    "--apply",
                ]
            )

        self.assertEqual(code, 0)
        self.assertIn("inserted 1 rows", buffer.getvalue())
        self.assertIn("fixture inside line", self.fixture.texts())


class RefusalTests(RestoreTestCase):
    def test_refuses_when_the_database_is_locked(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture beta line")

        holder = sqlite3.connect(str(self.fixture.db_path), isolation_level=None)
        try:
            holder.execute("BEGIN EXCLUSIVE")
            code, output = self.run_tool(
                window_from_ms=utc_ms(2026, 10, 30, 8, 0),
                window_to_ms=utc_ms(2026, 10, 30, 23, 0),
                apply=True,
            )
        finally:
            holder.execute("ROLLBACK")
            holder.close()

        self.assertEqual(code, restore.REFUSED)
        self.assertIn("locked", output)
        self.assertEqual(self.fixture.count(), 1)

    def test_refuses_when_the_app_runs_with_a_non_empty_wal(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture beta line")
        wal = self.fixture.db_path.with_name(self.fixture.db_path.name + "-wal")
        wal.write_bytes(b"\x00" * 64)
        restore._app_running = lambda: True

        code, output = self.run_tool(apply=True)

        self.assertEqual(code, restore.REFUSED)
        self.assertIn("refused", output)
        self.assertIn("running", output)
        self.assertEqual(self.fixture.count(), 1)

    def test_missing_database_is_an_error(self) -> None:
        code, output = self.run_tool()
        self.assertEqual(code, 1)
        self.assertIn("no database", output)

    def test_missing_transcripts_directory_is_an_error(self) -> None:
        self.fixture.create_db()
        self.fixture.transcripts.rmdir()
        code, output = self.run_tool()
        self.assertEqual(code, 1)
        self.assertIn("no transcript directory", output)


class JsonOverlapTests(RestoreTestCase):
    def write_history_json(self, entries) -> None:
        (self.fixture.root / "history.json").write_text(
            json.dumps({"entries": entries}, ensure_ascii=False), encoding="utf-8"
        )

    def test_entries_the_app_will_import_from_json_are_not_duplicated(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture json line")
        self.fixture.write_transcript("2026-10-30", 10, 0, "fixture missing line")
        self.write_history_json(
            [
                {
                    "id": "0b7f3d3a-1111-4222-8333-444455556666",
                    "createdAt": utc_ms(2026, 10, 30, 9, 0, 37),
                    "text": "fixture json line",
                }
            ]
        )

        code, output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        self.assertIn("inserted 1 rows, skipped 1 existing, ignored 0 empty", output)
        self.assertIn("already in history.json: 1 entry(ies)", output)
        self.assertNotIn("fixture json line", self.fixture.texts())

    def test_unreadable_history_json_is_ignored(self) -> None:
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), "fixture seed line", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, "fixture beta line")
        (self.fixture.root / "history.json").write_text("{not json", encoding="utf-8")

        code, output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        self.assertEqual(code, 0)
        self.assertIn("unreadable", output)
        self.assertIn("inserted 1 rows", output)


class PrivacyTests(RestoreTestCase):
    def test_output_never_contains_transcript_text(self) -> None:
        marker = "zzmarkerzz"
        body = f"{marker} first line\n{marker} second line"
        self.fixture.create_db(
            [("seed-id", utc_ms(2026, 10, 30, 8, 0), f"{marker} seed", None, None)]
        )
        self.fixture.write_transcript("2026-10-30", 9, 0, body, "TextEdit")

        _, dry_output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
        )
        _, apply_output = self.run_tool(
            window_from_ms=utc_ms(2026, 10, 30, 8, 0),
            window_to_ms=utc_ms(2026, 10, 30, 23, 0),
            apply=True,
        )

        for output in (dry_output, apply_output):
            self.assertNotIn(marker, output)


if __name__ == "__main__":
    unittest.main()
