#!/usr/bin/env bash
# Generate a starter eval corpus using macOS `say` (zero human recording).
#
# Every case is tagged "human": false in the manifest. The reference string
# is kept byte-identical to the text passed to `say`, which is what makes the
# WER number meaningful as a plumbing check: a healthy harness should score
# near-zero WER on its own synthesised audio.
#
# Idempotent: re-running overwrites the same files and never touches anything
# outside evals/corpus/.

set -euo pipefail

CORPUS_DIR="$(cd "$(dirname "$0")" && pwd)/corpus"
AUDIO_DIR="$CORPUS_DIR/audio"
MANIFEST="$CORPUS_DIR/manifest.jsonl"

mkdir -p "$AUDIO_DIR"

# id | tag | text
# The text MUST be identical to the reference in the manifest.
CASES=(
  "plain-01|plain|The deploy went out this morning and it is all good"
  "plain-02|plain|Please call me back after the standup meeting"
  "plain-03|plain|The build is green and the tests are passing"
  "filler-01|filler|Um the deploy went out this morning and uh it is all good"
  "filler-02|filler|I mean basically the build is green and actually all the tests pass"
  "filler-03|filler|So um like the standup is at nine and er I need to be there"
  "list-01|list|Things to do today: fix the login bug, update the docs, and review the pull request"
  "list-02|list|The steps are first clone the repo, second run the setup script, and third start the dev server"
  "numbers-01|numbers|The release is scheduled for the fourteenth of March at two thirty pm"
  "numbers-02|numbers|Our error budget is ninety nine point nine five percent and we have used up forty two percent of it"
  "names-01|names|Ask Priya Sharma about the Kafka consumer in the payments service"
  "names-02|names|The S3 bucket for the ETL pipeline is in the us east one region"
  "question-01|question|Did the migration finish before the maintenance window closed"
  "multi-01|multi|The build failed on CI. The error is in the auth module. I will take a look after lunch"
  "long-01|long|The quarterly review is on the fourteenth and we need to cover the launch of the mobile app, the results of the customer research, the state of the platform migration, the hiring plan for the second half of the year, and the budget proposal for next year. I will send the agenda by Friday and everyone should come prepared with a short update on their area. Please keep your updates under five minutes so we have time for discussion. The meeting will be recorded and the notes will go to the team channel after the call. If you cannot attend, please leave a written update in the document before the meeting starts so the facilitator can cover your area"
  "awkward-01|awkward|The semicolon in the SQL query is the one that is causing the timeout"
  "awkward-02|awkward|We need to upgrade from version two to version three before the end of the quarter"
)

json_escape() {
  # Escape backslashes and double quotes for JSON.
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '%s' "$s"
}

TMP_MANIFEST="$MANIFEST.tmp.$$"
: > "$TMP_MANIFEST"

for entry in "${CASES[@]}"; do
  IFS='|' read -r id tag text <<< "$entry"
  out="$AUDIO_DIR/$id.wav"
  # Regenerate when the stored text no longer matches the manifest entry, so
  # editing a line in this script takes effect on the next run.
  if [[ ! -f "$out" ]] || ! grep -qF "\"reference\":\"$(json_escape "$text")\"" "$MANIFEST" 2>/dev/null; then
    say -o "$out" --data-format=LEI16@16000 "$text"
  fi
  printf '{"id":"%s","audio":"audio/%s.wav","reference":"%s","tags":["%s"],"human":false}\n' \
    "$id" "$id" "$(json_escape "$text")" "$tag" >> "$TMP_MANIFEST"
done

mv "$TMP_MANIFEST" "$MANIFEST"
echo "generated ${#CASES[@]} cases in $CORPUS_DIR"
