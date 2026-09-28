#!/usr/bin/env bash
# Test the updater-manifest assembly in .github/workflows/release.yml.
#
# The manifest is a Python heredoc embedded in a YAML `run:` block, so there is
# no way to import it and no unit test can reach it. It is also the component
# with the worst failure mode in this repo: a manifest that points a platform
# key at the wrong binary produces an app that installs, passes its own
# signature check, and then refuses to launch. That is exactly what finding 16
# was, and it is not visible in any build log.
#
# This script extracts the step from the workflow and runs it against synthetic
# `dist/` trees, one per scenario. It is not a check of the real release: it is
# a check that the manifest's *decisions* are still the decisions we want.
#
#   bash scripts/check-updater-manifest.sh
#
# Scenarios that must publish, and the platforms they must produce:
#   all three artifacts          -> darwin-aarch64 darwin-x86_64 windows-x86_64
#   intel job failed             -> darwin-aarch64 windows-x86_64
#   intel unsigned               -> darwin-aarch64 windows-x86_64
#
# Scenarios that must REFUSE, because publishing either ships a broken update
# or silently drops an architecture:
#   intel payload is really arm64
#   arm64 payload is really x86_64
#   arm64 artifact missing
#   payload has no signature
#   two payloads in one artifact
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

if ! command -v python3 >/dev/null; then
  echo "SKIP: python3 not available" >&2
  exit 0
fi

# Pull the assembly step out of the workflow, substituting the two GitHub
# expressions it interpolates so it can run locally.
python3 - "$ROOT/.github/workflows/release.yml" manifest.sh <<'PY'
import re, sys, yaml

doc = yaml.safe_load(open(sys.argv[1]))
steps = doc["jobs"]["publish"]["steps"]
step = next(s for s in steps if s.get("name") == "Assemble the updater manifest")
body = step["run"]
body = re.sub(r"\$\{GITHUB_REF_NAME#v\}", "1.2.3", body)
body = re.sub(r"\$\{\{ github\.repository \}\}", "owner/repo", body)
body = re.sub(r"\$\{GITHUB_REF_NAME\}", "v1.2.3", body)
open("manifest.sh", "w").write(body)
print("extracted the manifest step from release.yml")
PY

fails=0
pass() { echo "ok   [$1]${2:+ -> $2}"; }
fail() { echo "FAIL [$1]${2:+ -> $2}"; fails=$((fails + 1)); }

platforms() {
  python3 -c \
    "import json;print(' '.join(sorted(json.load(open('latest.json'))['platforms'])))" \
    2>/dev/null || echo "<no manifest>"
}

# name | expected exit | expected platforms | setup
check() {
  local name="$1" want_rc="$2" want_platforms="$3" setup="$4"
  rm -rf dist latest.json out.txt
  eval "$setup"

  local rc=0
  bash manifest.sh >out.txt 2>&1 || rc=$?
  if [[ "$rc" != "$want_rc" ]]; then
    fail "$name" "exit $rc, wanted $want_rc"
    sed 's/^/       /' out.txt
    return
  fi
  if [[ -n "$want_platforms" ]]; then
    local got
    got="$(platforms)"
    if [[ "$got" != "$want_platforms" ]]; then
      fail "$name" "platforms [$got], wanted [$want_platforms]"
      sed 's/^/       /' out.txt
      return
    fi
  fi
  pass "$name" "$(platforms)"
  grep -E '^::warning' out.txt | sed 's/^/       /' || true
}

# Fake artifacts. The macOS payload name is deliberately identical in both
# artifacts, because that collision is the bug being guarded.
mac_arm64() {
  mkdir -p dist/macos-release/bundle/macos
  echo aarch64 >dist/macos-release/bundle/macos/ARCH
  head -c 2000 /dev/urandom >dist/macos-release/bundle/macos/Teletype.app.tar.gz
  echo sig >dist/macos-release/bundle/macos/Teletype.app.tar.gz.sig
}
mac_intel() {
  mkdir -p dist/macos-release-intel/bundle/macos
  echo x86_64 >dist/macos-release-intel/bundle/macos/ARCH
  head -c 3000 /dev/urandom >dist/macos-release-intel/bundle/macos/Teletype.app.tar.gz
  echo sig >dist/macos-release-intel/bundle/macos/Teletype.app.tar.gz.sig
}
mac_intel_unsigned() {
  mkdir -p dist/macos-release-intel/bundle/macos
  echo x86_64 >dist/macos-release-intel/bundle/macos/ARCH
  head -c 3000 /dev/urandom >dist/macos-release-intel/bundle/macos/Teletype.app.tar.gz
}
win() {
  mkdir -p dist/windows-release/bundle/nsis
  head -c 4000 /dev/urandom >dist/windows-release/bundle/nsis/Teletype_1.2.3_x64-setup.nsis.zip
  echo sig >dist/windows-release/bundle/nsis/Teletype_1.2.3_x64-setup.nsis.zip.sig
}

check "all three artifacts" 0 \
  "darwin-aarch64 darwin-x86_64 windows-x86_64" \
  "mac_arm64; mac_intel; win"

check "intel job failed (continue-on-error)" 0 \
  "darwin-aarch64 windows-x86_64" \
  "mac_arm64; win"

check "intel present but unsigned" 0 \
  "darwin-aarch64 windows-x86_64" \
  "mac_arm64; mac_intel_unsigned; win"

check "REFUSES: intel payload is really arm64" 1 "" \
  "mac_arm64; mkdir -p dist/macos-release-intel/bundle/macos; echo arm64 >dist/macos-release-intel/bundle/macos/ARCH; head -c 2000 /dev/urandom >dist/macos-release-intel/bundle/macos/Teletype.app.tar.gz; echo sig >dist/macos-release-intel/bundle/macos/Teletype.app.tar.gz.sig; win"

check "REFUSES: arm64 payload is really x86_64" 1 "" \
  "mkdir -p dist/macos-release/bundle/macos; echo x86_64 >dist/macos-release/bundle/macos/ARCH; head -c 2000 /dev/urandom >dist/macos-release/bundle/macos/Teletype.app.tar.gz; echo sig >dist/macos-release/bundle/macos/Teletype.app.tar.gz.sig; win"

check "REFUSES: arm64 artifact missing" 1 "" "win"

check "REFUSES: payload has no signature" 1 "" \
  "mkdir -p dist/macos-release/bundle/macos; echo aarch64 >dist/macos-release/bundle/macos/ARCH; head -c 2000 /dev/urandom >dist/macos-release/bundle/macos/Teletype.app.tar.gz; win"

check "REFUSES: two payloads in one artifact" 1 "" \
  "mac_arm64; cp dist/macos-release/bundle/macos/Teletype.app.tar.gz dist/macos-release/bundle/macos/Other.app.tar.gz; cp dist/macos-release/bundle/macos/Teletype.app.tar.gz.sig dist/macos-release/bundle/macos/Other.app.tar.gz.sig; win"

echo "---"
if [[ $fails -eq 0 ]]; then
  echo "ALL MANIFEST SCENARIOS PASS"
else
  echo "$fails SCENARIO(S) FAILED"
fi
exit $fails
