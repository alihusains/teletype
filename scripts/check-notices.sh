#!/usr/bin/env bash
# P3.23 — THIRD-PARTY-NOTICES.txt sync check.
#
# Fails if any DIRECT dependency (workspace Cargo.toml / per-crate
# Cargo.toml [dependencies], or ui/package.json dependencies +
# devDependencies) is missing from THIRD-PARTY-NOTICES.txt.
#
# It does NOT verify license-text correctness (that is a human review) —
# it only guards against a dep being added without updating the notices.
set -euo pipefail

# Allow testing against a doctored notices file without touching the repo:
#   NOTICES_OVERRIDE=/tmp/doctored.txt bash scripts/check-notices.sh
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NOTICES="${NOTICES_OVERRIDE:-$ROOT/THIRD-PARTY-NOTICES.txt}"

if [[ ! -f "$NOTICES" ]]; then
  echo "FAIL: $NOTICES not found" >&2
  exit 1
fi

missing=0

check_name() {
  local name="$1"
  if ! grep -q -- "$name" "$NOTICES"; then
    echo "FAIL: direct dependency '$name' not found in THIRD-PARTY-NOTICES.txt" >&2
    missing=1
  fi
}

# --- Rust direct dependencies: names declared in the workspace + per-crate
# Cargo.toml files ([dependencies], [build-dependencies], and target-specific
# dependency sections), minus the workspace's own member crates. ---
workspace_members="$(
  python3 - "$ROOT/Cargo.toml" <<'PY'
import re, sys, pathlib
text = pathlib.Path(sys.argv[1]).read_text()
m = re.search(r'members\s*=\s*\[(.*?)\]', text, re.S)
for entry in re.findall(r'"([^"]+)"', m.group(1)):
    print(pathlib.Path(entry).name)
PY
)"

declared="$(
  for toml in "$ROOT/Cargo.toml" "$ROOT"/crates/*/Cargo.toml; do
    python3 - "$toml" <<'PY'
import re, sys, pathlib
text = pathlib.Path(sys.argv[1]).read_text()
# dependency keys: top-level [dependencies] / [build-dependencies] sections
# and target-specific sections like [target.'cfg(...)'.dependencies].
in_deps = False
for line in text.splitlines():
    s = line.strip()
    if s.startswith('['):
        in_deps = bool(re.match(r'\[(target\.[^\]]+\.)?(build-)?dependencies\]$', s))
        continue
    if in_deps and s and not s.startswith('#'):
        m = re.match(r'([A-Za-z0-9_-]+)\s*=', s)
        if m:
            print(m.group(1))
PY
  done | sort -u
)"

while IFS= read -r name; do
  [[ -z "$name" ]] && continue
  # Skip the workspace's own crates (path deps).
  if printf '%s\n' "$workspace_members" | grep -qx "$name"; then
    continue
  fi
  check_name "$name"
done <<< "$declared"

# --- JavaScript direct dependencies (ui/package.json) ---
js_deps="$(
  python3 - "$ROOT/ui/package.json" <<'PY'
import json, sys
pkg = json.load(open(sys.argv[1]))
for section in ('dependencies', 'devDependencies'):
    for name in pkg.get(section, {}):
        print(name)
PY
)"

while IFS= read -r name; do
  [[ -z "$name" ]] && continue
  check_name "$name"
done <<< "$js_deps"

if [[ "$missing" -ne 0 ]]; then
  echo "FAIL: THIRD-PARTY-NOTICES.txt is out of sync with direct dependencies." >&2
  exit 1
fi

echo "OK: all direct dependencies are present in THIRD-PARTY-NOTICES.txt"
