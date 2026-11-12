#!/usr/bin/env python3
"""Generate docs/src/reference/cli.md, the command-line reference, from the program's own help.

Input: `jetcarp --help` and `jetcarp <command> [<subcommand>...] --help` for every command clap
lists (hidden ones, such as `jetcarp api`, are not listed). Each command's help is shown as the
program prints it, grouped by what the commands are for, each group linking to the guide that
explains it.

The page keeps its hand-written introduction. Everything between the markers
  <!-- BEGIN GENERATED: docs/tools/gen-cli.py -->
  <!-- END GENERATED: docs/tools/gen-cli.py -->
is replaced. A command that no group names is an error (exit status 1), so a new command cannot
be left off the page; so is a group naming a command that is gone.

The program runs with its folders pointed at an empty temporary folder, so nothing of the user's
is read or written, and with COLUMNS=100, so the help wraps the same everywhere.

Standard library only. Usage: python3 docs/tools/gen-cli.py [--jetcarp target/release/jetcarp]
                                                             [--check]   (check only)
"""
import argparse
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
REPO = DOCS.parent
PAGE = DOCS / "src" / "reference" / "cli.md"
BEGIN = "<!-- BEGIN GENERATED: docs/tools/gen-cli.py -->"
END = "<!-- END GENERATED: docs/tools/gen-cli.py -->"

# (heading, what the group is for, [(guide title, path from reference/)], [commands]).
GROUPS = [
    ("Worlds and scenes", "Run a scene headless, check that it replays, find its objects, jump far "
     "ahead with HashLife, and the computing demos.",
     [("Scenes", "../guides/scenes.md"), ("HashLife and very long runs", "../guides/hashlife.md")],
     ["run", "verify", "objects", "hashlife", "demo"]),
    ("Searches and jobs", "Start or carry on a search as a resumable job, and follow, stop, "
     "resume, report on and read the runs of any job, whatever started it.",
     [("Searching for patterns", "../guides/searching.md"), ("Large sandpiles", "../guides/large-sandpiles.md"),
      ("Search runs", "search-runs.md")],
     ["search", "jobs", "pile"]),
    ("The library and Quick Start", "List, show, curate and promote what the searches found; the "
     "Quick Start's built-in examples and an explore search's atlas.",
     [("The pattern library", "../guides/pattern-library.md"),
      ("Building your own Quick Start", "../tutorials/building-quick-start.md")],
     ["library", "quickstart"]),
    ("Packs", "Publish the library as a signed pack, export a version for a single download, and "
     "check, summarise or import one.",
     [("Packs: importing, exporting, checking", "../guides/packs.md"), ("Pack format", "pack-format.md")],
     ["pack"]),
    ("Pictures, media and the website", "Contact sheets, animations and posters of library records, "
     "desktop wallpapers, and the website export.",
     [("Pictures and animations", "../guides/pictures.md"), ("Website export", "../guides/website-export.md"),
      ("Wallpapers, screensavers and the background host", "../guides/wallpapers.md")],
     ["sheet", "media", "site", "wallpaper"]),
    ("Scripts", "Run a Rhai script, or one of the examples built in.",
     [("Scripting (Rhai) API", "scripting.md"), ("Scripting a rule sweep", "../tutorials/scripting-a-sweep.md")],
     ["script"]),
    ("The app, the background host and AI assistants", "Control the running app and the background "
     "host, and serve the MCP tools to an AI assistant.",
     [("Wallpapers, screensavers and the background host", "../guides/wallpapers.md"),
      ("Driving JETCARP from an AI assistant", "../tutorials/mcp.md"), ("MCP tools", "mcp.md")],
     ["app", "host", "mcp"]),
    ("Lists", "What there is to choose from.",
     [("Named rule catalogue", "../automata/named-rules.md"), ("Rule notation", "rule-notation.md"),
      ("Tiling catalogue", "../tilings/catalogue.md"), ("Palettes", "palettes.md")],
     ["rules", "tilings", "isohedral", "palettes"]),
    ("Setup and upkeep", "Where JETCARP keeps its files, what it sees on this machine, and how to "
     "take it off again.",
     [("Folders and environment variables", "folders.md"), ("Uninstalling", "../guides/uninstalling.md")],
     ["paths", "doctor", "uninstall"]),
]


