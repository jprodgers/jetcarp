#!/usr/bin/env python3
"""Keeps in the notices only the crates that some release target actually builds (#123).

cargo-about filters the dependency graph by all the targets in packaging/about.toml at once, so a
dependency built for one target keeps the dependencies it has on another: tray-icon is built only
for Windows and macOS, and its Linux-only GTK crates came in because Linux is in the list. Run once
per target, cargo-about gives each target's crates exactly. This takes cargo-about's JSON for all
the targets together (its texts, order and licence choices) and keeps only the crates that are in
at least one target's own run, then renders packaging/about.hbs as cargo-about would.

    notices_union.py TEMPLATE ALL.json TARGET.json... > THIRD_PARTY_LICENSES.html
    notices_union.py --render-only TEMPLATE ALL.json   (renders unfiltered, to check the renderer)

The renderer knows the handful of handlebars forms about.hbs uses ({{name}}, {{a.b}},
{{#each}}, {{#if}} with {{else}}), escapes as handlebars does, and removes the lines that hold only
a block tag, as handlebars does. notices.sh checks it against cargo-about's own rendering.

The page is set in Atkinson Hyperlegible Next and Mono, which it carries (#273): the line FONT_FACES in
about.hbs's style sheet becomes @font-face rules with the WOFF2 files of crates/jetcarp-core/fonts/
as data: addresses, as the clip-rating page carries them (jetcarp-core's web_fonts.rs), so the page
works as one file offline. The template, which people edit by hand, keeps no encoded font.
"""

import base64
import hashlib
import json
import pathlib
import re
import sys

FONT_FACES = "/* @font-face rules: filled in by packaging/notices_union.py from crates/jetcarp-core/fonts/ */"
FONTS = pathlib.Path(__file__).resolve().parent.parent / "crates" / "jetcarp-core" / "fonts"
# (family, file, weights), as web_fonts.rs gives them for text and monospaced text.
NEXT = "Atkinson Hyperlegible Next"
MONO = "Atkinson Hyperlegible Mono"
FACES = [
    (NEXT, "AtkinsonHyperlegibleNext-Regular.woff2", "100 549"),
    (NEXT, "AtkinsonHyperlegibleNext-Bold.woff2", "550 900"),
    (MONO, "AtkinsonHyperlegibleMono-Regular.woff2", "100 900"),
]

TAG = re.compile(r"\{\{\s*([#/]?)\s*([A-Za-z_][\w.]*)?(?:\s+([\w.]+))?\s*\}\}")
ESCAPES = {"<": "&lt;", ">": "&gt;", '"': "&quot;", "&": "&amp;", "'": "&#x27;", "`": "&#x60;", "=": "&#x3D;"}


def escape(s):
    return "".join(ESCAPES.get(c, c) for c in s)


def standalone(template):
    """Removes the whitespace and newline around a block tag that is alone on its line."""
    out = []
    for line in template.splitlines(keepends=True):
        bare = line.strip()
        m = TAG.fullmatch(bare)
        if m and (m.group(1) in ("#", "/") or m.group(2) == "else"):
            out.append(bare)
        else:
            out.append(line)
    return "".join(out)


def parse(template):
    """A tree of ('text', s), ('var', path), ('each', path, body), ('if', path, yes, no)."""
    pos = 0
    stack = [("root", None, [], None)]
    for m in TAG.finditer(template):
        if m.start() > pos:
            stack[-1][2].append(("text", template[pos : m.start()]))
        pos = m.end()
        kind, word, arg = m.group(1), m.group(2), m.group(3)
        if kind == "#":
            stack.append((word, arg, [], None))
        elif kind == "/":
            name, path, body, other = stack.pop()
            if name != word:
                raise SystemExit(f"about.hbs: {{{{/{word}}}}} closes {{{{#{name}}}}}")
            if name == "each":
                node = ("each", path, body)
            elif name == "if":
                node = ("if", path, other if other is not None else body, body if other is not None else [])
            else:
                raise SystemExit(f"about.hbs: the block {name} is not one this renderer knows")
            stack[-1][2].append(node)
        elif word == "else":
            name, path, body, _ = stack.pop()
            stack.append((name, path, [], body))
        else:
            stack[-1][2].append(("var", word))
    stack[-1][2].append(("text", template[pos:]))
    if len(stack) != 1:
        raise SystemExit("about.hbs: a block is not closed")
    return stack[0][2]


