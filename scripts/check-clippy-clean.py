#!/usr/bin/env python3
"""Fail if clippy produced any warning we are responsible for.

`cargo clippy -D warnings` cannot be used directly: the `teletype-desktop`
build script links macOS frameworks through the `cc` crate, and clang then
writes

    teletype-desktop@0.1.1: clang: warning: -framework Cocoa: 'linker' input
    unused [-Wunused-command-line-argument]

to the *build script's* stderr. Cargo forwards those as crate-level messages, so
they appear in the same stream as real lint output and `-D warnings` would fail
on code nobody here can change.

So this filters the JSON stream instead:

- messages with a `message.code` are real lints and must be zero;
- messages without one are Cargo build-script notes (the clang noise, the
  whisper.cpp download notice) and are reported but not fatal.

Run under CI as:

    cargo clippy --workspace --all-targets --message-format=json 2>/dev/null \
      | python3 scripts/check-clippy-clean.py

Also useful locally, where it prints where each warning is.
"""
from __future__ import annotations

import json
import sys

# Substrings of non-lint build-script noise. A Cargo message with no `code` is
# a build-script note by definition, so these are only used to label the output.
NOISE_MARKERS = (
    "linker' input unused",
    "Downloading whisper.cpp",
)


def main() -> int:
    warnings: list[tuple[str, str, str]] = []
    notes: list[str] = []

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            # Not JSON: cargo wrote something else (a compiler error, a panic).
            # Surface it rather than swallowing it.
            print(line, file=sys.stderr)
            return 1
        if msg.get("reason") != "compiler-message":
            continue
        body = msg.get("message", {})
        if body.get("level") != "warning":
            continue
        text = body.get("message", "")
        code = (body.get("code") or {}).get("code")
        if code is None:
            notes.append(text)
            continue
        for span in body.get("spans", []):
            if span.get("is_primary"):
                warnings.append((span["file_name"], str(span["line_start"]), text))

    for note in notes:
        label = "build script" if any(m in note for m in NOISE_MARKERS) else "other"
        print(f"note (non-fatal, {label}): {note}")

    if warnings:
        print(f"\n{len(warnings)} clippy warning(s):\n", file=sys.stderr)
        for file, line, text in warnings:
            print(f"  {file}:{line}  {text}", file=sys.stderr)
        return 1

    print("clippy: clean (no lint warnings)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