def run_help(jetcarp: str, args: list, env: dict) -> str:
    r = subprocess.run([jetcarp, *args, "--help"], capture_output=True, text=True, env=env)
    if r.returncode != 0:
        sys.exit(f"`jetcarp {' '.join(args)} --help` failed: {r.stderr.strip()}")
    return r.stdout.rstrip() + "\n"


def subcommands(help_text: str) -> list:
    """The commands a help text lists under `Commands:`, without `help`."""
    out, inside = [], False
    for line in help_text.splitlines():
        if line.strip() == "Commands:":
            inside = True
            continue
        if inside:
            m = re.match(r"^  (\S+)\s", line + " ")
            if not line.strip() or not line.startswith("  "):
                break
            if m and not line.startswith("   ") and m.group(1) != "help":
                out.append(m.group(1))
    return out


def command_tree(jetcarp: str, path: list, env: dict, out: list):
    """(path, help) for this command and every subcommand under it, depth first."""
    text = run_help(jetcarp, path, env)
    out.append((path, text))
    for sub in subcommands(text):
        command_tree(jetcarp, path + [sub], env, out)


def fence(text: str) -> str:
    return "```text\n" + text.rstrip() + "\n```\n"


def anchor(path: list) -> str:
    return "jetcarp-" + "-".join(path)


def generate(jetcarp: str) -> str:
    with tempfile.TemporaryDirectory(prefix="jetcarp-gen-cli-") as tmp:
        env = dict(os.environ, COLUMNS="100", NO_COLOR="1")
        for k, sub in [("JETCARP_CONFIG_DIR", "config"), ("JETCARP_DATA_DIR", "data"), ("JETCARP_OUT", "out"),
                       ("JETCARP_LIBRARY", "library"), ("JETCARP_RUNTIME_DIR", "runtime")]:
            env[k] = os.path.join(tmp, sub)
        top = run_help(jetcarp, [], env)
        commands = subcommands(top)
        grouped = [c for g in GROUPS for c in g[3]]
        problems = [f"`jetcarp {c}` is in no group of docs/tools/gen-cli.py" for c in commands if c not in grouped]
        problems += [f"the group list names `jetcarp {c}`, which the program no longer has"
                     for c in grouped if c not in commands]
        if problems:
            for p in problems:
                print(p, file=sys.stderr)
            sys.exit(1)
        trees = {}
        for c in commands:
            out = []
            command_tree(jetcarp, [c], env, out)
            trees[c] = out
    if tmp in top or any(tmp in t for tr in trees.values() for _, t in tr):
        sys.exit("a help text names the temporary folder: it would put a local path on the page")
    s = [BEGIN, "",
         "## `jetcarp` itself", "",
         "With no command, `jetcarp` opens the app; the app options below apply then. `--json` "
         "works with every command that reports a result, before or after the command's name.", "",
         fence(top),
         "## Commands by what they are for", ""]
    for heading, what, guides, cmds in GROUPS:
        links = ", ".join(f"[{t}]({p})" for t, p in guides)
        s.append(f"- **{heading}**: " + ", ".join(f"[`{c}`](#{anchor([c])})" for c in cmds) + f". See {links}.")
    s.append("")
    for heading, what, guides, cmds in GROUPS:
        s += [f"## {heading}", "", what + " " + "See " + ", ".join(f"[{t}]({p})" for t, p in guides) + ".", ""]
        for c in cmds:
            for path, text in trees[c]:
                level = "###" if len(path) == 1 else "####"
                # mdBook's id for this heading is anchor(path).
                s += [f'{level} `jetcarp {" ".join(path)}`', "", fence(text)]
    s.append(END)
    return "\n".join(s) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jetcarp", default=str(REPO / "target" / "release" / "jetcarp"))
    ap.add_argument("--check", action="store_true", help="only check that the page is up to date")
    args = ap.parse_args()
    page = PAGE.read_text()
    if BEGIN not in page or END not in page:
        sys.exit(f"{PAGE} has no generated section (markers {BEGIN} ... {END})")
    head, rest = page.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    new = head + generate(args.jetcarp).rstrip("\n") + tail
    if args.check:
        if new != page:
            print(f"{PAGE.relative_to(REPO)} is out of date: run python3 docs/tools/gen-cli.py")
            return 1
        print(f"ok: {PAGE.relative_to(REPO)} is up to date")
        return 0
    if new != page:
        PAGE.write_text(new)
        print(f"wrote {PAGE.relative_to(REPO)}")
    else:
        print(f"{PAGE.relative_to(REPO)} unchanged")
    return 0


if __name__ == "__main__":
    sys.exit(main())
