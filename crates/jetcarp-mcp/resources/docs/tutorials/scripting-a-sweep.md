# 5. Scripting a rule sweep

In this tutorial you write a short Rhai script that runs 64 cellular automaton rules on the Cairo
pentagon tiling, one world per rule, in parallel on every core. Then you read the results back,
lay your favourites out on a contact sheet, and put the best one in your Quick Start menu. It takes
about fifteen minutes, and the sweep itself runs in a couple of seconds.

You need JETCARP installed ([Installing](../getting-started/installing.md)) and a text editor.
Everything here runs from a terminal; the same scripts also run in the app, under
*Tools → Scripts and sweeps…*. Everything the scripts write goes into JETCARP's output folder,
`<Pictures>/JETCARP` (see [Folders](../reference/folders.md)).

## 1. One world

Start with a single world, to see what a script looks like. Save this as `first.rhai`:

```rhai
// Life on Cairo pentagons: run 300 generations and save a picture and a scene.
let w = world(#{ tiling: "cairo", size: "120x80" });
w.add_ca("B3/S23", #{ seed: "random:0.35" });
w.step(300);
out_dir("cairo-life");
w.save_png("cairo-life.png");
w.save_scene("cairo-life.json");
print(`generation ${w.generation()}, ${w.population()} live cells, hash ${w.hash()}`);
```

and run it:

```text
$ jetcarp script first.rhai
generation 300, 1527 live cells, hash 8c936c690b20f095
output: /home/ada/Pictures/JETCARP/2026-11-11_21-27-33_cairo-life
time:   0.10s
```

Line by line:

- `world(…)` makes a world: here 120 × 80 unit cells of the Cairo tiling, with dead edges.
- `add_ca` adds a cellular automaton to it, Conway's Life (`B3/S23`), seeded with 35% of its
  cells alive.
- `step(300)` runs 300 generations.
- `out_dir("cairo-life")` makes a new folder in the output folder, named with the time and the
  label, and later file names are relative to it.
- `save_png` and `save_scene` write a picture and a [scene](../guides/scenes.md).

Your hash will be the same as the one above: the same tiling, rules and seeds always give the same
world, on any computer and at any thread count. `jetcarp verify cairo-life.json` replays the scene
and checks that it reaches that hash again.

## 2. Sweep 64 rules

A *sweep* runs one world per rule. Instead of Life, take a random sample of 64 rules from a family:
births on 2 to 4 live neighbours, survival on 1 to 5, and four states. In a four-state
*Generations* rule, a dying cell fades through two more states before it is empty, which leaves
coloured trails behind anything that moves. Save this as `sweep.rhai`:

```rhai
let s = sweep(#{
    rules: "B/S", birth: "2-4", survive: "1-5", states: 4,
    sample: 64, sample_seed: 1,
    tiling: "cairo", size: "64x40",
    seed: "random:0.35", rng_seed: 1,
    max_gens: 300, palette: "Prism",
    png: #{ size: [480, 300] },
    skip: ["empty", "full"],
    out: "cairo-gen",
});
print(`${s.runs} runs, ${s.images} pictures`);
```

```text
$ jetcarp script sweep.rhai
sweep done: 64 of 64 runs, 50 images, 50 kept, 0 errors in 1.4s
64 runs, 50 pictures
output: /home/ada/Pictures/JETCARP/2026-11-11_21-29-35_cairo-gen
time:   1.37s
```

What the options do:

- `rules: "B/S"` with `birth` and `survive` describes every rule whose births and survivals fall in
  those ranges. `sample: 64` picks 64 of them at random, and `sample_seed` makes it the same 64
  every time.
- `tiling`, `size`, `seed` and `rng_seed` are the same for every run, so the rule is the only
  difference between them.
- `max_gens: 300` runs each world 300 generations; `until` can stop a run earlier, for instance when
  it stops changing.
- `png` sets each picture's size, and `palette` its colours.
- `skip: ["empty", "full"]` doesn't save pictures of runs that died out or filled up, which is why
  50 of the 64 runs have one.

