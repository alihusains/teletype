#!/usr/bin/env python3
"""Headless take-pair probe for the SOAK-1 quiet-end question.

Arms a fixed two-take sequence N times and reports how often the SECOND
take ends quietly, per sequence. The outcome of the second take is
attributable to the first take by construction: each rep is
first, then second, nothing else.

Headless only. This probe drives `start` and `stop` over the app's e2e
unix socket and nothing else. It never activates an app, never changes
the clipboard, and never pastes. No model flags, no sudo, no network.
Standard library only.

It measures; it never asserts a pass or fail. The lead judges.

Usage:
  python3 tasks/selftest/take_pair_probe.py \
    --sock PATH --log PATH --audio DIR \
    --first SPECS --second SPECS --reps N [--gap SECONDS] [--no-settle]
    [--out DIR] [--plan]

It attaches to an app that is already running, so the socket and the log
have to be the live app's, and the result id counter is wherever that
session left it.

Shapes (resolved to WAVs the way build_schedule in soak_e2e.py does it):
  speech-real   the first real-*.wav in the audio dir
  speech-2s     speech_2s.wav
  speech-30s    speech_30s.wav
  tap-short     tap_0s2.wav
  silence       sil_3s.wav
  spam          three start/stop pairs, 0.15 s apart, on speech-2s.wav

--plan prints the resolved shape to WAV paths and the rep count and
exits 0 without opening the socket. It reports missing files instead
of crashing.
"""
import argparse
import glob
import json
import os
import socket
import sys
import time

# The app's own window into what a take did: the four quiet ends in
# dictation.rs. None of them writes an e2e result record, so without
# them a driver cannot tell a deliberate silent end from a hung take.
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
# A take's own opening line. Markers are attributed to the window that
# starts at the arm line, so the count of arm lines in a window says how
# many takes the app actually armed while the driver thought it was
# driving one.
ARM = "streaming take armed"
# Result ids are monotonic per session and never cleared, so a result
# with an id above every id seen so far is the take that just finished.
# Probing a window covers the extra sessions a spam burst creates; the
# highest hit wins.
PROBE_WINDOW = 12
# A spam burst opens up to three sessions, so the first take of a rep
# can shift the counter by that much; the second take's probe window
# must clear it.
ID_WINDOW = PROBE_WINDOW + 4
QUIET_SETTLE = 3
IDLE_LIMIT = 30
QUIET_LIMIT = 25
TAKE_LIMIT = 45


def parse_args(argv):
    p = argparse.ArgumentParser(
        prog="take_pair_probe.py",
        description="Arm a fixed two-take sequence N times and count how "
                    "often the second take ends quietly, per sequence.")
    p.add_argument("--sock", required=True, help="path to the app's e2e unix socket")
    p.add_argument("--log", required=True, help="path to the app log file")
    p.add_argument("--audio", required=True, help="directory holding the WAV clips")
    p.add_argument("--first", required=True,
                   help="shape of the first take: speech-real, speech-2s, "
                        "speech-30s, tap-short, silence or spam")
    p.add_argument("--second", required=True,
                   help="shape of the second take: same shape names")
    p.add_argument("--reps", type=int, required=True, help="number of reps")
    p.add_argument("--out", default="/tmp/take-pair-probe",
                   help="directory for the JSONL and summary.json "
                        "(default /tmp/take-pair-probe)")
    p.add_argument("--gap", type=float, default=0.0,
                   help="seconds of idle time before each arm (default 0). "
                        "This is the variable under test: a previous take's "
                        "feeder thread can outlive its result and its trailing "
                        "release can land inside the next take.")
    p.add_argument("--no-settle", dest="settle", action="store_false",
                   help="skip the idle-pill and log-quiet gates before arming")
    p.add_argument("--max-id", type=int, default=4096,
                   help="upper bound for the one-time result id sweep that "
                        "finds where the live session's counter is (default 4096)")
    p.add_argument("--plan", action="store_true",
                   help="print the resolved plan and exit 0 without "
                        "opening the socket")
    return p.parse_args(argv)


