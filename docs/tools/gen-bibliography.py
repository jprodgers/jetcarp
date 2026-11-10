#!/usr/bin/env python3
"""Generate docs/src/research/bibliography.md from docs/data/bibliography.bib.

Standard library only. Each entry gets an HTML anchor equal to its BibTeX key, so pages cite
it as [Gardner 1970](../research/bibliography.md#gardner1970life).

Usage: python3 docs/tools/gen-bibliography.py   (from the repository root or anywhere)
"""
import re
import sys
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
BIB = DOCS / "data" / "bibliography.bib"
OUT = DOCS / "src" / "research" / "bibliography.md"

ACCENTS = {
    "'": {"a": "á", "e": "é", "i": "í", "o": "ó", "u": "ú", "n": "ń", "y": "ý", "c": "ć", "s": "ś", "z": "ź",
          "A": "Á", "E": "É", "I": "Í", "O": "Ó", "U": "Ú", "C": "Ć", "S": "Ś", "Z": "Ź"},
    '"': {"a": "ä", "e": "ë", "i": "ï", "o": "ö", "u": "ü", "A": "Ä", "E": "Ë", "O": "Ö", "U": "Ü", "y": "ÿ"},
    "`": {"a": "à", "e": "è", "i": "ì", "o": "ò", "u": "ù", "A": "À", "E": "È"},
    "^": {"a": "â", "e": "ê", "i": "î", "o": "ô", "u": "û", "A": "Â", "E": "Ê", "O": "Ô"},
    "~": {"n": "ñ", "a": "ã", "o": "õ", "N": "Ñ"},
    "v": {"c": "č", "s": "š", "z": "ž", "r": "ř", "e": "ě", "C": "Č", "S": "Š", "Z": "Ž", "R": "Ř"},
    "c": {"c": "ç", "C": "Ç", "s": "ş"},
    "H": {"o": "ő", "u": "ű", "O": "Ő", "U": "Ű"},
    "u": {"a": "ă", "g": "ğ"},
}
SPECIAL = {r"\ss": "ß", r"\o": "ø", r"\O": "Ø", r"\l": "ł", r"\L": "Ł", r"\ae": "æ", r"\aa": "å",
           r"\AA": "Å", r"\i": "ı"}
MATH = {r"$\mathbb{Z}^2$": "ℤ²", r"$q\to0$": "q → 0"}


def delatex(s: str) -> str:
    for k, v in MATH.items():
        s = s.replace(k, v)
    s = s.replace(r"\ldots{}", "…").replace(r"\ldots", "…")
    s = re.sub(r"\\emph\{([^{}]*)\}", r"*\1*", s)
    s = re.sub(r"\\textit\{([^{}]*)\}", r"*\1*", s)

    # \'{e}, \'e, {\'e}, \"\i, \v{C}
    def acc(m):
        cmd, letter = m.group(1), m.group(2)
        if letter == r"\i":
            letter = "i"
        return ACCENTS.get(cmd, {}).get(letter, letter)

    s = re.sub(r"\\(['\"`^~])\{?(\\i|[A-Za-z])\}?", acc, s)
    s = re.sub(r"\\([vcHu])\{(\\i|[A-Za-z])\}", acc, s)
    s = re.sub(r"\\([vcHu]) (\\i|[A-Za-z])", acc, s)
    for k, v in SPECIAL.items():
        s = re.sub(re.escape(k) + r"(?![A-Za-z])\s?", v, s)
    s = s.replace(r"\&", "&").replace(r"\_", "_").replace(r"\%", "%").replace(r"\#", "#")
    s = s.replace("---", "—").replace("--", "–").replace("~", " ")
    s = s.replace("{", "").replace("}", "")
    return re.sub(r"\s+", " ", s).strip()


def parse(text: str):
    entries = []
    i = 0
    while True:
        m = re.compile(r"@(\w+)\s*\{\s*([^,\s]+)\s*,").search(text, i)
        if not m:
            break
        etype, key = m.group(1).lower(), m.group(2)
        j = m.end()
        fields = {}
        while True:
            fm = re.compile(r"\s*,?\s*(\w+)\s*=\s*").match(text, j)
            if not fm:
                break
            name = fm.group(1).lower()
            j = fm.end()
            if text[j] == "{":
                depth, k = 0, j
                while True:
                    if text[k] == "{":
                        depth += 1
                    elif text[k] == "}":
                        depth -= 1
                        if depth == 0:
                            break
                    k += 1
                value, j = text[j + 1:k], k + 1
            elif text[j] == '"':
                k = text.index('"', j + 1)
                value, j = text[j + 1:k], k + 1
            else:
                vm = re.compile(r"[^,}\s]+").match(text, j)
                value, j = vm.group(0), vm.end()
            fields[name] = value
        close = text.index("}", j)
        i = close + 1
        entries.append((etype, key, fields))
    return entries


def names(field: str):
    out = []
    for raw in re.split(r"\s+and\s+", field.strip()):
        raw = raw.strip()
        if raw.startswith("{") and raw.endswith("}") and raw.count("{") == 1:
            out.append((delatex(raw), None))  # corporate author
            continue
        if "," in raw:
            last, first = raw.split(",", 1)
            out.append((delatex(last), delatex(first)))
        else:
            parts = raw.split()
            out.append((delatex(parts[-1]), delatex(" ".join(parts[:-1])) if len(parts) > 1 else ""))
    return out


