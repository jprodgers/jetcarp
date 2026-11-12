# Website export

`jetcarp site export` turns the public part of your library into a folder you can put on any web
host: one JSON file per pattern, ant run or Quick Start entry, its pictures and animation, a
`manifest.json` that lists everything, and a viewer page, `index.html`, that shows it all as a grid.
Run it again after curating and it writes only what changed and deletes what is no longer public, so
the folder can be synced to a host as it is.

A website export is for people to look at. To give someone data they can import, check and build on,
make a [pack](packs.md) instead.

## Export

```text
$ jetcarp site export --to site
wrote   patterns/21e9bac2df7b770a5ff0f34bd68a514a.json
wrote   media/pattern/21e9bac2df7b770a5ff0f34bd68a514a/anim.gif
wrote   media/pattern/21e9bac2df7b770a5ff0f34bd68a514a/poster.png
wrote   media/pattern/21e9bac2df7b770a5ff0f34bd68a514a/thumb.png
…
wrote   manifest.json
wrote   index.html
site: 2 patterns, 0 Quick Start entries, 0 ants; 10 files written, 0 deleted, 0 items unchanged, 2 media rendered
```

Pictures and animations a pattern doesn't have yet are rendered first, in the house style. Use
`--palette NAME` (or hex colours) to render them in another palette, or `--no-render` to export
only the media that already exist. They are the same files `jetcarp media render` makes (see
[Pictures and animations](pictures.md)). `--frames` copies each animation's frames as PNGs too.

Run the same command again and it compares each file's hash with the copy already in the folder:

```text
$ jetcarp site export --to site
site: 2 patterns, 0 Quick Start entries, 0 ants; 0 files written, 0 deleted, 2 items unchanged, 0 media rendered
```

## What counts as public

Only what you've curated goes out. By default that is:

- **patterns** you've marked `reviewed` or `promoted`
  (`jetcarp library update ID --state reviewed`, or in the app's library window);
- **ant and turmite runs** in the same two states;
- **Quick Start entries** you've added yourself.

A rejected pattern never goes out. Mark an exported pattern `rejected` (or back to `new`), export
again, and its files are deleted from the folder.

To narrow it further:

| Option | Effect |
| --- | --- |
| `--state LIST` | Pattern curation states to export, comma-separated (default `promoted,reviewed`) |
| `--class LIST` | Only these classes, such as `spaceship,oscillator` |
| `--min-score X` | Only patterns with at least this interest score |
| `--min-rating N` | Only patterns you've rated at least N stars (1–5) |
| `--min-osc-period N` | Leave out oscillators with a shorter period |
| `--ants LIST` | Ant curation states to export (`none` for no ants) |
| `--ant-outcome LIST` | Only ants with these outcomes, such as `highway,symmetric` |
| `--quickstart LIST` | Which Quick Start entries to export (`none` for none) |

## View it

Browsers don't let a page read files from a `file://` address, so serve the folder over HTTP to look
at it on your own computer:

```text
$ python3 -m http.server -d site 8000
```

Then open `http://localhost:8000/`. The page shows every item's animation in a grid, with filters by
kind and class, a sort order (highest score first by default), a search box over names, rules and
grids, and a detail view for each item.

The page is plain HTML and JavaScript with no build step and nothing loaded from elsewhere, so it
works from any static host. `--no-page` leaves it out, for instance if you've written your own
page over `manifest.json`. A page of your own can also read the data from another folder: the
viewer's `DATA` constant, near the top of its script, names the folder that holds
`manifest.json`.

## Put it online

Copy the folder to any static host: GitHub Pages, Netlify, an S3 bucket or your own server. Files
that haven't changed keep their names and contents, so a sync tool such as `rsync` or
`aws s3 sync` uploads only the new ones after each export. The page asks for each file with its hash
in the address, so browsers and caches never show a stale picture.

## The files

| Path | What it holds |
| --- | --- |
| `index.html` | The viewer page |
| `manifest.json` | Every item, with its kind, id, title, class, grid, rule, file path and hash, and the filter that made the export |
| `patterns/<id>.json` | A pattern's record: its rule and the range of rules it works in, tiling, period, displacement, cells, phases and scores |
| `ants/<id>.json` | An ant or turmite run's record |
| `quickstart/<id>.json` | A Quick Start entry, with its scene |
| `media/<kind>/<id>/` | `thumb.png`, `poster.png` and `anim.gif` (and frames with `--frames`) |

Each JSON file has a `format` field (`1`). The page refuses nothing, but warns in the browser's
console about a manifest format it doesn't know.
