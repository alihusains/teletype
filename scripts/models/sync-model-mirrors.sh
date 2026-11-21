#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT/models/manifest.json"
EXPECTED_TAG="models-v1"
EXPECTED_REPO="alihusains/teletype-models"
MAX_ASSET_BYTES=$((2 * 1024 * 1024 * 1024))
VALIDATE_ONLY=0

usage() {
  printf 'Usage: %s [--validate-only]\n' "$0"
  printf '  --validate-only  Validate %s locally; no gh, token, or network calls.\n' "$MANIFEST"
  printf 'Full sync requires: gh CLI, MODELS_REPO_TOKEN, python3, curl, shasum|sha256sum.\n'
}

case "${1:-}" in
  "") ;;
  --validate-only) VALIDATE_ONLY=1 ;;
  -h | --help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then
  usage >&2
  exit 2
fi

command -v python3 >/dev/null 2>&1 || {
  printf 'python3 is required to validate the manifest\n' >&2
  exit 1
}
[ -f "$MANIFEST" ] || {
  printf 'missing manifest: %s\n' "$MANIFEST" >&2
  exit 1
}

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
plan="$tmpdir/plan.tsv"

python3 - "$MANIFEST" "$plan" "$EXPECTED_TAG" "$EXPECTED_REPO" "$MAX_ASSET_BYTES" <<'PY'
import json
import re
import sys

manifest_path, plan_path, expected_tag, expected_repo, max_bytes = sys.argv[1:]
max_bytes = int(max_bytes)
failures = []


def check(condition, message):
    if not condition:
        failures.append(message)


def is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


try:
    with open(manifest_path, "rb") as handle:
        manifest = json.load(handle)
except Exception as exc:
    print(f"invalid JSON in {manifest_path}: {exc}", file=sys.stderr)
    raise SystemExit(1)

check(isinstance(manifest, dict), "manifest must be a JSON object")
if not isinstance(manifest, dict):
    print("manifest validation failed", file=sys.stderr)
    raise SystemExit(1)

check(manifest.get("schema_version") == 1 and is_int(manifest.get("schema_version")),
      "schema_version must be integer 1")
check(manifest.get("release_tag") == expected_tag,
      f"release_tag must be {expected_tag}")
check(isinstance(manifest.get("release_tag"), str)
      and not manifest["release_tag"].startswith("v"),
      "release_tag must never start with v")
check(manifest.get("target_repo") == expected_repo,
      f"target_repo must be {expected_repo}")
check(isinstance(manifest.get("notes"), str) and bool(manifest["notes"].strip()),
      "notes must be a non-empty string")

models = manifest.get("models")
check(isinstance(models, list) and len(models) > 0, "models must be a non-empty array")
if not isinstance(models, list):
    models = []

ids, assets, digests = set(), set(), set()
rows = []
for index, model in enumerate(models):
    prefix = f"models[{index}]"
    check(isinstance(model, dict), f"{prefix} must be an object")
    if not isinstance(model, dict):
        continue

    model_id = model.get("id")
    check(isinstance(model_id, str) and re.fullmatch(r"[a-z0-9][a-z0-9._-]*", model_id or "") is not None,
          f"{prefix}.id must be a lowercase slug")
    if isinstance(model_id, str):
        check(model_id not in ids, f"{prefix}.id duplicates {model_id}")
        ids.add(model_id)

    asset = model.get("asset_name")
    check(isinstance(asset, str) and asset not in ("", ".", "..")
          and "/" not in asset and "\\" not in asset and "\t" not in asset
          and asset.lower() != "latest.json" and asset.endswith(".gguf"),
          f"{prefix}.asset_name must be a unique .gguf basename, never latest.json")
    if isinstance(asset, str):
        check(asset not in assets, f"{prefix}.asset_name duplicates {asset}")
        assets.add(asset)

    url = model.get("source_url")
    check(isinstance(url, str) and url.startswith("https://huggingface.co/")
          and re.search(r"/resolve/[0-9a-f]{40}/", url or "") is not None
          and "/resolve/main" not in (url or "")
          and "/resolve/master" not in (url or "")
          and "/resolve/HEAD" not in (url or "")
          and "\t" not in (url or ""),
          f"{prefix}.source_url must be an https HF URL pinned to a 40-hex revision")
    check(isinstance(asset, str) and isinstance(url, str)
          and (asset is None or url is None or url.endswith("/" + asset)),
          f"{prefix}.source_url must end in asset_name")

    sha = model.get("sha256")
    check(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{64}", sha or "") is not None,
          f"{prefix}.sha256 must be 64 lowercase hex chars")
    if isinstance(sha, str):
        check(sha not in digests, f"{prefix}.sha256 duplicates {sha}")
        digests.add(sha)

    size = model.get("size_bytes")
    check(is_int(size) and size is not None and 0 < size < max_bytes,
          f"{prefix}.size_bytes must be a positive integer under 2 GiB")

    check(isinstance(model.get("size_bytes_source"), str)
          and bool(model["size_bytes_source"].strip()),
          f"{prefix}.size_bytes_source must document how the size was verified")
    check(isinstance(model.get("license"), str) and bool(model["license"].strip()),
          f"{prefix}.license must name the license")
    check(isinstance(model.get("license_url"), str)
          and model["license_url"].startswith("https://"),
          f"{prefix}.license_url must be an https URL")
    check(isinstance(model.get("attribution"), str)
          and bool(model["attribution"].strip()),
          f"{prefix}.attribution must name the upstream author and license")
    check(model.get("mirror_allowed") is True,
          f"{prefix}.mirror_allowed must be true")

    if all(isinstance(model.get(key), str) for key in ("id", "asset_name", "source_url", "sha256")):
        size_value = str(size) if is_int(size) and size is not None else "-"
        rows.append("\t".join((model["id"], model["asset_name"],
                               model["source_url"], model["sha256"], size_value)))

