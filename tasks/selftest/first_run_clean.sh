#!/usr/bin/env bash
# First run on a clean profile (release plan step 5, the part that is
# measurable without a window).
#
# What this proves: the app starts with no models and no dictionary, runs on
# shipped defaults with no settings file present, and refuses a take whose
# speech model's bytes are missing instead of hanging, showing nothing or
# writing a half-empty result. That refusal is also the "accidental tap does
# nothing" gate: a press before the user has installed a model must be inert.
#
# The row under test is chosen deliberately. The shipped default on this
# machine is parakeet-ultra-coreml, and `speech_model_installed` treats a
# working Core ML bridge as "installed" (commands/models.rs:1215-1217), so its
# first take is never gated: the FluidAudio bridge downloads its weights from
# Hugging Face on first load while the pill reads transcribing. That is the
# design, so this leg selects a ggml row whose files really are absent and
# asserts the gate. The Core ML download-on-first-take behaviour is reported
# separately as an observation, not as a failure.
#
# What this does NOT prove, and says so in its own output:
#   - the onboarding window, which needs the GUI,
#   - the TCC permission prompts, which belong to a real user account and a
#     signed app rather than a debug binary on this machine,
#   - the first model download, which the e2e socket cannot trigger (the
#     download lives behind the Models rail, not behind a controller event).
# Those three are listed as MANUAL at the end rather than reported as passing.
#
# Headless: TELETYPE_FORCE_DELIVERY=hold keeps the delivery cascade from
# touching the clipboard or typing into another app.
set -u

INTEG=/Users/a.sorathiya/Documents/Ali/teletype-wt/integration
BINARY="$INTEG/target/debug/teletype"
OUT="${1:-/tmp/firstrun}"
MODEL="${MODEL:-parakeet-tdt-v3}"
WAV="${WAV:-/tmp/soak500b/audio/speech_2s.wav}"

rm -rf "$OUT"
mkdir -p "$OUT/config" "$OUT/data"
SOCK="$OUT.sock"
LOG="$OUT/app.log"

if [ ! -x "$BINARY" ]; then
  echo "first_run: no binary at $BINARY"
  exit 1
fi

echo "first_run: clean profile at $OUT (config has $(ls -A "$OUT/config" | wc -l | tr -d ' ') files)"

launch() {
  env TELETYPE_E2E_SOCKET="$SOCK" TELETYPE_E2E_CONFIG_DIR="$OUT/config" \
    TELETYPE_E2E_DATA_DIR="$OUT/data" TELETYPE_FORCE_DELIVERY=hold \
    "$BINARY" --e2e >> "$LOG" 2>&1 &
  APP=$!
  for _ in $(seq 1 120); do [ -S "$SOCK" ] && break; sleep 0.5; done
  if [ ! -S "$SOCK" ]; then
    echo "first_run: the app did not open its e2e socket; last log lines:"
    tail -20 "$LOG"
    kill -9 "$APP" 2>/dev/null
    exit 1
  fi
  sleep 5
}

