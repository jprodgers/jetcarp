#!/usr/bin/env python3
"""Copy the book into the MCP server's crate, which serves it as resources.

The MCP server (`jetcarp mcp`) offers every page of this book as a resource
(jetcarp://docs/{page}) and the JSON Schemas of the scene file and the pack manifest
(jetcarp://schema/{scene,pack}.json). It embeds them at build time, and `cargo publish` packages
only what is inside a crate, so the crate keeps copies:

  docs/src/**/*.md                    ->  crates/jetcarp-mcp/resources/docs/**/*.md
  docs/src/reference/NAME.schema.json ->  crates/jetcarp-mcp/resources/schema/NAME.json

This script makes the copies match (writing changed files and deleting copies of pages that are
gone). With --check it only reports differences (exit status 1), as the crate's test
`resources_match_the_book` does. Run it after editing any page of the book.

Standard library only. Usage: python3 docs/tools/copy-mcp-resources.py [--check]
"""
import argparse
import sys
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
SRC = DOCS / "src"
CRATE = REPO / "crates" / "jetcarp-mcp" / "resources"


def wanted() -> dict:
    """{copy path: source path}."""
    out = {}
    for p in sorted(SRC.rglob("*.md")):
        out[CRATE / "docs" / p.relative_to(SRC)] = p
    for p in sorted((SRC / "reference").glob("*.schema.json")):
        out[CRATE / "schema" / p.name.replace(".schema.json", ".json")] = p
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="only report differences")
    args = ap.parse_args()
    want = wanted()
    problems = []
    for dst, src in want.items():
        data = src.read_bytes()
        if not dst.is_file() or dst.read_bytes() != data:
            problems.append(f"{dst.relative_to(REPO)} differs from {src.relative_to(REPO)}")
            if not args.check:
                dst.parent.mkdir(parents=True, exist_ok=True)
                dst.write_bytes(data)
    for sub, pat in (("docs", "*.md"), ("schema", "*.json")):
        for p in sorted((CRATE / sub).rglob(pat)):
            if p not in want:
                problems.append(f"{p.relative_to(REPO)} has no page in the book")
                if not args.check:
                    p.unlink()
    for d in sorted((CRATE / "docs").rglob("*"), reverse=True):
        if d.is_dir() and not any(d.iterdir()) and not args.check:
            d.rmdir()
    if args.check:
        for p in problems:
            print(p)
        if problems:
            print(f"{len(problems)} differences; run python3 docs/tools/copy-mcp-resources.py")
            return 1
        print(f"ok: {len(want)} files match")
        return 0
    print(f"{len(problems)} files updated; {len(want)} in all")
    return 0


if __name__ == "__main__":
    sys.exit(main())
