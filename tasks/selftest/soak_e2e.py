#!/usr/bin/env python3
"""Headless 500-take soak driven through the app's --e2e harness.

The companion to soak.sh. That leg presses the real hotkey and speaks through
the speakers, so it owns the desktop and the audio device for about three
hours. This leg feeds recorded WAVs straight into the app's take path and, in
the default headless mode, forces the cascade's hold route (delivery.rs returns
DeliveryOutcome::HeldForPasteLast before any clipboard write or keystroke), so
it takes no focus, touches no app, and can run while the machine is in use.

Both legs answer the same SOAK-1 question: zero crashes, zero stuck pills, an
unchanged recovery file count, and memory growth under 100 MB across a mix of
short taps, silence, 2 s, 30 s and 5 min takes, plus hotkey spam.

Coverage this leg does NOT have, and the reason:
  - the real capture path (CoreAudio input, the hotkey, the mic permission).
    That is soak.sh's job; this leg starts where a take is already armed.
  - in headless mode, the inject path. Every take ends at the hold outcome, so
    paste verification, caret safety and clipboard restore get no load. Pass
    --gui to soak_e2e.sh for a leg that pastes, and keep that leg short.
  - the mid-take app-switch shape, which needs to change the frontmost app and
    is therefore skipped in headless mode and marked in the output.

Nothing here writes dictated text, screen text or field text anywhere. The
per-take record carries a character count, the delivery label the app reported,
the pill phase, and lengths and counts only.

Usage (normally through soak_e2e.sh, which sets the environment):
  SOAK_SOCK=... SOAK_APP_PID=... SOAK_DATA_DIR=... SOAK_LOG=... \
  SOAK_OUT=... SOAK_AUDIO=... SOAK_TAKES=500 SOAK_HEADLESS=1 soak_e2e.py
"""
import glob
import json
import os
import socket
import subprocess
import sys
import time

SOCK = os.environ["SOAK_SOCK"]
APP_PID = int(os.environ["SOAK_APP_PID"])
DATA_DIR = os.environ["SOAK_DATA_DIR"]
LOG = os.environ["SOAK_LOG"]
OUT_DIR = os.environ["SOAK_OUT"]
AUDIO_DIR = os.environ["SOAK_AUDIO"]
TAKES = int(os.environ.get("SOAK_TAKES", "500"))
HEADLESS = os.environ.get("SOAK_HEADLESS", "1") == "1"
JSONL = os.path.join(OUT_DIR, "takes.jsonl")
SUMMARY = os.path.join(OUT_DIR, "summary.json")
# The app's own window into what a take did. These are the four quiet ends in
# dictation.rs (quick tap under the floor, quick tap ordinary stop, a gated
# press, and a take that transcribed to no speech); none of them writes an e2e
# result record, so without them a driver cannot tell a deliberate silent end
# from a hung take.
BENIGN = (
    # Not "quick tap:" on its own. dictation.rs:3292 logs "quick tap: ordinary
    # stop" for a short release that IS transcribed, so the bare prefix also
    # matches takes that go on to write a result, and a poll that lands between
    # the log line and the result reads a normal take as a silent end.
    "quick tap: take under the visible floor",
    "press gated",
    "skipping decode:",
    "no speech in take: ended quietly",
)
# A take's own opening line. Markers are attributed to the window that starts at
# the arm line, so the count of arm lines in a window says how many takes the
# app actually armed while the driver thought it was driving one.
ARM = "streaming take armed"


def path(name):
    return os.path.join(AUDIO_DIR, name)


