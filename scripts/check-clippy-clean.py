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

It also refuses to pass on an empty stream. That guard is not defensive
padding: CI once ran this for weeks without the clippy component installed,
so `cargo clippy` died instantly with its error on stderr, the JSON stdout
was empty, and this script cheerfully printed "clippy: clean" before the
step exited 1. A linter that reports success without having linted anything
is worse than a linter that is missing, because it is trusted.

Run under CI as:

    set -o pipefail
    cargo clippy --workspace --all-targets --message-format=json \
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
    errors: list[str] = []
    notes: list[str] = []
    artifacts = 0
    cargo_failed = False

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
        reason = msg.get("reason")
        if reason == "build-finished":
            # Cargo's own verdict on the run, independent of any lint.
            cargo_failed = not msg.get("success", True)
            continue
        if reason == "compiler-artifact":
            artifacts += 1
            continue
        if reason != "compiler-message":
            continue
        body = msg.get("message", {})
        level = body.get("level")
        text = body.get("message", "")
        if level == "error":
            errors.append(text)
            continue
        if level != "warning":
            continue
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

    if errors:
        print(f"\n{len(errors)} clippy/compiler error(s):\n", file=sys.stderr)
        for text in errors:
            print(f"  {text}", file=sys.stderr)
        return 1

    if warnings:
        print(f"\n{len(warnings)} clippy warning(s):\n", file=sys.stderr)
        for file, line, text in warnings:
            print(f"  {file}:{line}  {text}", file=sys.stderr)
        return 1

    # Did clippy actually run? `compiler-artifact` is the signal: cargo emits
    # one per compiled crate on every real run, and emits nothing at all when
    # clippy never started. Do NOT use `compiler-message` for this: a clean
    # workspace produces zero of those (only artifacts and build-finished), so
    # counting them would fail every healthy build.
    #
    # This guard exists because CI once ran for weeks with no clippy component
    # installed. `cargo clippy` died instantly, its error went to stderr, the
    # JSON stdout was empty, and this script printed "clippy: clean" right
    # before the step exited 1. A linter that reports success without having
    # linted anything is worse than a missing one, because it gets trusted.
    if artifacts == 0 or cargo_failed:
        print(
            "::error::clippy did not complete a build "
            f"({artifacts} compiler artifact(s), cargo success="
            f"{not cargo_failed}). Nothing was linted. Read the step's stderr "
            "above: the usual cause is a toolchain installed without the clippy "
            "component (`rustup component add clippy`).",
            file=sys.stderr,
        )
        return 1

    print(f"clippy: clean ({artifacts} crate(s), 0 lint warnings)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
