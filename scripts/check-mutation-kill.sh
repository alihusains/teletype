#!/usr/bin/env bash
# EW2-02 — mutation gate for the clipboard/paste boundary (injector.rs).
#
# Proves that the boundary's tests (the injector unit tests + tests/
# clipboard_safety.rs) actually kill a curated set of dangerous mutations —
# the historical data-loss bugs (BUG-011 and friends). Each mutation is
# applied to the source, the kill tests are run, and the file is restored
# byte-identical (verified with cmp against a pristine copy).
#
# KILLED   = at least one kill test exits nonzero (includes compile failure).
# SURVIVOR = all kill tests pass: a real coverage gap, reported honestly.
# PRECONDITION FAIL / rc 2 = a mutation's pattern is not present as expected
#                            (fail-closed: never guess a patch).
#
#   bash scripts/check-mutation-kill.sh --list          # show the table
#   bash scripts/check-mutation-kill.sh --only M1       # one mutation
#   bash scripts/check-mutation-kill.sh --dry-run       # print patches only
#   bash scripts/check-mutation-kill.sh                 # full gate
#
# Overrides:
#   --src PATH      target file (default: crates/teletype-core/src/injector.rs)
#   --test-args CMD single shell command; nonzero = killed (negative smokes)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${SRC_OVERRIDE:-$ROOT/crates/teletype-core/src/injector.rs}"
TEST_CMD=""
DRY_RUN=0
ONLY=()
LIST=0

usage() {
  cat <<'EOF'
check-mutation-kill.sh — mutation gate for the clipboard/paste boundary.

Usage: check-mutation-kill.sh [options]
  --list            Print the mutation table and exit 0.
  --only ID         Run only the named mutation (repeatable).
  --dry-run         Print the patches without applying or running tests.
  --src PATH        Target file (default: crates/teletype-core/src/injector.rs).
  --test-args CMD   Replace the default kill tests with one shell command.
                    Nonzero exit = killed.
  -h, --help        This help.

Exit codes: 0 = all mutations killed; 1 = survivors; 2 = usage/precondition.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --list) LIST=1; shift ;;
    --only) shift; ONLY+=("$1"); shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --src) shift; SRC="$1"; shift ;;
    --test-args) shift; TEST_CMD="$1"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "UNKNOWN ARG: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# `--src` may be relative (resolved against the caller's CWD); everything
