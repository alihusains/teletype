#!/usr/bin/env bash
# Fail if a built binary has been staged for commit.
#
# `crates/teletype-desktop/binaries/llama-server` is a 15 MB per-architecture
# executable that `scripts/build-llama-server.sh` produces. It is a build
# artifact, not source, and `.gitignore` covers it. This exists because a
# tracking exception for it was once present in `.gitignore`, and a 15 MB blob
# is expensive to undo after the fact.
#
# Checks the *index*, not the working tree: a developer who has built locally
# but staged nothing has not done anything wrong, and this must not fire on
# them. Only a staged artifact is a problem.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

bad=0

# Anything under binaries/ or target/ that is in the index.
staged_paths=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true)
if [[ -n "$staged_paths" ]]; then
  while read -r f; do
    [[ -n "$f" ]] || continue
    case "$f" in
      crates/teletype-desktop/binaries/*|target/*|*/node_modules/*)
        echo "FAIL: build artifact staged for commit: $f" >&2
        bad=1
        ;;
    esac
  done <<< "$staged_paths"
fi

# Belt and braces: even if a path is somehow tracked, refuse a large one.
for f in crates/teletype-desktop/binaries/llama-server \
         crates/teletype-desktop/binaries/llama-server.exe; do
  if git ls-files --error-unmatch "$f" >/dev/null 2>&1; then
    size=$(wc -c < "$f" | tr -d ' ')
    if [[ "$size" -gt 1000000 ]]; then
      echo "FAIL: $f is tracked and is $size bytes. It is a build artifact." >&2
      bad=1
    fi
  fi
done

if [[ $bad -eq 0 ]]; then
  echo "OK: no build artifacts staged"
fi
exit $bad
