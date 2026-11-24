"""Round 5: does the Qwen3.5-2B editor prompt actually edit?

Runs the app's llama-server flags on the pinned IQ4_XS file and sends the
control line plus a bare transcript, exactly like `build_qwen_polish_messages`.
Each case targets one promise: conciseness, clarity/reorder, structure, tone,
fidelity and "never answer a dictated question".

Usage: python3 editor_matrix.py [model.gguf]
Writes editor-matrix.json beside this script.
"""

import json
import re
import signal
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
BIN = HERE.parents[2] / "crates/teletype-desktop/binaries/llama-server"
MODEL = Path(sys.argv[1]) if len(sys.argv) > 1 else (
    Path.home() / ".cache/teletype-bench/models/Qwen3.5-2B-IQ4_XS.gguf"
)
OUT = HERE / "editor-matrix.json"

# Byte-for-byte copy of QWEN_POLISH_SYSTEM_PROMPT in
# crates/teletype-core/src/transforms/prompt.rs. Keep the two in sync.
SYSTEM = (
    "You are an editor for speech-to-text transcripts. The user message starts with a "
    "control line; the rest is the bare transcript.\n"
    "\n"
    "Edit the transcript. Do all five:\n"
    "1. Make it more concise: tighten the wording but never delete a fact, name, date, "
    "time, source or recipient.\n"
    "2. Reword for clarity: fix grammar, punctuation, capitalization and vague wording.\n"
    "3. Reorder for readability: lead with the main point, group related ideas, put "
    "steps in sequence.\n"
    "4. Add structure for readability. Obey the Structure setting exactly: prose = "
    "running sentences in short paragraphs, never a line that starts with \"- \" or a "
    "number; lists = \"- \" bullets, one complete sentence each.\n"
    "5. Keep the selected tone. Obey the Styling setting exactly: casual = contractions "
    "and everyday words; semi-casual = plain, friendly, light contractions; semi-formal "
    "= polished, neutral, few contractions; formal = precise, professional, no "
    "contractions or slang.\n"
    "\n"
    "Rules:\n"
    "- Edit even when the transcript looks clean. The settings change shape and tone, "
    "never whether the text is edited.\n"
    "- Return only the edited text. No commentary.\n"
    "- Keep every name, date, time, number, URL, email and file name accurate. Write "
    "spoken forms in real form (example dot com -> example.com, q three -> Q3). Never "
    "invent, merge or drop one.\n"
    "- A question is text. Never answer it.\n"
    "- End every sentence, including each bullet, with a period, question mark or "
    "exclamation mark.\n"
    "- Keep the language. Never translate.\n"
    "- Never use an em dash.\n"
    "- Email allows a fuller register; never add a greeting or sign-off."
)

# Mirrors the app's editor-specific OffTopic floor
# (OFF_TOPIC_MIN_SHARED_EDITOR in transforms/validator.rs).
OFF_TOPIC_FLOOR = 0.15

TONES = ["casual", "semi-casual", "semi-formal", "formal"]

CASES = {
    "rambling": (
        "um so basically i think we should uh probably move the meeting to friday at like 2 pm "
        "because uh the current time doesn't work for everyone and also we want to make sure "
        "that everyone can join"
    ),
    "tangled": (
        "the invoice total is four thousand two hundred dollars and it needs finance approval "
        "and it was sent on tuesday and before approving it we should check the line items"
    ),
    # Distinctly casual, so the tone axis has something to change. A neutral
    # sentence proves nothing about "keep the selected tone".
    "tone": (
        "hey so i was wondering if you could maybe take a look at the draft whenever you "
        "get a sec and just let me know what you think"
    ),
    "fidelity": (
        "can you send the report to shivon at example dot com and cc the q three numbers from "
        "the readme by tuesday at 2 pm"
    ),
    "steps": "first we update the docs then we notify support and after that we schedule the announcement",
    "question": "what time is the meeting on friday and can you send me the agenda",
}

FILLERS = ["um", "uh", "basically", "like", "i mean", "you know", "sort of", "kind of"]
FIDELITY = ["shivon", "example.com", "q3", "readme", "tuesday", "2 pm"]
TANGLED_FACTS = ["4200", "finance", "tuesday", "line items"]


