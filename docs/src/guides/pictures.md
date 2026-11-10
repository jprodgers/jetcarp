# Pictures and animations

JETCARP draws every picture with its own renderer, straight from a world, so a picture shows the tiles
in their true shapes at any size. This guide covers still pictures of a world, desktop wallpapers cut
to fit your monitors, animations, and contact sheets that put many runs side by side. Most still
pictures carry their scene inside the PNG, so a picture can always be traced back to the run that made
it (see [Scenes](scenes.md#get-the-scene-back-from-a-picture)).

| To make | In the app | On the command line | In a script |
| --- | --- | --- | --- |
| A picture of the world | *File → Export PNG…* | `jetcarp run --png` | `w.save_png(path)` |
| Desktop wallpapers | *File → Export wallpaper…* | `jetcarp wallpaper` | `jetcarp("wallpaper.export", #{…})` |
| An animation | | `jetcarp media render` | `w.media(#{…})`, `media(#{…})` |
| A contact sheet | *File → Export contact sheet…* | `jetcarp sheet`, `jetcarp quickstart --sheet`, `jetcarp isohedral --png` | `sweep(#{…})` writes one picture per run |

## A picture of the world

**In the app**, *File → Export PNG…* (Ctrl+E) saves the whole tiling as a PNG; the menu shows the size
it will be, such as *Export PNG (2400 × 1600)…*. *Export size*, just below it, sets the scale: *fit*
gives 2400 pixels on the longer side, and a number gives that many pixels per tile (up to 64). Large
images are written in strips, so they need little memory. Tile outlines are drawn where tiles are at
least 7 pixels across.

**On the command line**, `jetcarp run` writes a picture of the state it ends in:

```text
jetcarp run scene.json --gens 500 --png out.png                # 1600 pixels on the longer side
jetcarp run scene.json --gens 500 --png big.png --max-side 8000
jetcarp run scene.json --gens 500 --png exact.png --scale 6    # 6 pixels per world unit
```

A tile edge is about one world unit, so `--scale 6` gives tiles about six pixels across. Very large
pictures (a small scale on a huge grid) are streamed in strips too.

**In a script**, `w.save_png(path)` draws the world at the screen size given by `--screen` (1920 × 1080
by default), fitted and centred. Its options set the `size` (`[w, h]`), `scale`, `max_side`,
`outlines`, a `palette` for this picture only, and the `compression` (`fast`, `balanced` or `high`);
it returns the path, the size and the number of bytes.

## Desktop wallpapers

A wallpaper is a picture at exactly the size of a monitor, or of several monitors side by side.

**In the app**, *File → Export wallpaper…* opens a window that writes desktop-sized PNGs of the world
while the app keeps running:

- **Size:** one of your monitors (as detected), *All monitors as a span*, a preset, or *Custom size*.
  For a span, *Layout* gives each monitor as `WIDTHxHEIGHT+X+Y` (the way `xrandr` prints it;
  *Detected* fills in yours), and *Write* chooses *One span image*, *One per monitor* (cut so that they
  line up across the gaps) or *Both*.
- **Fit:** *Cover* (the grid fills the image and is cropped, so no stage colour shows), *Contain* (the
  whole grid, letterboxed in the palette's stage colour), *Current view* (what the grid view shows now,
  scaled to the image) or *Repeat* (a grid with wrapped edges, tiled seamlessly over the image at a
  chosen tile size).
- **Grid:** *Fresh grid sized for the image* builds a new grid of the same tiling, with this world's
  automata and seeds, big enough to fill the image at a given number of pixels per tile, and runs it
  for a given number of generations first (on the GPU if you like). The live world is left alone;
  *Open the fresh grid* afterwards replaces it with the new grid if you want to keep it.
- **Outlines** (*Tile outlines when tiles are at least* so many pixels across), **Palette** (as the
  automata are, or one palette for all), and the **Folder** (*Choose…*; by default the `wallpapers`
  folder of the output folder). Then *Export*, and *Open folder* to see the result.

**On the command line**, `jetcarp wallpaper` takes a scene file or a Quick Start id:

```text
$ jetcarp wallpaper life --size HD --gens 200 -o life.png
life.png (1920 x 1080)
$ jetcarp wallpaper life --monitors 2560x1440+0+0,1920x1080+2560+180 --split --gens 100 --palette Glacier
/home/ada/Pictures/JETCARP/wallpapers/Life-on-the-square-grid-1-2560x1440-2026-11-10_18-28-08.png (2560 x 1440)
/home/ada/Pictures/JETCARP/wallpapers/Life-on-the-square-grid-2-1920x1080-2026-11-10_18-28-08.png (1920 x 1080)
```

| Option | Effect |
| --- | --- |
| `--size` | `WIDTHxHEIGHT` or a preset: `HD` (1920 × 1080), `WUXGA` (1920 × 1200), `QHD` (2560 × 1440), `WQXGA` (2560 × 1600), `UWQHD` (3440 × 1440), `4K` (3840 × 2160, the default), `5K` (5120 × 2880), `8K` (7680 × 4320), `Phone` (1080 × 2400) |
| `--monitors`, `--split` | A span of monitors as `WIDTHxHEIGHT+X+Y,…`; with `--split`, one image per monitor instead of the whole span |
| `--fit` | `cover` (default), `contain` or `repeat` (a wrapped grid tiled at `--cell-px`) |
| `--center X,Y --scale S` | Look at this point of the world at this scale instead of fitting |
| `--fresh`, `--cell-px` | Build a fresh grid of the scene's tiling sized to fill the image at `--cell-px` pixels per tile (default 4), with the scene's automata and seeds but not its later edits |
| `--gens`, `--gpu` | Generations to run first, on the GPU where the rules allow it |
| `--palette`, `--outline` | One palette for every automaton (a name); tile outlines when tiles are at least this many pixels across |
| `-o`, `--out` | A file or a folder (default: the `wallpapers` folder of the output folder) |

`jetcarp --json quickstart` lists the built-in Quick Start entries with their ids (such as `life` or
`langtons-ant`); entries of your own are `find:…`. To show a wallpaper that moves, see
[Wallpapers, screensavers and the background host](wallpapers.md).

## Animations

`jetcarp media render` makes a set of media: a PNG frame for every step, an animation, a poster frame
and a thumbnail.

```text
$ jetcarp media render --scene docs/scenes/automata/turmite-llrr.json --gens 120 --every 2 --no-frames --out llrr
webp   llrr/anim.webp
poster llrr/poster.png
thumb  llrr/thumb.png
```

- **What to render.** `--scene FILE` renders any scene from its first generation; without `--out`, the
  files go to `media/<scene name>` in the output folder. The other targets come from the
  [pattern library](pattern-library.md): `--pattern ID` (a catalogued pattern, shown alone),
  `--quickstart ID` (one of the library's own Quick Start entries, under *My finds*; for a built-in
  entry, render its scene file instead), `--ant ID` (an ant run) and `--missing` (everything in the
  library whose media are missing or out of date, limited with `--class`, `--only-patterns`,
  `--only-quickstart` and `--limit`). Their media are kept in the library's `media` folder and
  rendered again only when something has changed (`--force` renders anyway); `jetcarp media list KIND
  ID` lists them.
- **How.** A pattern *alone* is shown on an empty field for `--periods` full periods (default 1), so
  the animation loops seamlessly; a spaceship's window moves with it. *From the seed*, the whole grid
  runs for `--gens` generations (default 200; row rules one frame per row), with a frame every
  `--every` generations. `--mode` overrides the choice.
- **Format.** With `--format auto` (the default), the animation is a GIF when each frame is at most
  320 × 320 pixels, the whole animation at most 8 million pixels and 256 colours, and an animated
  lossless WebP otherwise; `--format gif` and `--format webp` choose one. `--no-frames` skips the
  per-step PNGs.
- **Look.** Media have a house style: a fixed set of colours for each kind of field, cells coloured by
  state and never by age (so loops close), outlines where tiles are at least 8 pixels across, whole
  pixels per cell, about 320 pixels across for a pattern alone and 512 from the seed, and no text.
  `--palette NAME` uses a palette instead. Media files do not carry the scene inside them.

In a script, `w.media(#{ gens: 200, every: 2 })` renders a world from its current state into a new
`media` folder, and `media(#{ pattern: id })` renders library media, with the same options as the
command. The animations in this book are made from their scenes by a separate tool,
`docs/tools/render-animations.sh`, which uses Python to crop and label the frames.

## Contact sheets

A contact sheet lays many small pictures out in a grid, each labelled, to compare rules at a glance.

- **Elementary and hexagonal row rules.** *File → Export contact sheet…* draws the world's first row
  rule and the rules after it (64 of them, or all 16 binary hexagon rules) in its palette, and
  *Contact sheet…* on an automaton's card in the *CAs* pane does the same for that automaton's rule.
  From the command line,
  `jetcarp sheet --kind w --from 0 --count 64 --png sheet.png` does the same for any range, with
  `--cols`, `--thumb` (pixels per thumbnail), `--width` (cells across each), `--random-row DENSITY`
  (start from a random top row instead of one cell) and `--palette`. The kinds are `w` (rules 0 to
  255), `hexw` (0 to 15), `hexw3` (0 to 19,682), `hext` (0 to 255), `hext3` and `hext3sum` (see
  [Hexagonal row rules](../automata/hexagonal-rows.md)).
- **Sweep folders.** `jetcarp sheet --folder FOLDER --runs "#12,#40,n=588" --png picks.png` lays out
  chosen runs of a [sweep](../reference/scripting.md#sweeps), each under its label (`--labels`) and
  rule.
- **Quick Start.** `jetcarp quickstart --sheet sheet.png` draws every built-in entry, numbered as
  `jetcarp quickstart` lists them, after `--gens` generations (default 300); `--filter TEXT` keeps the
  entries whose name, rule, family or tiling contains the text.
- **Isohedral types.** `jetcarp isohedral --png sheet.png` draws all 81 types (see
  [Drawing and editing tiles](drawing-tiles.md#isohedral-types)).

The exploratory search makes contact sheets of its own for reviewing what it found (see
[Search methods](../research/search-methods.md#explore)).

## Other pictures

- `jetcarp hashlife … --png` draws a pattern far in the future: with JETCARP's renderer up to
  `--max-cells` cells (16 million by default), and as a density map beyond that (see
  [HashLife and very long runs](hashlife.md)).
- `jetcarp objects scene.json --png objects.png` colours each object of a world by its class: still
  life, oscillator, spaceship or other.
- `jetcarp library render ID --png` draws a catalogued pattern's phases side by side (see
  [The pattern library](pattern-library.md)).
- `jetcarp pile` draws a large sandpile (see [Large sandpiles](large-sandpiles.md)).

The still figures in this book are made with `jetcarp run --png` and the contact-sheet commands, from
the scene named under each, by `docs/tools/render-figures.sh`.