def build_schedule():
    """One cycle covering every shape the corpus builder can produce.

    Shapes whose clip is missing are dropped, so a corpus without the 5 minute
    clip still runs. Real recordings are rotated by index so the cycle covers
    as many distinct clips as are available.
    """
    reals = sorted(glob.glob(os.path.join(AUDIO_DIR, "real-*.wav")))
    if not reals:
        raise SystemExit("no real-*.wav clips in %s" % AUDIO_DIR)

    def real(i):
        return reals[i % len(reals)]

    has = lambda n: os.path.exists(path(n))  # noqa: E731
    cycle = [
        ("speech-real", real(0)),
        ("speech-real", real(1)),
        ("tap-short", path("tap_0s2.wav")) if has("tap_0s2.wav") else None,
        ("silence", path("sil_3s.wav")) if has("sil_3s.wav") else None,
        ("speech-real", real(2)),
        ("speech-2s", path("speech_2s.wav")) if has("speech_2s.wav") else None,
        ("speech-real", real(3)),
        ("switch-30s", path("speech_30s.wav")) if has("speech_30s.wav") else None,
        ("tap-short", path("tap_0s2.wav")) if has("tap_0s2.wav") else None,
        ("speech-real", real(4)),
        ("silence", path("sil_15s.wav")) if has("sil_15s.wav") else None,
        ("spam-2s", path("speech_2s.wav")) if has("speech_2s.wav") else None,
        ("speech-real", real(5)),
        ("speech-30s", path("speech_30s.wav")) if has("speech_30s.wav") else None,
        ("stop-early", real(6)),
        ("tap-short", path("tap_0s2.wav")) if has("tap_0s2.wav") else None,
        ("speech-real", real(7)),
        ("silence", path("sil_3s.wav")) if has("sil_3s.wav") else None,
        ("speech-2s", path("speech_2s.wav")) if has("speech_2s.wav") else None,
        ("speech-real", real(0)),
        ("spam-real", real(1)),
        ("speech-30s", path("speech_30s.wav")) if has("speech_30s.wav") else None,
        ("speech-real", real(2)),
        ("tap-short", path("tap_0s2.wav")) if has("tap_0s2.wav") else None,
        ("silence", path("sil_3s.wav")) if has("sil_3s.wav") else None,
        ("speech-2s", path("speech_2s.wav")) if has("speech_2s.wav") else None,
        ("switch-real", real(3)),
        ("speech-real", real(4)),
        ("stop-early", real(5)),
        ("speech-30s", path("speech_30s.wav")) if has("speech_30s.wav") else None,
        ("tap-short", path("tap_0s2.wav")) if has("tap_0s2.wav") else None,
        ("speech-real", real(6)),
        ("spam-2s", path("speech_2s.wav")) if has("speech_2s.wav") else None,
        ("silence", path("sil_15s.wav")) if has("sil_15s.wav") else None,
        ("speech-2s", path("speech_2s.wav")) if has("speech_2s.wav") else None,
        ("speech-real", real(7)),
        ("switch-30s", path("speech_30s.wav")) if has("speech_30s.wav") else None,
        ("fivemin", path("speech_5min.wav")) if has("speech_5min.wav") else None,
    ]
    return [slot for slot in cycle if slot]


def send(obj, timeout=30):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(SOCK)
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


def rss_kb():
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(APP_PID)],
                         capture_output=True, text=True).stdout.strip()
    return int(out) if out else -1


def alive():
    return subprocess.run(["ps", "-p", str(APP_PID)],
                          capture_output=True).returncode == 0


def spool_count():
    n = 0
    for root, _dirs, files in os.walk(DATA_DIR):
        n += sum(1 for f in files if f.startswith("spool-"))
    return n


def recovery_count():
    d = os.path.join(DATA_DIR, "recovery")
    return len(os.listdir(d)) if os.path.isdir(d) else 0


def pill_phase():
    """The UiState the pill renders from: {"phase": "idle"} at rest."""
    d = (send({"cmd": "status"}).get("data") or {}).get("pill")
    return str(d.get("phase", d)) if isinstance(d, dict) else str(d)


def activate(app):
    if HEADLESS:
        # Forced hold means nothing needs focus, so the driver never changes the
        # frontmost app and the machine stays usable during the run.
        return False
    subprocess.run(["osascript", "-e", 'tell application "%s" to activate' % app],
                   capture_output=True)
    return True


def log_size():
    try:
        return os.path.getsize(LOG)
    except OSError:
        return 0


def log_text(off):
    try:
        size = os.path.getsize(LOG)
    except OSError:
        return ""
    with open(LOG, "r", errors="replace") as fh:
        fh.seek(min(off, size))
        return fh.read()


