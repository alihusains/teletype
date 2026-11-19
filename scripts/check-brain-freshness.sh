#!/usr/bin/env bash
# Report whether PROJECT_BRAIN.md still describes the checkout it sits in.
#
# Read-only diagnostic; always exits 0. Run before starting meaningful work and
# again before reporting done (see AGENTS.md "Freshness is mandatory").
#
#   bash scripts/check-brain-freshness.sh

set -uo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null) || {
  printf 'WARN: not inside a git worktree\n'
  exit 0
}
brain="$root/.agents/brain/PROJECT_BRAIN.md"

if [[ ! -f "$brain" ]]; then
  printf 'WARN: no brain at %s\n' "$brain"
  exit 0
fi

warn=0
pass() { printf 'PASS: %s\n' "$*"; }
warn_msg() { printf 'WARN: %s\n' "$*"; warn=$((warn + 1)); }

branch=$(git -C "$root" rev-parse --abbrev-ref HEAD 2>/dev/null || printf '?')
head=$(git -C "$root" rev-parse --short HEAD 2>/dev/null || printf '?')
tag=$(git -C "$root" tag --sort=-creatordate | head -1)
tag_date=$(git -C "$root" log -1 --format=%ad --date=short "$tag" 2>/dev/null || printf '?')
printf 'checkout: %s @ %s | latest tag %s (%s)\n' "$branch" "$head" "${tag:-none}" "$tag_date"

marker=$(grep -m1 'brain-freshness:' "$brain" || true)
if [[ -z "$marker" ]]; then
  warn_msg 'no brain-freshness marker near the top of PROJECT_BRAIN.md'
else
  mk_version=$(printf '%s\n' "$marker" | sed -n 's/.*version=\([^ ]*\).*/\1/p')
  mk_head=$(printf '%s\n' "$marker" | sed -n 's/.*head=\([^ ]*\).*/\1/p')
  mk_updated=$(printf '%s\n' "$marker" | sed -n 's/.*updated=\([^ ]*\).*/\1/p')
  printf 'marker:   version=%s head=%s updated=%s\n' "$mk_version" "$mk_head" "$mk_updated"
  if [[ -n "$tag" && "$mk_version" == "$tag" ]]; then
    pass 'marker version matches the latest tag'
  else
    warn_msg "marker version '$mk_version' != latest tag '${tag:-none}'"
  fi
  mk_year=${mk_updated%%-*}
  topo_year=$(grep -o '\*\*Current work topology ([0-9]\{4\}' "$brain" | grep -o '[0-9]\{4\}' | sort -r | head -1)
  if [[ -n "$topo_year" && "$topo_year" == "$mk_year" ]]; then
    pass 'current-work topology section is in the marker year'
  else
    warn_msg "current-work topology year '${topo_year:-missing}' != marker year '$mk_year'"
  fi
fi

dups=$(grep '^### Decision D' "$brain" | sort | uniq -d)
if [[ -z "$dups" ]]; then
  pass 'decision IDs are unique'
else
  warn_msg "duplicate decision IDs: $(printf '%s' "$dups" | tr '\n' ' ')"
fi
printf 'decisions: %s\n' "$(grep -c '^### Decision D' "$brain")"

untracked=$(git -C "$root" status --porcelain -- .agents/brain/ | grep '^??' || true)
if [[ -z "$untracked" ]]; then
  pass 'no untracked brain files'
else
  warn_msg "untracked brain files (commit or resign): $(printf '%s\n' "$untracked" | sed 's/^?? //' | tr '\n' ' ')"
fi

if [[ "$warn" -eq 0 ]]; then
  printf '\nbrain is fresh\n'
else
  printf '\n%s freshness warning(s) — decide whether the brain needs an update\n' "$warn"
fi
exit 0
