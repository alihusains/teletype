#!/usr/bin/env python3
"""TTS command-utterance corpus generator (Addendum A.4, task 7).

Generates a deterministic corpus of spoken command + dictation utterances
with macOS `say` using five distinct TTS voices (Addendum A.5), converts
each to 16 kHz mono PCM WAV (raw PCM written by hand - no wave module),
and writes a `corpus.json` manifest with the canonical expected tool
call per clip.

Expected tool names mirror `crates/teletype-inference/src/needle.rs`
(`COMMAND_TOOLS_JSON`), the shared command-tool set used by both the
Needle and LLM-fallback extractors. The corpus is intentionally
engine-agnostic: only `open_app` has a canonical tool call. The
`edit_verb` and `focus_control` categories exercise capabilities (text
editing, app/window focus) that no engine in the chain declares as a
tool, so their `expected` is null (the correct decision for every
engine: no tool call / `continue_speaking`). `pointer` and
`itn_sensitive` are likewise null-expected.

Usage:
    python3 scripts/eval/gen_command_clips.py --out evals/commands/corpus/
    python3 scripts/eval/gen_command_clips.py --out evals/commands/corpus/ --subset negative

Idempotent: existing wavs are skipped; the manifest is always rewritten
from the current utterance list (deterministic).
"""

from __future__ import annotations

import argparse
import json
import struct
import subprocess
import sys
import time
from pathlib import Path

SCHEMA = "teletype-command-corpus-v1"

# ---------------------------------------------------------------------------
# Deterministic utterance list. (text, expected) where expected is the
# canonical tool call or None for negatives.
# ---------------------------------------------------------------------------

OPEN_APP = [
    "open Safari",
    "open Google Chrome",
    "open Visual Studio Code",
    "open iMessage",
    "open Terminal",
    "open iTerm",
    "open Mail",
    "open Notes",
    "open Finder",
    "open Slack",
    "open Discord",
    "open Calculator",
    "open Calendar",
    "open Photos",
    "open Maps",
    "open Music",
    "open Settings",
    "open App Store",
    "open Keynote",
    "open Reminders",
    "open FaceTime",
    "open Contacts",
    "open Activity Monitor",
    "open System Settings",
    "open Screen Time",
    "open Voice Memos",
    "open QuickTime Player",
    "open Stocks",
    "open Home",
    "open Shortcuts",
]

EDIT_VERB = [
    "make this sentence shorter",
    "make this paragraph longer",
    "bold this word",
    "italicize this word",
    "underline the selected text",
    "fix the grammar",
    "fix the spelling",
    "replace your company with IC Markets",
    "replace the date with October 17",
    "turn this into bullet points",
    "turn this into a table",
    "number the list",
    "capitalize the first letter of each sentence",
    "make this all lowercase",
    "make this all uppercase",
    "delete the last sentence",
    "delete the selected text",
    "insert a new paragraph",
    "move this paragraph up",
    "align this text to the center",
    "remove the typos",
    "rewrite this in short sentences",
    "make this more formal",
    "make this more casual",
    "add a comma after the first word",
    "fix the punctuation",
    "summarize this paragraph",
    "expand this into a full sentence",
    "highlight this word",
    "strikethrough this word",
]

FOCUS_CONTROL = [
    "focus the browser",
    "focus email",
    "bring up the terminal",
    "bring up the calendar",
    "bring up the calculator",
    "bring up the notes app",
    "switch to the browser",
    "switch to email",
    "switch to the terminal",
    "switch to the music app",
    "focus the code editor",
    "focus the messenger",
    "bring up the settings",
    "bring up the file manager",
    "bring up the photo app",
    "bring up the map app",
    "switch to the messages app",
    "switch to the video call app",
    "focus the spreadsheet",
    "focus the presentation",
    "focus the web browser",
]