The sweep's folder holds a picture per run (`03_B23_S1_C4.png` and so on) and a manifest, as
`manifest.csv` and `manifest.json`, with a row per run: its rule, picture, final hash, population,
density, period if it settled into one, and, for rules that write row by row, a description of
its rows (`row_class`, `row_hint`). `run.json` records how the sweep was made.

Open the folder and look through the pictures. Some rules die at once, some freeze into specks, some
crawl with worms, some boil.

## 3. Read the results back

The manifest is what makes a sweep useful beyond its pictures. `manifest(folder)` gives a script
its rows, so a second script can sort and filter them. Save this as `pick.rhai`:

```rhai
// The runs still busy at the end, most cells alive first.
let rows = manifest(params.folder).filter(|r| r.kept && r.period == ());
rows.sort(|a, b| b.population - a.population);
for r in rows {
    print(`#${r.index}  ${r.rule}  ${r.population} cells`);
}
```

and give it the sweep's folder with `--set`:

```text
$ jetcarp script pick.rhai --set folder=/home/ada/Pictures/JETCARP/2026-11-11_21-29-35_cairo-gen
#53  B234/S1234/C4  7581 cells
#7  B24/S234/C4  7116 cells
#58  B23/S134/C4  7095 cells
…
```

`--set key=value` puts values in the map `params`. A run that never settled has no period, which
Rhai writes as `()`. Every row has the manifest's columns, so you can sort by `density`, keep only
`row_class == "periodic"`, and so on.

**Measure more as you go.** A sweep can also call a function of yours on every world when its run
ends, and add what it returns to the manifest. Add this option to the sweep above to count each
run's still lifes and oscillators:

```rhai
    keep: |w, n| {
        let o = w.objects();
        #{ still_lifes: o.totals.still_life, oscillators: o.totals.oscillator }
    },
```

and the manifest gains `still_lifes` and `oscillators` columns. Running the sweep again with the same
folder (`folder: "…"` instead of `out:`) reuses the runs already done. Since this changes what each
run records, start a new folder instead.

## 4. A contact sheet of your favourites

`jetcarp sheet` lays chosen runs out on one picture, each labelled with its index and rule:

```text
$ jetcarp sheet --folder /home/ada/Pictures/JETCARP/2026-11-11_21-29-35_cairo-gen \
    --runs "#1,#2,#3,#4,#7" --cols 5 --thumb 240 --png favourites.png
sheet: favourites.png (1248x194)
```

Runs are named by their index in the manifest (`#3`), by their rule text (`"B23/S1/C4"`), or for a
numbered family by their number (`n=588`).

## 5. Keep the best one

When a run is worth coming back to, put it in your Quick Start menu:

```text
$ jetcarp library promote-run /home/ada/Pictures/JETCARP/2026-11-11_21-29-35_cairo-gen "#3" --title "Cairo worms"
promoted: B23/S1/C4 as "Cairo worms" (run #3)
```

The entry holds the run's tiling, rule and starting state, so opening it from the Quick Start menu
replays the run exactly as the sweep ran it. The run is also starred in the folder's
`favourites.json`. A script can do the same with `library().promote_run(folder, "#3")`.

## Where to go from here

- **Bigger sweeps.** A sweep runs on every core and can be stopped with Ctrl-C: it saves its
  manifest first and prints the command that carries it on. Rerunning it with `folder:` set to its
  folder skips the runs already done. The named families `"W"`, `"HexW"`, `"HexW3"`, `"HexT"` and
  `"HexT3 sum"` are numbered, so `rules: "HexW3", from: 0, to: 19682` sweeps all 19,683 ternary
  hexagon rules. The example scripts that ship with JETCARP (*Examples* in the app's script editor,
  and the `crates/jetcarp-script/scripts` folder of the source repository) include sweeps of every
  elementary rule.
- **Without writing code.** The app's *Sweep builder* tab, under *Tools → Scripts and sweeps…*,
  builds the same sweeps from a form, and *Sweep this world* sweeps a rule over whatever world is
  open.
- **More functions.** [Scripting](../reference/scripting.md) lists every function: worlds, runs,
  objects, pictures, the pattern library, searches and large sandpiles.
