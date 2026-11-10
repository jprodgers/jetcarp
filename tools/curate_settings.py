#!/usr/bin/env python3
"""Ask a local vision model to rate stage 1 settings from contact sheets (optional).

Python standard library only; the sheets are drawn by `jetcarp stage1 sheet`, and the model's
ratings are written to the catalog with `jetcarp stage1 rate ... --rater model:<name>`, so
`jetcarp stage1 refit` can fit the score to them (with the user's own ratings, which weigh double).

1. Candidates: the best `--candidates` settings of the chosen statuses (default the approval
   queue and auto entries), from `jetcarp stage1 top --json`, skipping settings this model has
   rated before.
2. Batches of `--batch` settings in a seeded random order, each one contact sheet (labels A, B,
   C, ...; every thumbnail shows soup 0 a tenth of the way through its run and at the end) with a
   line of facts per label unless `--no-facts`.
3. The model rates every picture 1 to 5 with a one-line reason (JSON). Each rating becomes a
   `ratings` row (target `setting`, rater `model:<name>`, the reason as the note).
4. With `--refit`, `jetcarp stage1 refit` runs at the end (it refuses below 20 rated settings).

    python3 tools/curate_settings.py                          # default catalog, LM Studio
    python3 tools/curate_settings.py --library /tmp/lib --candidates 48 --batch 8 --refit
    python3 tools/curate_settings.py --stub                   # no model: a fixed rule rates

`--stub` replaces the model with a deterministic stand-in (it rates from the facts: ships and
oscillators up, noise and empty frames down) so the whole path, sheets to ratings, can be tested
with LM Studio off; `tools/test_curate_settings.py` does that. A text log (prompts, answers,
timings; no images) goes to tools/curate_logs/.
"""

import argparse
import base64
import datetime
import json
import os
import random
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)

SYSTEM = (
    "You are rating pictures of cellular-automaton runs for a gallery of visually interesting "
    "settings. Each label shows one setting twice: early in the run (left) and at the end "
    "(right). Judge what you SEE: favour clear structure and variety - gliders or spaceships, "
    "oscillators, growing crystals or blobs, regions with different textures, patterns that "
    "keep changing without turning to noise. Rate low: empty or nearly empty pictures, a few "
    "dots, uniform fills, and pure noise. Reply with JSON only."
)

SCHEMA = {
    "type": "object",
    "properties": {
        "ratings": {"type": "array", "items": {
            "type": "object",
            "properties": {"label": {"type": "string"},
                           "rating": {"type": "integer", "minimum": 1, "maximum": 5},
                           "reason": {"type": "string"}},
            "required": ["label", "rating", "reason"]}},
    },
    "required": ["ratings"],
}

LABELS = [chr(ord("A") + i) for i in range(26)]


def run_jetcarp(args, *cmd):
    lib = ["--library", args.library] if args.library else []
    full = [args.jetcarp, "stage1"] + lib + list(cmd)
    r = subprocess.run(full, capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"{' '.join(full)} failed:\n{r.stderr.strip()}")
    return r.stdout


def candidates(args):
    rows = json.loads(run_jetcarp(args, "top", "--json", "--status", args.status,
                               "--limit", str(args.candidates * 4)))
    rater = "model:" + args.model_name()
    done = already_rated(args, rater)
    rows = [r for r in rows if str(r["id"]) not in done][: args.candidates]
    return rows


def already_rated(args, rater):
    """Settings this rater has rated (read with sqlite3 when the catalog is reachable)."""
    path = args.library or os.environ.get("JETCARP_LIBRARY")
    if not path:
        docs = os.path.join(os.path.expanduser("~"), "Documents")
        path = os.path.join(docs, "jetcarp-out", "catalog.sqlite")
    if os.path.isdir(path):
        path = os.path.join(path, "catalog.sqlite")
    if not os.path.exists(path):
        return set()
    import sqlite3
    c = sqlite3.connect(path)
    try:
        return {r[0] for r in c.execute(
            "SELECT target_id FROM ratings WHERE target_kind = 'setting' AND rater = ?", (rater,))}
    finally:
        c.close()


def facts(r):
    d = r["descriptors"]
    parts = [r["notation"], r["tiling_name"], f"score {r['score']:.0f}", r["descriptors"]["behaviour"]]
    if d.get("ship_kinds"):
        parts.append(f"{d['ship_kinds']} ship kinds")
    if d.get("oscillator_kinds"):
        parts.append(f"{d['oscillator_kinds']} oscillator kinds")
    parts.append(f"final density {d['final_density']:.2f}")
    return ", ".join(parts)