POINTER = [
    "click Save",
    "click the submit button",
    "click the close button",
    "click the search icon",
    "double click the file",
    "double click the folder",
    "right click the icon",
    "type hello",
    "type good morning team",
    "press enter",
    "press the escape key",
    "press the tab key",
    "scroll down the page",
    "scroll up the page",
    "scroll to the top",
    "click the back button",
    "click the play button",
    "double click the desktop icon",
    "right click the file name",
    "type the word hello",
    "press the return key",
    "press the space bar",
    "scroll to the bottom",
]

ITN_SENSITIVE = [
    "the price is 42 dollars 50 cents",
    "the meeting is at 2:30 PM",
    "the value of pi is 3.14",
    "call me at 555 123 4567",
    "the flight leaves at 6:45 AM",
    "the temperature is 21.5 degrees",
    "set the timer for 45 seconds",
    "the bill was 1,250 euros",
    "room number 402",
    "the ratio is 2.5 to 1",
    "it costs 99 cents",
    "the version is 3.10",
    "the date is March 14, 2026",
    "the number is 0.001",
    "the score was 3 to 2",
    "the meeting is at 2:30 PM on Friday",
    "the price is 12 dollars 75 cents",
    "the flight number is BA 123",
    "the order total is 89 dollars 99 cents",
    "the phone number is 555 987 6543",
    "the meeting is at 11:15 AM",
    "the value of e is 2.718",
    "the invoice is 3,450 dollars",
    "the room is number 1204",
    "the ratio is 3.75 to 2",
    "it costs 15 cents",
    "the version is 1.4.2",
    "the date is January 2, 2027",
    "the number is 0.25",
    "the score was 5 to 4",
    "the deadline is 5:00 PM",
]

NEGATIVE = [
    "the board meeting moved to the third floor",
    "I need to finish the report before the end of the day",
    "the weather was quite nice this morning",
    "she said the project would take a few more weeks",
    "we should probably review the numbers again",
    "the coffee in the break room is running low",
    "he mentioned that the client wanted changes",
    "the train was delayed by about twenty minutes",
    "I am looking forward to the weekend",
    "the presentation went better than expected",
    "she asked me to bring the documents to the office",
    "the restaurant was almost full when we arrived",
    "I have been meaning to call my mother for a while",
    "the meeting started a little later than planned",
    "we need to decide on a name for the new product",
    "the road to the airport was completely blocked",
    "he promised to send the files by tomorrow morning",
    "the library closes at nine in the evening",
    "I forgot my umbrella and ended up getting soaked",
    "the team celebrated after the deadline was met",
    "she recommended a book about machine learning",
    "the parking garage was full so we walked a few blocks",
    "I think the new hire is doing a great job",
    "the flight landed exactly on time this once",
    "we should order lunch before the next meeting",
    "the concert tickets sold out within an hour",
    "he drove to the city to pick up the package",
    "the garden looked beautiful after the rain",
    "I would prefer a window seat if possible",
    "the deadline was moved up by one week",
    "she took a long walk to clear her head",
    "the new policy takes effect at the start of next month",
    "I ran into an old colleague at the grocery store",
    "the movie turned out to be a bit longer than expected",
    "we will need more chairs for the gathering on Saturday",
    "the kitchen smelled like toast this morning",
    "she watered the plants before leaving for work",
    "the bus arrived a few minutes early today",
    "I left my keys on the counter again",
    "the store was closed for the holiday",
    "he cooked dinner for the whole family",
    "the office was quiet after everyone left",
    "I bought apples and bread at the market",
    "the dog waited by the door for the mail",
    "we watched the game on the big screen",
    "the soup was too hot to eat right away",
    "she folded the letters into small squares",
    "the road was covered in fallen leaves",
    "I saved a copy of the file to the desktop",
    "the train station was crowded at rush hour",
]

