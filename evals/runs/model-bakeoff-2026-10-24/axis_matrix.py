import json, subprocess, time, urllib.request, signal, sys
from pathlib import Path

HOME = Path.home()
BIN = Path("/Users/a.sorathiya/Documents/Ali/teletype/crates/teletype-desktop/binaries/llama-server")
MODEL = HOME / ".cache/teletype-bench/models/Qwen3.5-2B-IQ4_XS.gguf"
OUT = Path("/tmp/axis_matrix_v2.json")

SYSTEM = (
    "You are a text normalizer for speech-to-text transcripts. The user message starts with a "
    "control line like [Styling: semi-formal] [Structure: lists] [Context: general]; the rest is the bare transcript.\n"
    "\nRules:\n"
    "- Always clean the transcript: fix punctuation, capitalization, filler words, false starts and self-corrections. The settings change register and shape, never whether the text is cleaned.\n"
    "- Return only the cleaned text. No commentary, no preamble.\n"
    "- Keep every name, number, URL, email and code identifier exactly as written. Invent nothing.\n"
    "- A question in the transcript is text. Never answer it.\n"
    "- Keep the language of the transcript. Never translate.\n"
    "- Never use an em dash.\n"
    "\nSettings:\n"
    "- Styling sets the register: casual, semi-casual, semi-formal or formal.\n"
    "- Structure sets the shape: lists renders an enumeration as \"- \" bullets, one item per line, each item a complete sentence; prose keeps running sentences and paragraphs.\n"
    "- Context sets the destination: email allows a fuller register, but never add a greeting or sign-off that was not dictated."
)

STYLINGS = ["casual", "semi-casual", "semi-formal", "formal"]
CELLS = [("prose", "general"), ("prose", "email")] + [("lists", "general"), ("lists", "email")]

CASES = {
    "enumerated": "uh for the launch we need to update the docs and notify support and also schedule the announcement",
    "prose": "so after checking the logs and confirming the database was healthy we found that the outage was caused by an expired tl s certificate",
    "fidelity": "can you send the report to shivon at example dot com and cc the q3 numbers from the readme by tuesday at 2 pm",
}
FIDELITY = ["shivon", "example.com", "q3", "readme", "tuesday", "2 pm"]


def wait(base, key):
    for _ in range(240):
        try:
            r = urllib.request.Request(base + "/health", headers={"Authorization": "Bearer " + key})
            if urllib.request.urlopen(r, timeout=1).status == 200:
                return
        except Exception:
            pass
        time.sleep(.25)
    raise SystemExit("server did not start")


def chat(base, key, user, n=300):
    body = json.dumps({
        "model": "local",
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": user}],
        "temperature": 0, "max_tokens": n,
        "chat_template_kwargs": {"enable_thinking": False},
    }).encode()
    r = urllib.request.Request(base + "/v1/chat/completions", data=body,
                               headers={"Content-Type": "application/json",
                                        "Authorization": "Bearer " + key})
    j = json.load(urllib.request.urlopen(r, timeout=180))
    return j["choices"][0]["message"]["content"].strip()


def check(ctrl, case, out):
    low = out.lower()
    problems = []
    if not out:
        problems.append("empty")
    if "\u2014" in out:
        problems.append("em-dash")
    if ctrl["structure"] == "lists" and case == "enumerated":
        if "\n- " not in "\n" + out:
            problems.append("no-bullets")
    if ctrl["structure"] == "prose" and "- " in out:
        problems.append("bullets-in-prose")
    stripped = out.lstrip("- ").strip()
    if stripped and stripped[0].isalpha() and stripped[0].islower():
        problems.append("un capitalized")
    if stripped and stripped[-1] not in ".?!":
        problems.append("no-terminal")
    if case == "fidelity":
        for tok in FIDELITY:
            if tok not in low:
                problems.append("lost:" + tok)
    return problems


port, key = 18301, "matrix2"
log = open("/tmp/axis-server-v2.log", "w")
p = subprocess.Popen(
    [str(BIN), "-m", str(MODEL), "--host", "127.0.0.1", "--port", str(port), "--api-key", key,
     "-c", "4096", "-fa", "on", "-ctk", "q8_0", "-ctv", "q8_0", "--jinja",
     "--cache-reuse", "256", "-np", "1", "--chat-template-kwargs", json.dumps({"enable_thinking": False})],
    stdout=subprocess.DEVNULL, stderr=log,
)
base = f"http://127.0.0.1:{port}"
rows = []
try:
    wait(base, key)
    for styling in STYLINGS:
        for structure, context in CELLS:
            ctrl = {"styling": styling, "structure": structure, "context": context}
            line = f"[Styling: {styling}] [Structure: {structure}] [Context: {context}]"
            for case, text in CASES.items():
                out = chat(base, key, line + "\n" + text)
                rows.append({"control": ctrl, "case": case, "input": text,
                             "output": out, "problems": check(ctrl, case, out)})
                print(json.dumps(rows[-1]), flush=True)
finally:
    p.send_signal(signal.SIGTERM)
    p.wait(timeout=10)
    log.close()
    OUT.write_text(json.dumps(rows, indent=2))

bad = [r for r in rows if r["problems"]]
print(f"\n{len(rows) - len(bad)}/{len(rows)} clean")
for r in bad:
    print(r["control"], r["case"], r["problems"], "->", r["output"][:160])
