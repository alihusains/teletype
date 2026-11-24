#!/usr/bin/env bash
# Contract for ignored core integration tests.
#
# Read-only and fail-closed: exits non-zero when the set of ignored tests in
# crates/teletype-core/tests/*.rs changes. Exactly one committed exception is
# allowed (the phase-2b ITN parity suite). Removing it requires a real gate for
# the 263 failing rows, not a silent unignore.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

ALLOWED="crates/teletype-core/tests/itn_parity.rs"
# Match attribute lines, not prose in doc comments.
PATTERN='^[[:space:]]*#\[ignore'
drift=0
allowed_seen=0

for f in crates/teletype-core/tests/*.rs; do
  if grep -Eq "$PATTERN" "$f"; then
    if [ "$f" = "$ALLOWED" ]; then
      allowed_seen=1
      echo "ALLOWED EXCEPTION: $f"
      echo "  Reason: phase-2b ITN parity suite; 263/2416 fixture rows still fail."
      echo "  It is excluded from every gate. Removing this ignore requires the"
      echo "  separate ITN plan and a real CI gate for those rows."
    else
      echo "::error::unexpected ignored test in $f — add a CI gate or remove the ignore"
      drift=1
    fi
  fi
done

if [ "$allowed_seen" -ne 1 ]; then
  echo "::error::$ALLOWED no longer carries its ignore; removing it requires a CI gate for the 263 ITN rows"
  drift=1
fi

exit "$drift"