# (category, [(text, expected), ...])
CATEGORIES = [
    ("open_app", [(t, {"type": "open_app", "tool": "open_app", "args": {"app": t[5:].strip()}}) for t in OPEN_APP]),
    ("edit_verb", [
        ("make this sentence shorter", None),
        ("make this paragraph longer", None),
        ("bold this word", None),
        ("italicize this word", None),
        ("underline the selected text", None),
        ("fix the grammar", None),
        ("fix the spelling", None),
        ("replace your company with IC Markets", None),
        ("replace the date with October 17", None),
        ("turn this into bullet points", None),
        ("turn this into a table", None),
        ("number the list", None),
        ("capitalize the first letter of each sentence", None),
        ("make this all lowercase", None),
        ("make this all uppercase", None),
        ("delete the last sentence", None),
        ("delete the selected text", None),
        ("insert a new paragraph", None),
        ("move this paragraph up", None),
        ("align this text to the center", None),
        ("remove the typos", None),
        ("rewrite this in short sentences", None),
        ("make this more formal", None),
        ("make this more casual", None),
        ("add a comma after the first word", None),
        ("fix the punctuation", None),
        ("summarize this paragraph", None),
        ("expand this into a full sentence", None),
        ("highlight this word", None),
        ("strikethrough this word", None),
    ]),
    ("focus_control", [
        ("focus the browser", None),
        ("focus email", None),
        ("bring up the terminal", None),
        ("bring up the calendar", None),
        ("bring up the calculator", None),
        ("bring up the notes app", None),
        ("switch to the browser", None),
        ("switch to email", None),
        ("switch to the terminal", None),
        ("switch to the music app", None),
        ("focus the code editor", None),
        ("focus the messenger", None),
        ("bring up the settings", None),
        ("bring up the file manager", None),
        ("bring up the photo app", None),
        ("bring up the map app", None),
        ("switch to the messages app", None),
        ("switch to the video call app", None),
        ("focus the spreadsheet", None),
        ("focus the presentation", None),
        ("focus the web browser", None),
    ]),
    ("pointer", [
        ("click Save", None),
        ("click the submit button", None),
        ("click the close button", None),
        ("click the search icon", None),
        ("double click the file", None),
        ("double click the folder", None),
        ("right click the icon", None),
        ("type hello", None),
        ("type good morning team", None),
        ("press enter", None),
        ("press the escape key", None),
        ("press the tab key", None),
        ("scroll down the page", None),
        ("scroll up the page", None),
        ("scroll to the top", None),
        ("click the back button", None),
        ("click the play button", None),
        ("double click the desktop icon", None),
        ("right click the file name", None),
        ("type the word hello", None),
        ("press the return key", None),
        ("press the space bar", None),
        ("scroll to the bottom", None),
    ]),
    ("itn_sensitive", [
        ("the price is 42 dollars 50 cents", None),
        ("the meeting is at 2:30 PM", None),
        ("the value of pi is 3.14", None),
        ("call me at 555 123 4567", None),
        ("the flight leaves at 6:45 AM", None),
        ("the temperature is 21.5 degrees", None),
        ("set the timer for 45 seconds", None),
        ("the bill was 1,250 euros", None),
        ("room number 402", None),
        ("the ratio is 2.5 to 1", None),
        ("it costs 99 cents", None),
        ("the version is 3.10", None),
        ("the date is March 14, 2026", None),
        ("the number is 0.001", None),
        ("the score was 3 to 2", None),
        ("the meeting is at 2:30 PM on Friday", None),
        ("the price is 12 dollars 75 cents", None),
        ("the flight number is BA 123", None),
        ("the order total is 89 dollars 99 cents", None),
        ("the phone number is 555 987 6543", None),
        ("the meeting is at 11:15 AM", None),
        ("the value of e is 2.718", None),
        ("the invoice is 3,450 dollars", None),
        ("the room is number 1204", None),
        ("the ratio is 3.75 to 2", None),
        ("it costs 15 cents", None),
        ("the version is 1.4.2", None),
        ("the date is January 2, 2027", None),
        ("the number is 0.25", None),
        ("the score was 5 to 4", None),
        ("the deadline is 5:00 PM", None),
    ]),
    ("negative", [(t, None) for t in NEGATIVE]),
]

