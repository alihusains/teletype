#!/usr/bin/env python3
"""Score the Prompt Engineer instruction against a local chat model.

The Prompt Engineer transform turns a rough spoken instruction into a
10 section prompt. This runner reads the shipped instruction straight from
`crates/teletype-core/src/transforms/mod.rs` (never a copy), sends it as the
system prompt with each spoken input as the user message, and checks that the
reply carries all ten bold headings in the documented order.

Placeholder counts are reported but not scored: the runner cannot know which
inputs are under-specified, so `[FILL: ...]` is an observation, not a gate.
The overall score is the number of inputs whose headings were all present and
in order.

Usage:
    python3 scripts/eval/prompt_engineer_eval.py \
        --base-url http://127.0.0.1:8080 \
        --model lfm3-omni-2b

No spoken input text is written to disk. The optional JSON report carries only
per-input hashes, lengths and scores.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTRUCTION_FILE = REPO_ROOT / "crates/teletype-core/src/transforms/mod.rs"
INSTRUCTION_START = 'pub const PROMPT_ENGINEER_INSTRUCTION: &str = r#"'
INSTRUCTION_END = '"#;'
DEFAULT_INPUTS = "evals/prompt-engineer/inputs.jsonl"
PREVIEW_CHARS = 40
FILL_MARKER = "[FILL: "
HEADING_RE = re.compile(r"^\*\*(.+?)\*\*\s*$")
# Mirrors `default_chat_template_kwargs` in teletype-inference: these models
# default to thinking mode in their chat template and return empty content
# unless the request disables it.
THINKING_OFF_MODELS = {"s1-mini", "qwen3.5-2b"}


class UsageError(Exception):
    """Bad arguments or a malformed input file (exit 2)."""


class ModelError(Exception):
    """The model could not be reached or returned an unusable reply (exit 3)."""


def load_instruction(path: Path) -> str:
    """Return the raw instruction exactly as the shipped const stores it."""
    try:
        source = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise UsageError(f"cannot read {path}: {exc}") from exc
    start = source.find(INSTRUCTION_START)
    if start < 0:
        raise UsageError(f"{path}: {INSTRUCTION_START!r} not found")
    start += len(INSTRUCTION_START)
    end = source.find(INSTRUCTION_END, start)
    if end < 0:
        raise UsageError(f"{path}: closing {INSTRUCTION_END!r} not found")
    return source[start:end]


def parse_headings(instruction: str) -> list[str]:
    """The bold section headings, in the order the instruction lists them."""
    headings = []
    for line in instruction.splitlines():
        match = HEADING_RE.match(line)
        if match:
            headings.append(match.group(1))
    if len(headings) != 10:
        raise UsageError(
            f"expected 10 bold headings in the instruction, found {len(headings)}"
        )
    return headings


def load_inputs(path: Path) -> list[str]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise UsageError(f"cannot read {path}: {exc}") from exc
    inputs = []
    for line_no, line in enumerate(lines, 1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as exc:
            raise UsageError(f"{path}:{line_no}: invalid JSON: {exc.msg}") from exc
        text = record.get("text") if isinstance(record, dict) else None
        if not isinstance(text, str) or not text.strip():
            raise UsageError(f"{path}:{line_no}: missing non-empty text field")
        inputs.append(text)
    if not inputs:
        raise UsageError(f"{path}: no inputs")
    return inputs


def resolve_inputs(raw: str) -> Path:
    path = Path(raw)
    if path.is_absolute() or path.exists():
        return path
    return REPO_ROOT / path


def score_headings(output: str, headings: list[str]) -> int:
    """Count headings found as literal `**Heading**`, each after the last hit."""
    position = 0
    matched = 0
    for heading in headings:
        marker = f"**{heading}**"
        index = output.find(marker, position)
        if index < 0:
            continue
        matched += 1
        position = index + len(marker)
    return matched


def chat_template_kwargs(model: str) -> dict[str, bool] | None:
    """The request-level chat template kwargs the app would send for `model`."""
    stem = Path(model).name
    if stem.endswith(".gguf"):
        stem = stem[: -len(".gguf")]
    if stem in THINKING_OFF_MODELS:
        return {"enable_thinking": False}
    return None


def request_completion(
    base_url: str,
    model: str,
    api_key: str,
    instruction: str,
    text: str,
    timeout: float,
) -> tuple[str, int]:
    payload = {
        "model": model,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": instruction},
            {"role": "user", "content": text},
        ],
    }
    kwargs = chat_template_kwargs(model)
    if kwargs is not None:
        payload["chat_template_kwargs"] = kwargs
    headers = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    request = urllib.request.Request(
        f"{base_url.rstrip('/')}/v1/chat/completions",
        data=json.dumps(payload).encode("utf-8"),
        headers=headers,
        method="POST",
    )
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read()
    except urllib.error.HTTPError as exc:
        raise ModelError(f"HTTP {exc.code} from {base_url}: {exc.reason}") from exc
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise ModelError(f"cannot reach {base_url}: {exc}") from exc
    latency_ms = round((time.perf_counter() - started) * 1000)
    try:
        data = json.loads(body)
        content = data["choices"][0]["message"]["content"]
    except (json.JSONDecodeError, KeyError, IndexError, TypeError) as exc:
        raise ModelError(f"unexpected reply shape from {base_url}") from exc
    if not isinstance(content, str) or not content.strip():
        raise ModelError(f"empty reply from {base_url}")
    return content, latency_ms


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Score the Prompt Engineer instruction on a local chat model.",
    )
    parser.add_argument(
        "--base-url",
        default="http://127.0.0.1:8080",
        help="OpenAI compatible server root (default: http://127.0.0.1:8080)",
    )
    parser.add_argument(
        "--model",
        required=True,
        help="model name sent in the request",
    )
    parser.add_argument(
        "--api-key",
        default="",
        help="optional bearer token for servers that require one",
    )
    parser.add_argument(
        "--inputs",
        default=DEFAULT_INPUTS,
        help=f"JSONL inputs, one text object per line (default: {DEFAULT_INPUTS})",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=120.0,
        help="per request timeout in seconds (default: 120)",
    )
    parser.add_argument(
        "--json",
        type=Path,
        metavar="PATH",
        help="write a JSON report to PATH",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.timeout <= 0:
        print("ERROR: --timeout must be positive", file=sys.stderr)
        return 2

    try:
        instruction = load_instruction(INSTRUCTION_FILE)
        headings = parse_headings(instruction)
        inputs = load_inputs(resolve_inputs(args.inputs))
    except UsageError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    header = (
        f"{'#':>2}  {'input':<{PREVIEW_CHARS}}  {'heads':>5}  "
        f"{'fills':>5}  {'ms':>7}  result"
    )
    print(header)
    print("-" * len(header))

    results = []
    passes = 0
    try:
        for index, text in enumerate(inputs, 1):
            output, latency_ms = request_completion(
                args.base_url,
                args.model,
                args.api_key,
                instruction,
                text,
                args.timeout,
            )
            matched = score_headings(output, headings)
            placeholders = output.count(FILL_MARKER)
            passed = matched == len(headings)
            passes += int(passed)
            preview = " ".join(text.split())[:PREVIEW_CHARS]
            print(
                f"{index:>2}  {preview:<{PREVIEW_CHARS}}  "
                f"{matched:>2}/{len(headings):<2}  {placeholders:>5}  "
                f"{latency_ms:>7}  {'pass' if passed else 'fail'}"
            )
            results.append(
                {
                    "index": index,
                    "input_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
                    "input_chars": len(text),
                    "headings_matched": matched,
                    "headings_total": len(headings),
                    "placeholders": placeholders,
                    "latency_ms": latency_ms,
                    "pass": passed,
                }
            )
    except ModelError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 3

    total = len(inputs)
    print(f"SCORE: {passes}/{total}")

    if args.json:
        report = {
            "model": args.model,
            "base_url": args.base_url,
            "timeout_s": args.timeout,
            "instruction_sha256": hashlib.sha256(instruction.encode("utf-8")).hexdigest(),
            "headings_total": len(headings),
            "score": passes,
            "total": total,
            "results": results,
        }
        try:
            args.json.write_text(
                json.dumps(report, indent=2) + "\n", encoding="utf-8"
            )
        except OSError as exc:
            print(f"ERROR: cannot write {args.json}: {exc}", file=sys.stderr)
            return 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