def resolve_shape(name, audio_dir):
    """Resolve a shape name to (wav, is_spam), the way build_schedule in
    soak_e2e.py maps shapes to clips. A missing first real-*.wav is an
    error; every other missing clip is reported, not raised."""
    if name == "speech-real":
        reals = sorted(glob.glob(os.path.join(audio_dir, "real-*.wav")))
        if not reals:
            raise SystemExit("no real-*.wav clips in %s" % audio_dir)
        return reals[0], False
    clips = {
        "speech-2s": "speech_2s.wav",
        "speech-30s": "speech_30s.wav",
        "tap-short": "tap_0s2.wav",
        "silence": "sil_3s.wav",
    }
    if name in clips:
        return os.path.join(audio_dir, clips[name]), False
    if name == "spam":
        return os.path.join(audio_dir, "speech_2s.wav"), True
    raise SystemExit("unknown shape %r (expected one of: %s)" % (
        name, ", ".join(["speech-real", "speech-2s", "speech-30s",
                         "tap-short", "silence", "spam"])))


def plan(args):
    """Print the resolved plan and exit 0 without opening the socket.
    Missing files are reported, not fatal."""
    problems = []
    for label in ("first", "second"):
        shape = getattr(args, label)
        wav, spam = resolve_shape(shape, args.audio)
        if not os.path.exists(wav):
            problems.append("%s: %s (%s) is missing" % (label, shape, wav))
    print("take_pair_probe plan")
    print("  audio dir: %s" % args.audio)
    print("  reps: %d" % args.reps)
    wav1, spam1 = resolve_shape(args.first, args.audio)
    wav2, spam2 = resolve_shape(args.second, args.audio)
    for label, shape, wav, spam in (
            ("first", args.first, wav1, spam1),
            ("second", args.second, wav2, spam2)):
        note = " (spam burst: 3 start/stop pairs at 0.15 s)" if spam else ""
        print("  %s: %-12s -> %s%s" % (label, shape, wav, note))
    for p in problems:
        print("  MISSING %s" % p)
    print("  out dir: %s" % args.out)
    print("plan ok (no socket opened)")
    sys.exit(0)


def send(sock, obj, timeout=30):
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


def pill_phase(sock):
    """The UiState the pill renders from: {"phase": "idle"} at rest."""
    d = (send(sock, {"cmd": "status"}).get("data") or {}).get("pill")
    return str(d.get("phase", d)) if isinstance(d, dict) else str(d)


def log_size(log):
    try:
        return os.path.getsize(log)
    except OSError:
        return 0


def log_text(log, off):
    try:
        size = os.path.getsize(log)
    except OSError:
        return ""
    with open(log, "r", errors="replace") as fh:
        fh.seek(min(off, size))
        return fh.read()


def wait_for_idle(sock, limit):
    """Release gate: never arm a take while the previous one is still in a
    live phase, or the two takes' results arrive interleaved."""
    deadline = time.time() + limit
    while time.time() < deadline:
        if pill_phase(sock) in ("idle", "Idle"):
            return True
        time.sleep(1)
    return pill_phase(sock) in ("idle", "Idle")


def wait_for_quiet(log, limit, settle=QUIET_SETTLE):
    """Settle gate: never arm while the previous take's feeder is in flight.

    A quiet end is written when the controller discards the take, but the
    feeder thread keeps sampling until its own file is exhausted and then
    sends a release of its own. That trailing release lands in whatever take
    is armed next, so a speech take can be cut short and end with "no speech
    in take". Waiting for the log to stop producing end markers for at least
    settle seconds is what separates that harness artifact from a real lost
    take."""
    deadline = time.time() + limit
    quiet_since = time.time()
    last = log_size(log)
    while time.time() < deadline:
        time.sleep(1)
        now = log_size(log)
        if now != last:
            if any(m in log_text(log, last).lower() for m in BENIGN):
                quiet_since = time.time()
            last = now
        if time.time() - quiet_since >= settle:
            return True
    return False