if failures:
    for failure in failures:
        print(f"error: {failure}", file=sys.stderr)
    print(f"manifest validation failed with {len(failures)} error(s)", file=sys.stderr)
    raise SystemExit(1)

with open(plan_path, "w", encoding="utf-8") as handle:
    handle.write("\n".join(rows) + "\n")

total = sum(int(row.split("\t")[4]) for row in rows if row.split("\t")[4] != "-")
print(f"manifest OK: {expected_repo}@{expected_tag}, {len(rows)} models, {total} bytes")
for row in rows:
    model_id, asset, _, sha, size = row.split("\t")
    print(f"  {model_id}: {asset} ({size} bytes, sha256 {sha[:12]}...)")
PY

if [ "$VALIDATE_ONLY" -eq 1 ]; then
  printf 'validate-only: no release or asset changes attempted\n'
  exit 0
fi

for tool in gh curl; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf '%s is required for a full sync\n' "$tool" >&2
    exit 1
  }
done
if command -v sha256sum >/dev/null 2>&1; then
  sha256_file() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256_file() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  printf 'sha256sum or shasum is required\n' >&2
  exit 1
fi
if command -v stat >/dev/null 2>&1 && stat -c%s "$MANIFEST" >/dev/null 2>&1; then
  file_size() { stat -c%s "$1"; }
else
  file_size() { stat -f%z "$1"; }
fi

if [ -z "${MODELS_REPO_TOKEN:-}" ]; then
  printf 'MODELS_REPO_TOKEN is required; set it to a token with contents:write on %s\n' "$EXPECTED_REPO" >&2
  exit 1
fi
export GH_TOKEN="$MODELS_REPO_TOKEN"

tag="$EXPECTED_TAG"
repo="$EXPECTED_REPO"

case "$tag" in
  v*) printf 'refusing to publish a v* tag: %s\n' "$tag" >&2; exit 1 ;;
esac
[ "$tag" = "$EXPECTED_TAG" ] || {
  printf 'refusing unexpected tag: %s\n' "$tag" >&2
  exit 1
}
[ "$repo" = "$EXPECTED_REPO" ] || {
  printf 'refusing unexpected repo: %s\n' "$repo" >&2
  exit 1
}

if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  release_exists=1
else
  err="$tmpdir/release-view.err"
  if gh api "repos/$repo/releases/tags/$tag" >/dev/null 2>"$err"; then
    release_exists=1
  elif grep -Eqi 'not found|404' "$err"; then
    release_exists=0
  else
    cat "$err" >&2
    printf 'could not determine whether release %s exists\n' "$tag" >&2
    exit 1
  fi
fi

if [ "$release_exists" -eq 0 ]; then
  notes_file="$tmpdir/release-notes.md"
  {
    printf '# %s\n\n' "$tag"
    python3 - "$MANIFEST" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    print(json.load(handle)["notes"])
PY
    printf '\nAssets are immutable. Additions only; no overwrite, no latest.json, never marked latest.\n\n'
    printf '| id | asset | sha256 | bytes |\n| --- | --- | --- | --- |\n'
    while IFS=$'\t' read -r id asset _url sha size; do
      printf '| %s | %s | %s | %s |\n' "$id" "$asset" "$sha" "$size"
    done < "$plan"
  } > "$notes_file"
  gh release create "$tag" --repo "$repo" --prerelease \
    --title "$tag - pinned permissive model artifacts" \
    --notes-file "$notes_file"
  printf 'created prerelease %s\n' "$tag"
fi

is_prerelease="$(gh release view "$tag" --repo "$repo" \
  --json isPrerelease --jq '.isPrerelease')"
