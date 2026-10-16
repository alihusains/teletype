#!/usr/bin/env bash
# Best-effort model catalog URL check (Phase 9).
#
# Extracts every model download URL from crates/teletype-inference/src/catalog.rs
# and crates/teletype-speech/src/catalog.rs, does a HEAD request to each, and
# (when a SHA-256 is recorded next to the URL) fetches the first bytes and
# reports a size mismatch is NOT checked — full-file SHA verification of
# multi-GB GGUFs is a nightly job, not a weekly CI one. Here we only check
# that the URL still resolves (HTTP 200/301/302/303/308).
#
# Continue-on-error by design: a flaky huggingface HEAD is not a release
# blocker. The weekly cron job in ci.yml runs this with
# `continue-on-error: true` and posts the report as an artifact.

set -uo pipefail

CATALOGS=(
  "crates/teletype-inference/src/catalog.rs"
  "crates/teletype-speech/src/catalog.rs"
)

# Pull every `url: "..."` and `backup_url: Some("...")` value out of the
# catalog sources. These are the URLs the app actually downloads.
collect_urls() {
  local file="$1"
  [[ -f "$file" ]] || return 0
  grep -oE 'url: *"[^"]+"|backup_url: *Some\("[^"]+"\)' "$file" \
    | sed -E 's/^[^"]*"//; s/"$//' \
    | sort -u
}

overall=0
for catalog in "${CATALOGS[@]}"; do
  [[ -f "$catalog" ]] || { echo "models-check: $catalog not found, skipping"; continue; }
  echo "== $catalog =="
  while IFS= read -r url; do
    [[ -z "$url" ]] && continue
    # -L follows redirects; -I HEAD; --max-time keeps a dead endpoint from
    # stalling the job. Some hosts 405 HEAD; fall back to GET --max-bytes=0.
    code=$(curl -sIL --max-time 30 -o /dev/null -w '%{http_code}' "$url" 2>/dev/null || echo "000")
    if [[ "$code" == "405" || "$code" == "501" ]]; then
      code=$(curl -s --max-time 30 --max-filesize 1 -o /dev/null -w '%{http_code}' "$url" 2>/dev/null || echo "000")
    fi
    case "$code" in
      2*|3*)
        echo "  OK   $code  $url"
        ;;
      *)
        echo "  FAIL $code  $url"
        overall=1
        ;;
    esac
  done < <(collect_urls "$catalog")
done

echo "models-check: overall=$overall"
exit "$overall"
