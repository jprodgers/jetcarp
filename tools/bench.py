#!/usr/bin/env python3
"""Run JETCARP's benchmarks and record their numbers, or compare two records.

    python3 tools/bench.py run [--out DIR] [--only NAME ...]
    python3 tools/bench.py compare OLD/bench.json NEW/bench.json [--noise PERCENT]

`run` builds the benchmarks (in release, `--locked`), runs each with fixed arguments, and writes to
DIR (default `bench-out`): `bench.json` (every number, with the commit, the date, the machine and
the Rust that built them), `bench.csv` (the same, one number a line), `summary.md` (a table for
people) and `logs/` (each benchmark's own output). The benchmarks are the engine's examples
(`crates/jetcarp-core/examples/*_bench.rs`), the GPU's (`crates/jetcarp-gpu/examples/bench.rs`
and `batch_bench.rs`), the hunter's probes (`tests/hunt_bench.rs` in jetcarp-core and
jetcarp-search), the benchmark behind the JETCARP Hardware Score (`jetcarp score`) and the test
that checks its answers on the GPU (`crates/jetcarp-gpu/tests/bench.rs`). Set `JETCARP_GPU` as
for any run: the weekly workflow (`.github/workflows/measure.yml`) sets `software`, Mesa's
lavapipe, so its GPU numbers are a software renderer's.

`compare` prints a Markdown table of every number the two records share (with `--changed`, only
those that moved): old, new, the change, and whether it is better or worse by more than the noise
(default 10%, as GitHub's runners vary that much from run to run; a time under 50 ms is too short
to tell). An answer that changed (a population, a hash, a check) is marked whatever its size.
Anything only one record has is listed after.

Every program runs with JETCARP's folders pointed at scratch folders inside DIR, so nothing touches
the machine's own settings, data or pictures. Standard library only.
"""
import argparse
import csv
import datetime
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FORMAT = 1

# ------------------------------------------------------------------------------------- parsing

DURATION = re.compile(r"([\d.]+)(ns|µs|us|ms|s)\b")
SCALE = {"ns": 1e-9, "µs": 1e-6, "us": 1e-6, "ms": 1e-3, "s": 1.0}


def seconds(text):
    """A Rust `Duration` printed with `{:?}` ("26.69ms", "1.02s") in seconds."""
    m = DURATION.fullmatch(text.strip())
    if not m:
        raise ValueError(f"not a duration: {text!r}")
    return float(m.group(1)) * SCALE[m.group(2)]


def better_for(name, unit):
    """Which way is better for a number: "higher", "lower", "same" (an answer, which must not
    change) or "" (a setting or a size, which says nothing)."""
    n = f"{name} {unit}".lower()
    if any(w in n for w in ("hash", "population", "pop ", "final live", "answer", "correct", "check")):
        return "same"
    if "/s" in n or "speed-up" in n or n.startswith("score"):
        return "higher"
    if any(w in n for w in ("second", "secs", " ms", "(ms)", "mb", "nodes", "bytes/cell", "wall")):
        return "lower"
    if unit in ("s", "ms", "us"):
        return "lower"
    return ""


class Record:
    def __init__(self):
        self.results = []
        self.failures = []

    def add(self, bench, case, metric, value, unit="", better=None):
        if better is None:
            better = better_for(metric, unit)
        case = " ".join(case.split())
        self.results.append(
            {"bench": bench, "case": case, "metric": metric, "value": value, "unit": unit, "better": better}
        )


def number(cell):
    """A table cell as a number and a unit ("2.43x" is 2.43 "x"), or None."""
    m = re.fullmatch(r"(-?[\d.]+(?:e[-+]?\d+)?)\s*(x|%)?", cell.strip())
    if not m:
        return None
    try:
        v = float(m.group(1))
    except ValueError:
        return None
    return (int(v) if re.fullmatch(r"-?\d+", m.group(1)) else v), m.group(2) or ""