def find_high_water(sock, max_id, stop_after=ID_WINDOW * 3):
    """The highest recorded result id, or -1 if the map is empty.

    soak_e2e.py can start its probe window at 0 because it launches the app
    itself and the controller's session counter starts with it. This probe
    attaches to an app that is already live, so its counter is wherever that
    session left it and a window at 0 would never reach a result: every take
    would read as a timeout. The e2e protocol has no list command (e2e.rs
    `E2eCommand::Result` only does a map lookup), so the starting point has to
    be discovered by sweeping. Quiet ends write no record, so the ids are not
    dense and the sweep takes the maximum rather than the first miss. The
    sweep stops once `stop_after` ids past the last hit are all empty, which
    bounds the cost for a short session."""
    highest = -1
    for candidate in range(1, max_id + 1):
        if highest >= 0 and candidate - highest > stop_after:
            break
        found = (send(sock, {"cmd": "result", "id": candidate}, timeout=5).get("data")
                 or {}).get("result")
        if found:
            highest = candidate
    return highest


def run_take(sock, log, wav, spam, seen, limit, gap=0):
    """Arm one take (a spam burst or a single start) and wait for its end.

    Returns the per-take record fields: delivery, marker, text_chars,
    arms and markers. The window is the log from the byte offset recorded
    before arming; arms and markers are counted in that window, which is
    what makes a misattribution visible (zero arms: the press never
    reached the controller; two markers: one belongs to a neighbouring
    take)."""
    t0 = time.time()
    if gap:
        # The variable under test: how long the app sits idle between the
        # previous take ending and this one arming. A feeder thread from the
        # previous take keeps sampling until its own file is exhausted and
        # then sends a release of its own, so the gap decides whether that
        # trailing release can land inside this take.
        time.sleep(gap)
    off = log_size(log)
    if spam:
        # Hotkey spam is the take, not an add-on to one. A user hammering
        # the shortcut presses AND releases, so each entry is a down/up
        # pair; a bare down would leave a take armed into the next slot.
        for _ in range(3):
            started = send(sock, {"cmd": "start", "input": wav, "command": False})
            if not started.get("ok"):
                break
            time.sleep(0.15)
            send(sock, {"cmd": "stop"})
            time.sleep(0.15)
    else:
        send(sock, {"cmd": "start", "input": wav, "command": False})

    session = None
    res = None
    marker = None
    while time.time() - t0 < limit:
        time.sleep(2)
        hit = None
        for candidate in range(seen["id"] + 1, seen["id"] + 1 + ID_WINDOW):
            found = (send(sock, {"cmd": "result", "id": candidate}).get("data") or {})
            found = found.get("result")
            if found:
                hit = candidate
        if hit is not None:
            data = (send(sock, {"cmd": "result", "id": hit}).get("data") or {})
            session, res, marker = hit, data["result"], None
            break
        low = log_text(log, off).lower()
        arm = low.rfind("streaming take armed")
        start_at = arm + 1 if arm >= 0 else 0
        for m in BENIGN:
            if low.find(m, start_at) != -1:
                marker = m
                break
        if marker is not None:
            break

    window = log_text(log, off).lower()
    arms_in = window.count(ARM)
    markers_in = sum(window.count(m) for m in BENIGN)
    if session is not None:
        seen["id"] = session
    text = (res or {}).get("text") or ""
    delivery = (res or {}).get("delivery") or "none"
    if res is None and marker is None:
        delivery = "timeout"
    elif res is None:
        delivery = "silent-end"
    return {
        "delivery": delivery,
        "marker": marker,
        "text_chars": len(text),
        "arms": arms_in,
        "markers": markers_in,
    }


