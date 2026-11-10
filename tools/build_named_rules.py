"""Build crates/jetcarp-core/data/rules/named.json, the named-rule catalog beyond rules.rs CATALOG.

Input: tools/data/named_rules.tsv (name, notation, family, author, blurb, start, source_url,
status), plus the hand-written EXTRA list below. The rule list (names, notations, credits, start
hints) was collected from LifeWiki's rule lists (Wayback copies, Dec 2025); source_url links each
rule to the page its facts came from. The blurbs are JETCARP's own wording, not LifeWiki's text,
which is GFDL. Three Generations notations (Burst, Chenille, Glissergy) were normalised from
LifeWiki's S/B/C column because its B/S/C column had typos.
Rows with an empty name (unnamed rules) are skipped, as are rules JETCARP cannot run yet (HROT
custom/weighted/hash neighbourhoods); they stay in the TSV as a complete record.
Start hints (`start`) are chosen by `jetcarp quickstart --survey`; see STARTS.
Run: python3 tools/build_named_rules.py
"""
import csv, json, os
here = os.path.dirname(os.path.abspath(__file__))
rows = list(csv.DictReader(open(os.path.join(here, "data/named_rules.tsv"), encoding="utf-8"), delimiter="\t"))

SKIP_NOTATION = {"R2,C2,S4,6-9,B6-8,N@03ddef", "R2,C2,S4-6,B5-6,N#", "R1,C2,S0,B1,NN"}
FAMILY = {
    "Life-like": "Life-like",
    "Life-like (B0 strobing)": "Life-like (B0)",
    "Generations": "Generations",
    "Larger than Life": "Larger than Life",
    "Higher-range outer-totalistic": "Larger than Life",
    "Hexagonal": "Life-like, hexagonal",
    "Von Neumann": "Life-like, von Neumann",
}

out = []
seen = set()
for r in rows:
    name, nota = r["name"].strip(), r["notation"].strip()
    if not name or nota in SKIP_NOTATION or nota in seen:
        continue
    seen.add(nota)
    name = name.replace("×", "x")
    fam = r["family"].strip()
    fam = "Larger than Life" if fam.startswith("Higher-range") else FAMILY.get(fam, fam)
    e = {"name": name, "notation": nota, "family": fam, "blurb": r["blurb"].strip()}
    if r["author"].strip():
        e["author"] = r["author"].strip()
    e["source"] = r["source_url"].strip()
    if r["status"].strip() != "VERIFIED":
        e["note"] = "Name not confirmed on a LifeWiki rule page."
    out.append(e)

WIKI_ANT = "https://en.wikipedia.org/wiki/Langton%27s_ant"
EXTRA = [
    ("Turmite RLR", "Turmite RLR", "Agents", "Three colours; spreads as an irregular, chaotic blob.", WIKI_ANT),
    ("Turmite LRRRRRLLR", "Turmite LRRRRRLLR", "Agents", "Nine colours; its trail fills out an ever larger square.", WIKI_ANT),
    ("Turmite LLRRRLRLRLLR", "Turmite LLRRRLRLRLLR", "Agents", "Twelve colours; lays down a winding, intricate highway.", WIKI_ANT),
    ("Turmite RRLLLRLLLRRR", "Turmite RRLLLRLLLRRR", "Agents", "Twelve colours; its trail becomes a solid triangle that grows as it travels.", WIKI_ANT),
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
