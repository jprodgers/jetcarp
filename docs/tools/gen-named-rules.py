#!/usr/bin/env python3
"""Generate docs/src/automata/named-rules.md, the named-rule catalogue.

Inputs:
  crates/jetcarp-core/data/rules/named.json   name, notation, family, author, source (the program's data)
  docs/data/named-rule-credits.tsv            credits checked against LifeWiki, MCell and the literature
  `jetcarp rules` (optional, --jetcarp PATH)  which names the program accepts, to flag any it cannot run

Standard library only. Usage: python3 docs/tools/gen-named-rules.py [--jetcarp target/release/jetcarp]
"""
import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
NAMED = REPO / "crates" / "jetcarp-core" / "data" / "rules" / "named.json"
CREDITS = DOCS / "data" / "named-rule-credits.tsv"
OUT = DOCS / "src" / "automata" / "named-rules.md"

FAMILY_PAGE = {
    "Life-like": "life-like.md",
    "Life-like (B0)": "life-like.md#b0-rules",
    "Life-like, hexagonal": "life-like.md",
    "Life-like, von Neumann": "life-like.md#edge-neighbours-the-v-suffix",
    "Generations": "generations.md",
    "Larger than Life": "larger-than-life.md",
    "Elementary": "elementary.md",
    "Agents": "turmites.md",
    "Layered": "layered.md",
}
ORDER = list(FAMILY_PAGE)


def source_links(codes: str) -> str:
    out = []
    for code in [c.strip() for c in codes.split(";") if c.strip()]:
        if code.startswith("LW:"):
            page = code[3:].split(" ")[0]
            label = page.replace("_", " ").replace("%27", "'")
            out.append(f"[LifeWiki: {label}](https://conwaylife.com/wiki/{page})")
        elif code.startswith("MC"):
            out.append("[MCell lexicon](https://mcell.ca/pages/rules.html)")
        elif code.startswith("CAM"):
            out.append("[Toffoli & Margolus 1987](../research/bibliography.md#toffoli1987cellular)")
        elif code.startswith("WP:"):
            page = code[3:]
            out.append(f"[Wikipedia: {page.replace('_', ' ')}](https://en.wikipedia.org/wiki/{page})")
        elif code.startswith("forum p="):
            p = code.split("=", 1)[1]
            out.append(f"[ConwayLife forum](https://conwaylife.com/forums/viewtopic.php?p={p})")
        elif code == "JETCARP":
            out.append("[JETCARP](layered.md)")
        else:
            out.append(code)
    # keep order, drop duplicates
    seen, uniq = set(), []
    for o in out:
        if o not in seen:
            seen.add(o)
            uniq.append(o)
    return "; ".join(uniq)