def lookup(ctx, path):
    for part in path.split("."):
        if not isinstance(ctx, dict) or part not in ctx:
            return None
        ctx = ctx[part]
    return ctx


def render(nodes, ctx):
    out = []
    for node in nodes:
        if node[0] == "text":
            out.append(node[1])
        elif node[0] == "var":
            v = lookup(ctx, node[1])
            out.append("" if v is None else escape(str(v)))
        elif node[0] == "each":
            for item in lookup(ctx, node[1]) or []:
                out.append(render(node[2], item))
        elif node[0] == "if":
            out.append(render(node[2] if lookup(ctx, node[1]) else node[3], ctx))
    return "".join(out)


def font_faces():
    """The @font-face rules for FONT_FACES, each file the one the fonts' README names by SHA-256."""
    readme = (FONTS / "README.md").read_text(encoding="utf-8")
    rules = []
    for family, name, weights in FACES:
        data = (FONTS / name).read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if not any(name in line and digest in line for line in readme.splitlines()):
            raise SystemExit(f"{name}'s SHA-256 is not the one crates/jetcarp-core/fonts/README.md gives")
        rules.append(
            f'@font-face {{ font-family: "{family}"; font-style: normal; '
            f"font-weight: {weights}; font-display: swap; "
            f'src: url("data:font/woff2;base64,{base64.b64encode(data).decode()}") format("woff2"); }}'
        )
    return ("\n  ").join(
        ["/* Atkinson Hyperlegible Next and Mono (SIL Open Font License 1.1), the Braille Institute's, as JETCARP ships them. */"]
        + rules
    )


def with_fonts(page):
    if page.count(FONT_FACES) != 1:
        raise SystemExit("about.hbs: the style sheet must hold the FONT_FACES line once")
    return page.replace(FONT_FACES, font_faces())


def built_ids(target_json):
    ids = set()
    for lic in target_json["licenses"]:
        ids.update(u["crate"]["id"] for u in lic["used_by"])
    ids.update(c["package"]["id"] for c in target_json.get("crates", []))
    return ids


def keep_only(all_json, ids):
    """cargo-about's JSON with only these crates: empty licences dropped, the overview recounted."""
    licenses = []
    for lic in all_json["licenses"]:
        used = [u for u in lic["used_by"] if u["crate"]["id"] in ids]
        if used:
            licenses.append(dict(lic, used_by=used))
    seen = set()
    for lic in licenses:
        lic["first_of_kind"] = lic["id"] not in seen
        seen.add(lic["id"])
    overview = []
    for item in all_json["overview"]:
        indices = [i for i, lic in enumerate(licenses) if lic["id"] == item["id"]]
        count = sum(len(licenses[i]["used_by"]) for i in indices)
        if count:
            overview.append(dict(item, indices=indices, count=count))
    crates = [c for c in all_json.get("crates", []) if c["package"]["id"] in ids]
    return dict(all_json, overview=overview, licenses=licenses, crates=crates)


def main(argv):
    render_only = argv[:1] == ["--render-only"]
    if render_only:
        argv = argv[1:]
    if len(argv) < (2 if render_only else 3):
        raise SystemExit(__doc__)
    template = parse(standalone(open(argv[0], encoding="utf-8").read()))
    data = json.load(open(argv[1], encoding="utf-8"))
    if not render_only:
        ids = set()
        for path in argv[2:]:
            ids |= built_ids(json.load(open(path, encoding="utf-8")))
        before = {u["crate"]["id"] for lic in data["licenses"] for u in lic["used_by"]}
        dropped = sorted(i.split("#")[-1] for i in before - ids)
        if dropped:
            print(f"left out, built for no release target: {', '.join(dropped)}", file=sys.stderr)
        data = keep_only(data, ids)
        sys.stdout.write(with_fonts(render(template, data)))
        return
    sys.stdout.write(render(template, data))


if __name__ == "__main__":
    main(sys.argv[1:])
