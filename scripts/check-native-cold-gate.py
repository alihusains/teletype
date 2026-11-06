#!/usr/bin/env python3
"""Assert the vendored ggml really compiled with the 15.0 floor.

Reads the `cargo build --release -vv` log written by the native-cold-macos
job and fails unless every cmake configure of the vendored transcribe-cpp-sys
crate carries at least one C/CXX/ASM deployment target, and every such target
is well-formed and at or above the 15.0 release floor. Other tokens in the
flag values (-O2, -isysroot and its path, ...) are valid compiler flags and
are accepted; a target value with a second '=' is malformed and fails.

The log is parsed token by token with shlex, not by line substring: a line
that merely mentions the crate name (a `cmake --build` line, a Compiling
line, a path of some other crate) cannot satisfy the gate. The executable
may be the bare word `cmake` or an absolute path, as cargo -vv prints it.

Usage: check-native-cold-gate.py LOG [FLOOR]
Exit 0 when the floor is proven, exit 1 with a reason on stderr otherwise.
Stdlib only, no network.
"""

import re
import shlex
import sys

CRATE = "transcribe-cpp-sys"
FLAG = "-mmacosx-version-min="
VERSION = re.compile(r"^\d+(\.\d+)?$")


def version_ok(value, floor):
    """A deployment target is good when well formed and at or above floor."""
    return bool(VERSION.match(value)) and float(value) >= floor


def is_cmake_exec(token):
    """The program token: bare cmake or an absolute path ending in /cmake.

    The executable is the first word of the command; cargo -vv does not
    quote it, so it is already a single shlex token.
    """
    return token == "cmake" or (token.startswith("/") and token.endswith("/cmake"))


def is_cmake_line(line):
    """A cargo -vv Running line whose command is cmake (bare or absolute)."""
    if "Running" not in line:
        return False
    m = re.search(r"Running `([^`]+)`", line)
    if not m:
        return False
    first = m.group(1).split()[0]
    return is_cmake_exec(first)


def configure_records(lines):
    """Yield the argument tokens of every cmake configure line in the log.

    A configure line is a cargo -vv `Running` line whose program is cmake
    and which carries a source dir argument (positional or -S). The build
    line (`cmake --build ...`) has no source dir and is not a configure.
    """
    for line in lines:
        if not is_cmake_line(line):
            continue
        # shlex.split would choke on the trailing backtick cargo prints
        # after the last argument, so tokenize the command inside the
        # backticks only.
        m = re.search(r"Running `([^`]+)`", line)
        if not m:
            continue
        try:
            tokens = shlex.split(m.group(1))
        except ValueError:
            continue
        if len(tokens) < 2:
            continue
        # A configure carries a source dir: -S <dir> or a positional path.
        # The build line (`cmake --build <dir> ...`) has none.
        has_source = False
        for i, tok in enumerate(tokens[1:]):
            if tok == "-S" and i + 1 < len(tokens) - 1:
                has_source = True
            elif not tok.startswith("-"):
                has_source = True
        if has_source:
            yield tokens


def flag_values(tokens):
    """Yield the values of -DCMAKE_<C|CXX|ASM>_FLAGS=... in one configure."""
    for tok in tokens:
        m = re.match(r"^-DCMAKE_(?:C|CXX|ASM)_FLAGS=(.*)$", tok)
        if m:
            yield m.group(1)


def main(argv):
    if len(argv) < 2:
        print("usage: check-native-cold-gate.py LOG [FLOOR]", file=sys.stderr)
        return 2
    log_path = argv[1]
    floor = float(argv[2]) if len(argv) > 2 else 15.0
    try:
        with open(log_path, encoding="utf-8", errors="replace") as f:
            lines = f.readlines()
    except OSError as e:
        print("cannot read build log: %s" % e, file=sys.stderr)
        return 1

    records = list(configure_records(lines))
    if not any(CRATE in tok for rec in records for tok in rec):
        print(
            "no cmake configure of %s found in the build log; the vendored "
            "ggml C++ was not compiled in this run" % CRATE,
            file=sys.stderr,
        )
        return 1

    for rec in records:
        if not any(CRATE in tok for tok in rec):
            continue
        values = [v for val in flag_values(rec) for v in val.split()]
        # Only the deployment-target tokens are judged; any other token in
        # the value is a compiler flag (-O2, -isysroot, its path, ...) and
        # is accepted as-is.
        targets = [v[len(FLAG):] for v in values if v.startswith(FLAG)]
        if not targets:
            print(
                "no C/CXX/ASM deployment target found in the %s cmake "
                "configure; the 15.0 floor is not proven" % CRATE,
                file=sys.stderr,
            )
            return 1
        low = [t for t in targets if not version_ok(t, floor)]
        if low:
            print(
                "deployment target below the %.1f floor or malformed in the "
                "%s cmake configure: %s" % (floor, CRATE, " ".join(low)),
                file=sys.stderr,
            )
            return 1

    print("cold build configured %s with a deployment target at or above the floor" % CRATE)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