def cell(s: str) -> str:
    return s.replace("|", "\\|")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jetcarp", default=str(REPO / "target" / "release" / "jetcarp"))
    args = ap.parse_args()

    rules = json.loads(NAMED.read_text(encoding="utf-8"))["rules"]
    credits = {}
    for line in CREDITS.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or line.startswith("name\t") or not line.strip():
            continue
        name, notation, credit, sources, status = (line.split("\t") + [""] * 5)[:5]
        credits[name] = (notation, credit, sources, status)

    accepted = None
    try:
        out = subprocess.run([args.jetcarp, "rules"], capture_output=True, text=True, check=True).stdout
        accepted = set()
        listed = out.splitlines()
        for r in rules:
            # `jetcarp rules` prints each accepted rule as "<name>  <notation>  <description>"
            # (a hand-written built-in entry with the same notation may show under another name)
            if any(line.lower().startswith(r["name"].lower() + " ") or f" {r['notation']} " in line
                   for line in listed):
                accepted.add(r["name"])
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"note: could not run {args.jetcarp} rules ({e}); not checking which rules parse", file=sys.stderr)

    # Legend for the contact sheet docs/src/images/automata/named-generations-sheet.png, which
    # render-figures.sh draws with `jetcarp quickstart --sheet ... --filter Generations`; the sheet
    # numbers entries in the order `jetcarp quickstart --filter Generations` lists them.
    legend = []
    try:
        out = subprocess.run([args.jetcarp, "quickstart", "--filter", "Generations"], capture_output=True,
                             text=True, check=True).stdout
        for k, line in enumerate(l for l in out.splitlines() if l.strip()):
            m = re.match(r"^\s*\d+\s+(.+?)\s{2,}", line)
            if m:
                legend.append(f"{k} {m.group(1)}")
    except (OSError, subprocess.CalledProcessError):
        pass

    fams = Counter(r["family"] for r in rules)
    credited = sum(1 for r in rules if credits.get(r["name"], ("", "", "", ""))[1] or r.get("author"))
    lines = [
        "# Named rule catalogue",
        "",
        "<!-- GENERATED FILE: do not edit by hand. Sources: crates/jetcarp-core/data/rules/named.json and",
        "     docs/data/named-rule-credits.tsv; generator: docs/tools/gen-named-rules.py. -->",
        "",
        "> **Generated page.** Built by `docs/tools/gen-named-rules.py` from the program's own rule list",
        "> (`crates/jetcarp-core/data/rules/named.json`) and the credits file `docs/data/named-rule-credits.tsv`.",
        "> Edit those files, not this page.",
        "",
        f"JETCARP knows {len(rules)} rules by name, collected from the rule lists that the Life community",
        "has kept for decades. You can type a name anywhere a rule is expected (`jetcarp rules` lists them, with",
        "a few more built-in names for families that have no standard list). Every one of them runs on any",
        "tiling, though most were found on the square grid and behave differently elsewhere; see",
        "[How JETCARP generalises automata](generalising.md).",
        "",
        "**About the credits.** Most of these rules were named, and often found, by hobbyists: through",
        "Mirek Wójtowicz's MCell program and its rules lexicon in the late 1990s, the ConwayLife.com forums,",
        "and LifeWiki's lists. The *Credit* column names a person only when a source does, with the role the",
        "source gives (found, named, studied). An empty cell means no source we checked names anyone, not",
        "that nobody did. Where sources disagree, both are shown. "
        f"{credited} of the {len(rules)} rules have a named person. The credits were checked in November 2026",
        "against LifeWiki (read through its mirror, since conwaylife.com refuses automated reading), the",
        "MCell lexicon, Toffoli and Margolus's *Cellular Automata Machines* (1987) and Wikipedia.",
        "If you know of an earlier or missing credit, please open an issue.",
        "",
        "The *Source* column links the list each rule was taken from. The short descriptions JETCARP shows",
        "for each rule are written in our own words.",
        "",
        "| Family | Rules |",
        "| --- | --- |",
    ]
    if legend:
        fig = [
            "",
            "<figure>",
            '<img src="../images/automata/named-generations-sheet.png" alt="A contact sheet of small '
            'pictures, one per Generations rule" width="906">',
            "<figcaption>The Quick Start entries for the Generations family, each 200 generations from its "
            "usual start, drawn by <code>jetcarp quickstart --sheet --filter Generations</code>. Numbers: "
            + "; ".join(legend) + ".</figcaption>",
            "</figure>",
            "",
        ]
    else:
        fig = []
    for fam in ORDER:
        if fam in fams:
            lines.append(f"| [{fam}]({FAMILY_PAGE[fam]}) | {fams[fam]} |")
    lines.append("")
    lines += fig

    for fam in ORDER:
        rs = [r for r in rules if r["family"] == fam]
        if not rs:
            continue
        page = FAMILY_PAGE[fam]
        lines += [f"## {fam}", "", f"Family page: [{fam}]({page}).", ""]
        lines += ["| Name | Notation | Credit | Sources |", "| --- | --- | --- | --- |"]
        for r in sorted(rs, key=lambda r: r["name"].lower()):
            notation, credit, sources, status = credits.get(r["name"], ("", "", "", ""))
            if notation and notation != r["notation"]:
                print(f"warning: {r['name']}: notation {r['notation']} != credits file {notation}", file=sys.stderr)
            if not credit and r.get("author"):
                credit = r["author"]
            src = source_links(sources)
            if r.get("source"):
                main_src = r["source"]
                label = "rule list"
                if src:
                    src = f"[{label}]({main_src}); " + src
                else:
                    src = f"[{label}]({main_src})"
            name = r["name"]
            flag = ""
            if accepted is not None and name not in accepted:
                flag = " ¹"
            nota = cell(r["notation"])
            nota = f'<code class="wrap">{nota}</code>' if len(nota) > 40 else f"`{nota}`"
            lines.append(f"| {cell(name)}{flag} | {nota} | {cell(credit)} | {src} |")
        lines.append("")
    if accepted is not None:
        missing = [r["name"] for r in rules if r["name"] not in accepted]
        if missing:
            lines += [
                "¹ Listed in the data file but not yet accepted by this version of JETCARP: "
                + ", ".join(missing)
                + ". Coexistence uses a weighted Larger than Life neighbourhood, which JETCARP does not read yet.",
                "",
            ]
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT} ({len(rules)} rules)", file=sys.stderr)


if __name__ == "__main__":
    main()
