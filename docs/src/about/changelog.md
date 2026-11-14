# Changelog

All notable changes to JETCARP are listed here, newest first, in the style of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before version 1.0, a release may still
change the scene format, the command line, the scripting API and the MCP tools; 1.0 is the release
that will declare them stable.

## [Unreleased]

### Fixed

- **The live desktop works on GNOME 51 (Fedora 45).** The GNOME extension claimed only GNOME
  Shell 50, so GNOME 51 would not load it and the host waited for it with nothing on the desktop.
  ([#2](https://github.com/jprodgers/jetcarp/issues/2))
- **The GNOME live desktop stood still on the desktop and ran only behind other windows.** The
  extension took the host's window out of the window lists before hiding it, and GNOME will not
  minimize a window that is out of the window lists. The window stayed up, full screen, so GNOME
  counted the desktop as covered by a full screen app and the host paused whenever the desktop
  showed. The window is now hidden first. ([#2](https://github.com/jprodgers/jetcarp/issues/2))

  For both, log out and back in once after upgrading (see below).
- **A CPU-only explore search never confirmed its elites, and ran only about a quarter of the
  candidates it made.** The search works in steps of 2 seconds, and a round of runs on the
  processor nearly always outlasts one; the step's deadline was checked inside the round, so the
  candidates not yet started were dropped and the confirmation every fourth round never came.
  Searches of families that run only on the processor (JACA, and every family on a computer without
  a GPU) now confirm their best runs on bigger grids, and run every candidate. A round under way now
  runs to its end; a stop still takes effect within moments.
  ([#3](https://github.com/jprodgers/jetcarp/issues/3))
- **A stale example script is gone.** `explore-supervise.sh`, beside the example scripts in the
  scripting crate's source, called `jetcarp explore run`, a command of JETCARP's predecessor, so it
  failed at once. An explore search stops on time by itself and carries on from its archive after a
  crash, so keeping one going needs only the one-line loop around `jetcarp jobs resume` that the
  [searching guide](../guides/searching.md#waiting-in-a-script) now shows.
  ([#4](https://github.com/jprodgers/jetcarp/issues/4))

- **The two signs of JACA looked alike once their territories went still.** JACA colours each sign
  by age, and its default palettes, Tide and Ember, both end in nearly the same dark indigo, so old
  cells of either sign showed the same colour. The −1 cells now default to Koi, whose reds stay apart
  from Tide's blues at every age; so do the 13 Quick Start presets and the book's scenes that paired
  Tide with Ember. A test keeps every JACA palette pair apart as cells age.
  ([#6](https://github.com/jprodgers/jetcarp/issues/6))

### Changed

- **Upgrading JETCARP upgrades its GNOME extension.** The background host, when it starts, and any
  `jetcarp host` command bring an installed extension up to the program's version (an extension you
  have switched off stays off). GNOME loads the new files at the next login, so log out and back in
  once after upgrading; until then `jetcarp host gnome status` says that an older version is running.
- **A `V` may follow the B or S counts of a rule.** `B3/S23V/C4` and `B47V/S13568/4` now mean the
  same as `B3/S23/C4V`, which is how JETCARP writes them. A `V` anywhere else is refused with the
  rule written the right way; the message used to name only the letter.
  ([#5](https://github.com/jprodgers/jetcarp/issues/5))
- **Hunts, bounded searches and censuses read every rule before they start.** A rule in `--rules`
  or `--rules-file` that cannot run stops the job from starting, and the error names each bad rule
  by its line in the file. A hunt used to start anyway and report the rule once per tiling, with no
  soups run. ([#5](https://github.com/jprodgers/jetcarp/issues/5))

### Added

- **`--max-rss-gb` for hunts and bounded searches,** as explore searches have it: past that much
  memory the search pauses, resumable, and says why. The job keeps the cap, so a resumed run keeps
  to it too. Scripts and the MCP server's `search_start` take it as `max_rss_gb`.
- **`jetcarp jobs wait JOB`** waits until a job stops running, in this process or another, and its
  exit status says how it ended: 0 done, 3 paused, 4 failed, 5 still running at `--timeout S`, and
  6 when the job is marked running but its process crashed or was killed. Scripts call it as the
  operation `jobs.wait`. A search started or resumed with `--detach` is now marked running as the
  command returns, so a `jobs wait` or `jobs status` straight after sees it running.
- **Scripts can read a tiling's geometry.** `w.neighbours(i)` and `w.neighbours(i, "edge")` give a
  cell's touch or edge neighbours, `w.live_cells()` the cells that are not empty with their states,
  and `w.tile(i)` a tile's centre and corners, on any tiling, aperiodic patches too. `w.cas()` now
  says each CA's neighbourhood. A new example script, `jaca-territories`, uses them to follow a JACA
  world's sign balance, how sorted its territories are, its fronts, and the ground the signs trade.

## [0.4.1] – 2026-11-13

### Fixed

- **Popping a window out could crash the app.** The ↗ button in a tool window's title bar, or
  *Own window* for a pane, closed the whole app when the window held a table (the AI control
  window with AI control on, among others). Popped-out windows now draw as the main window does.
  ([#1](https://github.com/jprodgers/jetcarp/issues/1))
- **The AI control window opens on the Quick Start.** It showed only with a world open, so from
  the Quick Start, *Tools → AI control…* ticked in the menu but showed nothing, and AI control could
  not be switched on there. ([#1](https://github.com/jprodgers/jetcarp/issues/1))
- **A newer version of a pack replaces its Quick Start entries.** Importing one kept every entry
  the earlier version had brought, so a library that had a release candidate's starter pack showed
  both versions' entries, some of them twice. Entries the new version no longer has now leave the
  Quick Start, unless you hid or rated them. To tidy a library that already has both, import the
  starter pack again: `jetcarp pack import <folder>/jetcarp-starter`, where `jetcarp paths` names the
  folder as *bundled*.
- **Packs that came with a Homebrew or Scoop install are named by the link that stays.** `jetcarp
  paths` and the setup's import job named Homebrew's `Cellar/jetcarp/<version>` and Scoop's
  `apps\jetcarp\<version>`, folders the next upgrade deletes. They now name
  `/opt/homebrew/share/jetcarp/packs` and `apps\jetcarp\current\packs`.
- **`jetcarp jobs status` on a hunt that came with a pack** printed a hunt's empty counters ("0
  soups … status , 0 s run"). It now says the hunt was imported, as the Hunts window does, and
  names the pack rather than its `sha256:` hash, here and in `jobs list`. Packs imported from now
  on are named by their title (*JETCARP starter pack*) there and in the Hunts window.
- **Windows lists the screen saver as JETCARP,** not "Jetcarp": the programs now carry the name
  Windows reads for that list. `jetcarp host saver install` also says that Windows chooses JETCARP
  as Screen Saver Settings opens, so Cancel there keeps it chosen.
- **`jetcarp doctor` notes a data folder that is gone.** When the folder chosen in setup
  (`paths.json`) was moved or deleted, it said only "not created yet"; it now says so and how to
  name the new place.

### Changed

- **More atlas titles tell entries apart.** A JACA entry is titled by its rules up to 40 characters
  (*Battle JACA +B14/S124 -B12/S345 blast 1*, not a plain *Battle JACA*), and a Priority entry by its
  number of states (*Oscillating Priority 6*). Entries from earlier builds keep their titles.
- **One DOI to cite.** The citing page, the README and `CITATION.cff` give Zenodo's DOI for every
  version, [10.5281/zenodo.24191765](https://doi.org/10.5281/zenodo.24191765), which resolves to the
  newest release, and ask for the version you ran.

## [0.4.0] – 2026-11-13

The first public release.

### What JETCARP 0.4.0 does

- **Tilings.** Squares, hexagons and triangles, all eleven Archimedean tilings, the 1,248 k-uniform
  tilings up to k = 6, the 81 isohedral tile types, tiles you draw yourself, and the aperiodic
  Penrose (kites and darts, and rhombs), hat and spectre tilings, built on exact patches.
- **Automata.** Life and the Life-like rules, Generations, Larger than Life, elementary rules on
  squares and hexagons, Langton's ant and the turmites, WireWorld, Greenberg–Hastings, the cyclic
  and rock-paper-scissors rules, hodgepodge, sandpiles, Lenia and other continuous rules, JACA
  (Jimmie Rodgers's annihilating two-sided Life), layered rules, Margolus block rules, rule tables
  and von Neumann's 29-state automaton. Several can run at once, on shared or separate cell fields.
- **The app.** A Quick Start menu of examples, an editor for worlds, tiles and unit cells, thirty
  palettes and your own, pictures and animations, and screensaver mode.
- **On the desktop.** A background host that runs scenes as a live wallpaper (Windows, macOS, and
  GNOME through its own extension), on the GNOME lock screen, as the system's screen saver (Windows
  and macOS), or as a still picture; see [Wallpapers](../guides/wallpapers.md).
- **One program for every front end.** `jetcarp` opens the app, and its subcommands are the command
  line, the background host (`jetcarp host`) and an MCP server for AI assistants (`jetcarp mcp`).
  Rhai scripts run worlds and parallel rule sweeps. All of them call one layer of 79 named
  operations, each with JSON Schemas for its input and output (`jetcarp api list`).
- **AI assistants.** `jetcarp mcp` gives an assistant such as Claude Code or Claude Desktop 21 tools
  over those operations (worlds, scripts, the library, searches, media and packs), the app's
  displays when it drives the app, and this book as resources, within folder and time limits; see
  [Driving JETCARP from an AI assistant](../tutorials/mcp.md).
- **Searches.** Nine search strategies (soup hunts on the plane and on bounded grids, a rule census,
  ants and turmites, movers on aperiodic tilings, large sandpiles, collisions, an evolutionary glider
  search, and an exploratory search over tilings, rule families and rules) run as resumable jobs
  whose results don't depend on the thread count; see [Search methods](../research/search-methods.md).
- **A verified pattern library.** Every catalogued pattern is verified exactly, named by a content
  id that any program can recompute, and pinned to its tiling's geometry and its rule family's
  meaning, so a change to either is caught; see [Library format](../reference/library-format.md).
- **Packs.** Format 1.0 for sharing data: signed, content-addressed stores of Parquet tables and
  canonical JSON that other tools read without JETCARP, checked hash by hash and id by id on import;
  ratings go in only with each rater's consent. See [Packs](../guides/packs.md) and
  [Pack format](../reference/pack-format.md).
- **A starter pack** beside the program: 79 Quick Start entries that Jimmie Rodgers picked (36 from
  the atlas of an exploratory search, 18 spaceships, 8 oscillators, 8 ant and turmite highways, 7
  sandpile identities and 2 hat gliders), with the 26 pattern records and 8 ant records they show.
  The 63 built-in examples picked with them are featured in the Quick Start menu. See
  [The starter pack](../getting-started/installing.md#the-starter-pack).
- **Reproducibility.** Every run is a scene that replays bit for bit to a recorded state hash, and
  every picture JETCARP saves carries its scene; see [Reproducibility](../research/reproducibility.md).
- **Installing.** Archives for Linux (x86_64 and ARM, glibc 2.28 or newer), Windows and macOS (Apple
  silicon and Intel), installers that check every download, Python wheels for pipx, uv and pip, a
  Homebrew tap, a Scoop bucket and crates.io, with a starter pack beside the program; see
  [Installing JETCARP](../getting-started/installing.md).
- **First-run setup.** On its first launch the app asks what JETCARP is for, in five profiles
  (Desktop art, Explorer, Researcher, Everything, Blank slate), and sets it up: a data folder of your
  choosing for big libraries, the starter pack and packs of your own imported, a Quick Start build
  that searches for so many minutes and adds its best runs to the Quick Start, and the live
  wallpaper and screen saver offered, never turned on unasked. The work runs as background jobs;
  `jetcarp setup` does the same in a terminal. See [First run and setup](../getting-started/first-run.md).
- **No network, no telemetry.** JETCARP makes no network connections; see `SECURITY.md`.
- **This book**, with a page on the history and credits of every automaton and tiling.

JETCARP grew out of CART, a private prototype Jimmie Rodgers built in early November 2026, and was
reworked for release in a new repository with a fresh history; see
[How JETCARP was made](how-made.md).
