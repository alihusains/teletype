#!/usr/bin/env python3
"""EW-05 — model license inventory sync check (catalog.rs ↔ THIRD-PARTY-NOTICES.txt).

Ties the MODEL LICENSES section of THIRD-PARTY-NOTICES.txt to its source of
truth, crates/teletype-inference/src/catalog.rs:

  for every non-exempt catalog entry:
    (a) license_name must be Some("<name>")
    (b) the section must contain a block whose header matches
        `===.*catalog id: <id> ===`
    (c) inside that block: a line `License: <license_name>` (alone, or
        followed by the notices' attribution-note convention, e.g. a comma
        plus a naming term)
    (d) if license_url is Some, the exact URL string appears in the block

Future extension point (out of scope now): the ASR/speech model catalog at
crates/teletype-speech/src/catalog.rs — the same check applies verbatim.

Catalog parsing approach (documented per task): scan forward from each
`id: "<id>"` line to the next `id: "` line (or the closing `];` of CATALOG),
then require license_name and license_url fields inside that span. This is
robust to entries with multi-line `Some("...")` values (e.g. s1-mini's
license_url spans three lines) and to the shards array of neudecide-q4.
Only struct-literal field syntax (`field: value,`) is parsed, so `e.id == id`
in tests cannot collide.

Exemptions (id -> reason):
  "test-multi": test-only fixture entry inside #[cfg(test)] — verified by
      inspection at cb0e640 (multi_shard_entry() in mod tests,
      license_name: None by design; never shipped in CATALOG).

Env overrides (for smoke tests): CATALOG_RS, NOTICES_FILE.
Exit codes: 0 ok, 1 violation, 2 usage.
"""

import os
import re
import sys

CATALOG_PATH = os.environ.get(
    "CATALOG_RS", "crates/teletype-inference/src/catalog.rs"
)
NOTICES_PATH = os.environ.get("NOTICES_FILE", "THIRD-PARTY-NOTICES.txt")

EXEMPT = {
    "test-multi": "test-only fixture entry inside #[cfg(test)] — verified by inspection",
}

MIN_ENTRIES = 9
MAX_ENTRIES = 15


def parse_catalog(text):
    """Return a list of (id, license_name, license_url); each is a str or None.

    Scan-forward: from each `id: "<id>"` field, look at the lines up to the
    next `id: "` field (or the closing `];` of the CATALOG literal) and pull
    out license_name / license_url. Values may span multiple lines (Rust
    struct-literal style: `license_url: Some(\n "...",\n ),`), so the field
    scan continues past the field line until the value is consumed (a line
    ending in a comma). Only `field:`-style lines start a new field, so the
    `e.id == id` comparison in the find() test cannot match.
    """
    lines = text.splitlines()
    id_positions = [
        idx for idx, line in enumerate(lines) if re.match(r'\s*id:\s*"', line)
    ]
    entries = []
    for n, pos in enumerate(id_positions):
        m = re.match(r'\s*id:\s*"([^"]+)"', lines[pos])
        if m is None:  # pragma: no cover — id_positions is built from the same pattern
            continue
        entry_id = m.group(1)
        if n + 1 < len(id_positions):
            end = id_positions[n + 1]
        else:
            # Last id: the entry ends at the closing `];` of CATALOG.
            end = len(lines)
            for j in range(pos, len(lines)):
                if lines[j].strip() == "];":
                    end = j
                    break
        name, url = None, None
        j = pos + 1
        while j < end:
            line = lines[j]
            nm = re.match(r'\s*license_name:\s*(.*)$', line)
            um = re.match(r'\s*license_url:\s*(.*)$', line)
            if nm is not None:
                field = "license_name"
                value_match = nm
            elif um is not None:
                field = "license_url"
                value_match = um
            else:
                j += 1
                continue
            # The value starts right after `field: `. Strip the field name and
            # colon so the paren-depth scan starts at the value, not the name.
            after_colon = value_match.group(0).split(":", 1)[1].strip()
            # Multi-line value: a struct-literal field value ends at its closing
            # delimiter — for `Some(...)` that is the paren that returns depth
            # to 0 (the trailing comma comes after it); a bare `None` ends at
            # its own top-level comma. Scan the raw value source and stop at
            # the first depth-0 `)` or `,`.
            value_lines = [after_colon] + lines[j + 1 : end]
            value_text_all = "\n".join(value_lines)
            depth = 0
            value_text = None
            for ci, ch in enumerate(value_text_all):
                if ch == "(":
                    depth += 1
                elif ch == ")":
                    depth -= 1
                    if depth == 0:
                        value_text = value_text_all[: ci + 1]
                        break
                elif ch == "," and depth == 0:
                    value_text = value_text_all[:ci]
                    break
            if value_text is None:
                # No closing paren: bare `None` or an unbalanced value.
                # Stop at the first top-level comma, or take the whole rest.
                value_text = value_text_all.split(",")[0]
            expr = " ".join(value_text.split()).strip()
            if field == "license_name":
                name = _opt_value(expr)
            else:
                url = _opt_value(expr)
            # Advance past the line that contained the terminating comma.
            consumed = value_text.count("\n")
            j = j + consumed + 1
        entries.append((entry_id, name, url))
    return entries