# below works with an absolute path.
if [[ "$SRC" != /* ]]; then
  if ! SRC_DIR="$(cd "$(dirname "$SRC")" 2>/dev/null && pwd)"; then
    echo "PRECONDITION FAIL: cannot resolve --src path: $SRC" >&2
    exit 2
  fi
  SRC="$SRC_DIR/$(basename "$SRC")"
fi

# ---------------------------------------------------------------------------
# The mutation table.
#
# Tab-separated rows: ID<TAB>description<TAB>find_pattern<TAB>expected<TAB>replace
#   find_pattern   python3 re pattern matched against the target file
#   expected       number of non-overlapping matches the pattern must have
#   replace        python3 re replacement (\1 style groups); empty = deletion
#
# The table is built via printf so that literal tab characters are
# unambiguous and regex metacharacters (|, \, etc.) need no escaping.
# ---------------------------------------------------------------------------
build_table() {
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M1" \
    "gen-check-inverted: invert the generation comparison in restore_should_run" \
    'latest\.load\(Ordering::Relaxed\) <= token' \
    "1" \
    'latest.load(Ordering::Relaxed) >= token'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M2" \
    "gen-check-always: restore_should_run returns true unconditionally" \
    'pub\(crate\) fn restore_should_run\(token: u64, latest: &AtomicU64\) -> bool \{\n    latest\.load\(Ordering::Relaxed\) <= token\n\}' \
    "1" \
    'pub(crate) fn restore_should_run(token: u64, latest: &AtomicU64) -> bool {\n    true\n}'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M3" \
    "skip-restore: restore-skip branch becomes unconditional (restore never runs)" \
    'pub\(crate\) fn restore_should_run\(token: u64, latest: &AtomicU64\) -> bool \{\n    latest\.load\(Ordering::Relaxed\) <= token\n\}' \
    "1" \
    'pub(crate) fn restore_should_run(token: u64, latest: &AtomicU64) -> bool {\n    false\n}'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M4" \
    "retention-weakened: retention_extra conjunction becomes disjunction" \
    '\(keep_text && retain_text\)\.then_some\(text\)' \
    "1" \
    '(keep_text || retain_text).then_some(text)'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M5" \
    "our-item-filter-noop: without_our_own_items returns snapshot unchanged" \
    'fn without_our_own_items\(&self\) -> ClipboardSnapshot \{[\s\S]*?\n    \}' \
    "1" \
    'fn without_our_own_items(&self) -> ClipboardSnapshot {\n        self.clone()\n    }'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M6" \
    "dictation-item-not-last: restored_with drops the reorder putting dictation last" \
    '        if let Some\(text\) = extra \{\n            out\.push\(vec!\[' \
    "1" \
    '        if false {'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M7" \
    "cap-removed: the MAX_ITEMS truncation disappears" \
    '            if out\.len\(\) >= Self::MAX_ITEMS \{\n                break;\n            \}' \
    "1" \
    ""
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M8" \
    "is-empty-always-false: ClipboardSnapshot::is_empty returns false unconditionally" \
    '    pub fn is_empty\(&self\) -> bool \{\n        self\.items\.is_empty\(\)\n    \}' \
    "1" \
    '    pub fn is_empty(&self) -> bool {\n        false\n    }'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M9" \
    "clear-instead-of-restore: the restore selection hands the guard an empty snapshot (historical data-loss bug)" \
    'OwnedSaved::Snapshot\(s\) => Some\(s\.clone\(\)\),' \
    "1" \
    'OwnedSaved::Snapshot(_) => Some(ClipboardSnapshot::default()),'
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "M10" \
    "first-text-any-part: first_text returns first part text regardless of kind" \
    '\.find\(\|p\| p\.uti == Self::TEXT \|\| p\.uti == "text/plain"\)' \
    "1" \
    '.find(|p| true)'
}

MUTATION_TABLE="$(build_table)"

# ---------------------------------------------------------------------------
# CLI: --list
# ---------------------------------------------------------------------------
if [[ $LIST -eq 1 ]]; then
  echo "MUTATION TABLE (default src: ${SRC#"$ROOT"/})"
  while IFS=$'\t' read -r mid desc _; do
    [[ -z "$mid" ]] && continue
    printf '  %-4s  %s\n' "$mid" "$desc"
  done <<<"$MUTATION_TABLE"
  exit 0
fi

# ---------------------------------------------------------------------------
# Preconditions on the target file.
# ---------------------------------------------------------------------------
if [[ ! -f "$SRC" ]]; then
  echo "PRECONDITION FAIL: target file not found: $SRC" >&2
  exit 2
fi
case "$SRC" in
  "$ROOT"/*) : ;;
  *) echo "PRECONDITION FAIL: --src must be inside the repo root: $SRC" >&2; exit 2 ;;
esac

# Pristine copy for byte-identity verification.
PRISTINE_BACKUP="$(mktemp "${TMPDIR:-/tmp}/mutgate-pristine.XXXXXX")"
TMP_MUT=""
trap 'rm -f "$PRISTINE_BACKUP" "$TMP_MUT"' EXIT
cp "$SRC" "$PRISTINE_BACKUP"

# ---------------------------------------------------------------------------
# The kill tests.
# ---------------------------------------------------------------------------
run_kill_tests() {
  # Returns 0 (killed) if ANY kill test exits nonzero.
  if [[ -n "$TEST_CMD" ]]; then
    # Test command exits 0 = tests pass = SURVIVED; nonzero = KILLED.
    bash -c "$TEST_CMD" 2>/dev/null
    local rc=$?
    return $(( rc == 0 ? 1 : 0 ))
  fi
  local r1=0 r2=0
  (cd "$ROOT" && cargo test -p teletype-core --lib injector) >/dev/null 2>&1 || r1=1
  (cd "$ROOT" && cargo test -p teletype-core --test clipboard_safety) >/dev/null 2>&1 || r2=1
  [[ $r1 -eq 1 || $r2 -eq 1 ]]
}

# Which mutations to run.
SELECTED=()
if [[ ${#ONLY[@]} -eq 0 ]]; then
  SELECTED=(M1 M2 M3 M4 M5 M6 M7 M8 M9 M10)
else
  for id in "${ONLY[@]}"; do
    if ! python3 -c "import sys; ids=[l.split(chr(9))[0] for l in sys.argv[2].splitlines() if l.strip()]; sys.exit(0 if sys.argv[1] in ids else 1)" "$id" "$MUTATION_TABLE"; then
      echo "PRECONDITION FAIL: unknown mutation id: $id" >&2
      exit 2
    fi
  done
  SELECTED=("${ONLY[@]}")
fi

# ---------------------------------------------------------------------------
# Run the gate.
# ---------------------------------------------------------------------------
echo "MUTATION GATE: src=${SRC#"$ROOT"/} mutations=${#SELECTED[@]}"
if [[ -n "$TEST_CMD" ]]; then
  echo "MUTATION GATE: tests=override('$TEST_CMD')"
else
  echo "MUTATION GATE: tests='cargo test -p teletype-core --lib injector' + 'cargo test -p teletype-core --test clipboard_safety'"
fi

KILLED=()
SURVIVORS=()
PRECONDITION_FAILS=()

for id in "${SELECTED[@]}"; do
  # Extract this mutation's row (tab-separated) via python.
  row="$(python3 -c 'import sys
for line in sys.stdin.read().splitlines():
    if line.strip() and line.split(chr(9))[0] == sys.argv[1]:
        print(line)
        break' "$id" <<<"$MUTATION_TABLE")"
  if [[ -z "$row" ]]; then
    echo "PRECONDITION FAIL: $id (row missing from table)" >&2
    PRECONDITION_FAILS+=("$id")
    continue
  fi
  # Split on tabs: id, desc, find, expected, replace
  IFS=$'\t' read -r _mid desc find_pat expected replace <<<"$row"

  # Apply the mutation (or print the patch in dry-run).
  result="$(python3 - "$SRC" "$DRY_RUN" "$find_pat" "$expected" "$replace" <<'PY'
import re, sys, difflib
path, dry, find, expected, replace = sys.argv[1], sys.argv[2] == "1", sys.argv[3], int(sys.argv[4]), sys.argv[5]
src = open(path).read()
found = re.findall(find, src)
if len(found) != expected:
    print(f"FAIL\tfound {len(found)} matches, expected {expected}")
    sys.exit(0)
patched = re.sub(find, replace, src, count=1)
if dry:
    old = src.splitlines(keepends=True)
    new = patched.splitlines(keepends=True)
    for line in difflib.unified_diff(old, new, fromfile=path.split('/')[-1], tofile=path.split('/')[-1], n=1):
        sys.stdout.write(line)
    print("OK")
else:
    print(f"OK\t{patched}")
PY
)"
  # The python script prints either "FAIL\treason" or "OK\t<content>".
  status="${result%%$'\t'*}"

  if [[ "$status" == "FAIL" ]]; then
    reason="${result#*$'\t'}"
    echo "PRECONDITION FAIL: $id — $reason" >&2
    PRECONDITION_FAILS+=("$id")
    continue
  fi

  if [[ $DRY_RUN -eq 1 ]]; then
    echo "DRY-RUN: $id ($desc)"
    # Python prints the unified diff followed by "OK" on the last line.
    # Strip the trailing OK line.
    printf '%s\n' "$result" | sed '$ d'
    continue
  fi

  # The payload after "OK\t" is the full mutated file content.
  payload="${result#*$'\t'}"

  # Write the mutated file, restoring the pristine copy in a trap even on
  # error or interrupt. The write goes through a same-directory temp file:
  # on a full disk a partial write must never be mistaken for a mutation
  # result, and the original must survive it untouched.
  mutate_and_run() {
    trap 'cp "$PRISTINE_BACKUP" "$SRC"' ERR
    TMP_MUT="$(dirname "$SRC")/.mutgate-src.$$"
    if ! printf '%s' "$payload" > "$TMP_MUT"; then
      rm -f "$TMP_MUT"
      return 2
    fi
    if ! mv "$TMP_MUT" "$SRC"; then
      rm -f "$TMP_MUT"
      return 2
    fi
    TMP_MUT=""
    run_kill_tests
  }
  rc=0
  mutate_and_run || rc=$?
  if [[ $rc -eq 0 ]]; then
    echo "KILLED: $id (${desc%%:*})"
    KILLED+=("$id")
  elif [[ $rc -eq 2 ]]; then
    echo "PRECONDITION FAIL: $id — could not write the mutated source (disk full?)" >&2
    PRECONDITION_FAILS+=("$id")
    continue
  else
    echo "SURVIVOR: $id (${desc%%:*})"
    SURVIVORS+=("$id")
  fi
  trap - ERR
  # Restore and verify byte-identity.
  cp "$PRISTINE_BACKUP" "$SRC"
  if ! cmp -s "$PRISTINE_BACKUP" "$SRC"; then
    echo "PRECONDITION FAIL: $id — file not byte-identical after restore" >&2
    PRECONDITION_FAILS+=("$id")
    continue
  fi
done

if [[ ${#PRECONDITION_FAILS[@]} -gt 0 ]]; then
  echo "MUTATION GATE: PRECONDITION FAIL: ${PRECONDITION_FAILS[*]}" >&2
  exit 2
fi

if [[ $DRY_RUN -eq 1 ]]; then
  echo "MUTATION GATE: DRY-RUN OK (${#SELECTED[@]} patches printed)"
  exit 0
fi

if [[ ${#SURVIVORS[@]} -eq 0 ]]; then
  echo "MUTATION GATE: PASS (${#KILLED[@]} killed)"
  exit 0
else
  echo "MUTATION GATE: SURVIVORS: ${SURVIVORS[*]}"
  exit 1
fi
