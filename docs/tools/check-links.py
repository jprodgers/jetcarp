#!/usr/bin/env python3
"""Check internal links in the JETCARP book: every relative link and image must point to a file in
docs/src, and every #anchor must exist on the target page (a heading or an explicit id).

Standard library only. Usage: python3 docs/tools/check-links.py   (exit status 1 on problems)
"""
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "src"


def heading_id(text: str) -> str:
    # mdBook's normalisation: lower case, keep letters, digits, '-' and '_', spaces become '-'.
    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"`", "", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    out = []
    for ch in text.strip().lower():
        if ch.isalnum() or ch in "-_":
            out.append(ch)
        elif ch.isspace():
            out.append("-")
    return "".join(out)


def anchors(path: Path, cache={}):
    if path in cache:
        return cache[path]
    ids = set()
    in_code = False
    seen = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("```"):
            in_code = not in_code
            continue
        if in_code:
            continue
        m = re.match(r"^(#{1,6})\s+(.*)$", line)
        if m:
            base = heading_id(m.group(2))
            n = seen.get(base, 0)
            ids.add(base if n == 0 else f"{base}-{n}")
            seen[base] = n + 1
        for m in re.finditer(r'\bid="([^"]+)"', line):
            ids.add(m.group(1))
    cache[path] = ids
    return ids


def main():
    problems = 0
    checked = 0
    for page in sorted(SRC.rglob("*.md")):
        text = page.read_text(encoding="utf-8")
        text = re.sub(r"```.*?```", "", text, flags=re.S)
        text = re.sub(r"`[^`\n]*`", "", text)
        targets = re.findall(r"\]\(([^)\s]+)\)", text) + re.findall(r'src="([^"]+)"', text)
        for t in targets:
            if re.match(r"^[a-z]+:", t) or t.startswith("<"):
                continue
            checked += 1
            file_part, _, frag = t.partition("#")
            dest = (page.parent / file_part).resolve() if file_part else page
            if not dest.exists():
                print(f"{page.relative_to(SRC)}: missing file {t}")
                problems += 1
                continue
            if frag and dest.suffix == ".md" and frag not in anchors(dest):
                print(f"{page.relative_to(SRC)}: missing anchor {t}")
                problems += 1
    print(f"checked {checked} internal links, {problems} problem(s)", file=sys.stderr)
    sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