phase() {
  python3 - "$SOCK" "$LOG" "$OUT" "$WAV" "$1" "$MODEL" <<'PY'
import json, os, socket, sys, time

sock, log, out, wav, phase, model = sys.argv[1:7]


def send(obj, timeout=30):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(sock)
    s.sendall((json.dumps(obj) + "\n").encode())
    buf = b""
    while b"\n" not in buf:
        try:
            chunk = s.recv(65536)
        except socket.timeout:
            break
        if not chunk:
            break
        buf += chunk
    s.close()
    return json.loads(buf.decode().split("\n")[0]) if buf else {}


def log_text(off=0):
    try:
        size = os.path.getsize(log)
    except OSError:
        return ""
    with open(log, "r", errors="replace") as fh:
        fh.seek(min(off, size))
        return fh.read()


results = []


def check(label, ok, detail=""):
    results.append((label, bool(ok), detail))
    print("%-4s %s%s" % ("PASS" if ok else "FAIL", label, (": " + detail) if detail else ""),
          flush=True)


print("first_run phase: %s" % phase, flush=True)
st = (send({"cmd": "status"}).get("data") or {})
pill = st.get("pill") or {}
check("socket status answers", st.get("running") is True, "running=%s" % st.get("running"))
settings_path = os.path.join(out, "config", "settings.json")

if phase == "defaults":
    # The app must run with no settings file at all: the shipped defaults
    # come from the Settings struct, not from a file the first launch writes.
    check("launches with no settings file present",
          not os.path.exists(settings_path), settings_path)
    check("the default speech row resolves", bool(st.get("speechModel")),
          "model=%s engine=%s" % (st.get("speechModel"), st.get("speechEngine")))
    check("storage initialised on a clean profile",
          os.path.exists(os.path.join(out, "config", "teletype.db")),
          os.path.join(out, "config", "teletype.db"))
    check("the pill is at rest for a new user",
          str(pill.get("phase")) in ("idle", "Idle"), "phase=%s" % pill.get("phase"))
    # The Core ML default is never gated by design (the bridge alone counts
    # as installed), so say what its first take would do instead of asserting
    # a gate here.
    print("OBSERVED: default row %s reports modelOnDisk=%s. A Core ML row reads "
          "as installed from the bridge alone, so its first take downloads "
          "weights from Hugging Face while the pill reads transcribing."
          % (st.get("speechModel"), st.get("modelOnDisk")), flush=True)
else:
    settings = json.load(open(settings_path))
    check("the seeded row is the selected row",
          settings.get("selectedSpeechModel") == model,
          "selectedSpeechModel=%s" % settings.get("selectedSpeechModel"))
    check("the pill is at rest before the press",
          str(pill.get("phase")) in ("idle", "Idle"), "phase=%s" % pill.get("phase"))
    check("the seeded row's bytes are absent", st.get("modelOnDisk") is False,
          "modelOnDisk=%s model=%s" % (st.get("modelOnDisk"), st.get("speechModel")))
    off = os.path.getsize(log) if os.path.exists(log) else 0
    if os.path.exists(wav):
        send({"cmd": "start", "input": wav, "command": False})
        time.sleep(8)
        window = log_text(off)
        gated = "press gated" in window.lower()
        check("a take with no model installed is gated, not hung", gated,
              "looked for the gate marker in this take's window")
        phase_after = str(((send({"cmd": "status"}).get("data") or {}).get("pill") or {}).get("phase"))
        check("the gated press leaves the pill at rest",
              phase_after in ("idle", "Idle"), "phase=%s" % phase_after)
        files = []
        for root, _dirs, names in os.walk(os.path.join(out, "data")):
            files += [n for n in names if n.endswith((".wav", ".tmp"))]
        check("the gated press wrote no recording", not files,
              "%d audio file(s)" % len(files))
    else:
        check("speech clip available for the gated press", False, "missing %s" % wav)

print()
print("MANUAL, not covered by this run:")
print("  - the onboarding window (needs the GUI, not the e2e socket)")
print("  - TCC permission prompts (need a signed app on a real user account)")
print("  - the first model download (behind the Models rail; the e2e protocol")
print("    has no download command)")
print()
failed = [r for r in results if not r[1]]
print("first_run (%s): %d checks, %d failed" % (phase, len(results), len(failed)))
sys.exit(1 if failed else 0)
PY
}

launch
phase "defaults"
P1=$?
kill -9 "$APP" 2>/dev/null
rm -f "$SOCK"
sleep 1

# Phase 2: a row whose bytes really are absent. The app reads settings.json
# at startup, so the file is seeded before the second launch.
cat > "$OUT/config/settings.json" <<EOF
{"selectedSpeechModel": "$MODEL"}
EOF
launch
phase "gate"
P2=$?
kill -9 "$APP" 2>/dev/null
echo "first_run: log at $LOG"
if [ "$P1" -ne 0 ] || [ "$P2" -ne 0 ]; then
  echo "first_run: FAILED (defaults=$P1 gate=$P2)"
  exit 1
fi
echo "first_run: both phases passed"
exit 0