def main():
    schedule = build_schedule()
    crash_dir = os.path.expanduser("~/Library/Logs/DiagnosticReports")
    base_crash = len(glob.glob(os.path.join(crash_dir, "teletype*")))
    base_recovery = recovery_count()
    base_rss = rss_kb()
    warm_rss = None
    WARMUP_TAKE = 10
    start = time.time()
    print("soak_e2e: %d takes, %d shapes, mode=%s" % (
        TAKES, len(schedule), "headless (hold)" if HEADLESS else "gui"), flush=True)

    out = open(JSONL, "w")
    # Results are keyed by a monotonic session id the controller assigns per
    # take, and the map is never cleared, so a result with an id above every id
    # seen so far is the take that just finished. Probing a window covers the
    # extra sessions a spam burst creates; the highest hit wins. Jumping the
    # counter forward on a miss is what made a single anomaly permanent: every
    # later take then looked like a timeout.
    seen = {"id": 0}
    PROBE_WINDOW = 12
    fails = []
    notes = []
    silent_ends = 0
    pill_messages = 0
    refused = 0
    i = 0

    def wait_for_idle(limit):
        """Release gate: never arm a take while the previous one is still in a
        live phase, or the two takes' results arrive interleaved."""
        deadline = time.time() + limit
        while time.time() < deadline:
            if pill_phase() in ("idle", "Idle"):
                return True
            time.sleep(1)
        return pill_phase() in ("idle", "Idle")

    def wait_for_take(t0, limit, off):
        """Return (session_id, result, silent_marker). Either channel ending the
        take is a finished take; a speech take with neither is the real timeout.

        The log channel is anchored on this take's own "streaming take armed"
        line. Without the anchor a marker written late by the PREVIOUS take lands
        in this window and a speech take reads as a quiet end, which is the
        misattribution the unanchored version produced.
        """
        while time.time() - t0 < limit:
            time.sleep(2)
            hit = None
            for candidate in range(seen["id"] + 1, seen["id"] + 1 + PROBE_WINDOW):
                found = (send({"cmd": "result", "id": candidate}).get("data") or {})
                found = found.get("result")
                if found:
                    hit = candidate
            if hit is not None:
                data = (send({"cmd": "result", "id": hit}).get("data") or {})
                return hit, data["result"], None
            low = log_text(off).lower()
            arm = low.rfind("streaming take armed")
            start_at = arm + 1 if arm >= 0 else 0
            for marker in BENIGN:
                if low.find(marker, start_at) != -1:
                    return None, None, marker
        return None, None, None

    def wait_for_quiet(limit, settle=3):
        """Settle gate: never arm while the previous take's feeder is in flight.

        A quiet end is written when the controller discards the take, but the
        feeder thread keeps sampling until its own file is exhausted and then
        sends a release of its own. That trailing release lands in whatever take
        is armed next, so a 30 s speech take can be cut short and end with
        "no speech in take". Waiting for the log to stop producing end markers
        is what separates that harness artifact from a real lost take.
        """
        deadline = time.time() + limit
        quiet_since = time.time()
        last = log_size()
        while time.time() < deadline:
            time.sleep(1)
            now = log_size()
            if now != last:
                if any(m in log_text(last).lower() for m in BENIGN):
                    quiet_since = time.time()
                last = now
            if time.time() - quiet_since >= settle:
                return True
        return False

    for i in range(1, TAKES + 1):
        shape, wav = schedule[(i - 1) % len(schedule)]
        if not wait_for_idle(30):
            fails.append({"take": i, "shape": shape,
                          "why": "pill still busy before arming: %s" % pill_phase()})
        settled = wait_for_quiet(25)
        t0 = time.time()
        off = log_size()
        activate("TextEdit")
        time.sleep(0.3)

        spam = shape.startswith("spam")
        if spam:
            # Hotkey spam is the take, not an add-on to one. A user hammering
            # the shortcut presses AND releases, so each entry is a down/up
            # pair; a bare down would leave a take armed into the next slot.
            # Whatever survives is the answer, so text is not asserted here.
            for _ in range(3):
                started = send({"cmd": "start", "input": wav, "command": False})
                if not started.get("ok"):
                    break
                time.sleep(0.15)
                send({"cmd": "stop"})
                time.sleep(0.15)
        else:
            started = send({"cmd": "start", "input": wav, "command": False})
            if not started.get("ok"):
                fails.append({"take": i, "shape": shape, "why": "start rejected"})
                break
        switch_applied = False
        if shape.startswith("switch"):
            time.sleep(2.0)
            switch_applied = activate("Calculator")
        if shape.startswith("stop"):
            time.sleep(1.0)
            send({"cmd": "stop"})

        limit = 470 if shape == "fivemin" else (90 if "30s" in shape else 45)
        session, res, silent = wait_for_take(t0, limit, off)
        # What the app itself says happened inside this take's window. A window
        # with no arm line means the press never reached the controller; a window
        # with two markers means one of them belongs to a neighbouring take, so
        # the attribution above is not trustworthy for that take.
        window = log_text(off).lower()
        arms_in = window.count(ARM)
        markers_in = sum(window.count(m) for m in BENIGN)
        timed_out = res is None and silent is None
        if timed_out:
            fails.append({"take": i, "shape": shape,
                          "why": "no result and no silent-end marker in %ds" % limit})
            if not alive():
                fails.append({"take": i, "shape": shape, "why": "app process died"})
                break
            # Leave the counter alone. A late arrival for this take still has an
            # id above the last one seen, so the next probe window reaches it.
        elif session is not None:
            seen["id"] = session

        after = pill_phase()
        if after not in ("idle", "Idle"):
            time.sleep(3)
            after = pill_phase()
            if after not in ("idle", "Idle"):
                # A pill that still shows the skip message is a terminal
                # notice, not a stuck pipeline: it is the app telling the user
                # why their text was not pasted, and it clears when the next
                # take arms. wait_for_idle is the proof, and it never fails in
                # a healthy run. Only a live phase (recording, transcribing,
                # processing) means the take never released the pill.
                if after == "message":
                    pill_messages += 1
                else:
                    fails.append({"take": i, "shape": shape,
                                  "why": "stuck pill: %s" % after})

        # Spam takes are truncated by their own burst, so an empty transcript is
        # the expected answer; what is asserted is that nothing crashed, stuck or
        # leaked.
        quiet = shape in ("silence", "tap-short", "stop-early") or spam
        text = (res or {}).get("text") or ""
        delivery = (res or {}).get("delivery") or "none"
        error = (res or {}).get("error") or ""
        if timed_out:
            delivery, error = "timeout", "no result and no quiet-end marker"
        elif res is None:
            silent_ends += 1
            delivery, error = "silent-end", silent
            if not quiet:
                fails.append({"take": i, "shape": shape,
                              "why": "speech take ended silently: %s" % silent})
        elif not quiet:
            if len(text) == 0:
                fails.append({"take": i, "shape": shape, "why": "empty text"})
            if not any(k in delivery for k in ("verified", "clipboard", "hold")):
                notes.append({"take": i, "shape": shape, "why": "delivery=%s" % delivery})
            if "refused" in delivery or "kept" in delivery:
                refused += 1
        if shape.startswith("switch") and not switch_applied:
            notes.append({"take": i, "shape": shape,
                          "why": "mid-take app switch skipped (headless)"})

        if not settled:
            notes.append({"take": i, "shape": shape,
                          "why": "log still producing end markers when armed"})
        out.write(json.dumps({
            "index": i, "shape": shape, "start": round(t0, 1),
            "end": round(time.time(), 1), "text_chars": len(text),
            "delivery": delivery, "pill_after": after, "error": error,
            "arms": arms_in, "markers": markers_in,
            "rss_kb": rss_kb(), "spool": spool_count(),
            "recovery": recovery_count(),
        }) + "\n")
        out.flush()
        if warm_rss is None:
            # The model loads on the first takes, so a baseline taken before
            # launch would charge that ~50 MB of load-in to the soak. Measure
            # the growth the criterion cares about from a warm process.
            if i >= WARMUP_TAKE:
                warm_rss = rss_kb()
        if not alive():
            fails.append({"take": i, "shape": shape, "why": "app process died"})
            break
        if i % 25 == 0 or shape == "fivemin" or i == TAKES:
            print("take %d/%d %s chars=%d delivery=%s pill=%s rss=%dKB wall=%.0fm" % (
                i, TAKES, shape, len(text), delivery, after, rss_kb(),
                (time.time() - start) / 60), flush=True)

    end_rss = rss_kb()
    end_recovery = recovery_count()
    end_crash = len(glob.glob(os.path.join(crash_dir, "teletype*")))
    growth_total = (end_rss - base_rss) if end_rss > 0 and base_rss > 0 else None
    growth_warm = (end_rss - warm_rss) if end_rss > 0 and warm_rss else None
    soak1 = {
        "crashes": sum(1 for f in fails if "process died" in f["why"])
                  + max(0, end_crash - base_crash),
        "stuck_pills": sum(1 for f in fails if "stuck pill" in f["why"]),
        "terminal_skip_messages": pill_messages,
        "recovery_files_changed": end_recovery - base_recovery,
        "memory_growth_kb_from_warmup": growth_warm,
        "limit_memory_growth_kb": 100,
    }
    soak1["pass"] = (soak1["crashes"] == 0 and soak1["stuck_pills"] == 0
                     and soak1["recovery_files_changed"] == 0
                     and growth_warm is not None
                     and growth_warm < soak1["limit_memory_growth_kb"])
    summary = {
        "takes": i, "fails": len(fails), "silent_ends": silent_ends,
        "timeouts": sum(1 for f in fails if "no result" in f["why"]),
        "refused_or_kept": refused, "mode": "headless" if HEADLESS else "gui",
        "soak1": soak1,
        "rss_kb_before_launch": base_rss, "rss_kb_at_warmup": warm_rss,
        "rss_kb_end": end_rss,
        "rss_growth_since_launch_kb": growth_total,
        "warmup_take": WARMUP_TAKE,
        "recovery_before": base_recovery, "recovery_after": end_recovery,
        "crash_reports_before": base_crash, "crash_reports_after": end_crash,
        "wall_minutes": round((time.time() - start) / 60, 1),
    }
    print(json.dumps(summary), flush=True)
    for f in fails[:25]:
        print("FAIL", json.dumps(f), flush=True)
    for n in notes[:25]:
        print("NOTE", json.dumps(n), flush=True)
    with open(SUMMARY, "w") as fh:
        json.dump({"summary": summary, "fails": fails, "notes": notes}, fh, indent=2)
    out.close()
    sys.exit(0 if not fails else 1)


main()