def post(base_url, body, timeout=1800):
    req = urllib.request.Request(base_url.rstrip("/") + "/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def ask_model(args, prompt, png):
    with open(png, "rb") as f:
        b64 = base64.b64encode(f.read()).decode()
    content = [{"type": "text", "text": prompt},
               {"type": "image_url", "image_url": {"url": "data:image/png;base64," + b64}}]
    body = {
        "model": args.model, "temperature": 0.3, "max_tokens": args.max_tokens,
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": content}],
        "response_format": {"type": "json_schema",
                            "json_schema": {"name": "ratings", "schema": SCHEMA, "strict": True}},
    }
    try:
        r = post(args.base_url, body)
    except urllib.error.HTTPError:
        body.pop("response_format")  # servers without structured output
        r = post(args.base_url, body)
    msg = r["choices"][0]["message"]
    if not (msg.get("content") or "").strip():
        content[0]["text"] += "\n/no_think"
        r = post(args.base_url, body)
        msg = r["choices"][0]["message"]
    return msg.get("content") or ""


def ask_stub(batch, labels):
    """A deterministic stand-in for the model: rates from the facts."""
    out = []
    for lab, r in zip(labels, batch):
        d = r["descriptors"]
        x = 2.0
        x += 1.5 * min(1, d.get("ship_kinds", 0))
        x += 0.5 * min(2, d.get("oscillator_kinds", 0)) / 2
        if d.get("final_density", 0) < 0.01:
            x -= 1
        if d.get("compress", 0) > 0.95:
            x -= 1
        out.append({"label": lab, "rating": int(max(1, min(5, round(x)))),
                    "reason": "stub: rated from the facts"})
    return json.dumps({"ratings": out})


def parse_json(text):
    text = re.sub(r"<think>.*?</think>", "", text or "", flags=re.S).strip()
    m = re.search(r"\{.*\}", text, flags=re.S)
    if not m:
        raise ValueError(f"no JSON in answer: {text[:200]!r}")
    return json.loads(m.group(0))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--jetcarp", default=os.path.join(REPO, "target", "release", "jetcarp"))
    ap.add_argument("--library", help="catalog (a folder or .sqlite file; default: $JETCARP_LIBRARY or <data>/catalog.sqlite)")
    ap.add_argument("--base-url", default="http://localhost:1234/v1")
    ap.add_argument("--model", default="qwen/qwen3.8-27b")
    ap.add_argument("--status", default="candidate,auto", help="statuses to rate (comma separated)")
    ap.add_argument("--candidates", type=int, default=48, help="settings rated in this session")
    ap.add_argument("--batch", type=int, default=8, help="settings per contact sheet")
    ap.add_argument("--thumb", type=int, default=200, help="frame size (pixels)")
    ap.add_argument("--seed", type=int, default=1, help="batch order")
    ap.add_argument("--no-facts", action="store_true", help="show only the pictures")
    ap.add_argument("--max-tokens", type=int, default=8000)
    ap.add_argument("--stub", action="store_true", help="no model: a fixed rule rates (for tests)")
    ap.add_argument("--refit", action="store_true", help="refit the score at the end")
    ap.add_argument("--dry-run", action="store_true", help="ask but write no ratings")
    ap.add_argument("--no-log", action="store_true")
    ap.add_argument("--sheets", help="keep the contact sheets shown to the model in this folder")
    args = ap.parse_args(argv)
    args.model_name = lambda: "stub" if args.stub else args.model.split("/")[-1]

    rows = candidates(args)
    if not rows:
        print("no settings to rate")
        return 0
    rng = random.Random(args.seed)
    rng.shuffle(rows)
    log = []
    written = 0
    if args.sheets:
        os.makedirs(args.sheets, exist_ok=True)
        tmp = args.sheets
    else:
        tmp = tempfile.mkdtemp(prefix="jetcarp-curate-settings-")
    rater = "model:" + args.model_name()
    session = datetime.datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
    for b in range(0, len(rows), args.batch):
        batch = rows[b: b + args.batch]
        labels = LABELS[: len(batch)]
        png = os.path.join(tmp, f"{session}-batch{b // args.batch}.png")
        run_jetcarp(args, "sheet", "--ids", ",".join(str(r["id"]) for r in batch),
                 "--labels", ",".join(labels), "--png", png, "--thumb", str(args.thumb),
                 "--cols", "4", "--label-scale", "3")
        lines = "" if args.no_facts else "\n".join(
            f"{lab}: {facts(r)}" for lab, r in zip(labels, batch))
        prompt = (f"Rate each of the {len(batch)} labelled settings ({', '.join(labels)}) from 1 "
                  f"(dull, empty, or noise) to 5 (very interesting) with a one-line reason."
                  + (f"\n\nWhat was measured:\n{lines}" if lines else ""))
        t0 = time.time()
        try:
            text = ask_stub(batch, labels) if args.stub else ask_model(args, prompt, png)
            got = parse_json(text)["ratings"]
        except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
            print(f"batch {b // args.batch}: no answer ({e})", file=sys.stderr)
            log.append({"batch": b // args.batch, "error": str(e)})
            continue
        by_label = dict(zip(labels, batch))
        for g in got:
            r = by_label.get(str(g.get("label", "")).strip().upper())
            if r is None:
                continue
            rating = int(max(1, min(5, int(g.get("rating", 3)))))
            note = str(g.get("reason", ""))[:300]
            if not args.dry_run:
                run_jetcarp(args, "rate", str(r["id"]), str(rating), "--rater", rater,
                         "--note", note)
                written += 1
            print(f"{r['id']:>7} {r['notation']:20} {rating}  {note}")
        log.append({"batch": b // args.batch, "ids": [r["id"] for r in batch], "prompt": prompt,
                    "answer": got, "seconds": round(time.time() - t0, 1)})
    print(f"{written} ratings by {rater}")
    if args.refit and not args.dry_run:
        r = subprocess.run([args.jetcarp, "stage1"] + (["--library", args.library] if args.library else [])
                           + ["refit"], capture_output=True, text=True)
        print(r.stdout.strip() or r.stderr.strip())
    if not args.no_log:
        os.makedirs(os.path.join(HERE, "curate_logs"), exist_ok=True)
        stamp = datetime.datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
        with open(os.path.join(HERE, "curate_logs", f"settings_{stamp}.json"), "w") as f:
            json.dump({"model": args.model, "stub": args.stub, "batches": log}, f, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
