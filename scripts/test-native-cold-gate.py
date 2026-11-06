#!/usr/bin/env python3
"""Regression test for the native-cold-macos deployment floor gate in ci.yml.

Executes the actual "Assert the vendored ggml really compiled with the 15.0
floor" step script extracted from .github/workflows/ci.yml against synthetic
build logs. No duplicate evaluator lives in this file: the step text is pulled
straight out of the workflow YAML on every run, so a change to the step is
tested directly.

The synthetic logs mirror what a real `cargo build --release -vv` run prints.
The vendored transcribe-cpp-sys (0.4.1) is configured through the cmake crate,
which passes the source dir positionally, then -B, then -D defines; cargo
prints each command as `Running` with every argument shell-quoted. The
C/CXX/ASM compiler flag vars arrive as -DCMAKE_<LANG>_FLAGS=... and the
deployment floor can ride on any of them as -mmacosx-version-min=V.

Exit 0 when every case gets the expected verdict, exit 1 otherwise.
Stdlib only, no network.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile

CI_PATH = os.path.join(os.path.dirname(__file__), "..", ".github", "workflows", "ci.yml")
STEP_NAME = "Assert the vendored ggml really compiled with the 15.0 floor"
LOG_NAME = "native-cold-macos.log"
FLOOR = "15.0"

# One cmake configure of the vendored crate, as cargo -vv prints it.
CONFIGURE = (
    '     Running `cmake '
    '"-S" "/Users/runner/work/teletype/teletype/target/release/build/'
    'transcribe-cpp-sys-0.4.1/out" '
    '"-B" "/Users/runner/work/teletype/teletype/target/release/build/'
    'transcribe-cpp-sys-0.4.1/out/build" '
    '"-DCMAKE_BUILD_TYPE=Release" '
    '{flags}'
    '`'
)
# Same shape, different crate: an unrelated floor must not satisfy the gate.
OTHER_CONFIGURE = (
    '     Running `cmake '
    '"-S" "/Users/runner/work/teletype/teletype/target/release/build/'
    'some-other-cpp-sys-9.9.9/out" '
    '"-B" "/Users/runner/work/teletype/teletype/target/release/build/'
    'some-other-cpp-sys-9.9.9/out/build" '
    '"-DCMAKE_BUILD_TYPE=Release" '
    '{flags}'
    '`'
)
# Same configure, executable printed as an absolute path: cargo -vv does
# not quote the program, and cmake_executable() may be a full path when
# the CMAKE env var is set.
CONFIGURE_ABS = (
    '     Running `/usr/local/bin/cmake '
    '"-S" "/Users/runner/work/teletype/teletype/target/release/build/'
    'transcribe-cpp-sys-0.4.1/out" '
    '"-B" "/Users/runner/work/teletype/teletype/target/release/build/'
    'transcribe-cpp-sys-0.4.1/out/build" '
    '"-DCMAKE_BUILD_TYPE=Release" '
    '{flags}'
    '`'
)
NOISE = "   Compiling teletype-speech v0.2.17\n"

# The cmake crate (0.1.58) does not let cargo print its commands as `Running`
# lines. It prints them through the build script's stdout, which cargo -vv
# attributes to the script as `[crate] running: cd <dir> && <env> "cmake" ...`.
# The vendored transcribe-cpp-sys configure arrives in this shape, not the
# `Running` shape above, so the gate must recognize both.
BUILD_SCRIPT_CONFIGURE = (
    '[transcribe-cpp-sys 0.4.1] running: cd "/tmp/teletype-native-cold/release/build/'
    'transcribe-cpp-sys-84953261480dc422/out/build" && CMAKE_PREFIX_PATH="" LC_ALL="C" '
    '"cmake" "/Users/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/'
    'transcribe-cpp-sys-0.4.1" "-B" "/tmp/teletype-native-cold/release/build/'
    'transcribe-cpp-sys-84953261480dc422/out/build" "-DCMAKE_BUILD_TYPE=Release" '
    '{flags}'
)
# Same crate, the cmake --build line: a build, not a configure. Its positional
# build dir must not be mistaken for a source dir.
BUILD_SCRIPT_BUILD = (
    '[transcribe-cpp-sys 0.4.1] running: cd "/tmp/teletype-native-cold/release/build/'
    'transcribe-cpp-sys-84953261480dc422/out/build" && LC_ALL="C" "cmake" "--build" '
    '"/tmp/teletype-native-cold/release/build/transcribe-cpp-sys-84953261480dc422/out/build" '
    '"--target" "install" "--config" "Release" "--parallel" "3"'
)


def step_script():
    """Extract the raw `run:` block of the floor assert step from ci.yml."""
    with open(CI_PATH, encoding="utf-8") as f:
        text = f.read()
    m = re.search(
        r"^      - name: " + re.escape(STEP_NAME) + r"\n"
        r"(?:^        .*\n)*?"
        r"^        run: \|\n((?:^          .*\n)+)",
        text,
        re.M,
    )
    if not m:
        sys.exit("FATAL: could not find step %r in %s" % (STEP_NAME, CI_PATH))
    lines = m.group(1).splitlines()
    if not all(l.startswith("          ") for l in lines):
        sys.exit("FATAL: unexpected indentation in extracted step block")
    return "\n".join(l[10:] for l in lines) + "\n"


VALIDATOR = os.path.join(os.path.dirname(__file__), "check-native-cold-gate.py")


def run_case(label, log_lines, expect_pass):
    """Run the real step script against one synthetic log; return a result line."""
    tmp = tempfile.mkdtemp(prefix="native-cold-gate-")
    try:
        # The CI step resolves the validator under $GITHUB_WORKSPACE, which
        # this harness points at a temp dir; copy the real validator in.
        os.makedirs(os.path.join(tmp, "scripts"), exist_ok=True)
        shutil.copyfile(VALIDATOR, os.path.join(tmp, "scripts", "check-native-cold-gate.py"))
        with open(os.path.join(tmp, LOG_NAME), "w", encoding="utf-8") as f:
            f.write("\n".join(log_lines) + "\n")
        env = dict(os.environ)
        env["GITHUB_WORKSPACE"] = tmp
        p = subprocess.run(
            ["/bin/bash", "-c", step_script()],
            env=env,
            capture_output=True,
            text=True,
        )
        got_pass = p.returncode == 0
        ok = got_pass == expect_pass
        want = "PASS" if expect_pass else "FAIL"
        got = "PASS" if got_pass else "FAIL"
        detail = ""
        if not ok:
            detail = " | stderr: %s" % (p.stderr.strip().splitlines() or ["<empty>"])[-1]
        return ok, "case %-28s expected %-4s got %-4s%s" % (
            label,
            want,
            got,
            detail,
        )
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    script = step_script()
    if "transcribe-cpp-sys" not in script:
        sys.exit("FATAL: extracted step does not look like the floor assert step")

    cases = [
        # (label, log lines, should pass)
        ("valid CXX 15.0", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"')], True),
        ("valid CXX 16.4", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=16.4"')], True),
        ("valid C 15.0", [NOISE, CONFIGURE.format(flags='"-DCMAKE_C_FLAGS=-mmacosx-version-min=15.0"')], True),
        ("valid ASM 15.0", [NOISE, CONFIGURE.format(flags='"-DCMAKE_ASM_FLAGS=-mmacosx-version-min=15.0"')], True),
        ("below floor CXX 10.13", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=10.13"')], False),
        ("below floor CXX 11", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=11"')], False),
        ("below floor CXX 14", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=14"')], False),
        ("no cmake configure at all", [NOISE], False),
        ("configure but no deployment target flag", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS= -O2"')], False),
        ("malformed target flag", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=1"')], False),
        ("mixed 15.0 and 10.13", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0 -mmacosx-version-min=10.13"')], False),
        ("unrelated crate target alone", [NOISE, OTHER_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"')], False),
        ("unrelated crate below floor", [NOISE, OTHER_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=10.13"')], False),
        # --- cases the first awk version missed (2026-11-06 lead review) ---
        # multi-flag value: awk -F= split on the first '=' only, so the
        # second target in '-mmacosx-version-min=15.0 -O2' was never checked.
        ("flags 15.0 then -O2", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0 -O2"')], True),
        ("flags -O2 then 15.0", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-O2 -mmacosx-version-min=15.0"')], True),
        ("flags -O2 then 10.13", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-O2 -mmacosx-version-min=10.13"')], False),
        # garbage after a second '=': awk -F= only validated $2, the
        # token between the first and second '='.
        ("garbage 15.0=garbage", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0=garbage"')], False),
        ("garbage 10.13=garbage", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=10.13=garbage"')], False),
        # malformed non-numeric token: the extraction pattern
        # -mmacosx-version-min=[^"]* keeps it, the version regex must not.
        ("malformed abc", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=abc"')], False),
        # target order reversed inside the value: the old extraction
        # stopped at the first occurrence.
        ("mixed 10.13 then 15.0", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=10.13 -mmacosx-version-min=15.0"')], False),
        # duplicate valid flags: every occurrence must be well formed.
        ("duplicate 15.0 twice", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0 -mmacosx-version-min=15.0"')], True),
        # -isysroot arrives in the same -D value and must not be confused
        # with a deployment target.
        ("valid isysroot plus 15.0", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-isysroot /Library/Developer/CommandLineTools/SDKs/MacOSX.sdk -mmacosx-version-min=15.0"')], True),
        # two configure records of the same crate: the old step only
        # looked at the first matching line; every record must be clean.
        ("second configure 10.13", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"'), CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=10.13"')], False),
        ("second configure no target", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"'), CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS= -O2"')], False),
        ("two valid configures", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"'), CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=16.4"')], True),
        # unrelated crate good plus relevant crate missing: the floor
        # must come from a transcribe-cpp-sys configure, not any other.
        ("relevant missing unrelated good", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS= -O2"'), OTHER_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"')], False),
        # unrelated crate good plus relevant crate below floor.
        ("relevant low unrelated good", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=10.13"'), OTHER_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"')], False),
        # unrelated crate good plus relevant crate valid.
        ("relevant good unrelated good", [NOISE, CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"'), OTHER_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"')], True),
        # a cmake line that mentions the crate but is not a configure
        # (no source dir argument) cannot satisfy the gate.
        ("noise build mention", [NOISE, '     Running `cmake "--build" "/Users/runner/work/teletype/teletype/target/release/build/transcribe-cpp-sys-0.4.1/out/build" "--target" "install" "--config" "Release"`'], False),
        # the executable may be an absolute path instead of bare cmake,
        # as cargo -vv prints it when CMAKE points at a full path.
        ("abs path executable valid", [NOISE, CONFIGURE_ABS.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=15.0"')], True),
        ("abs path executable low", [NOISE, CONFIGURE_ABS.format(flags='"-DCMAKE_CXX_FLAGS=-mmacosx-version-min=10.13"')], False),
        # --- the cmake crate's real `[crate] running:` line shape (2026-11-06) ---
        # The CI cold build prints the configure this way, not as a `Running`
        # line. The old gate missed it and failed a correct build.
        ("build-script configure valid", [NOISE, BUILD_SCRIPT_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS= -ffunction-sections -fPIC --target=arm64-apple-macosx -mmacosx-version-min=15.0 -w"')], True),
        ("build-script configure low", [NOISE, BUILD_SCRIPT_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS= -ffunction-sections -fPIC --target=arm64-apple-macosx -mmacosx-version-min=10.13 -w"')], False),
        # the --build line of the same crate is not a configure and must not
        # satisfy the gate on its own.
        ("build-script build line only", [NOISE, BUILD_SCRIPT_BUILD], False),
        # a configure plus the build line of the same crate: the build line
        # must be ignored and the configure judged on its own.
        ("build-script configure plus build", [NOISE, BUILD_SCRIPT_CONFIGURE.format(flags='"-DCMAKE_CXX_FLAGS= -mmacosx-version-min=15.0 -w"'), BUILD_SCRIPT_BUILD], True),
    ]

    failures = 0
    print("step script extracted from %s (%d bytes)" % (CI_PATH, len(script)))
    for label, lines, expect_pass in cases:
        ok, line = run_case(label, lines, expect_pass)
        print(("ok   " if ok else "FAIL ") + line)
        if not ok:
            failures += 1
    print("%d/%d cases passed" % (len(cases) - failures, len(cases)))
    print("RESULT: %s" % ("FAIL" if failures else "PASS"))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