def parse_table(rec, bench, text, key, columns=None):
    """Markdown table rows (`| a | b |`). The header names the columns; a run that prints rows
    without one (`bench --only K`) gives `columns`. `key` names the columns that say which case a
    row is; every other column is a number (or a check, such as "equal")."""
    header = None
    found = 0
    for line in text.splitlines():
        line = line.strip()
        if not (line.startswith("|") and line.endswith("|")):
            continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        if all(re.fullmatch(r":?-+:?", c) for c in cells):
            continue
        # Every data row has a number in it (its cells); a row with none is the header.
        if not any(number(c) for c in cells):
            header = cells
            continue
        names = header or columns
        if names is None or len(cells) != len(names):
            raise ValueError(f"a row does not match its columns: {line}")
        row = dict(zip(names, cells))
        case = ", ".join(row[k] for k in key)
        for name, cell in row.items():
            if name in key:
                continue
            n = number(cell)
            if n is not None:
                rec.add(bench, case, name, n[0], n[1])
            else:
                rec.add(bench, case, name, cell, "", "same")
        found += 1
    if not found:
        raise ValueError("no table rows in the output")


def parse_lines(rec, bench, text, pattern, fields):
    """Each line matching `pattern`: its named groups, through `fields` (group -> metric, unit,
    and a function from the group's text to its value; the group `case` names the case)."""
    found = 0
    for m in re.finditer(pattern, text, re.M):
        g = m.groupdict()
        case = g.get("case", "").strip()
        for group, (metric, unit, conv) in fields.items():
            rec.add(bench, case, metric, conv(g[group]), unit)
        found += 1
    if not found:
        raise ValueError(f"no line matched {pattern!r}")


def parse_score(rec, bench, text):
    """`jetcarp score --json`: the score, and each part on each device."""
    s = json.loads(text)["score"]
    rec.add(bench, "JETCARP Hardware Score", "score", s["score"], "", "higher")
    rec.add(bench, "JETCARP Hardware Score", "benchmark seconds", s["seconds"], "s", "lower")
    for p in s["parts"]:
        case = f"{p['id']} on {p['device']}"
        unit = p.get("unit", "")
        rec.add(bench, case, p.get("label", p["id"]), p["value"], unit,
                "higher" if "/s" in unit else "lower" if unit == "bytes/cell" else "")
        if "correct" in p:
            rec.add(bench, case, "answer correct", "yes" if p["correct"] else "NO", "", "same")


def parse_libtest(rec, bench, text):
    """A test binary's summary line: the tests passed, and how long they took."""
    m = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed;.*finished in ([\d.]+)s", text)
    if not m:
        raise ValueError("no test result line")
    rec.add(bench, "the test", "result", m.group(1), "", "same")
    rec.add(bench, "the test", "seconds", float(m.group(4)), "s", "lower")


# ----------------------------------------------------------------------------------- benchmarks

HASHLIFE = r"^(?P<case>\S+\s+gen\s+\d+):\s+(?P<secs>[\d.]+) s\s+pop\s+(?P<pop>\d+)\s+nodes (?P<nodes>\d+)"
GPU_UNPACED = ["Tiling", "Cells", "Reads/gen", "Gens", "CPU gens/s",
               "GPU gens/s (incl. upload + readback)", "GPU steady gens/s", "Speed-up", "Hashes"]
GPU_PACED = ["Tiling", "Cells", "CPU ms/frame", "GPU ms/frame", "GPU / CPU",
             "CPU path: processor ms/frame", "GPU path: processor ms/frame", "Hashes"]
GPU_FAMILIES = ["Family", "Cells", "CPU gens/s", "GPU gens/s", "Speed-up", "Hashes"]


