#!/usr/bin/env python3
"""Check the rule notation page's "Typed | Written back" table against the program.

Each typed rule in docs/src/reference/rule-notation.md's table is given to `jetcarp api call
world.new` (on the square grid, or the first of a few other tilings that accepts it: the hexagon
grids, the spectre), and the rule the program writes back, and its name where the table gives one,
must be what the table says.
A row with several typed forms and one written form expects that form for each; with as many
written forms as typed ones, they pair up in order.

Standard library only. Usage: python3 docs/tools/check-rule-notation.py [--jetcarp target/release/jetcarp]
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
PAGE = DOCS / "src" / "reference" / "rule-notation.md"
HEAD = "| Typed | Written back |"
TILINGS = [
    {"tiling": "square", "width": 16, "height": 16},
    {"tiling": "hex", "width": 16, "height": 16},
    {"tiling": "hex_flat", "width": 16, "height": 16},
    {"tiling": "spectre", "level": 2},
]


def codes(cell):
    """The `code` spans of a table cell, with markdown's escaped bars restored."""
    return [c.replace("\\|", "|") for c in re.findall(r"`((?:[^`\\]|\\.)+)`", cell)]


def rows(page):
    lines = page.splitlines()
    start = lines.index(HEAD) + 2
    for line in lines[start:]:
        if not line.startswith("|"):
            break
        typed, written = re.split(r"(?<!\\)\|", line.strip().strip("|"))[:2]
        name = re.search(r"\(named \*([^*]+)\*\)", written)
        yield codes(typed), codes(written), name.group(1) if name else None


def written_back(jetcarp, rule):
    errors = []
    for t in TILINGS:
        args = dict(t, cas=[{"rule": rule}])
        out = subprocess.run([jetcarp, "api", "call", "world.new", json.dumps(args)],
                             capture_output=True, text=True).stdout
        try:
            d = json.loads(out)
        except json.JSONDecodeError:
            errors.append(out.strip()[:200])
            continue
        if "error" in d:
            errors.append(d["error"]["message"])
            continue
        ca = d["status"]["cas"][0]
        return ca["rule"], ca.get("name")
    raise ValueError("; ".join(errors))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jetcarp", default=str(REPO / "target" / "release" / "jetcarp"))
    args = ap.parse_args()
    bad, n = [], 0
    for typed, written, name in rows(PAGE.read_text(encoding="utf-8")):
        if len(written) not in (1, len(typed)):
            bad.append(f"{typed}: {len(typed)} typed forms, {len(written)} written: can't pair them")
            continue
        for i, t in enumerate(typed):
            want = written[0] if len(written) == 1 else written[i]
            n += 1
            try:
                got, got_name = written_back(args.jetcarp, t)
            except ValueError as e:
                bad.append(f"`{t}`: refused ({e})")
                continue
            if got != want:
                bad.append(f"`{t}`: the page says `{want}`, the program writes `{got}`")
            elif name and got_name != name:
                bad.append(f"`{t}`: the page names it {name}, the program {got_name}")
    if bad:
        print(f"{PAGE.name}: {len(bad)} of {n} examples disagree with the program:", file=sys.stderr)
        for b in bad:
            print("  " + b, file=sys.stderr)
        sys.exit(1)
    print(f"ok: the {n} examples in {PAGE.name}'s table parse as it says")


if __name__ == "__main__":
    main()