def _opt_value(expr):
    """`Some("Apache-2.0")` -> "Apache-2.0"; `None` -> None; else -> expr.

    Tolerates a trailing comma inside `Some(...)` (Rust allows `Some("x", )`).
    """
    expr = expr.strip()
    m = re.match(r'Some\(\s*"([^"]+)"\s*,?\s*\)$', expr)
    if m:
        return m.group(1)
    if expr == "None":
        return None
    return expr  # unexpected shape; surfaced verbatim in failures


def extract_model_licenses_section(notices_text):
    """Slice the MODEL LICENSES section (banner to PROJECT LICENSE banner).

    Returns the list of lines, or None if the banners are not found in order.
    """
    lines = notices_text.splitlines()
    start = None
    for idx, line in enumerate(lines):
        if "MODEL LICENSES" in line:
            start = idx
            break
    if start is None:
        return None
    end = len(lines)
    for j in range(start + 1, len(lines)):
        if "PROJECT LICENSE" in lines[j]:
            end = j
            break
    return lines[start:end]


def check(catalog_text, notices_text, count_guard=True):
    """Return a list of FAIL messages (empty = pass).

    count_guard=False skips the parser-count sanity check (used by
    --self-test, whose fixtures are smaller than the production range).
    """
    fails = []
    entries = parse_catalog(catalog_text)
    non_exempt = [e for e in entries if e[0] not in EXEMPT]

    if count_guard and not (MIN_ENTRIES <= len(non_exempt) <= MAX_ENTRIES):
        return [
            f"parser found {len(non_exempt)} entries; refusing to pass on a suspicious parse"
        ]

    section = extract_model_licenses_section(notices_text)
    if section is None:
        fails.append(
            "MODEL LICENSES section (banner ... PROJECT LICENSE banner) "
            "not found in notices"
        )
        return fails

    # Index blocks: header line starting `=== ` up to the next `=== ` line.
    blocks = {}
    current = None
    buf = []
    for line in section:
        if line.startswith("=== "):
            if current is not None:
                blocks[current] = buf
            current = line
            buf = []
        elif current is not None:
            buf.append(line)
    if current is not None:
        blocks[current] = buf

    def find_block(entry_id):
        pat = re.compile(
            r"^===.*catalog id: " + re.escape(entry_id) + r" ===\s*$"
        )
        for header in blocks:
            if pat.match(header):
                return header, blocks[header]
        return None

    for entry_id, license_name, license_url in non_exempt:
        if license_name is None:
            fails.append(
                f"catalog entry '{entry_id}' has no license recorded"
            )
            continue
        found = find_block(entry_id)
        if found is None:
            fails.append(
                f"catalog entry '{entry_id}' has no notices block "
                f"(expected header '=== ... catalog id: {entry_id} ===')"
            )
            continue
        _header, body = found
        # The notices state the license name after `License:`. A block may add
        # an attribution note after the name (e.g. s1-mini carries the naming
        # term after a comma), so accept either the bare name or the name
        # followed by a comma — but not a longer name such as
        # `License: Apache-2.0-Modified`.
        prefix = f"License: {license_name}"
        if not any(
            line.strip() == prefix or line.strip().startswith(prefix + ",")
            for line in body
        ):
            fails.append(
                f"catalog entry '{entry_id}': notices block missing a "
                f"'License: {license_name}' line"
            )
        if license_url is not None and not any(
            license_url in line for line in body
        ):
            fails.append(
                f"catalog entry '{entry_id}': notices block missing license URL {license_url}"
            )
    return fails


def read_text(path):
    """Read a text file; caller is expected to have checked existence."""
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except OSError as exc:
        print(f"FAIL: could not read {path}: {exc}", file=sys.stderr)
        raise SystemExit(2) from None


def cmd_list(catalog_text):
    entries = parse_catalog(catalog_text)
    width = max(len(e[0]) for e in entries)
    print("id  license_name  license_url  status")
    for entry_id, name, url in entries:
        if entry_id in EXEMPT:
            status = "EXEMPT: " + EXEMPT[entry_id]
        else:
            status = "checked"
        print(
            f"{entry_id:<{width}}  "
            f"{(name if name is not None else '<none>'):22}  "
            f"{(url if url is not None else '-'):60}  {status}"
        )
    return 0


