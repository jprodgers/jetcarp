# Scenes: saving, sharing, reproducing

A *scene* is a small JSON file that holds everything needed to reproduce a run: the tiling, the
automata with their rules and seeds, the starting state of the random-number generator, and the log
of every change made since. Opening a scene replays that log, so the world comes back exactly as it
was. A saved scene also records the generation and the *state hash* it reached, so anyone can check
that it still gets there. This guide covers saving and opening scenes in the app, running and checking
them from the command line, sharing them, and getting a scene back out of a picture. The file itself
is described in [Scene file format](../reference/scene-format.md).

## Save and open a scene in the app

- **File → Save scene…** (Ctrl+S) writes the current world as a scene. The file holds the tiling, the
  automata, the random state and every change since the last reset or change of tiling (the
  *Command log* pane lists them), together with the view (where the camera is, the zoom and the speed)
  and a record of the generation and state hash. The Scene pane confirms it with the file's path and
  the hash.
- **File → Open scene…** (Ctrl+O) opens one. The world is rebuilt and its log replayed, so it opens at
  the saved generation and in the saved view. You can also open a scene when the app starts:
  `jetcarp --scene scene.json`.
- **Quick Start entries are scenes.** Open one, change what you like, and save it.

Everything you do to a world goes into its log: painting cells, pasting library patterns, adding and
changing automata, reseeding, editing palettes, running. Consecutive runs are merged into one `step`
entry, so a long session does not make a long file. *Edit → Reset*, choosing another tiling, and a
new random state start the log again from a new base.

The state hash is shown at the right end of the menu bar, after the generation: `hash 1adf6e32`, the
first eight of its sixteen digits. Click it, or use *Edit → Copy state hash*, to copy the whole hash.
Worlds of more than about a million cells compute the hash only while paused, and show `hash …`
while they run.

## Run a scene from the command line

`jetcarp run` loads a scene, replays its log, runs more generations if asked, and reports where it
ended:

```text
$ jetcarp run crates/jetcarp-core/data/scenes/rule30-life-ants.json --gens 100 --png out.png --save out.json
scene:      crates/jetcarp-core/data/scenes/rule30-life-ants.json
tiling:     Square (parallelogram) (square): 38400 cells, 240x160 unit cells, 1 tile type(s), storage dense, edges dead
backend:    cpu (32 threads)
generation: 100
hash:       1adf6e3234b7f199
status:     Running
done:       no
field 0:    shared binary, 2124 live: Rule 30 [W30], Life [B3/S23], Langton's ant [Ant RL]
png:        out.png (1600x1067)
saved:      out.json
time:       0.03s on 32 threads
```

`--gens N` counts from where the scene ends: run `out.json` (saved at generation 100) with
`--gens 50` and it stops at generation 150. The other options:

| Option | Effect |
| --- | --- |
| `--png FILE` | A picture of the final state, with the scene and hash inside it (see [below](#get-the-scene-back-from-a-picture)) |
| `--max-side N`, `--scale S` | The picture's longest side in pixels (default 1600), or its scale in pixels per world unit (a tile edge is about one unit) |
| `--save FILE` | The resulting scene, with its generation and hash recorded |
| `--threads N` | Worker threads (default: all cores); the hash does not depend on it |
| `--expect HASH` | Fail unless the run ends on this hash |
| `--gpu` | Run the generations on the GPU where the rules allow it; the result is identical |
| `--reseed`, `--reseed-now` | Automatic reseed policies, or one reseed before running (see [Reseeding](reseeding.md)) |
| `--json` | The report as JSON |

## Check that a scene still reproduces

`jetcarp verify` replays a saved scene and compares the generation and hash it reaches with the ones
it recorded:

```text
$ jetcarp verify out.json
ok: generation 100 hash 1adf6e3234b7f199
```

If they differ it says so and exits with status 1:

```text
Error: MISMATCH: recorded gen 100 hash 1adf6e3234b7f198, replayed gen 100 hash 1adf6e3234b7f199
```

A scene with no record, such as one written by hand, cannot be verified (*scene has no recorded
hash*); run it with `jetcarp run --save` to record one. For a check inside a script or a test, use
`jetcarp run scene.json --gens N --expect HASH`, which fails with *hash mismatch: expected …, got …*
unless the run ends on that hash. `jetcarp verify` always replays on the CPU, so it is also the way to
check a run made with `--gpu`.

## The state hash

The state hash is a 64-bit number, printed as sixteen hexadecimal digits, computed from everything
that decides what the world does next: every cell's state and age, sandpile heights, Lenia's values,
the generation, the ants, and the progress of any script or reseed policy. Two worlds with the same
hash are, in practice, in the same state. The hash does not depend on the number of threads or on
whether the CPU or the GPU did the work, and by design it does not depend on the operating system.

One case needs care: a HashLife jump reaches exactly the same cells as stepping, but restarts every
live cell's age at 1, so its state hash differs. Compare the hash of the cell states alone instead
(`states_hash()` in scripts, `cells_hash` in the world status that MCP tools read). How the hash is
computed and tested is in [Reproducibility](../research/reproducibility.md#the-state-hash).

## Share a scene

A scene is one text file, and it carries everything it needs: the rule tables and palettes it uses,
and the cells of any library pattern pasted into it, so it does not need your pattern library. Send
the file; the other person opens it in the app or runs `jetcarp verify` on it to confirm that their
machine reaches the same hash. Scenes work well in version control, and with the output of
`jetcarp doctor` they make a complete bug report.

A few limits apply:

- A scene names its format (now 1). A JETCARP that only knows an older format refuses a newer scene
  rather than guessing.
- Until JETCARP 1.0, a release may change how a rule or a tiling behaves. Such changes are deliberate
  and listed in the [changelog](../about/changelog.md), but an old scene may then reach a different
  hash. The Penrose rhombs already lay out their tiles differently from those of CART, JETCARP's
  predecessor.
- Palettes you made travel inside the scene. A scene that names a palette the other person's
  JETCARP does not know (a built-in palette added in a later release, say) still opens, drawn in
  Ember, with a warning; the name is kept, so saving the scene again keeps it.

## Get the scene back from a picture

Every PNG made by `jetcarp run --png`, *File → Export PNG…*, *File → Export wallpaper…*,
`jetcarp wallpaper`, `jetcarp hashlife --png` and a script's `save_png` carries its scene inside it,
as PNG text chunks: `jetcarp:scene` (the whole scene), `jetcarp:version`, `jetcarp:generation`,
`jetcarp:hash` and `jetcarp:dead_boundary`. A picture can therefore always be traced back to the run
that made it. (Animations and frames from `jetcarp media` do not carry them.)

JETCARP does not yet open a scene from a PNG itself, but any program that reads PNG text chunks can
take it out. With Python and the Pillow library:

```python
from PIL import Image
open("scene.json", "w").write(Image.open("out.png").text["jetcarp:scene"])
```

```text
$ jetcarp verify scene.json
ok: generation 100 hash 1adf6e3234b7f199
```

## Scenes everywhere else

- **Scripts** read and write scenes with `load_scene(path)`, `scene_world(json)`, `w.save_scene(path)`
  and `w.scene()` (see [Scripting (Rhai) API](../reference/scripting.md)).
- **MCP tools** have `load_scene` and `get_scene` (see [MCP tools](../reference/mcp.md)).
- **Pictures and the desktop.** `jetcarp wallpaper scene.json`, `jetcarp media render --scene`,
  screensaver profiles and rotations all take scene files (see [Pictures and animations](pictures.md)
  and [Wallpapers, screensavers and the background host](wallpapers.md)).
- **This book.** Every figure is made from a scene, named under it, and for the still pictures
  `jetcarp run <scene> --gens N --png out.png` makes it again.

To write a scene by hand, start from the short example in
[Scene file format](../reference/scene-format.md#an-example); anything you leave out takes its
default.
