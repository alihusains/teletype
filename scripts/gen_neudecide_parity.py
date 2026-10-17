"""Generate the Python reference outputs for the `parity_with_python` test.

Runs the PyPI `neudecide` package over the 50 TTS command WAVs in
`evals/commands/tts/` and writes the decision JSON for each one to
`crates/teletype-decider/tests/fixtures/parity/expected.json`.

The venv at `~/.venvs/neudecide` (python3.11) must have `neudecide`
installed. The model is loaded from the HF cache (or `NEUDECIDE_MODEL_DIR`).

Usage:
  python3.11 scripts/gen_neudecide_parity.py
"""

import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TTS_DIR = REPO / "evals" / "commands" / "tts"
OUT = REPO / "crates" / "teletype-decider" / "tests" / "fixtures" / "parity" / "expected.json"
APPS = ["terminal", "safari", "notes", "mail", "finder", "code"]
SNIPPETS = ["thanks", "regards", "br", "signature"]
STYLES = ["body", "heading", "code"]
# The 10 v1 tools, exactly as `v1_tools` in src/tool_json.rs builds them.
# The enum values injected at runtime are the same in the test (a4_no_mic.rs)
# and here, so the tool list is byte-identical on both sides.
def v1_tools(apps, snippets, styles):
    """Mirror of `v1_tools` in src/tool_json.rs (byte-identical tool list)."""
    def str_prop(v):
        m = {"type": "string"}
        if isinstance(v, list):
            m["enum"] = v
        return m

    def obj_schema(props, required):
        m = {"type": "object", "properties": props}
        if required:
            m["required"] = required
        return m

    return [
        {
            "name": "open_app",
            "description": "Open an application by name.",
            "parameters": obj_schema(
                {"app": str_prop(apps)}, ["app"]
            ),
        },
        {
            "name": "web_search",
            "description": "Search the web for a query.",
            "parameters": obj_schema(
                {"query": {"type": "string"}}, ["query"]
            ),
        },
        {
            "name": "transform_selection",
            "description": "Transform the current text selection.",
            "parameters": obj_schema(
                {
                    "transform": str_prop(
                        ["uppercase", "lowercase", "titlecase", "capitalize"]
                    ),
                    "language": str_prop(["en", "fr", "de", "ja"]),
                },
                ["transform"],
            ),
        },
        {
            "name": "insert_autotext",
            "description": "Insert a saved autotext snippet.",
            "parameters": obj_schema(
                {"snippet": str_prop(snippets)}, ["snippet"]
            ),
        },
        {
            "name": "switch_style",
            "description": "Switch the current text style.",
            "parameters": obj_schema(
                {"style": str_prop(styles)}, ["style"]
            ),
        },
        {
            "name": "draft_message",
            "description": "Draft a message to a recipient.",
            "parameters": obj_schema(
                {
                    "recipient": {"type": "string"},
                    "body": {"type": "string"},
                },
                ["recipient", "body"],
            ),
        },
        {
            "name": "media",
            "description": "Control media playback.",
            "parameters": obj_schema(
                {
                    "action": str_prop(
                        [
                            "play_pause",
                            "next",
                            "previous",
                            "volume_up",
                            "volume_down",
                            "mute",
                        ]
                    )
                },
                ["action"],
            ),
        },
        {
            "name": "edit_text",
            "description": "Edit the surrounding text.",
            "parameters": obj_schema(
                {
                    "action": str_prop(
                        [
                            "undo_last_dictation",
                            "delete_last_sentence",
                            "new_line",
                            "new_paragraph",
                            "select_all",
                        ]
                    )
                },
                ["action"],
            ),
        },
        {
            "name": "paste_last",
            "description": "Paste the last dictated text.",
            "parameters": {"type": "object", "properties": {}},
        },
        {
            "name": "no_action",
            "description": "Do nothing.",
            "parameters": {"type": "object", "properties": {}},
        },
    ]


def main():
    tools = v1_tools(APPS, SNIPPETS, STYLES)

    # Find the model dir.
    model_dir = os.environ.get("NEUDECIDE_MODEL_DIR")
    if not model_dir:
        home = os.path.expanduser("~")
        model_dir = (
            f"{home}/.cache/huggingface/hub/models--neuphonic--neudecide/"
            "snapshots/fb0157e06da16d887caf041ca7929f96da419e6b"
        )
    if not os.path.isfile(os.path.join(model_dir, "config.json")):
        print(f"ERROR: model dir not found: {model_dir}", file=sys.stderr)
        sys.exit(1)

    from neudecide.model import NeuDecide

    print(f"Loading model from {model_dir} ...")
    model = NeuDecide(model_dir)

    manifest = json.loads((TTS_DIR / "manifest.json").read_text())
    results = {}
    for entry in manifest:
        wav = TTS_DIR / f"{entry['id']}.wav"
        calls = model.generate(str(wav), tools)
        # Normalize to the same shape the Rust test produces.
        results[entry["id"]] = [
            {"name": c["name"], "arguments": c.get("arguments", {})} for c in calls
        ]
        print(f"  {entry['id']}: {json.dumps(calls)}")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(results, indent=2))
    print(f"\nWrote {OUT}")


if __name__ == "__main__":
    main()
