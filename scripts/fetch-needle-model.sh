#!/usr/bin/env bash
# Fetch Needle 3 (the 35 MB text model used by the voice-command work).
#
# The weights are NOT committed: a 35 MB blob in git would be paid by every
# clone forever. The tests that need it (`needle_e2e`, `benchmark`,
# `voice_command_e2e`) skip when it is absent, so this is opt-in.
#
#   ./scripts/fetch-needle-model.sh            # download to models/needle3.cact
#   ./scripts/fetch-needle-model.sh --check    # verify an existing file only
#
# The SHA-256 below is pinned to the file that used to be committed at
# models/needle3.cact, so a swapped or truncated upstream file is refused.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/models/needle3.cact"
URL="https://huggingface.co/Cactus-Compute/needle3/resolve/main/needle3.cact"
SHA256="c9d915eca282ed42d1a09b143b592adb4cc6744ffe2d294adf5cfc5548170c38"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

check_existing() {
  local actual
  actual="$(sha256_of "$DEST")"
  if [[ "$actual" == "$SHA256" ]]; then
    echo "OK: $DEST matches the pinned SHA-256"
    return 0
  fi
  echo "MISMATCH: $DEST has SHA-256 $actual, expected $SHA256" >&2
  echo "Delete it and re-run without --check to download a fresh copy." >&2
  return 1
}

if [[ "${1:-}" == "--check" ]]; then
  if [[ ! -f "$DEST" ]]; then
    echo "SKIP: $DEST does not exist (run without --check to download)" >&2
    exit 1
  fi
  check_existing
  exit $?
fi

mkdir -p "$ROOT/models"

if [[ -f "$DEST" ]]; then
  if check_existing >/dev/null 2>&1; then
    echo "Already present and verified: $DEST"
    exit 0
  fi
  echo "Existing file does not match the pin; re-downloading." >&2
fi

TMP="$DEST.part"
trap 'rm -f "$TMP"' EXIT

echo "Downloading $URL"
curl -fL --retry 3 --progress-bar -o "$TMP" "$URL"

actual="$(sha256_of "$TMP")"
if [[ "$actual" != "$SHA256" ]]; then
  echo "ERROR: downloaded file has SHA-256 $actual, expected $SHA256" >&2
  echo "Refusing to install it. The upstream file may have changed." >&2
  exit 1
fi

mv "$TMP" "$DEST"
echo "Installed $DEST ($(wc -c < "$DEST" | tr -d ' ') bytes, SHA-256 verified)"
