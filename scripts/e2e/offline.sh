#!/usr/bin/env bash
# Offline e2e (plan §9.6): verify Teletype works with all network blocked
# via pfctl. Requires sudo to add/remove pf rules; this script NEVER
# auto-elevates — it prints the sudo command for the operator to run.
#
# Usage:
#   scripts/e2e/offline.sh --dry            # print rules + wrapped command
#   scripts/e2e/offline.sh --block          # print the exact sudo command
#   scripts/e2e/offline.sh --unblock        # print the exact sudo command
#   sudo scripts/e2e/offline.sh --run 'CMD' # actually wrap CMD (run as
#                                           # root, which CI/self-hosted
#                                           # runners provide)
#
# Rules live in offline_rules.txt (one pf rule per line; '#' comments ok).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RULES_FILE="$SCRIPT_DIR/offline_rules.txt"
PF_RULES="/tmp/teletype-offline.pf"

MODE="${1:---dry}"

print_rules() {
  echo "pf rules (from offline_rules.txt):"
  sed 's/^/  /' "$RULES_FILE"
}

case "$MODE" in
  --dry)
    print_rules
    echo
    echo "wrap: block -> run CMD -> unblock, e.g.:"
    echo "  sudo $0 --run 'scripts/e2e/delivery_matrix_macos.sh'"
    echo
    echo "manual equivalent (operator runs these, no auto-elevation):"
    echo "  sudo pfctl -E && sudo pfctl -f $PF_RULES"
    echo "  <command>"
    echo "  sudo pfctl -d && sudo pfctl -f /etc/pf.conf"
    exit 0
    ;;
  --block)
    print_rules
    echo
    echo "run:  sudo pfctl -E && sudo pfctl -f $PF_RULES"
    exit 0
    ;;
  --unblock)
    echo "run:  sudo pfctl -d && sudo pfctl -f /etc/pf.conf"
    exit 0
    ;;
  --run)
    CMD="${2:-}"
    if [ -z "$CMD" ]; then
      echo "usage: sudo $0 --run '<command>'" >&2
      exit 2
    fi
    if [ "$(id -u)" != "0" ]; then
      echo "must be run as root (sudo $0 --run '<command>') — not auto-elevating" >&2
      exit 2
    fi
    # materialize rules
    grep -v '^\s*#' "$RULES_FILE" | grep -v '^\s*$' > "$PF_RULES"
    pfctl -E
    pfctl -f "$PF_RULES"
    echo "network blocked; running: $CMD"
    set +e
    bash -c "$CMD"
    RC=$?
    set -e
    pfctl -d
    pfctl -f /etc/pf.conf
    echo "network restored; command exited $RC"
    exit "$RC"
    ;;
  *)
    echo "unknown mode: $MODE (use --dry, --block, --unblock, or --run)" >&2
    exit 2
    ;;
esac
