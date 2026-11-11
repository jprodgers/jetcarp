"""The command line: one command per conformance level, plus `check` for levels 1 to N.

Exit status: 0 when nothing failed, 1 when a check failed or the pack was refused, 2 for a usage
error or a store that cannot be opened.
"""

import argparse
import json
import sys

from . import authenticity, browse, integrity, replay
from .limits import Limits
from .pack import Pack, open_pack
from .report import Report
from .store import StoreError, open_store


def _dump(value):
    print(json.dumps(value, indent=1, ensure_ascii=False, default=str))


def main(argv=None):
    p = argparse.ArgumentParser(prog="jetcarp-pack", description="An independent reader for JETCARP pack format 1.0 "
                                                                 "(a store, or an export: a folder with pack.json).")
    sub = p.add_subparsers(dest="command", required=True)

    def add(name, help_):
        c = sub.add_parser(name, help=help_)
        c.add_argument("store", help="the store's folder")
        c.add_argument("--json", action="store_true", help="print the report as JSON")
        c.add_argument("--verbose", "-v", action="store_true", help="also show INFO findings")
        return c

    def pick(c):
        c.add_argument("--manifest", help="a manifest hash (sha256:...) instead of the store's ref")
        c.add_argument("--feed", help="which ref to follow when the store has several")

    c = add("info", "level 0: the store's refs and keys")
    c = add("tables", "level 0: a version's tables, parts and row counts"); pick(c)
    c = add("rows", "level 0: rows of a table as JSON"); pick(c)
    c.add_argument("table")
    c.add_argument("--where", action="append", default=[], help="column=value (repeatable)")
    c.add_argument("--columns", help="comma-separated columns to show")
    c.add_argument("--limit", type=int, default=20)
    c = add("find", "level 0: a pattern or ant by id, prefix, alias or name"); pick(c)
    c.add_argument("id")
    c = add("integrity", "level 1: object hashes, manifest hashes and content ids")
    c.add_argument("--manifest")
    c.add_argument("--no-scan", action="store_true", help="skip hashing every file under objects/")
    c = add("authenticity", "level 2: signatures, key statements and ref seq")
    c.add_argument("--manifest")
    c.add_argument("--state", help="file of the last accepted seq per feed (for rollback checks)")
    c.add_argument("--accept", action="store_true", help="record the refs accepted in --state")
    c = add("replay", "level 3: re-run periodic patterns from the tilings and rules tables")
    c.add_argument("--manifest")
    c.add_argument("--periods", type=int, default=2, help="periods to replay each pattern for (default 2)")
    c.add_argument("--all-versions", action="store_true", help="replay parents and retained versions too")
    c = add("check", "levels 1 to --level in turn")
    c.add_argument("--level", type=int, default=3, choices=(1, 2, 3))
    c.add_argument("--manifest")
    c.add_argument("--state")
    c.add_argument("--accept", action="store_true")
    c.add_argument("--periods", type=int, default=2)

    args = p.parse_args(argv)
    try:
        store = open_store(args.store, Limits())
    except StoreError as e:
        print(f"jetcarp-pack: {e.message}", file=sys.stderr)
        return 2
    report = Report()
    output = None
    try:
        if args.command == "info":
            output = browse.info(store, report)
        elif args.command in ("tables", "rows", "find"):
            h = open_pack(store, report, args.manifest, args.feed)
            if h:
                pack = Pack(store, h, report)
                if pack.ok:
                    if args.command == "tables":
                        output = {"version": browse.describe_manifest(pack), "tables": browse.tables(pack)}
                    elif args.command == "rows":
                        cols = args.columns.split(",") if args.columns else None
                        output = browse.rows(pack, args.table, args.where, cols, args.limit)
                        if output is None:
                            print(f"jetcarp-pack: this version has no table {args.table!r}", file=sys.stderr)
                            return 2
                    else:
                        output = browse.find(pack, args.id)
        elif args.command == "integrity":
            integrity.check_store(store, report, args.manifest, scan=not args.no_scan)
        elif args.command == "authenticity":
            authenticity.check_authenticity(store, report, args.manifest, args.state, args.accept)
        elif args.command == "replay":
            replay.check_replay(store, report, args.manifest, args.periods, args.all_versions)
        elif args.command == "check":
            integrity.check_store(store, report, args.manifest)
            if args.level >= 2:
                authenticity.check_authenticity(store, report, args.manifest, args.state, args.accept)
            if args.level >= 3:
                replay.check_replay(store, report, args.manifest, args.periods)
    except StoreError as e:  # a refusal that stopped the command
        report.fail(0, e.code, store.root, e.message)
    except Exception as e:  # a hostile input this reader did not foresee: reported, never a traceback
        report.fail(0, "check-error", store.root, f"stopped on {type(e).__name__}: {str(e)[:200]}")
    if output is not None:
        _dump(output)
    if args.json:
        print(report.as_json())
    elif output is None or report.findings:
        print(report.text(args.verbose), file=sys.stderr if output is not None else sys.stdout)
    return 1 if report.failed() else 0
