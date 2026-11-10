"""Build crates/jetcarp-core/data/rules/named.json, the named-rule catalog beyond rules.rs CATALOG.

Input: tools/data/named_rules_lifewiki.tsv (name, notation, family, description, start, source_url,
status), collected from LifeWiki's rule lists (Wayback copies, Dec 2025), plus the hand-written
EXTRA list below. Rows whose "name" is really a description (unnamed rules) are skipped, as are
rules JETCARP cannot run yet (HROT custom/weighted/hash neighbourhoods).
Start hints (`start`) are chosen by `jetcarp quickstart --survey`; see STARTS.
Run: python3 tools/build_named_rules.py
"""
import csv, json, re, os
here = os.path.dirname(os.path.abspath(__file__))
rows = list(csv.DictReader(open(os.path.join(here, "data/named_rules_lifewiki.tsv")), delimiter="\t"))

SKIP_NOTATION = {"R2,C2,S4,6-9,B6-8,N@03ddef", "R2,C2,S4-6,B5-6,N#", "R1,C2,S0,B1,NN"}
def is_description(name):
    return len(name) > 34 or name.endswith(".") or name.startswith(("An ", "A ", "Similar to", "This rule"))

FAMILY = {
    "Life-like": "Life-like",
    "Life-like (B0 strobing)": "Life-like (B0)",
    "Generations": "Generations",
    "Larger than Life": "Larger than Life",
    "Higher-range outer-totalistic": "Larger than Life",
    "Hexagonal": "Life-like, hexagonal",
    "Von Neumann": "Life-like, von Neumann",
}

def blurb(desc):
    d = re.sub(r"\s*\[.*?\]\s*", " ", desc).strip()
    if d.startswith("(LifeWiki gives no description)"):
        return ""
    # MCell-style "Exploding; by X. Text" -> "Text (by X)."
    m = re.match(r"^(\w+); by ([^.]*?)(?: \(([^)]*)\))?\.\s*(.*)$", d)
    if m:
        kind, who, when, rest = m.groups()
        first = re.split(r"(?<=[.!?])\s", rest.strip().strip('"'), maxsplit=1)[0] if rest else ""
        who = "" if who.strip() in ("?", "") else f" By {who.strip()}."
        d = (first + " " if first else "") + f"{kind}.{who}"
    else:
        d = re.split(r"(?<=[.!?])\s", d, maxsplit=1)[0]
    d = d.replace('"', "").strip()
    if len(d) > 140:
        d = d[:137].rsplit(" ", 1)[0] + "..."
    return d

out = []
seen = set()
for r in rows:
    name, nota = r["name"].strip(), r["notation"].strip()
    if is_description(name) or nota in SKIP_NOTATION or nota in seen:
        continue
    seen.add(nota)
    name = name.replace("×", "x")
    fam = r["family"].strip()
    fam = "Larger than Life" if fam.startswith("Higher-range") else FAMILY.get(fam, fam)
    e = {"name": name, "notation": nota, "family": fam, "blurb": blurb(r["description"]),
         "source": r["source_url"].strip()}
    if r["status"].strip() != "VERIFIED":
        e["note"] = "Name not confirmed on a LifeWiki rule page."
    out.append(e)

WIKI_ANT = "https://en.wikipedia.org/wiki/Langton%27s_ant"
EXTRA = [
    ("Turmite RLR", "Turmite RLR", "Agents", "Three colours; grows chaotically.", WIKI_ANT),
    ("Turmite LRRRRRLLR", "Turmite LRRRRRLLR", "Agents", "Nine colours; fills a growing square around itself.", WIKI_ANT),
    ("Turmite LLRRRLRLRLLR", "Turmite LLRRRLRLRLLR", "Agents", "Twelve colours; builds a convoluted highway.", WIKI_ANT),
    ("Turmite RRLLLRLLLRRR", "Turmite RRLLLRLLLRRR", "Agents", "Twelve colours; grows a filled triangle that moves.", WIKI_ANT),
]
W = "https://en.wikipedia.org/wiki/Elementary_cellular_automaton"
for n, b in [(18, "Class 3: Sierpinski-like triangles from one cell, chaotic from random rows."),
             (22, "Class 3: nested triangles from one cell."),
             (45, "Class 3: chaotic, grows lopsided."),
             (54, "Class 4: interacting particles on a periodic background."),
             (60, "Additive: left XOR centre; a skewed Sierpinski triangle."),
             (73, "Class 2: isolated walls separate periodic regions."),
             (105, "Additive: XNOR of three cells; complement of rule 150."),
             (126, "Class 3: Sierpinski triangle with filled edges."),
             (150, "Additive: XOR of three cells."),
             (184, "Traffic flow: particles move right when the next cell is empty.")]:
    EXTRA.append((f"Rule {n}", f"W{n}", "Elementary", b, W))
EXTRA += [
    ("Layered parity", "Layer 2/01", "Layered",
     "Each layer outward from the seed is the XOR of its parents: Rule 90 grown on any tiling.", ""),
    ("Layered sum mod 3", "Layer 3/012", "Layered",
     "Three states; each cell is the sum of its parents mod 3: Pascal's triangle mod 3 on any tiling.", ""),
]
for name, nota, fam, b, src in EXTRA:
    if nota not in seen:
        seen.add(nota)
        out.append({"name": name, "notation": nota, "family": fam, "blurb": b, "source": src})

json.dump({"source": "tools/build_named_rules.py", "rules": out},
          open(os.path.join(here, "../crates/jetcarp-core/data/rules/named.json"), "w"), indent=1, ensure_ascii=False)
print(len(out), "rules")
from collections import Counter
print(Counter(e["family"] for e in out))