if [ "$is_prerelease" != "true" ]; then
  printf 'release %s must be a prerelease, never latest\n' "$tag" >&2
  exit 1
fi
latest_err="$tmpdir/latest.err"
if gh api "repos/$repo/releases/latest" >"$tmpdir/latest.json" 2>"$latest_err"; then
  latest_tag="$(python3 - "$tmpdir/latest.json" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    print(json.load(handle).get("tag_name", ""))
PY
)"
  if [ "$latest_tag" = "$tag" ]; then
    printf 'refusing to publish %s as the latest release\n' "$tag" >&2
    exit 1
  fi
elif ! grep -Eqi 'not found|404' "$latest_err"; then
  cat "$latest_err" >&2
  printf 'could not verify that %s is not the latest release\n' "$tag" >&2
  exit 1
fi

assets_json="$tmpdir/assets.json"
gh release view "$tag" --repo "$repo" --json assets > "$assets_json"
assets_file="$tmpdir/assets.txt"
python3 - "$assets_json" "$assets_file" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
    assets = data["assets"] if isinstance(data, dict) else data
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    for asset in assets:
        print(asset["name"], file=handle)
PY

asset_digest() {
  python3 - "$assets_json" "$1" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
    assets = data["assets"] if isinstance(data, dict) else data
for asset in assets:
    if asset.get("name") == sys.argv[2]:
        print(asset.get("digest") or "")
        break
PY
}

verify_existing() {
  local asset="$1" sha="$2" digest
  digest="$(asset_digest "$asset")"
  case "$digest" in
    "sha256:$sha") return 0 ;;
    "") printf 'warning: %s has no API digest; trusting release assets as immutable\n' "$asset" >&2; return 0 ;;
    *) printf 'error: existing asset %s digest %s does not match manifest %s\n' "$asset" "$digest" "$sha" >&2; return 1 ;;
  esac
}

verify_existing_metadata() {
  local asset="$1" sha="$2" dir="$tmpdir/existing-$(printf '%s' "$1" | tr -cd '[:alnum:]')"
  local actual
  mkdir -p "$dir"
  gh release download "$tag" --repo "$repo" --pattern "$asset" --dir "$dir"
  actual="$(sha256_file "$dir/$asset")"
  if [ "$actual" != "$sha" ]; then
    printf 'error: existing %s sha256 %s differs from generated %s\n' "$asset" "$actual" "$sha" >&2
    return 1
  fi
}

upload_new() {
  local file="$1" asset
  asset="$(basename "$file")"
  if grep -Fxq "$asset" "$assets_file"; then
    printf 'skip: %s already present\n' "$asset"
    return 0
  fi
  gh release upload "$tag" "$file" --repo "$repo"
  printf '%s\n' "$asset" >> "$assets_file"
  printf 'uploaded: %s\n' "$asset"
}

mkdir -p "$tmpdir/assets"
while IFS=$'\t' read -r id asset url sha size; do
  if grep -Fxq "$asset" "$assets_file"; then
    printf 'skip model asset: %s\n' "$asset"
    verify_existing "$asset" "$sha"
    continue
  fi
  dest="$tmpdir/assets/$asset"
  printf 'download: %s\n' "$url"
  curl -fL --retry 3 --output "$dest" "$url"
  if [ "$size" != "-" ]; then
    actual_size="$(file_size "$dest")"
    if [ "$actual_size" != "$size" ]; then
      printf 'error: %s size %s does not match manifest %s\n' "$asset" "$actual_size" "$size" >&2
      exit 1
    fi
  fi
  actual_sha="$(sha256_file "$dest")"
  if [ "$actual_sha" != "$sha" ]; then
    printf 'error: %s sha256 %s does not match manifest %s\n' "$asset" "$actual_sha" "$sha" >&2
    exit 1
  fi
  printf 'verified: %s (%s bytes, sha256 %s)\n' "$asset" "$size" "$sha"
  upload_new "$dest"
done < "$plan"

sums="$tmpdir/SHA256SUMS"
: > "$sums"
while IFS=$'\t' read -r id asset url sha size; do
  printf '%s  %s\n' "$sha" "$asset" >> "$sums"
done < "$plan"
snapshot="$tmpdir/manifest.snapshot.json"
cp "$MANIFEST" "$snapshot"

for metadata in "$sums" "$snapshot"; do
  name="$(basename "$metadata")"
  meta_sha="$(sha256_file "$metadata")"
  if grep -Fxq "$name" "$assets_file"; then
    verify_existing_metadata "$name" "$meta_sha" || {
      printf 'error: %s on %s differs from this manifest; changed bytes require a new tag\n' "$name" "$tag" >&2
      exit 1
    }
  fi
  upload_new "$metadata"
done

printf 'sync complete: %s@%s\n' "$repo" "$tag"