def self_test():
    """Build fixtures covering: clean pass, missing block, license-name
    mismatch, missing URL, None license, parser-count guard."""
    clean_catalog = (
        "pub const CATALOG: &[CatalogEntry] = &[\n"
        + "".join(
            f"    CatalogEntry {{\n"
            f'        id: "{mid}",\n'
            f'        name: "M{mid}",\n'
            f'        license_name: Some("{lic}"),\n'
            f'        license_url: Some("https://example.com/{mid}/LICENSE"),\n'
            "        recommended: false,\n"
            "    },\n"
            for mid, lic in [
                ("m1", "Apache-2.0"),
                ("m2", "MIT"),
                ("m3", "Apache-2.0"),
                ("m4", "Apache-2.0"),
                ("m5", "Apache-2.0"),
            ]
        )
        + "];\n"
    )

    clean_notices = (
        "MODEL LICENSES banner\n"
        "\n"
        + "".join(
            f"=== M{mid[1:]} — catalog id: {mid} ===\n"
            f"License: {lic}\n"
            f"https://example.com/{mid}/LICENSE\n"
            "\n"
            for mid, lic in [
                ("m1", "Apache-2.0"),
                ("m2", "MIT"),
                ("m3", "Apache-2.0"),
                ("m4", "Apache-2.0"),
                ("m5", "Apache-2.0"),
            ]
        )
        + "PROJECT LICENSE\n"
    )

    errors = []

    # 1. clean pass
    f = check(clean_catalog, clean_notices, count_guard=False)
    if f:
        errors.append(f"clean pass should succeed, got: {f!r}")

    # 2. missing block (drop m3's block)
    n_missing = "\n".join(
        line for line in clean_notices.splitlines() if "catalog id: m3" not in line
    )
    f = check(clean_catalog, n_missing, count_guard=False)
    if not f or "m3" not in f[0]:
        errors.append(f"missing block not caught: {f!r}")

    # 3. license-name mismatch (m2 block says Apache-2.0, catalog says MIT)
    n_mismatch = clean_notices.replace(
        "=== M2 — catalog id: m2 ===\nLicense: MIT\n",
        "=== M2 — catalog id: m2 ===\nLicense: Apache-2.0\n",
    )
    assert n_mismatch != clean_notices, "mismatch fixture did not rewrite"
    f = check(clean_catalog, n_mismatch, count_guard=False)
    if not f or "m2" not in f[0]:
        errors.append(f"license mismatch not caught: {f!r}")

    # 4. missing URL (drop m4's URL line)
    n_nourl = clean_notices.replace("https://example.com/m4/LICENSE\n", "")
    f = check(clean_catalog, n_nourl, count_guard=False)
    if not f or "m4" not in f[0]:
        errors.append(f"missing URL not caught: {f!r}")

    # 5. None license (m5 entry has no license_name)
    c_none = clean_catalog.replace(
        '        license_name: Some("Apache-2.0"),\n'
        '        license_url: Some("https://example.com/m5/LICENSE"),\n',
        "        license_name: None,\n"
        '        license_url: Some("https://example.com/m5/LICENSE"),\n',
    )
    f = check(c_none, clean_notices, count_guard=False)
    if not f or "m5" not in f[0]:
        errors.append(f"None license not caught: {f!r}")

    # 6. parser-count guard (catalog with only 2 entries)
    c_small = clean_catalog.split('id: "m3"')[0] + "];\n"
    f = check(c_small, clean_notices)
    if not f or "suspicious parse" not in f[0]:
        errors.append(f"parser-count guard not caught: {f!r}")

    if errors:
        for err in errors:
            print(f"SELF-TEST: FAIL — {err}", file=sys.stderr)
        return 1
    print("SELF-TEST: PASS")
    return 0


def main(argv):
    args = argv[1:]
    if not args:
        return run_check()
    if args[0] == "--list":
        if len(args) > 1:
            return _usage()
        if not os.path.exists(CATALOG_PATH):
            print(f"FAIL: {CATALOG_PATH} not found", file=sys.stderr)
            return 2
        return cmd_list(read_text(CATALOG_PATH))
    if args[0] == "--self-test":
        if len(args) > 1:
            return _usage()
        return self_test()
    return _usage()


def run_check():
    for path in (CATALOG_PATH, NOTICES_PATH):
        if not os.path.exists(path):
            print(f"FAIL: {path} not found", file=sys.stderr)
            return 2
    fails = check(read_text(CATALOG_PATH), read_text(NOTICES_PATH))
    if fails:
        for msg in fails:
            print("FAIL: " + msg, file=sys.stderr)
        print(
            "FAIL: model license inventory is out of sync with catalog.rs",
            file=sys.stderr,
        )
        return 1
    print(
        "OK: model license inventory in THIRD-PARTY-NOTICES.txt is in sync "
        "with catalog.rs"
    )
    return 0


def _usage():
    print("usage: check-model-licenses.py [--list | --self-test]", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
