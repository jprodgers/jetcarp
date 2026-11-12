# Large sandpiles

`jetcarp pile` drops N grains of sand on one tile of an unbounded grid and lets them settle, for N
from one to 2⁶⁴ − 1, on any tiling: the regular grids, every periodic tiling in the catalogue, and
exact patches of the aperiodic ones. It draws the settled pile as a picture, records how it grew, and
saves its progress so a pile that takes hours can be stopped and carried on. The method (it never
builds a tower of N grains, but doubles a smaller pile again and again) is explained under
[Very large piles](../automata/sandpile.md#very-large-piles) in the sandpile chapter.

## Build a pile

```text
$ jetcarp pile --grains 1e6
pile job 1 in /home/ada/.local/share/jetcarp/library.sqlite
pile 1: 1000000 grains on square, step 16/20 (62500 grains), 4.633e7 topplings, 2.8 s; step 17 running 3.0 s
pile 1: 1000000 grains on square, step 18/20 (250000 grains), 7.321e8 topplings, 7.0 s; step 19 running 4.0 s
pile 1: 1000000 grains on square, step 19/20 (500000 grains), 2.909e9 topplings, 15.7 s; step 20 running 1.0 s
done: 1000000 grains, 11583796400 topplings, 296811 sweeps, 20.6 s of toppling
image:   /home/ada/Pictures/JETCARP/piles/2026-11-11_21-20-08_square-1000000/pile.png
preview: /home/ada/Pictures/JETCARP/piles/2026-11-11_21-20-08_square-1000000/preview.png
folder:  /home/ada/Pictures/JETCARP/piles/2026-11-11_21-20-08_square-1000000
job:     1
```

A million grains take 20 steps, one for each binary digit of 1,000,000, and each step is about four
times the work of the one before. N can be written `1000000`, `1e7` or `2^30`.

**On the GPU,** `--gpu` topples periodic tilings on the graphics card. It gives the same pile, and on
the desktop these numbers come from (a Radeon 8060S), a million grains on squares took 1.7 seconds
instead of 20.6 on 32 CPU threads. Aperiodic tilings are toppled on the CPU.

| Option | Effect |
| --- | --- |
| `--grains N` | How many grains (default 100,000) |
| `--tiling ID` | Any tiling id: `square` (the default), `hex`, `triangle`, `snubsq`, `cairo`, `k3-012`, `ih41`, `spectre`, `hat`… (see [Tiling catalogue](../tilings/catalogue.md)) |
| `--nb edge` / `--nb touch` | Neighbours that share an edge (the default; von Neumann on squares), or that touch at all (Moore on squares) |
| `--centre cell` / `vertex` / `edge` | Drop the grains on one tile (the default), or share them among the tiles that meet at a vertex or an edge |
| `--level N` | Aperiodic tilings: the patch level (by default the smallest that should hold the pile, up to 8) |
| `--gpu` | Topple on the GPU (periodic tilings) |
| `--threads N` | CPU threads (default all); the result doesn't depend on it |

The pile is the same however it is built: on the CPU or the GPU, with any number of threads, in one
run or resumed many times.

## The pile's folder

Each pile gets a folder under `<Pictures>/JETCARP/piles/`, named after when it started, its tiling
and its grains (`--out DIR` puts it elsewhere):

| File | What it holds |
| --- | --- |
| `pile.png` | The settled pile, cropped to it, at `--px` pixels per cell (default 2) with a `--margin` of empty cells (default 4) |
| `preview.png` | The same picture scaled to `--preview` pixels on its longest side (default 1024; `0` for none) |
| `pile.json` | What was built and how: tiling, grains, neighbours, centre, the board's size, the pile's width and height, the picture's settings, and a record for every step |
| `growth.csv` | One row per step: grains so far, topplings, sweeps, seconds, the pile's width and height, the board's cells, and what toppled it |
| `look.json` | The palette and its direction |
| `steps/step-NN.pile` | The pile after each step, for resuming |
| `steps/step-NN.partial.pile` | A step stopped part way: the pile as it stood, part-toppled, until the step finishes (see [Stop and carry on](#stop-and-carry-on)) |

The step files are small, because a settled pile holds at most a few grains per tile: the folder of
the million-grain pile above is 592 KB in all. `--no-png` skips the full-size picture of a pile too
big to want one.

## Colours

Heights run along a palette: height 1 at its first stop and the tallest at its last, with empty
tiles in its background colour. The default is Ember. `--palette NAME` picks another (`jetcarp
palettes` lists them, or give hex colours), and `--reverse` runs it the other way. See
[Palettes and colouring](palettes.md).

To recolour a pile after it is built, resume it with the picture options you want:

```sh
jetcarp pile --resume /home/ada/Pictures/JETCARP/piles/2026-11-11_21-20-08_square-1000000 --palette Tide --reverse
```

A finished pile given `--palette`, `--reverse`, `--px`, `--margin`, `--preview` or `--no-png` is
exported again in them (`pile.png`, `preview.png` and `pile.json`), and the options it wasn't given
stay as they were. Without any, it is exported again only when its `look.json` has other colours
than its pictures: the app's sandpile window writes them there. In the app, open the pile in the
sandpile window (below), choose the colours, and use *Write pile.png, preview.png, and pile.json in
these colours*.

## Stop and carry on

Every pile is a job in your library, like a search, so the usual job commands work on it:

```text
$ jetcarp pile --grains 1e9 --gpu --minutes 0.25 --label big
…
pile 5: 1000000000 grains on square, step 22/30 (3906250 grains), 1.750e11 topplings, 13.5 s
pile 5 paused (at its budget); `jetcarp jobs resume 5` resumes it exactly

$ jetcarp jobs list
    5  pile     paused   2026-11-12T02:21:08Z  pile 5: 1000000000 grains on square, step 22/30 (3906250 grains), 1.750e11 topplings, 13.5 s
    …
```

- `--minutes M` or `--until UNIX-TIME` stops it after a while, and `jetcarp jobs stop JOB` or *Stop*
  in the app stops it at once.
- `jetcarp jobs resume JOB` carries it on. So does `jetcarp pile --resume JOB-OR-FOLDER`, which keeps
  the pile's settings (its picture options too, until it is finished: see [Colours](#colours)).
  Add `--gpu` to resume on the GPU again: it is chosen for each run, not stored with the pile.
- `--detach` runs the pile in a process of its own and returns at once, so `jetcarp jobs status JOB`
  can follow it.

A step stopped part way keeps its work. The last few steps of a billion-grain pile each take
minutes, so a stop, or the end of a run's `--minutes`, usually comes in the middle of one. The pile
is then saved as it stands, with that step's grains added and its tiles part-toppled, in
`steps/step-NN.partial.pile`, and the next run carries on toppling it from there; it doesn't add the
grains again or start the step over. The finished pile doesn't depend on the order its tiles
topple in (the sandpile is abelian), so it is the same pile, with the same toppling and sweep
counts, as a run that was never stopped. So a run of any length makes progress, on the CPU or the
GPU. Once two steps have run, `jetcarp jobs status` and the app show an estimate of the time left.

## In the app

*Tools → Large sandpile…* builds the same piles. It has the options above, draws the pile as it
grows, and shows each step's progress and the time left. *Stop (resumable)* stops the pile, and
*Resume* carries it on from where it stopped, in the middle of a step too; a pile stopped elsewhere
(from the command line, or the *Hunts…* window) can be resumed there too.

## From scripts and other tools

The same piles are `jetcarp search pile` (the pile as a search strategy, taking the same options),
`sandpile_pile(#{ … })` in Rhai scripts (see [Scripting](../reference/scripting.md#large-sandpiles)),
and the MCP server's search tools (see [MCP tools](../reference/mcp.md)).
