#!/usr/bin/env python3
"""Generate tool-specific adapters from canonical Teletype agent definitions."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CANON = ROOT / ".agents" / "agents"
BT = chr(96)

GIT_DENY = [
    "git checkout *",
    "git restore *",
    "git reset *",
    "git stash *",
    "git clean *",
    "git commit*",
]


def parse_role(path):
    text = path.read_text(encoding="utf-8")
    if not text.startswith("---\n"):
        raise ValueError(f"{path}: missing frontmatter")
    end = text.index("\n---\n", 4)
    fm = {}
    key = None
    for line in text[4:end].splitlines():
        if line.startswith("  - "):
            if key is None or not isinstance(fm[key], list):
                raise ValueError(f"{path}: list item without list key")
            fm[key].append(line[4:].strip().strip("\""))
            continue
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        key = key.strip()
        value = value.strip()
        fm[key] = [] if value == "" else value.strip("\"")
    for required in ("id", "name", "description", "owns", "escalates_to", "routing"):
        if not fm.get(required):
            raise ValueError(f"{path}: missing {required}")
    if not isinstance(fm["owns"], list):
        raise ValueError(f"{path}: owns must be a list")
    if fm["id"] != path.stem or not fm["id"].startswith("teletype-"):
        raise ValueError(f"{path}: id must start with teletype- and match filename")
    return fm


def permissions_block():
    lines = ["permissions:"]
    for pattern in GIT_DENY:
        lines += ["  - action: shell", "    resource: \"" + pattern + "\"", "    effect: deny"]
    return "\n".join(lines)


def bootstrap(fm):
    role = fm["id"]
    parts = [
        "You are the " + fm["name"] + " for this repository (role id: " + role + ").",
        "",
        "FIRST ACTION: read " + BT + ".agents/agents/" + role + ".md" + BT + " in full. "
        "It is your authoritative role definition: scope, verification, and handoff. "
        + BT + "AGENTS.md" + BT + " and the project brain still apply.",
        "",
        "Run " + BT + "bash scripts/check-brain-freshness.sh" + BT + " before and after "
        "meaningful work. Keep the repo minimal final response: what changed, what was "
        "verified, whether project knowledge was updated, and any unresolved uncertainty.",
    ]
    return "\n".join(parts) + "\n"


def opencode_agent(fm):
    return ("---\ndescription: \"" + fm["description"] + "\"\nmode: all\n"
            + permissions_block() + "\n---\n" + bootstrap(fm))


def pi_prompt(fm):
    body = bootstrap(fm) + "\nTask: $" + "{@:-the current request}\n"
    return ("---\ndescription: \"" + fm["description"] + "\"\nargument-hint: \"[task]\"\n---\n" + body)


def skill(fm):
    return ("---\nname: \"" + fm["id"] + "\"\ndescription: \"" + fm["description"] + "\"\n"
            + "disable-model-invocation: true\n---\n" + bootstrap(fm))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if adapters are stale")
    args = parser.parse_args()

    roles = [parse_role(p) for p in sorted(CANON.glob("*.md")) if p.name != "REGISTRY.md"]
    ids = {fm["id"] for fm in roles}
    for fm in roles:
        target = fm["escalates_to"]
        if target != "none" and target not in ids:
            raise ValueError(fm["id"] + ": unknown escalation target " + target)
    expected = {}
    for fm in roles:
        role = fm["id"]
        expected[ROOT / ".opencode" / "agents" / (role + ".md")] = opencode_agent(fm)
        expected[ROOT / ".pi" / "prompts" / (role + ".md")] = pi_prompt(fm)
        expected[ROOT / ".agents" / "skills" / role / "SKILL.md"] = skill(fm)

    stale = []
    for path, content in expected.items():
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != content:
                stale.append(str(path.relative_to(ROOT)))
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    orphans = []
    for base in (ROOT / ".opencode" / "agents", ROOT / ".pi" / "prompts"):
        for p in base.glob("teletype-*.md"):
            if p not in expected:
                orphans.append(p)
    for d in (ROOT / ".agents" / "skills").glob("teletype-*"):
        p = d / "SKILL.md"
        if p.is_file() and p not in expected:
            orphans.append(p)
    if args.check:
        stale += [str(p.relative_to(ROOT)) for p in orphans]
    else:
        for p in orphans:
            p.unlink()

    if args.check:
        if stale:
            print("Stale or orphaned generated agent adapters:")
            print("\n".join(stale))
            return 1
        print("OK: " + str(len(expected)) + " generated adapters are current")
    else:
        print("Wrote " + str(len(expected)) + " adapters from " + str(len(roles)) + " role definitions")
    return 0


if __name__ == "__main__":
    sys.exit(main())