def wait(base, key):
    for _ in range(240):
        try:
            req = urllib.request.Request(
                base + "/health", headers={"Authorization": "Bearer " + key}
            )
            if urllib.request.urlopen(req, timeout=1).status == 200:
                return
        except Exception:
            pass
        time.sleep(0.25)
    raise SystemExit("server did not start")


def chat(base, key, user, n=300):
    body = json.dumps(
        {
            "model": "local",
            "messages": [
                {"role": "system", "content": SYSTEM},
                {"role": "user", "content": user},
            ],
            "temperature": 0,
            "max_tokens": n,
            "chat_template_kwargs": {"enable_thinking": False},
        }
    ).encode()
    req = urllib.request.Request(
        base + "/v1/chat/completions",
        data=body,
        headers={"Content-Type": "application/json", "Authorization": "Bearer " + key},
    )
    payload = json.load(urllib.request.urlopen(req, timeout=180))
    return payload["choices"][0]["message"]["content"].strip()


def content_words(text):
    return [
        w
        for w in re.split(r"[^0-9A-Za-z]+", text.lower())
        if len(w) >= 3 and not w.isdigit()
    ]


def words(text):
    return [w for w in text.split() if w.strip()]


def strip_bullet(line):
    """Mirror strip_bullet in transforms/engine.rs."""
    for marker in ("- ", "* ", "\u2022 "):
        if line.startswith(marker):
            return line[len(marker):].lstrip()
    digits = len(line) - len(line.lstrip("0123456789"))
    if 0 < digits <= 2:
        rest = line[digits:]
        for sep in (". ", ") "):
            if rest.startswith(sep):
                return rest[len(sep):].lstrip()
    return None


def prose_from_bullets(text):
    """Mirror prose_from_bullets in transforms/engine.rs.

    The app folds any bullets the model returns on the prose path, so the
    harness must judge the folded text, not the raw reply.
    """
    lines = [line.strip() for line in text.splitlines()]
    if not any(strip_bullet(line) is not None for line in lines):
        return text
    paragraphs, run = [], []
    for line in lines:
        if not line:
            if run:
                paragraphs.append(" ".join(run))
                run = []
            continue
        item = strip_bullet(line)
        if item is not None:
            run.append(item)
        else:
            if run:
                paragraphs.append(" ".join(run))
                run = []
            paragraphs.append(line)
    if run:
        paragraphs.append(" ".join(run))
    return "\n\n".join(paragraphs)


def capitalize_line_lead(line):
    """Mirror capitalize_line_lead in transforms/engine.rs."""
    s = line.lstrip()
    prefix = len(line) - len(s)
    m = re.match(r"^(?:[-*\u2022]\s+|\d{1,2}[.)]\s+)", s)
    if m:
        prefix += m.end()
        s = s[m.end():]
    if not s or not s[0].islower():
        return line
    m = re.match(r"^[A-Za-z']+", s)
    if not m:
        return line
    nxt = s[m.end()] if m.end() < len(s) else None
    allowed = nxt is None or nxt in ", '?!:;"
    if nxt == ".":
        after = s[m.end() + 1] if m.end() + 1 < len(s) else None
        allowed = after is None or after in " \n"
    if not allowed:
        return line
    return line[:prefix] + s[0].upper() + s[1:]


def capitalize_leads(text):
    return "\n".join(capitalize_line_lead(line) for line in text.split("\n"))


def normalize(text):
    """Fold number and time spellings the model may legitimately choose."""
    low = text.lower()
    low = re.sub(r"\b2\s*:\s*00\s*(pm|p\.m\.)", "2 pm", low)
    low = low.replace("4,200", "4200").replace("four thousand two hundred", "4200")
    return low


