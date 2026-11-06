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

A cmake configure is recognized in two line shapes: cargo's own
``Running `... cmake ...` `` for build scripts it runs, and the cmake crate's
``[crate] running: cd <dir> && <env> "cmake" ...`` lines (cmake 0.1.58
prints each command through the build script's stdout, which is the shape
the vendored transcribe-cpp-sys configure actually takes in the CI log).

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


def cmake_command(line):
    """Return the cmake command string for a line that runs cmake, else None.

    Two emitters print a cmake invocation in a `cargo build -vv` log:

      * cargo itself, for a build script it runs, prints
        ``Running `... cmake ...```; and
      * the cmake crate (0.1.58) prints every command it runs through
        ``println!``, which cargo -vv attributes to the build script as
        ``[transcribe-cpp-sys 0.4.1] running: cd <dir> && "cmake" ...``.

    The vendored transcribe-cpp-sys configure is emitted by the second form,
    which the first check alone would miss and fail the gate on a correct
    build, so both are recognized.
    """
    m = re.search(r"Running `([^`]+)`", line)
    if m:
        cmd = m.group(1)
        if is_cmake_exec(cmd.split()[0]):
            return cmd
        return None
    m = re.search(r"running:\s*(.+)$", line)
    if not m:
        return None
    # The command follows `cd "<dir>" &&`; strip it. The executable may be
    # quoted ("cmake") or bare, and an absolute path, so shlex the rest.
    cmd = m.group(1)
    if "&&" in cmd:
        cmd = cmd.split("&&", 1)[1]
    try:
        tokens = shlex.split(cmd.strip())
    except ValueError:
        return None
    # The cmake crate prefixes the command with environment assignments
    # (CMAKE_PREFIX_PATH="" LC_ALL="C" ...); skip them to reach the
    # executable.
    for tok in tokens:
        if "=" in tok and not tok.startswith("-"):
            continue
        return cmd if is_cmake_exec(tok) else None
    return None


def configure_records(lines):
    """Yield the argument tokens of every cmake configure line in the log.

    A configure line is a cmake invocation that carries a source dir
    argument (positional or -S). The build line (`cmake --build ...`) has no
    source dir and is not a configure.
    """
    for line in lines:
        cmd = cmake_command(line)
        if not cmd:
            continue
        # shlex.split would choke on the trailing backtick cargo prints
        # after the last argument, so tokenize the command inside the
        # backticks only.
        try:
            tokens = shlex.split(cmd)
        except ValueError:
            continue
        if len(tokens) < 2:
            continue
        # Skip leading env assignments, then the cmake executable, to reach
        # the first real argument.
        args = tokens[1:]
        while args and "=" in args[0] and not args[0].startswith("-"):
            args = args[1:]
        if args and is_cmake_exec(args[0]):
            args = args[1:]
        # A `cmake --build <dir> ...` line is a build, not a configure: it
        # runs the install target and has no -D flags. Reject it outright so
        # its positional build dir cannot masquerade as a source dir.
        if args and args[0] == "--build":
            continue
        # A configure carries a source dir: -S <dir> or a positional path.
        has_source = False
        for i, tok in enumerate(args):
            if tok == "-S" and i + 1 < len(args) - 1:
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
