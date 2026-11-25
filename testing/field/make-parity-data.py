#!/usr/bin/env python3
"""Make the field test's parity data from a release cycle's parity set.

    python3 testing/field/make-parity-data.py PARITY_FOLDER --from VERSION [--column NAME] [--scenes]

PARITY_FOLDER is a cycle's parity set as the release-candidate test records keep it
(`v0.4.3/parity/`: `scenes/<n>.json`, `scenes/index.json`, `expected.csv` and `long/expected.csv`,
whose scenes are copies of the main set's). This writes, beside this script:

- with `--scenes`, `parity/scenes.tsv`: one scene a line, `n <tab> Quick Start id <tab> the scene
  as compact JSON` (one download instead of 371 files; the scripts write each line back out as
  `<n>.json` before running it);
- `parity/expected-VERSION.tsv` and `parity/long-VERSION.tsv`: `n <tab> id <tab> hash`, the hash
  each scene must reach on the CPU and on every GPU after 300 generations (`expected.csv`) and
  after 5,000 (`long/expected.csv`), taken from the column NAME (default `hash`; `hash_v1` for the
  state hash version 1 that releases before 0.4.3 print).

`tests.json`'s `parity.sets` then names each pair of files with the releases it is for. Standard
library only.
"""
import argparse
import csv
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def rows(path: Path, column: str):
    with path.open(newline="", encoding="utf-8") as fh:
        for r in csv.DictReader(fh):
            h = r.get(column, "").strip()
            if not h:
                sys.exit(f"{path.name}: scene {r['n']} has no {column}")
            if "\t" in r["id"]:
                sys.exit(f"{path.name}: scene {r['n']}'s id has a tab")
            yield int(r["n"]), r["id"], h


def write_tsv(path: Path, lines):
    path.write_text("".join(lines), encoding="utf-8", newline="\n")
    print(f"wrote {path.relative_to(HERE)} ({len(lines)} lines)")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("parity", type=Path, help="the cycle's parity folder")
    ap.add_argument("--from", dest="since", required=True, help="the first release these hashes are for")
    ap.add_argument("--column", default="hash", help="expected.csv's column (hash, hash_v1)")
    ap.add_argument("--scenes", action="store_true", help="also write parity/scenes.tsv")
    a = ap.parse_args()
    out = HERE / "parity"
    out.mkdir(exist_ok=True)
    short = sorted(rows(a.parity / "expected.csv", a.column))
    long = sorted(rows(a.parity / "long" / "expected.csv", a.column))
    known = {n for n, _, _ in short}
    if any(n not in known for n, _, _ in long):
        sys.exit("long/expected.csv names a scene the main set doesn't have")
    if a.scenes:
        lines = []
        for n, i, _ in short:
            scene = json.loads((a.parity / "scenes" / f"{n}.json").read_text(encoding="utf-8"))
            text = json.dumps(scene, separators=(",", ":"), ensure_ascii=False)
            if "\n" in text or "\t" in text:
                sys.exit(f"scene {n} does not fit on one line")
            lines.append(f"{n}\t{i}\t{text}\n")
        write_tsv(out / "scenes.tsv", lines)
    write_tsv(out / f"expected-{a.since}.tsv", [f"{n}\t{i}\t{h}\n" for n, i, h in short])
    write_tsv(out / f"long-{a.since}.tsv", [f"{n}\t{i}\t{h}\n" for n, i, h in long])
    return 0


if __name__ == "__main__":
    sys.exit(main())