def fmt_names(field: str) -> str:
    ns = names(field)
    shown = []
    for idx, (last, first) in enumerate(ns):
        if first is None:
            shown.append(last)
        elif idx == 0:
            shown.append(f"{last}, {first}" if first else last)
        else:
            shown.append(f"{first} {last}".strip())
    if len(shown) == 1:
        return shown[0]
    return ", ".join(shown[:-1]) + " and " + shown[-1]


def fmt(etype, key, f):
    parts = []
    who = fmt_names(f["author"]) if "author" in f else (fmt_names(f["editor"]) + " (ed.)" if "editor" in f else "")
    year = delatex(f.get("year", "n.d."))
    parts.append(f"{who} ({year})." if who else f"({year}).")
    title = delatex(f.get("title", ""))
    if title:
        if etype in ("book", "phdthesis"):
            parts.append(f"*{title}*.")
        else:
            parts.append(f"“{title}”.")
    venue = []
    if etype == "article":
        j = delatex(f.get("journal", ""))
        vol = delatex(f.get("volume", ""))
        num = delatex(f.get("number", ""))
        pages = delatex(f.get("pages", ""))
        s = f"*{j}*" if j else ""
        if vol:
            s += f" {vol}"
        if num and vol:
            s += f"({num})"
        elif num:
            s += f", no. {num}"
        if pages:
            s += f": {pages}" if (vol or num) else f", {pages}"
        if s:
            venue.append(s)
    elif etype in ("incollection", "inproceedings"):
        b = delatex(f.get("booktitle", ""))
        s = "In "
        if "editor" in f:
            s += fmt_names(f["editor"]) + " (ed.), "
        s += f"*{b}*" if b else ""
        if f.get("pages"):
            s += f", pp. {delatex(f['pages'])}"
        venue.append(s)
        if f.get("publisher"):
            venue.append(delatex(f["publisher"]))
    elif etype == "book":
        if f.get("edition"):
            venue.append(delatex(f["edition"]) + " edition")
        if f.get("series"):
            venue.append(delatex(f["series"]))
        if f.get("publisher"):
            pub = delatex(f["publisher"])
            if f.get("address"):
                pub = delatex(f["address"]) + ": " + pub
            venue.append(pub)
    elif etype == "phdthesis":
        venue.append("Thesis" + (", " + delatex(f["school"]) if f.get("school") else ""))
    else:  # misc
        if f.get("howpublished"):
            venue.append(delatex(f["howpublished"]))
        if f.get("publisher"):
            venue.append(delatex(f["publisher"]))
    if venue:
        parts.append(". ".join(v for v in venue if v) + ".")
    links = []
    if f.get("doi"):
        d = f["doi"].strip()
        links.append(f"doi:[{d}](https://doi.org/{d})")
    eprint = f.get("eprint")
    if eprint and f.get("archiveprefix", "arXiv").lower() == "arxiv":
        links.append(f"[arXiv:{eprint}](https://arxiv.org/abs/{eprint})")
    if f.get("url") and not f.get("doi") and not (eprint and "arxiv.org" in f["url"]):
        u = f["url"].strip().replace(r"\_", "_").replace(r"\%", "%").replace(r"\#", "#")
        links.append(f"<{u}>")
        if f.get("urldate"):
            links.append(f"accessed {f['urldate']}")
    if f.get("isbn") and etype == "book":
        links.append(f"ISBN {f['isbn']}")
    if links:
        parts.append("; ".join(links) + ".")
    # The `note` fields hold research bookkeeping (how and when an entry was checked) and are
    # deliberately not shown.
    return " ".join(parts)


def main():
    text = BIB.read_text(encoding="utf-8")
    entries = parse(text)
    lines = [
        "# Bibliography",
        "",
        "<!-- GENERATED FILE: do not edit by hand. Source: docs/data/bibliography.bib;",
        "     generator: docs/tools/gen-bibliography.py. -->",
        "",
        "> **Generated page.** This list is built from `docs/data/bibliography.bib` by",
        "> `docs/tools/gen-bibliography.py`. Edit the BibTeX file, not this page.",
        "",
        f"These are the {len(entries)} works cited across the JETCARP documentation. Every entry was",
        "checked against a primary record (a DOI landing page or Crossref, arXiv, the publisher, archive.org,",
        "the author's or project's own page, or LifeWiki) when the history research was compiled in",
        "November 2026. Web pages are given with the date they were read. Each entry's anchor is its",
        "BibTeX key, so you can link to it directly.",
        "",
    ]
    letter = None
    for etype, key, f in sorted(entries, key=lambda e: e[1].lower()):
        first = key[0].upper()
        if first != letter:
            letter = first
            lines += [f"## {letter}", ""]
        lines.append(f'<span id="{key}"></span>{fmt(etype, key, f)} <small>`{key}`</small>')
        lines.append("")
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT} ({len(entries)} entries)", file=sys.stderr)


if __name__ == "__main__":
    main()
