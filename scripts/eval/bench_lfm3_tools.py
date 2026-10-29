#!/usr/bin/env python3
"""Score LFM3-Audio-2B native tool calling on the command-suite corpus.

The 185-clip corpus (`scripts/eval/gen_command_clips.py`) is the same one the
NeuDecide / Needle command suite uses. This runner sends each clip as audio
to a resident `llama-server` (llama.cpp b11960+, `--jinja`, `--mmproj`) with
the app's shared 13-tool schema, then scores the reply with the suite's rules:

- a call is correct when the tool name matches and the argument object
  matches after trimming strings and dropping nulls (`score_clip` in
  `crates/teletype-decider/examples/command_suite.rs`);
- a null-expected clip is correct only when no call is produced;
- strict scoring is case-sensitive, like the Rust suite; the loose column
  also lowercases string values, because the executor resolves app names
  case-insensitively.

Usage:
    python3 scripts/eval/bench_lfm3_tools.py \
        --corpus evals/commands/corpus \
        --url http://127.0.0.1:8196 \
        --out evals/runs/command_suite_lfm3_2026-10-28/report.json
"""

from __future__ import annotations

import argparse
import base64
import json
import re
import statistics
import time
import urllib.error
import urllib.request
from pathlib import Path

TOOL_SYSTEM_PROMPT = (
    "Respond with function calls, and interleaved text and audio.\n\nList of tools:"
)
NONE_TOOL = {
    "name": "none",
    "description": "Return this when the utterance is not a command or no other tool fits.",
    "parameters": {"type": "object", "properties": {}},
}
NEEDLE_RS = (
    Path(__file__).resolve().parents[2]
    / "crates/teletype-inference/src/needle.rs"
)


def needle_constant(name: str, source: str | None = None) -> str:
    source = source or NEEDLE_RS.read_text(encoding="utf-8")
    rest = source.split(f"pub const {name}: &str = ", 1)[1]
    if rest.startswith('r#"'):
        return rest[3:].split('"#;', 1)[0]
    return rest[1:].split('";', 1)[0]


def load_tools() -> list[dict]:
    return json.loads(needle_constant("COMMAND_TOOLS_JSON"))


def norm_args(value, loose: bool):
    if isinstance(value, dict):
        out = {}
        for key, item in value.items():
            if item is None:
                continue
            out[key] = norm_args(item, loose)
        return out
    if isinstance(value, str):
        value = value.strip()
        return value.lower() if loose else value
    return value


def parse_tool_call(payload: dict):
    """Return (name, arguments) from an OpenAI reply, or None."""
    message = payload["choices"][0]["message"]
    calls = message.get("tool_calls") or []
    if calls:
        function = calls[0].get("function", {})
        raw = function.get("arguments") or "{}"
        try:
            arguments = json.loads(raw) if isinstance(raw, str) else raw
        except json.JSONDecodeError:
            return None
        return _named(function.get("name"), arguments)
    # A server build without tool parsing leaves the XML in content.
    content = message.get("content") or ""
    name = re.search(r"<function=([^>\s]+)>", content)
    if not name:
        return None
    arguments = {
        key: value.strip()
        for key, value in re.findall(
            r"<parameter=([^>\s]+)>\s*(.*?)\s*</parameter>", content, re.S
        )
    }
    return _named(name.group(1), arguments)


def _named(name, arguments):
    """The `none` sentinel is the model declining, not a tool call."""
    if not name or name == "none":
        return None
    return name, arguments


