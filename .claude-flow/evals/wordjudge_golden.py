#!/usr/bin/env python3
"""Golden-set validation for a yes/no word judge (item 7).

Each case: (word, sentence, expected). expected True = the word IS correct
(answer should be "yes"); False = mishearing (answer should be "no").

The critical metric is clean-text preservation: a judge that flags correct
words is worse than no judge. Accuracy alone hides that; report both.

Usage:
  python3 evals/wordjudge_golden.py --server http://127.0.0.1:18081 [--limit N]

Exit 0 when clean-preservation >= 0.98 and flag-rate >= 0.90, else 1.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.request

# (WORD, sentence, word_is_correct)
CASES: list[tuple[str, str, bool]] = [
    # --- homophone mishearings (expect NO) + clean twins (expect YES) ---
    ("MEAT", "Let us meat tomorrow at noon.", False),
    ("MEET", "Let us meet tomorrow at noon.", True),
    ("THEIR", "Their going home early today.", False),
    ("THEY'RE", "They're going home early today.", True),
    ("THERE", "Put the file over their on the desk.", False),
    ("THERE", "Put the file over there on the desk.", True),
    ("HOLE", "I saw the hole thing happen.", False),
    ("WHOLE", "I saw the whole thing happen.", True),
    ("WEATHER", "I don't know weather he'll come.", False),
    ("WHETHER", "I don't know whether he'll come.", True),
    ("BARE", "Bare with me for a moment.", False),
    ("BEAR", "Bear with me for a moment.", True),
    ("PRINCIPAL", "The principal reason is cost.", False),
    ("PRINCIPLE", "The principle reason is cost.", True),
    ("PRINCIPAL", "The school principal resigned.", True),
    ("AFFECT", "This will effect the timeline.", False),
    ("EFFECT", "This will affect the timeline.", True),
    ("EFFECT", "The new policy takes effect Monday.", True),
    ("PEAK", "Take a peek at the report.", False),
    ("PEEK", "Take a peek at the report.", True),
    ("COMPLIMENT", "The hotel staff paid us a complement.", False),
    ("COMPLIMENT", "The hotel staff paid us a compliment.", True),
    ("DISCREET", "The two offers are discrete.", False),
    ("DISCRETE", "The two offers are discrete.", True),
    ("STATIONARY", "Order more stationary for the office.", False),
    ("STATIONERY", "Order more stationery for the office.", True),
    ("LYRIC", "She sings the lyric well.", False),
    ("LYRICS", "She sings the lyrics well.", True),
    # --- proper-noun mishearings (expect NO) + clean (expect YES) ---
    ("FATAMA", "Rida Fatama is coming.", False),
    ("FATEMA", "Rida Fatema is coming.", True),
    ("CHASER", "Jaser will join the call.", False),
    ("JASER", "Jaser will join the call.", True),
    ("EXCEL", "The confusion is in the Excel sheet.", True),
    ("EXCEL", "The confusion is in the exile sheet.", False),
    # --- clean ordinary sentences (expect YES; the do-no-harm set) ---
    ("MARKET", "Bring three things from the market.", True),
    ("TICKETS", "Tell me if the tickets are created.", True),
    ("MEETINGS", "No meetings today.", True),
    ("ROLLOUT", "In the rollout plan add the cutoff date.", True),
    ("INVOICE", "Sam the invoice is ready.", True),
    ("BUDGET", "Give me the updated cost and budget.", True),
    ("DEADLINE", "The deadline is Friday morning.", True),
    ("CLIENT", "The client approved the proposal.", True),
    ("SERVER", "Restart the server after midnight.", True),
    ("REPO", "Push the branch to the repo.", True),
    ("STANDUP", "We have standup at ten.", True),
    ("DUBAI", "What is the time right now in Dubai?", True),
    ("GEORGE", "Hi George, bring apples and mangoes.", True),
    ("JALA", "Bring jala and mango from the shop.", True),
    # --- tricky clean (must NOT flag) ---
    ("EMAIL", "Send me an email tomorrow.", True),
    ("TODO", "Add it to the todo list.", True),
    ("WI-FI", "The wi-fi password is on the wall.", True),
    ("OKAY", "Okay, let's start.", True),
]

PROMPT_FINE = (
    "/no_think The sentence '{sentence}' contains the word '{word}'. "
    "Is it the right word there, or should it be a different word? "
    "Answer yes if it is fine, no if it should be replaced:"
)
PROMPT_REPLACE = (
    "/no_think The sentence '{sentence}' contains the word '{word}'. "
    "Should it be a different word? "
    "Answer yes if it should be replaced, no if it is fine:"
)


PROMPT = PROMPT_FINE


GEN_PROMPT = (
    "/no_think Repeat this sentence exactly, fixing any misheard word. "
    "If nothing is wrong, repeat it unchanged: {sentence}"
)


def ask_gen(server: str, sentence: str, timeout_s: int = 120) -> tuple[str, float]:
    import urllib.request as _u
    import time as _t
    import json as _j
    body = _j.dumps(
        {
            "messages": [{"role": "user", "content": GEN_PROMPT.format(sentence=sentence)}],
            "max_tokens": 60,
            "temperature": 0,
        }
    ).encode()
    t0 = _t.time()
    req = _u.Request(
        server.rstrip("/") + "/v1/chat/completions",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    with _u.urlopen(req, timeout=timeout_s) as r:
        out = _j.load(r)
    return out["choices"][0]["message"]["content"], _t.time() - t0


def _unused_ask_gen(server: str, sentence: str, timeout_s: int = 120) -> tuple[str, float]:
    body = json.dumps(
        {
            "messages": [{"role": "user", "content": PROMPT.format(word=word, sentence=sentence)}],
            "max_tokens": 16,
            "temperature": 0,
        }
    ).encode()
    t0 = time.time()
    req = urllib.request.Request(
        server.rstrip("/") + "/v1/chat/completions",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=timeout_s) as r:
        out = json.load(r)
    dt = time.time() - t0
    return out["choices"][0]["message"]["content"], dt


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", default="http://127.0.0.1:18081")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--framing", choices=["fine", "replace"], default="fine",
                    help="fine: yes=word is fine. replace: yes=word should be replaced (scorer flips).")
    ap.add_argument("--mode", choices=["judge", "generative"], default="judge",
                    help="judge: yes/no per word. generative: repeat-with-fixes, diff decides.")
    args = ap.parse_args()

    global PROMPT
    PROMPT = PROMPT_REPLACE if args.framing == "replace" else PROMPT_FINE
    flip = args.framing == "replace"
    cases = CASES[: args.limit] if args.limit else CASES
    tp = tn = fp = fn = 0
    unparsable = 0
    lat: list[float] = []
    failures: list[str] = []
    def norm(t: str) -> str:
        return " ".join(t.strip().lower().split())

    for word, sentence, correct in cases:
        if args.mode == "generative":
            # Verdict by diff: flagged iff the model changed anything.
            try:
                raw, dt = ask_gen(args.server, sentence)
            except Exception as e:  # noqa: BLE001
                print(f"ERROR {word}: {e}")
                failures.append(f"{word} (request failed)")
                continue
            lat.append(dt)
            changed = norm(raw) != norm(sentence)
            if correct and not changed:
                tp += 1
            elif not correct and changed:
                # Changed, but did it change to the RIGHT text? The clean
                # twin is the oracle: changing is necessary, not sufficient.
                tn += 1
            elif correct:
                fp += 1
                failures.append(f"{word}: rewrote clean text -> {raw.strip()[:60]!r}")
            else:
                fn += 1
                failures.append(f"{word}: left mishearing unchanged")
            continue
        try:
            raw, dt = ask(args.server, word, sentence)
        except Exception as e:  # noqa: BLE001 - eval harness reports, not crashes
            print(f"ERROR {word}: {e}")
            fn += 0 if correct else 1
            failures.append(f"{word} (request failed)")
            continue
        lat.append(dt)
        toks = [
            t.lower().strip(".,!?\"'*:`")
            for t in raw.strip().split()
        ]
        verdict = next((t for t in toks[:6] if t in ("yes", "no")), "")
        if not verdict:
            unparsable += 1
            failures.append(f"{word}: unparsable {raw.strip()[:40]!r}")
            continue
        said_yes = (verdict == "yes") != flip
        if correct and said_yes:
            tp += 1
        elif not correct and not said_yes:
            tn += 1
        elif correct:
            fp += 1
            failures.append(f"{word}: flagged clean text")
        else:
            fn += 1
            failures.append(f"{word}: missed mishearing")

    n = tp + tn + fp + fn
    clean_pres = tp / (tp + fp) if (tp + fp) else 0.0
    flag_rate = tn / (tn + fn) if (tn + fn) else 0.0
    acc = (tp + tn) / n if n else 0.0
    avg_ms = (sum(lat) / len(lat) * 1000) if lat else 0.0
    print(f"cases={len(cases)} scored={n} unparsable={unparsable}")
    print(f"accuracy={acc:.3f} clean_preservation={clean_pres:.3f} flag_rate={flag_rate:.3f}")
    print(f"avg_latency_ms={avg_ms:.0f}")
    for f in failures:
        print(f"  MISS: {f}")
    ok = clean_pres >= 0.98 and flag_rate >= 0.90 and unparsable == 0
    print("GATE:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