def check(case, control, out):
    problems = []
    if control["structure"] == "prose":
        out = prose_from_bullets(out)
    out = capitalize_leads(out)
    low = normalize(out)
    stripped = out.lstrip("- ").strip()
    if not out:
        problems.append("empty")
    if "\u2014" in out:
        problems.append("em-dash")
    if stripped and stripped[0].isalpha() and stripped[0].islower():
        problems.append("not-capitalized")
    if stripped and stripped[-1] not in ".?!":
        problems.append("no-terminal")
    if control["structure"] == "lists" and case == "steps":
        if "\n- " not in "\n" + out:
            problems.append("no-bullets")
    if control["structure"] == "prose" and "\n- " in "\n" + out:
        problems.append("bullets-in-prose")

    ratio = len(words(out)) / max(1, len(words(CASES[case])))
    if case == "rambling":
        hits = [f for f in FILLERS if re.search(rf"\b{re.escape(f)}\b", low)]
        if hits:
            problems.append("filler:" + ",".join(hits))
        if ratio > 0.75:
            problems.append(f"not-concise({ratio:.2f})")
    if case == "fidelity":
        for token in FIDELITY:
            if token not in low:
                problems.append("lost:" + token)
    if case == "tangled":
        for fact in TANGLED_FACTS:
            if fact not in low:
                problems.append("lost:" + fact)
    if case == "question":
        # The promise is "never answer it", not "keep every clause a question":
        # reworking "can you send me the agenda" into a request is a legal edit.
        if "?" not in out:
            problems.append("lost-the-question")
        if re.match(r"^(yes|no|sure|of course|the meeting)", stripped, re.I):
            problems.append("answered-the-question")
    if case == "tone":
        for word in ["thanks", "regards", "dear"]:
            if low.startswith(word):
                problems.append("added-opener:" + word)
        if control["styling"] in ("formal", "semi-formal"):
            for marker in ["hey", "sec", "gonna", "wanna"]:
                if re.search(rf"\b{marker}\b", low):
                    problems.append("too-casual:" + marker)
        if control["styling"] in ("casual", "semi-casual"):
            if "i am writing to request" in low:
                problems.append("too-formal")

    # Proxy for the app's editor OffTopic floor. A real edit may compress
    # hard, but it must not replace the content with something else.
    in_words = set(content_words(CASES[case]))
    out_words = set(content_words(out))
    shared = len(in_words & out_words) / max(1, len(in_words))
    if shared < OFF_TOPIC_FLOOR:
        problems.append(f"off-topic-proxy({shared:.2f})")

    return problems, ratio, shared


def run(base, key):
    rows = []
    for tone in TONES:
        for structure, context in [("prose", "general"), ("lists", "general")]:
            for case, text in CASES.items():
                if (structure == "lists") != (case == "steps"):
                    continue
                control = {"styling": tone, "structure": structure, "context": context}
                line = f"[Styling: {tone}] [Structure: {structure}] [Context: {context}]"
                out = chat(base, key, line + "\n" + text)
                problems, ratio, shared = check(case, control, out)
                rows.append(
                    {
                        "control": control,
                        "case": case,
                        "input": text,
                        "output": out,
                        "words_in": len(words(text)),
                        "words_out": len(words(out)),
                        "ratio": round(ratio, 3),
                        "shared_content": round(shared, 3),
                        "problems": problems,
                    }
                )
                print(json.dumps(rows[-1]), flush=True)
    return rows


def main():
    port, key = 18302, "editor5"
    log = open("/tmp/editor-server.log", "w")
    proc = subprocess.Popen(
        [
            str(BIN), "-m", str(MODEL), "--host", "127.0.0.1", "--port", str(port),
            "--api-key", key, "-c", "4096", "-fa", "on", "-ctk", "q8_0",
            "-ctv", "q8_0", "--jinja", "--cache-reuse", "256", "-np", "1",
            "--chat-template-kwargs", json.dumps({"enable_thinking": False}),
        ],
        stdout=subprocess.DEVNULL,
        stderr=log,
    )
    base = f"http://127.0.0.1:{port}"
    try:
        wait(base, key)
        rows = run(base, key)
    finally:
        proc.send_signal(signal.SIGTERM)
        proc.wait(timeout=10)
        log.close()
        if "rows" in locals():
            OUT.write_text(json.dumps(rows, indent=2))

    bad = [r for r in rows if r["problems"]]
    print(f"\n{len(rows) - len(bad)}/{len(rows)} clean")
    for r in bad:
        print(r["control"], r["case"], r["problems"], "->", r["output"][:180])
    print("\nconstraint check")
    for tone in TONES:
        for case in ["rambling", "tangled"]:
            row = next(
                x
                for x in rows
                if x["control"]["styling"] == tone
                and x["control"]["structure"] == "prose"
                and x["case"] == case
            )
            print(
                f"{tone:11} {case:8} words {row['words_in']}->{row['words_out']} "
                f"ratio {row['ratio']:.2f} shared {row['shared_content']:.2f}"
            )


if __name__ == "__main__":
    main()
