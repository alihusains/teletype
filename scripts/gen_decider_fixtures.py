#!/usr/bin/env python3
"""Generate NeuDecide tool-list JSON fixtures for the compat test.

Builds 20 varied tool lists (all 10 v1 tools covered; unicode names, escapes,
empty args, long descriptions, nested enums) and writes each one to
crates/teletype-decider/tests/fixtures/tools_json/NN.expected using
`json.dumps(tools, ensure_ascii=True)` — the exact format the model was
trained on. `tests/json_dumps_compat.rs` rebuilds the same 20 lists in Rust
and asserts byte-identical output.
"""
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "crates" / "teletype-decider" / "tests" / "fixtures" / "tools_json"

APPS = ["terminal", "safari", "notes", "mail", "finder", "code"]
SNIPPETS = ["thanks", "regards", "br", "signature"]
STYLES = ["body", "heading", "code"]


def v1_tools(apps, snippets, styles):
    """The 10 v1 tools, enums injected (mirrors src/tools.rs)."""
    return [
        {
            "name": "open_app",
            "description": "Open an application by name.",
            "parameters": {
                "type": "object",
                "properties": {"app": {"type": "string", "enum": list(apps)}},
                "required": ["app"],
            },
        },
        {
            "name": "web_search",
            "description": "Search the web for a query.",
            "parameters": {
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"],
            },
        },
        {
            "name": "transform_selection",
            "description": "Transform the current text selection.",
            "parameters": {
                "type": "object",
                "properties": {
                    "transform": {
                        "type": "string",
                        "enum": ["uppercase", "lowercase", "titlecase", "capitalize"],
                    },
                    "language": {
                        "type": "string",
                        "enum": ["en", "fr", "de", "ja"],
                    },
                },
                "required": ["transform"],
            },
        },
        {
            "name": "insert_autotext",
            "description": "Insert a saved autotext snippet.",
            "parameters": {
                "type": "object",
                "properties": {"snippet": {"type": "string", "enum": list(snippets)}},
                "required": ["snippet"],
            },
        },
        {
            "name": "switch_style",
            "description": "Switch the current text style.",
            "parameters": {
                "type": "object",
                "properties": {"style": {"type": "string", "enum": list(styles)}},
                "required": ["style"],
            },
        },
        {
            "name": "draft_message",
            "description": "Draft a message to a recipient.",
            "parameters": {
                "type": "object",
                "properties": {
                    "recipient": {"type": "string"},
                    "body": {"type": "string"},
                },
                "required": ["recipient", "body"],
            },
        },
        {
            "name": "media",
            "description": "Control media playback.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "play_pause",
                            "next",
                            "previous",
                            "volume_up",
                            "volume_down",
                            "mute",
                        ]
                    }
                },
                "required": ["action"],
            },
        },
        {
            "name": "edit_text",
            "description": "Edit the surrounding text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "undo_last_dictation",
                            "delete_last_sentence",
                            "new_line",
                            "new_paragraph",
                            "select_all",
                        ]
                    }
                },
                "required": ["action"],
            },
        },
        {"name": "paste_last", "description": "Paste the last dictated text.", "parameters": {"type": "object", "properties": {}}},
        {"name": "no_action", "description": "Do nothing.", "parameters": {"type": "object", "properties": {}}},
    ]


def build_lists():
    """The 20 fixture tool lists, in order (index+1 == file name)."""
    full = v1_tools(APPS, SNIPPETS, STYLES)
    lists = []
    for i in range(10):  # 01..10: the full v1 set with growing enum lists
        lists.append(v1_tools(APPS[: i + 1], SNIPPETS[: max(1, i)], STYLES[: max(1, i)]))
    # 11: paste_last + no_action only (empty parameters, no properties)
    lists.append([full[8], full[9]])
    # 12: empty tool list
    lists.append([])
    # 13: unicode names and descriptions
    lists.append(
        [
            {"name": "ouvrir_terminal", "description": "Ouvre le terminal. 日本語のツールです。", "parameters": {"type": "object", "properties": {}}},
            {"name": "càffè", "description": "☕ — \"quotes\" and \\ backslashes", "parameters": {"type": "object", "properties": {}}},
        ]
    )
    # 14: escapes in descriptions and enum values
    lists.append(
        [
            {
                "name": "escaped",
                "description": "line1\nline2\ttabbed \"quoted\" \\ backslash \b\f\r ctrl",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "choice": {"type": "string", "enum": ["a\"b", "c\\d", "e\nf", "g\th"]},
                    },
                },
            }
        ]
    )
    # 15: long description (multiple paragraphs)
    lists.append(
        [
            {
                "name": "verbose_tool",
                "description": (
                    "This tool does a lot of things. First it does one thing, "
                    "then it does another thing, and finally it does a third "
                    "thing that is closely related to the first two. "
                    "It is described in detail because the NeuDecide model "
                    "encodes the serialized tool list into its context, so "
                    "longer descriptions cost tokens but may improve "
                    "disambiguation between similar tools. "
                    "Second paragraph: the description may span multiple "
                    "sentences and contain punctuation such as (parentheses), "
                    "[brackets], {braces}, and a slash in a path like "
                    "/usr/local/bin/tool. "
                    "Third paragraph: unicode accents éàü and symbols ©®™ "
                    "are escaped as \\u sequences in the serialized form."
                ),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "option": {"type": "string", "enum": ["one", "two", "three"]},
                    },
                },
            }
        ]
    )
    # 16: nested enum-ish structure (enum of objects is not JSON-schema; use
    # nested objects/arrays in descriptions of parameters)
    lists.append(
        [
            {
                "name": "nested",
                "description": "Tool with nested parameter structure.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "target": {
                            "type": "object",
                            "properties": {
                                "kind": {"type": "string", "enum": ["file", "folder"]},
                                "path": {"type": "string"},
                            },
                        },
                        "flags": {"type": "array", "items": {"type": "string"}},
                        "count": {"type": "integer", "minimum": 0},
                        "ratio": {"type": "number"},
                    },
                    "required": ["target"],
                },
            }
        ]
    )
    # 17: a tool with no "parameters" key at all (model treats as empty)
    lists.append([{"name": "bare", "description": "No parameters key."}])
    # 18: flat parameter mapping (not JSON-Schema shape)
    lists.append(
        [
            {
                "name": "flat",
                "description": "Flat parameter mapping, not a JSON-Schema object.",
                "parameters": {"when": {"type": "string"}, "who": {"type": "string"}},
            }
        ]
    )
    # 19: duplicate-ish tool names with different descriptions
    lists.append(
        [
            {"name": "open_app", "description": "Open an application.", "parameters": {}},
            {"name": "open_app", "description": "Open an application (alias).", "parameters": {}},
        ]
    )
    # 20: the full v1 set exactly as shipped (top lists, all enums)
    lists.append(v1_tools(APPS, SNIPPETS, STYLES))
    return lists


def main():
    lists = build_lists()
    assert len(lists) == 20, len(lists)
    OUT.mkdir(parents=True, exist_ok=True)
    for i, tools in enumerate(lists, 1):
        (OUT / f"{i:02d}.expected").write_text(
            json.dumps(tools, ensure_ascii=True), encoding="ascii"
        )
    print(f"wrote {len(lists)} fixtures to {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