def main(argv):
    args = parse_args(argv)
    if args.reps < 1:
        raise SystemExit("--reps must be at least 1")
    if args.plan:
        plan(args)

    wav1, spam1 = resolve_shape(args.first, args.audio)
    wav2, spam2 = resolve_shape(args.second, args.audio)
    os.makedirs(args.out, exist_ok=True)
    jsonl = os.path.join(args.out, "takes.jsonl")
    summary_path = os.path.join(args.out, "summary.json")
    print("take_pair_probe: reps=%d first=%s second=%s" % (
        args.reps, args.first, args.second), flush=True)

    seen = {"id": find_high_water(args.sock, args.max_id)}
    print("  result id high water: %s" % seen["id"], flush=True)
    out = open(jsonl, "w")
    counts = {
        "first": {"reps": 0, "quiet": 0, "timeout": 0},
        "second": {"reps": 0, "quiet": 0, "timeout": 0},
    }
    start = time.time()
    for rep in range(1, args.reps + 1):
        # Gate on the app being at rest before arming, on both channels
        # the soak driver uses: an idle pill, and no quiet-end marker in
        # the log for at least three seconds. --no-settle turns both gates
        # off, which is how the gap experiment reproduces the artifact on
        # purpose instead of only guarding against it.
        if args.settle:
            idle = wait_for_idle(args.sock, IDLE_LIMIT)
            settled = wait_for_quiet(args.log, QUIET_LIMIT)
        else:
            idle = settled = True
        if not idle:
            print("rep %d: pill still busy before arming: %s" % (
                rep, pill_phase(args.sock)), flush=True)
        if not settled:
            print("rep %d: log still producing end markers when armed" % rep,
                  flush=True)
        rows = []
        for position, (shape, wav, spam) in (
                (1, (args.first, wav1, spam1)),
                (2, (args.second, wav2, spam2))):
            r = run_take(args.sock, args.log, wav, spam, seen, TAKE_LIMIT,
                         gap=args.gap)
            after = pill_phase(args.sock)
            if after not in ("idle", "Idle"):
                time.sleep(3)
                after = pill_phase(args.sock)
            row = {
                "rep": rep,
                "position": position,
                "shape": shape,
                "delivery": r["delivery"],
                "marker": r["marker"],
                "text_chars": r["text_chars"],
                "pill_after": after,
                "arms": r["arms"],
                "markers": r["markers"],
                "gap": args.gap,
                "settle": bool(args.settle),
            }
            out.write(json.dumps(row) + "\n")
            out.flush()
            counts["first" if position == 1 else "second"]["reps"] += 1
            if r["delivery"] == "silent-end":
                counts["first" if position == 1 else "second"]["quiet"] += 1
            elif r["delivery"] == "timeout":
                counts["first" if position == 1 else "second"]["timeout"] += 1
            rows.append(row)
        print("rep %d/%d: first=%s(%s) second=%s(%s)" % (
            rep, args.reps,
            rows[0]["delivery"], rows[0]["text_chars"],
            rows[1]["delivery"], rows[1]["text_chars"]), flush=True)
    out.close()

    def pair_block(c):
        reps = c["reps"]
        quiet = c["quiet"]
        return {
            "reps": reps,
            "quiet_ends": quiet,
            "quiet_rate": (quiet / reps) if reps else None,
            "timeouts": c["timeout"],
        }

    summary = {
        "pair": {"first": args.first, "second": args.second},
        "reps": args.reps,
        "gap_seconds": args.gap,
        "settle_gates": bool(args.settle),
        "first": pair_block(counts["first"]),
        "second": pair_block(counts["second"]),
        # The one number the question turns on: with a fixed first take, how
        # often does the take after it end quietly?
        "second_take_quiet": counts["second"]["quiet"],
        "second_take_quiet_rate": (
            counts["second"]["quiet"] / counts["second"]["reps"]
            if counts["second"]["reps"] else None),
        "wall_minutes": round((time.time() - start) / 60, 1),
    }
    with open(summary_path, "w") as fh:
        json.dump(summary, fh, indent=2)

    print("take_pair_probe summary: first=%s second=%s reps=%d" % (
        args.first, args.second, args.reps), flush=True)
    for label, c in (("first", counts["first"]), ("second", counts["second"])):
        rate = (c["quiet"] / c["reps"]) if c["reps"] else 0.0
        print("  %-6s: quiet-ends %d/%d (%.1f%%), timeouts %d" % (
            label, c["quiet"], c["reps"], rate * 100, c["timeout"]), flush=True)
    print("jsonl: %s" % jsonl, flush=True)
    print("summary: %s" % summary_path, flush=True)
    sys.exit(0)


if __name__ == "__main__":
    main(sys.argv[1:])