def benchmarks(target):
    """(name, what it measures, command, parse). Each runs in a few seconds to a few minutes on
    a four-core runner. The arguments are fixed, so two records compare like with like."""
    ex = target / "release" / "examples"
    jetcarp = target / "release" / "jetcarp"
    cargo_test = ["cargo", "test", "--release", "--locked", "-q"]
    out = []

    out.append(("hashlife", "HashLife on classic patterns and soups (jetcarp-core hashlife_bench)",
                [ex / "hashlife_bench"],
                lambda r, n, t: parse_lines(r, n, t, HASHLIFE, {
                    "secs": ("seconds", "s", float),
                    "pop": ("population", "", int),
                    "nodes": ("peak nodes", "", int)})))
    for fam, level, what in [("spectre", 7, "graph"), ("spectre", 7, "verify"),
                             ("spectre", 7, "topology"), ("hat", 6, "topology")]:
        name = f"patch-{fam}-{level}-{what}"
        args = [ex / "patch_bench", fam, str(level), what]
        if what == "graph":
            parse = lambda r, n, t: parse_lines(r, n, t,
                r"^(?P<case>\w+ level \d+): (?P<tiles>\d+) tiles; places (?P<places>\S+), graph (?P<graph>\S+);.*peak (?P<peak>[\d.]+) MB", {
                    "tiles": ("tiles (answer)", "", int), "places": ("places seconds", "s", seconds),
                    "graph": ("graph seconds", "s", seconds), "peak": ("peak memory MB", "MB", float)})
        elif what == "verify":
            parse = lambda r, n, t: parse_lines(r, n, t,
                r"^(?P<case>\w+ level \d+): .*?in (?P<secs>[\d.]+(?:ns|µs|us|ms|s))\b.*peak (?P<peak>[\d.]+) MB", {
                    "secs": ("verify seconds", "s", seconds), "peak": ("peak memory MB", "MB", float)})
        else:
            parse = lambda r, n, t: parse_lines(r, n, t,
                r"^(?P<case>.+?) in (?P<secs>[\d.]+(?:ns|µs|us|ms|s)); peak (?P<peak>[\d.]+) MB; holds (?P<held>[\d.]+) MB \((?P<per>[\d.]+) bytes per tile\)", {
                    "secs": ("build seconds", "s", seconds), "peak": ("peak memory MB", "MB", float),
                    "per": ("bytes per tile", "bytes", float)})
        out.append((name, f"An exact aperiodic patch: {fam} level {level}, {what} (patch_bench)", args, parse))
    for fam, grains in [("square", "1e6"), ("hex", "1e6")]:
        out.append((f"pile-{fam}-{grains}", f"A sandpile of {grains} grains on {fam}, on every core (pile_bench)",
                    [ex / "pile_bench", fam, grains, "edge"],
                    lambda r, n, t: parse_lines(r, n, t,
                        r"^(?P<case>\w+ \d+ \w+): (?P<secs>[\d.]+) s, (?P<top>\d+) topplings, (?P<sweeps>\d+) sweeps", {
                            "secs": ("seconds", "s", float), "top": ("topplings (answer)", "", int),
                            "sweeps": ("sweeps", "", int)})))
    out.append(("window-spectre", "Sliding-window rebuilds on the Spectre, one thread (window_bench)",
                [ex / "window_bench", "spectre", "24"],
                lambda r, n, t: parse_lines(r, n, t, r"^(?P<case>[^:\n]+): (?P<ms>[\d.]+) ms", {
                    "ms": ("ms per window", "ms", float)})))
    for k, what in [(2, "squares, 65,536 cells"), (4, "squares, a million cells"),
                    (8, "4.8.8, 65,522 cells"), (15, "3.4.6.4, 259,584 cells")]:
        out.append((f"gpu-life-{k}", f"Life on the CPU and the GPU, {what} (jetcarp-gpu bench --only {k})",
                    [ex / "bench", "--only", str(k)],
                    lambda r, n, t: parse_table(r, n, t, ["Tiling", "Cells"], GPU_UNPACED)))
    out.append(("gpu-life-paced", "Life a frame at a time, squares, 65,536 cells (jetcarp-gpu bench --paced --only 2)",
                [ex / "bench", "--paced", "--only", "2"],
                lambda r, n, t: parse_table(r, n, t, ["Tiling", "Cells"], GPU_PACED)))
    out.append(("gpu-families", "Every rule family of the world stepper, 128k cells, CPU and GPU (bench --families --size 1)",
                [ex / "bench", "--families", "--size", "1"],
                lambda r, n, t: parse_table(r, n, t, ["Family", "Cells"], GPU_FAMILIES)))
    out.append(("gpu-batch", "The GPU batch stepper: 32 worlds a case, 500 generations (batch_bench)",
                [ex / "batch_bench", "--worlds", "32"],
                lambda r, n, t: parse_table(r, n, t, ["Case"])))
    out.append(("hunt-throughput", "Soup hunts, one thread: Life on squares and hexagons (jetcarp-core hunt_bench)",
                cargo_test + ["-p", "jetcarp-core", "--test", "hunt_bench", "--", "--ignored", "--nocapture",
                              "--exact", "hunt_throughput"],
                lambda r, n, t: parse_lines(r, n, t,
                    r"^(?P<case>\S+ on \S+, \d+ threads): \d+ soups in (?P<secs>[\d.]+)s = (?P<rate>\d+) soups/s, mean (?P<gens>\d+) gens", {
                        "secs": ("seconds", "s", float), "rate": ("soups/s", "soups/s", float),
                        "gens": ("mean generations (answer)", "", int)}),
                {"N": "2000", "T": "1", "W": "B3/S23@square,B2/S34@hex"}))
    out.append(("hunt-profile", "Where a soup's time goes, a hunt's default limits (jetcarp-search hunt_bench)",
                cargo_test + ["-p", "jetcarp-search", "--test", "hunt_bench", "--", "--ignored", "--nocapture",
                              "--exact", "hunt_profile"],
                lambda r, n, t: parse_lines(r, n, t,
                    r"^(?P<case>[^:\n]+): \d+ soups in (?P<secs>[\d.]+)s \((?P<rate>\d+)/s\): setup (?P<setup>[\d.]+)s, step (?P<step>[\d.]+)s \((?P<us>[\d.]+) us/gen, mean (?P<gens>\d+) gens\), post (?P<post>[\d.]+)s, census (?P<census>[\d.]+)s", {
                        "secs": ("seconds", "s", float), "rate": ("soups/s", "soups/s", float),
                        "setup": ("setup seconds", "s", float), "step": ("step seconds", "s", float),
                        "us": ("µs per generation", "us", float), "post": ("post seconds", "s", float),
                        "census": ("census seconds", "s", float),
                        "gens": ("mean generations (answer)", "", int)}),
                {"N": "1000"}))
    out.append(("score", "The benchmark behind the JETCARP Hardware Score (jetcarp score --json --no-save)",
                [jetcarp, "score", "--json", "--no-save"], parse_score))
    out.append(("gpu-score-answers", "The Hardware Score's answers on the GPU (jetcarp-gpu tests/bench.rs)",
                cargo_test + ["-p", "jetcarp-gpu", "--test", "bench", "--", "--nocapture"], parse_libtest))
    return [b if len(b) == 5 else b + ({},) for b in out]