def expected_call(clip: dict):
    expected = clip.get("expected")
    if not expected:
        return None
    return expected.get("tool"), expected.get("args") or {}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", default="evals/commands/corpus")
    parser.add_argument("--url", default="http://127.0.0.1:8196")
    parser.add_argument("--out", required=True)
    parser.add_argument("--temperature", type=float, default=0.0)
    parser.add_argument("--max-tokens", type=int, default=512)
    parser.add_argument(
        "--system-prompt",
        default=None,
        help="override the system prompt (default: the model-card tool prompt)",
    )
    parser.add_argument(
        "--app-prompt",
        action="store_true",
        help="use the app's COMMAND_SYSTEM_PROMPT from needle.rs",
    )
    parser.add_argument(
        "--add-none-tool",
        action="store_true",
        help="offer a `none` tool, the same escape hatch the LLM fallback grammar has",
    )
    args = parser.parse_args()

    if args.system_prompt and args.app_prompt:
        parser.error("--system-prompt and --app-prompt are mutually exclusive")
    system_prompt = (
        args.system_prompt
        or (needle_constant("COMMAND_SYSTEM_PROMPT") if args.app_prompt else None)
        or TOOL_SYSTEM_PROMPT
    )

    corpus_dir = Path(args.corpus)
    clips = json.loads((corpus_dir / "corpus.json").read_text(encoding="utf-8"))["clips"]
    tool_defs = load_tools()
    if args.add_none_tool:
        tool_defs.append(NONE_TOOL)
    tools = [{"type": "function", "function": tool} for tool in tool_defs]

    rows = []
    started = time.time()
    for index, clip in enumerate(clips, start=1):
        audio = (corpus_dir.parent.parent / clip["path"]).read_bytes()
        body = json.dumps(
            {
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_audio",
                                "input_audio": {
                                    "data": base64.b64encode(audio).decode("ascii"),
                                    "format": "wav",
                                },
                            }
                        ],
                    },
                ],
                "tools": tools,
                "tool_choice": "auto",
                "temperature": args.temperature,
                "max_tokens": args.max_tokens,
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            f"{args.url}/v1/chat/completions",
            data=body,
            headers={"Content-Type": "application/json"},
        )
        call_start = time.perf_counter()
        got = None
        error = None
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                got = parse_tool_call(json.load(response))
        except urllib.error.HTTPError as exc:
            error = f"HTTP {exc.code}: {exc.read().decode('utf-8', 'replace')[:300]}"
        except Exception as exc:  # one bad clip must not abort the suite
            error = str(exc)
        elapsed_ms = (time.perf_counter() - call_start) * 1000.0

        want = expected_call(clip)
        strict = (got is None and want is None) or (
            got is not None
            and want is not None
            and got[0] == want[0]
            and norm_args(got[1], loose=False) == norm_args(want[1], loose=False)
        )
        loose = (got is None and want is None) or (
            got is not None
            and want is not None
            and got[0] == want[0]
            and norm_args(got[1], loose=True) == norm_args(want[1], loose=True)
        )
        rows.append(
            {
                "path": clip["path"],
                "text": clip["text"],
                "category": clip["category"],
                "expected": want,
                "got": got,
                "strict": strict,
                "loose": loose,
                "ms": round(elapsed_ms, 1),
                "error": error,
            }
        )
        if index % 25 == 0 or index == len(clips):
            print(f"[{index}/{len(clips)}] {time.time() - started:.0f}s", flush=True)

    categories = {}
    for row in rows:
        bucket = categories.setdefault(
            row["category"], {"n": 0, "strict": 0, "loose": 0, "hallucinated": 0}
        )
        bucket["n"] += 1
        bucket["strict"] += int(row["strict"])
        bucket["loose"] += int(row["loose"])
        if row["expected"] is None and row["got"] is not None:
            bucket["hallucinated"] += 1

    latencies = sorted(row["ms"] for row in rows)

    def percentile(p: float) -> float:
        index = max(0, min(len(latencies) - 1, round(p / 100 * len(latencies)) - 1))
        return latencies[index]

    report = {
        "engine": "lfm3-audio-2b (native tool calling)",
        "corpus": str(corpus_dir),
        "tool_system_prompt": system_prompt,
        "temperature": args.temperature,
        "clips": len(rows),
        "strict": {
            "correct": sum(row["strict"] for row in rows),
            "total": len(rows),
        },
        "loose_case_insensitive": {
            "correct": sum(row["loose"] for row in rows),
            "total": len(rows),
        },
        "by_category": categories,
        "latency_ms": {
            "p50": percentile(50),
            "p95": percentile(95),
            "mean": round(statistics.fmean(latencies), 1),
        },
        "errors": [row for row in rows if row["error"]],
        "wall_seconds": round(time.time() - started, 1),
        "rows": rows,
    }
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=1), encoding="utf-8")
    summary = {key: report[key] for key in report if key != "rows"}
    print(json.dumps(summary, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
