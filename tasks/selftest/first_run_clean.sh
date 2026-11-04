#!/usr/bin/env bash
# First run on a clean profile (release plan step 5, the part that is
# measurable without a window).
#
# What this proves: the app starts with no config and no models, creates its
# own settings, and refuses a take whose speech model is not installed instead
# of hanging, showing nothing or writing a half-empty result. That refusal is
# also the "accidental tap does nothing" gate: a press before the user has
# installed a model must be inert.
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

env TELETYPE_E2E_SOCKET="$SOCK" TELETYPE_E2E_CONFIG_DIR="$OUT/config" \
  TELETYPE_E2E_DATA_DIR="$OUT/data" TELETYPE_FORCE_DELIVERY=hold \
  "$BINARY" --e2e > "$LOG" 2>&1 &
APP=$!

for _ in $(seq 1 120); do [ -S "$SOCK" ] && break; sleep 0.5; done
if [ ! -S "$SOCK" ]; then
  echo "first_run: the app did not open its e2e socket; last log lines:"
  tail -20 "$LOG"
  kill -9 "$APP" 2>/dev/null
  exit 1
fi
sleep 5

python3 - "$SOCK" "$LOG" "$OUT" "$WAV" "$APP" <<'PY'
import json, os, socket, sys, time

sock, log, out, wav, app = sys.argv[1:6]


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


st = (send({"cmd": "status"}).get("data") or {})
pill = st.get("pill") or {}
check("socket status answers", st.get("running") is True, "running=%s" % st.get("running"))
check("settings file created on first launch",
      os.path.exists(os.path.join(out, "config", "settings.json")),
      os.path.join(out, "config", "settings.json"))

settings_path = os.path.join(out, "config", "settings.json")
if os.path.exists(settings_path):
    settings = json.load(open(settings_path))
    check("first-run settings are the shipped defaults",
          settings.get("selectedSpeechModel") in (None, "parakeet-tdt-v3"),
          "selectedSpeechModel=%s" % settings.get("selectedSpeechModel"))

check("the pill is at rest for a new user",
      str(pill.get("phase")) in ("idle", "Idle"), "phase=%s" % pill.get("phase"))
model_on_disk = st.get("modelOnDisk")
check("a clean profile has no speech model installed", model_on_disk is False,
      "modelOnDisk=%s model=%s" % (model_on_disk, st.get("speechModel")))

off = os.path.getsize(log) if os.path.exists(log) else 0
if os.path.exists(wav):
    send({"cmd": "start", "input": wav, "command": False})
    time.sleep(8)
    window = log_text(off)
    gated = "press gated" in window.lower()
    check("a take with no model installed is gated, not hung", gated,
          "looked for the gate marker in this take's window")
    phase = str(((send({"cmd": "status"}).get("data") or {}).get("pill") or {}).get("phase"))
    check("the gated press leaves the pill at rest", phase in ("idle", "Idle"),
          "phase=%s" % phase)
    spool = os.path.join(out, "data")
    files = []
    for root, _dirs, names in os.walk(spool):
        files += [n for n in names if n.endswith((".wav", ".tmp"))]
    check("the gated press wrote no recording", not files, "%d audio file(s)" % len(files))
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
print("first_run: %d checks, %d failed" % (len(results), len(failed)))
sys.exit(1 if failed else 0)
PY
DRIVER=$?

kill -9 "$APP" 2>/dev/null
echo "first_run: log at $LOG"
exit $DRIVER