# ------------------------------------------------------------------------------------- running

def git(*args):
    try:
        return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return ""


def machine():
    cpu = platform.processor()
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    mem = None
    try:
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                mem = round(int(line.split()[1]) / 1048576, 1)
    except OSError:
        pass
    image = " ".join(x for x in (os.environ.get("ImageOS", ""), os.environ.get("ImageVersion", "")) if x)
    return {
        # A GitHub runner's name; never the host name of a machine of one's own.
        "name": os.environ.get("RUNNER_NAME", ""),
        "os": f"{platform.system()} {platform.release()}",
        "image": image,
        "cpu": cpu,
        "threads": os.cpu_count(),
        "memory_gb": mem,
    }


def run(args):
    out = Path(args.out).resolve()
    if out.exists():
        shutil.rmtree(out)
    (out / "logs").mkdir(parents=True)
    scratch = out / "scratch"
    env = dict(os.environ)
    for var, sub in [("JETCARP_CONFIG_DIR", "config"), ("JETCARP_DATA_DIR", "data"), ("JETCARP_OUT", "out"),
                     ("JETCARP_RUNTIME_DIR", "run")]:
        (scratch / sub).mkdir(parents=True, exist_ok=True)
        env[var] = str(scratch / sub)
    env["JETCARP_LIBRARY"] = str(scratch / "data" / "library.sqlite")

    meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
                                     cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    target = Path(meta["target_directory"])
    benches = benchmarks(target)
    if args.only:
        unknown = set(args.only) - {b[0] for b in benches}
        if unknown:
            sys.exit(f"bench.py: no benchmark named {', '.join(sorted(unknown))}")
        benches = [b for b in benches if b[0] in args.only]

    print("Building the benchmarks (release)", flush=True)
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "jetcarp-core", "-p", "jetcarp-gpu",
                    "--examples", "-p", "jetcarp", "--bin", "jetcarp"], cwd=ROOT, check=True)
    subprocess.run(["cargo", "test", "--release", "--locked", "--no-run", "-p", "jetcarp-core", "--test", "hunt_bench",
                    "-p", "jetcarp-search", "--test", "hunt_bench", "-p", "jetcarp-gpu", "--test", "bench"],
                   cwd=ROOT, check=True)

    rec = Record()
    for name, what, cmd, parse, extra in benches:
        print(f"{name}: {what}", flush=True)
        e = dict(env, **extra)
        t0 = time.monotonic()
        p = subprocess.run([str(c) for c in cmd], cwd=ROOT, env=e, stdin=subprocess.DEVNULL,
                           capture_output=True, text=True)
        wall = time.monotonic() - t0
        text = p.stdout + ("\n" if p.stdout and not p.stdout.endswith("\n") else "") + p.stderr
        (out / "logs" / f"{name}.txt").write_text(f"$ {' '.join(str(c) for c in cmd)}\n{text}")
        if p.returncode != 0:
            rec.failures.append({"bench": name, "error": f"exit status {p.returncode}"})
            print(f"  failed: exit status {p.returncode} (logs/{name}.txt)", flush=True)
            continue
        # The score's JSON is on stdout alone.
        try:
            parse(rec, name, p.stdout if parse is parse_score else text)
        except (ValueError, KeyError, json.JSONDecodeError) as err:
            rec.failures.append({"bench": name, "error": f"its output did not parse: {err}"})
            print(f"  its output did not parse: {err}", flush=True)
            continue
        rec.add(name, "the whole program", "wall seconds", round(wall, 3), "s", "lower")
        print(f"  {wall:.1f} s", flush=True)

    rustc = subprocess.run(["rustc", "--version"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    # The GPU the score measured, if any: a device that is neither the processor nor the disk.
    gpu = next((r["case"].split(" on ", 1)[1] for r in rec.results
                if r["bench"] == "score" and " on " in r["case"]
                and not r["case"].split(" on ", 1)[1].startswith(("processor", "the data"))), "")
    record = {
        "format": FORMAT,
        "commit": os.environ.get("GITHUB_SHA") or git("rev-parse", "HEAD"),
        "ref": os.environ.get("GITHUB_REF_NAME") or git("rev-parse", "--abbrev-ref", "HEAD"),
        "date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "runner": machine(),
        "toolchain": rustc,
        "jetcarp_gpu": os.environ.get("JETCARP_GPU", ""),
        "gpu": gpu,
        "benchmarks": [{"name": n, "what": w} for n, w, *_ in benches],
        "results": rec.results,
        "failures": rec.failures,
    }
    (out / "bench.json").write_text(json.dumps(record, indent=1, ensure_ascii=False) + "\n")
    with open(out / "bench.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["commit", "date", "runner", "toolchain", "bench", "case", "metric", "value", "unit", "better"])
        for r in rec.results:
            w.writerow([record["commit"], record["date"], record["runner"]["cpu"], rustc,
                        r["bench"], r["case"], r["metric"], r["value"], r["unit"], r["better"]])
    (out / "summary.md").write_text(summary(record))
    shutil.rmtree(scratch, ignore_errors=True)
    print(f"Wrote {out / 'bench.json'}, bench.csv and summary.md", flush=True)
    return 1 if rec.failures else 0


def fmt(v):
    if isinstance(v, float):
        if v == 0 or 0.01 <= abs(v) < 1e7:
            return f"{v:,.3f}".rstrip("0").rstrip(".")
        return f"{v:.4g}"
    if isinstance(v, int):
        return f"{v:,}"
    return str(v)


def summary(record):
    r = record["runner"]
    lines = [
        "## Benchmarks",
        "",
        f"Commit `{record['commit'][:12]}` ({record['ref']}), {record['date']}; {record['toolchain']}.",
        f"Runner: {r['cpu']}, {r['threads']} threads, {r['memory_gb']} GB, {r['os']}"
        + (f", image {r['image']}" if r["image"] else "") + ".",
        f"GPU: `JETCARP_GPU={record['jetcarp_gpu'] or '(unset)'}`" + (f", {record['gpu']}" if record["gpu"] else "") + ".",
        "",
        "Times on shared runners vary by 10% or so between runs: compare two records with",
        "`python3 tools/bench.py compare OLD/bench.json NEW/bench.json`.",
        "",
    ]
    if record["failures"]:
        lines += ["**Failed:** " + "; ".join(f"{f['bench']} ({f['error']})" for f in record["failures"]), ""]
    # One table per benchmark: a row for each case, a column for each measure.
    what = {b["name"]: b["what"] for b in record.get("benchmarks", [])}
    by_bench = {}
    for x in record["results"]:
        by_bench.setdefault(x["bench"], []).append(x)
    for bench, rows in by_bench.items():
        measures, cases = [], {}
        for x in rows:
            unit = f" ({x['unit']})" if x["unit"] and x["unit"] not in x["metric"] else ""
            m = x["metric"] + unit
            if m not in measures:
                measures.append(m)
            cases.setdefault(x["case"], {})[m] = fmt(x["value"])
        lines += [f"**{bench}**: {what.get(bench, '')}", "",
                  "| Case | " + " | ".join(measures) + " |",
                  "| --- |" + " ---: |" * len(measures)]
        for case, vals in cases.items():
            lines.append(f"| {case} | " + " | ".join(vals.get(m, "") for m in measures) + " |")
        lines.append("")
    return "\n".join(lines)


# ------------------------------------------------------------------------------------ comparing

TO_SECONDS = {"s": 1.0, "ms": 1e-3, "us": 1e-6}
SHORTEST_S = 0.05


def compare(args):
    old, new = (json.loads(Path(p).read_text()) for p in (args.old, args.new))
    key = lambda r: (r["bench"], r["case"], r["metric"])
    a = {key(r): r for r in old["results"]}
    b = {key(r): r for r in new["results"]}
    noise = args.noise / 100
    print(f"Old: `{old['commit'][:12]}` {old['date']}, {old['toolchain']}, {old['runner']['cpu']}")
    print(f"New: `{new['commit'][:12]}` {new['date']}, {new['toolchain']}, {new['runner']['cpu']}")
    for what in ("toolchain", "jetcarp_gpu", "gpu"):
        if old.get(what) != new.get(what):
            print(f"\n**Not like with like:** {what} differs ({old.get(what)!r} and {new.get(what)!r}).")
    if old["runner"].get("cpu") != new["runner"].get("cpu"):
        print("\n**Not like with like:** the runners' processors differ.")
    print("\n| Benchmark | Case | Measure | Old | New | Change | |")
    print("| --- | --- | --- | ---: | ---: | ---: | --- |")
    worse = changed = 0
    for k in [k for k in a if k in b]:
        x, y = a[k]["value"], b[k]["value"]
        better, unit = b[k]["better"], b[k]["unit"]
        mark, change = "", ""
        if isinstance(x, (int, float)) and isinstance(y, (int, float)) and better != "same":
            if x:
                ratio = y / x - 1
                change = f"{ratio:+.1%}"
                # A time of a few milliseconds is mostly the timer and the machine's noise.
                short = max(x, y) * TO_SECONDS.get(unit, 1) < SHORTEST_S if unit in TO_SECONDS else False
                if better in ("higher", "lower") and abs(ratio) > noise:
                    if short:
                        mark = "(too short to tell)"
                    else:
                        good = (ratio > 0) == (better == "higher")
                        mark = "better" if good else "**worse**"
                        worse += not good
        elif x != y:
            mark = "**answer changed**"
            changed += 1
        if args.changed and not mark.strip("()").startswith(("better", "**")):
            continue
        print(f"| {k[0]} | {k[1]} | {k[2]} | {fmt(x)} | {fmt(y)} | {change} | {mark} |")
    only_old = [k for k in a if k not in b]
    only_new = [k for k in b if k not in a]
    for label, ks in (("Only in the old record", only_old), ("Only in the new record", only_new)):
        if ks:
            print(f"\n{label}: " + "; ".join(" / ".join(k) for k in ks))
    print(f"\n{worse} worse by more than {args.noise:g}%; {changed} answer{'' if changed == 1 else 's'} changed.")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="build and run the benchmarks, and record their numbers")
    r.add_argument("--out", default="bench-out", help="the folder to write (default: bench-out)")
    r.add_argument("--only", nargs="+", metavar="NAME", help="run only these benchmarks")
    c = sub.add_parser("compare", help="compare two records")
    c.add_argument("old")
    c.add_argument("new")
    c.add_argument("--noise", type=float, default=10.0, help="the change, in percent, below which a "
                   "difference counts as noise (default 10)")
    c.add_argument("--changed", action="store_true", help="list only the numbers that got better or "
                   "worse, and the answers that changed")
    args = ap.parse_args()
    sys.exit(run(args) if args.cmd == "run" else compare(args))


if __name__ == "__main__":
    main()