# Per-category deterministic pitch (Hz) and rate (wpm) for `say`.
# Command categories use a steady, neutral voice; negatives vary slightly
# to sound like natural dictation.
PITCH_RATE = {
    "open_app": (120, 180),
    "edit_verb": (115, 175),
    "focus_control": (125, 180),
    "pointer": (120, 185),
    "itn_sensitive": (118, 170),
    "negative": (110, 165),
}

# Five distinct macOS `say` voices used round-robin across clips
# (Addendum A.5: >= 5 voices so a single-voice artifact cannot gate).
VOICES = ["Alex", "Fred", "Samantha", "Karen", "Moira"]

SAMPLE_RATE = 16000

def say_to_wav(text: str, out_path: Path, pitch: int, rate: int, voice: str) -> None:
    """Synthesize `text` with macOS `say` and write 16 kHz mono PCM WAV."""
    # `say` outputs 22050 Hz AIFF by default. Capture to a temp file,
    # resample + downmix, write raw WAV.
    aiff = out_path.with_suffix(".aiff.tmp")

    def _aiff_has_audio(blob: bytes) -> bool:
        """True if the AIFF/AIFF-C blob carries an SSND chunk with data.

        `say` occasionally exits 0 but writes a header-only file (no SSND)
        under load or right after idle; retry on that.
        """
        pos = 12
        while pos + 8 <= len(blob):
            cid = blob[pos:pos + 4]
            size = int.from_bytes(blob[pos + 4:pos + 8], "big")
            if cid == b"SSND" and size > 8:
                return True
            pos = pos + 8 + size + (size & 1)
        return False

    data = b""
    for _attempt in range(3):
        try:
            subprocess.run(
                ["say", "-v", voice, "-r", str(rate), "-o", str(aiff), text],
                check=True,
                capture_output=True,
            )
        except FileNotFoundError:
            raise SystemExit("ERROR: `say` is not available on this system")
        except subprocess.CalledProcessError as e:
            raise SystemExit(f"ERROR: `say` failed for {text!r}: {e.stderr.decode(errors='replace')}")
        data = aiff.read_bytes()
        if _aiff_has_audio(data):
            break
        aiff.unlink(missing_ok=True)
        time.sleep(0.5)
    if not _aiff_has_audio(data):
        raise SystemExit(f"ERROR: `say` wrote corrupt (header-only) AIFF for {text!r}")
    aiff.unlink(missing_ok=True)

    # AIFF header: COMM chunk carries sample rate + channels + sample size.
    # Find COMM chunk.
    pos = 12
    sample_rate: int = 0
    channels: int = 1
    sample_size: int = 16
    snd_data: bytes = b""
    while pos + 8 <= len(data):
        chunk_id = data[pos:pos + 4]
        chunk_size = int.from_bytes(data[pos + 4:pos + 8], "big")
        body = pos + 8
        if chunk_id == b"COMM":
            # COMM layout (AIFF spec):
            #   numChannels(2)  sampleCount(4)  sampleRate(10: 80-bit
            #   IEEE 754 extended precision)  compressionType(2)
            #   sampleSize(2)  [padding to even]
            # macOS `say` leaves compressionType/sampleSize zeroed;
            # 16-bit PCM is the only format it emits.
            channels = int.from_bytes(data[body:body + 2], "big")
            # 80-bit extended float: 16-bit biased exponent (bias 16383,
            # sign in bit 15) + 64-bit mantissa (bit 63 = explicit 1).
            raw_exp = int.from_bytes(data[body + 8:body + 10], "big")
            raw_man = int.from_bytes(data[body + 10:body + 18], "big")
            sign = -1.0 if raw_exp & 0x8000 else 1.0
            exp = raw_exp & 0x7FFF
            if exp == 0:
                sample_rate = 0
            elif raw_man & 0x8000000000000000:
                frac = (raw_man & 0x7FFFFFFFFFFFFFFF) / float(1 << 63)
                sample_rate = int(sign * (1.0 + frac) * 2.0 ** (exp - 16383) + 0.5)
            else:
                sample_rate = int(sign * (raw_man / float(1 << 63)) * 2.0 ** (exp - 16383 - 63) + 0.5)
            sample_size = 16  # macOS `say` always emits 16-bit PCM
        elif chunk_id == b"SSND":
            # SSND: 4-byte offset + 4-byte balance, then samples.
            snd_data = data[body + 8:body + 8 + chunk_size]
        pos = body + chunk_size + (chunk_size & 1)

    if sample_rate == 0 or not snd_data:
        raise SystemExit(f"ERROR: could not parse AIFF for {text!r}")

    # Convert samples to int list (AIFF is big-endian).
    if sample_size == 16:
        n = len(snd_data) // 2
        samples = [int.from_bytes(snd_data[i * 2:i * 2 + 2], "big", signed=True) for i in range(n)]
    elif sample_size == 8:
        samples = [b - 128 for b in snd_data]
    else:
        raise SystemExit(f"ERROR: unsupported AIFF sample size {sample_size}")

    # Downmix to mono.
    if channels > 1:
        mono = []
        for i in range(0, len(samples) - (len(samples) % channels), channels):
            chunk = samples[i:i + channels]
            mono.append(sum(chunk) // channels)
        samples = mono

    # Resample to 16 kHz (linear interpolation).
    if sample_rate != SAMPLE_RATE:
        ratio = SAMPLE_RATE / sample_rate
        out_len = int(len(samples) * ratio)
        resampled = [0] * out_len
        for i in range(out_len):
            src = i / ratio
            i0 = int(src)
            i1 = min(i0 + 1, len(samples) - 1)
            frac = src - i0
            resampled[i] = int(samples[i0] * (1 - frac) + samples[i1] * frac)
        samples = resampled

    # Clamp to int16.
    pcm = b"".join(struct.pack("<h", max(-32768, min(32767, s))) for s in samples)

    # Write WAV header + PCM.
    n = len(pcm)
    header = struct.pack(
        "<4sI4s", b"RIFF", 36 + n, b"WAVE"
    ) + struct.pack(
        "<4sIHHIIHH",
        b"fmt ", 16, 1, 1, SAMPLE_RATE, SAMPLE_RATE * 2, 2, 16,
    ) + struct.pack(
        "<4sI", b"data", n,
    )
    out_path.write_bytes(header + pcm)


def clip_filename(idx: int, category: str) -> str:
    return f"{category}_{idx:03d}.wav"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", required=True, help="Output directory for wavs + corpus.json")
    ap.add_argument("--subset", default=None, help="Comma-separated category subset (default: all)")
    args = ap.parse_args()

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    subsets = None
    if args.subset:
        subsets = {s.strip() for s in args.subset.split(",") if s.strip()}

    clips = []
    counts: dict[str, int] = {}
    voice_counts: dict[str, int] = {v: 0 for v in VOICES}
    skipped = 0
    generated = 0
    global_idx = 0

    for category, utterances in CATEGORIES:
        if subsets and category not in subsets:
            continue
        cat_count = 0
        for idx, (text, expected) in enumerate(utterances):
            fname = clip_filename(idx, category)
            fpath = out_dir / fname
            voice = VOICES[global_idx % len(VOICES)]
            if fpath.exists():
                skipped += 1
            else:
                pitch, rate = PITCH_RATE.get(category, (120, 175))
                say_to_wav(text, fpath, pitch, rate, voice)
                generated += 1
            voice_counts[voice] += 1
            clips.append({
                "path": f"commands/corpus/{fname}",
                "text": text,
                "category": category,
                "voice": voice,
                "expected": expected,
            })
            cat_count += 1
            global_idx += 1
        counts[category] = cat_count

    manifest = {"schema": SCHEMA, "clips": clips}
    manifest_path = out_dir / "corpus.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")

    print(f"Generated {generated} new clips, skipped {skipped} existing.")
    for cat, n in counts.items():
        print(f"  {cat}: {n}")
    for v, n in voice_counts.items():
        print(f"  voice {v}: {n}")
    print(f"Total: {len(clips)} clips -> {manifest_path}")


if __name__ == "__main__":
    main()
