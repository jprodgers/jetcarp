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
  Searches run without a GPU (every family on a computer without one, or a search started without
  `--gpu`), and the families that only ever run on the processor (turmites, layered and elementary
  rules among them), now confirm their best runs on bigger grids, and run every candidate. A round under way now
  runs to its end; a stop still takes effect within moments.
  ([#3](https://github.com/jprodgers/jetcarp/issues/3))
- **The JACA page said JACA runs only on the CPU.** The explore search runs JACA on the GPU, in its
  batches, when given one, and a JACA world now runs on the GPU in the app and the background host
  too (see below); the page says so. ([#9](https://github.com/jprodgers/jetcarp/issues/9))
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

- **An edge-neighbour rule was named as the plain rule.** A world given `B3/S23V` named its
  automaton "B3/S23", the touch-neighbour rule's notation, on cards, captions and status lines. The
  default name now keeps the `V` (and a JACA rule's `V`s), as the rule's notation does.
  ([#7](https://github.com/jprodgers/jetcarp/issues/7))
- **A hunt over many rules and tilings worded its progress oddly.** "100/100 x 1200 groups soups"
  now reads "100/100 soups in each of 1,200 groups", and the count of new patterns is grouped by
  thousands like the others. ([#8](https://github.com/jprodgers/jetcarp/issues/8))

- **A hunt asked for the GPU failed when there was none.** `jetcarp search hunt --gpu` on a computer
  without a usable GPU stopped with an error, and a pile asked for the GPU did the same in a program
  without a GPU toppler. Both now run on the processor, with the same results and one note in their
  log. ([#10](https://github.com/jprodgers/jetcarp/issues/10))
- **Each part of JETCARP chose its graphics adapter its own way.** The command line asked for the
  fastest adapter, the app took its toolkit's choice, the host's windows asked for high performance
  and its headless device for no preference, and only the command line refused software adapters. On
  a computer with two GPUs, the app and the command line could use different ones, and there was no
  way to pick one. They now all choose the same way (see [The GPU](../guides/gpu.md#which-adapter)),
  and `JETCARP_GPU` or the GPU setting can name the adapter.
  ([#11](https://github.com/jprodgers/jetcarp/issues/11))
- **The GPU parity tests never ran in CI.** CI installs Mesa's software Vulkan, but the GPU tests
  refused software adapters, so every one of them skipped. CI now sets `JETCARP_GPU=software`, and
  they run on lavapipe. ([#12](https://github.com/jprodgers/jetcarp/issues/12))
- **A GPU that failed part way through a run left the world ahead of its cells.** When a later
  batch of a run failed (a readback that failed, a lost device), the generations the earlier
  batches had stepped were counted and logged but their cells never read back, so the processor
  carried on from the wrong cells. The world is now put back as it was before the run, and the
  processor carries on from there to the same result. A failed readback no longer stops the program
  either. ([#13](https://github.com/jprodgers/jetcarp/issues/13))

### Changed

- **The desktop background has its own pane.** *View → Desktop background settings…* (linked from
  the *Display* and *Screensaver* panes) holds everything about the background host that was in the
  *Screensaver* pane's *Background host* section, and more: each screen's rotation and monitor,
  *Add a screen*, the lock screen, the still picture, the frame rate, *GPU simulation* (see
  below), the caption, the pauses, start at login, GNOME's extension, and the new *Performance*
  group. The *Screensaver* pane keeps the screensaver's own settings, its profiles
  and rotations (which the desktop background shares), and the system screen saver's rotation.
  `host.json` and `app.json` are unchanged.
- **The still picture the host leaves carries its scene,** as every picture JETCARP writes does, so
  the desktop's picture can be opened in JETCARP and run again.

- **JACA worlds run on the GPU.** With *GPU simulation* on, the app and the background host (so
  live desktops, the lock screen and the screen saver) step JACA on the graphics card: lattices and
  aperiodic patches, both neighbourhoods, blast radii up to 64, every edge. The result is the CPU's
  to the bit, the annihilation glow and the counts on the card and in the status included. On an
  AMD Radeon 8060S a 1-million-cell JACA world runs about 48 times as fast as on 32 CPU threads.
- **B0 Life-like rules and Margolus blocks run on the GPU,** in the app and the background host,
  with the CPU's results. Both depend on the step count's parity, which the GPU now keeps.
- **Larger than Life runs on the GPU,** on lattices and on aperiodic patches, in the app and the
  background host, with the CPU's results. A range-5 rule on a million cells runs about 40 times as
  fast.
- **The forest fire runs on the GPU,** in the app and the background host. The GPU makes the CPU's
  64-bit random numbers and compares them with the probabilities as whole numbers, so every fire
  starts on the same tree at the same generation.
- **Sandpiles run on the GPU,** in the app and the background host, on lattices and aperiodic
  patches: the same grains, drops and losses as on the CPU, piles past 2^32 grains included. A script
  step that waits for a sandpile to be done still runs on the CPU.
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
- **A JACA run that lost a sign scores low and has a niche of its own.** A JACA run whose sign
  fell below 1% of live cells, at any point in the run, keeps 0.3 of its score, under the built-in
  score and fitted models alike, and a JACA run's niche now includes whether both signs held, so
  runs that turned into plain Life-like runs stop competing with battles for a place.
- **Run segments are schema 2** (`jetcarp-explore-runs 2`), with the sign columns. Segments of
  schema 1, from earlier searches and packs, still read, as runs without sign measures, and
  publishing and importing copy segments as they are. JETCARP 0.4.1 cannot read a segment of
  schema 2.

- **The GPU is the default wherever it gains, and the processor the fallback.** Under the new
  automatic GPU setting, the app's world, wallpapers and screen savers step on the GPU from 16,000
  cells; `jetcarp run`, `jetcarp wallpaper`, scripts (`w.step`, sweeps) and the MCP server's
  `world_step` and `world_render` from a million neighbour reads a generation; hunts and explore
  searches always; piles from 100,000 grains. The results are the same, bit for bit. The app's
  world used to start on the processor, and the command line's runs and searches stayed there unless
  given `--gpu`. See [The GPU](../guides/gpu.md).
- **`--cpu` joins `--gpu`** on `jetcarp run`, `wallpaper`, `search hunt`, `search explore`, `pile`,
  `jobs resume`, `jobs refit` and `jobs rescore`; either wins over the GPU setting. The operations'
  `gpu` options are now true, false or left out (the setting decides), and an explore search or pile
  made with `--gpu` or `--cpu` keeps that choice. `jetcarp setup --no-gpu` is unchanged.
- **Displays use the adapter driving the screen.** The app, the host, the lock screen and the screen
  savers prefer it (the integrated GPU on a laptop with two), so a wallpaper does not keep a
  discrete GPU awake; searches and runs use the fastest adapter.
- **One GPU setting, shared, with a choice of their own for the app and the host.** The shared
  setting (`gpu.json`: Automatic, On, Off) is *Shared setting* in the *Desktop background* pane.
  The app's *Run → GPU simulation* and the pane's *GPU simulation* each offer *Follow the shared
  setting* (the default), *On* or *Off*, saved in `app.json` and `host.json`; so do
  `jetcarp app display --gpu follow|on|off` and MCP `app_display {gpu}`. A `host.json` from before
  reads `true` as follow and `false` as off. First to last: `JETCARP_GPU` (when it names a mode, it
  now comes before a command's `--gpu` or `--cpu`), the command's own choice, the app's or host's
  own, the shared setting, automatic. `jetcarp doctor` says what the app and the host each do, and
  where that came from.
- **A wallpaper on the GPU costs the processor far less.** Reading a small world back from the GPU
  woke every worker thread each frame, about 4 ms of processor time; worlds under a million cells now
  do it on one thread, about 0.04 ms.

### Added

- **Fit the desktop background to this computer.** *Fit to this computer…* in the new *Desktop
  background* pane, or `jetcarp host tune`, sweeps each scene of the wallpaper's rotation through a
  ladder of sizes on the desktop itself, two seconds each: 1 to 64 screen pixels a cell on a
  lattice (from the finest within 4 million cells to 60 cells across), patch levels on an aperiodic
  tiling. It measures JETCARP's own processor and GPU time (the system's count on Linux) and frame
  rate at each, against a processor limit (default 10% of the machine), a GPU limit (default 25%)
  and a smooth frame rate, and then each scene gets a row of cards: a picture of the screen at that
  size at its own pixels, the numbers, and whether it is within the limits. The finest within them is
  recommended and shown unless you click another card; a pattern placed on an empty field keeps its
  own size. The host sweeps again when the monitors change, keeping a size you chose while it still
  fits. With the host stopped it sweeps off screen. `--pick recommended|finest|own|PX`, `--crops
  DIR`, `--dry-run`, `--stop` and `--json` (every size's numbers); scripts and AI assistants reach it
  through `host_control` (`{"action": "tune"}`). See
  [Fit to this computer](../guides/wallpapers.md#fit-to-this-computer).
- **`jetcarp host scene`** prints or saves (`--save FILE`) the scene a screen shows, its grid's size
  included, so a tuned wallpaper replays exactly; `host_control` gives it as `{"action": "scene"}`.
  The host's status gives each screen's cell size and grid, and says when tuning chose them.

- **A page for rating clips.** `jetcarp jobs clips --page` also writes `index.html` into the clips
  folder: the clips in batches, each with Keep, Maybe and Drop and a ★, with a progress bar, a
  tally, a note box, and the keyboard (← →, K, M, D, S). It works offline from the folder and
  follows the system's light or dark setting. The browser keeps the ratings as you go, and *Save
  ratings* downloads them as one file, which `jetcarp jobs review JOB --from FILE` records (with
  `--rater NAME` to name someone else). Stars are kept as five-star ratings of the runs, apart from
  the verdicts. Scripts and `jetcarp api call` reach it as `search.clips` with `page`.
- **`jetcarp library soup ID`** rebuilds the exact soup a hunt's find came from, checks with the
  hunt's own engine that it makes the find, and gives a scene that replays to it (`--save FILE`).
  `--best N` runs the hunt's soups again and gives the N that made the most copies of the find.
  Hunt finds now keep the soup's shape and the hunt's seed in their record, so their soups can be
  made again without the hunt's job; earlier finds' own soups are rebuilt too, but `--best` needs
  their job. Scripts reach it as the operation `library.soup`.
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
- **JACA runs record how their two signs share the board.** An explore search measures five more
  things about each JACA run: which sign is ahead, how balanced the signs are, how well they sort
  into territories, how much territory changes hands late, and when a sign was lost. They are new
  columns of the run segments (null for other families) and show in `jetcarp jobs runs
  --measures`. In a JACA-only search, more than half of the best-scoring runs had lost a sign in
  the first half, which leaves a plain Life-like run; see
  [Search methods](../research/search-methods.md#explore).
- **Your taste as the score.** `jetcarp jobs refit JOB` fits a score model to the keep, maybe and
  drop verdicts on a job's runs, says how well it ranks runs it did not see, and with `--out`
  writes it as a file. `jetcarp search explore --score FILE` searches with it, and `jetcarp jobs
  rescore JOB --score FILE` rescores a job's runs with it; the job keeps the model, so resuming
  the search keeps it. Fitted to 227 ratings of JACA runs, a model ranked keeps above drops 90% of
  the time on runs held out of the fit, against 74% for the score they were stored with. Older
  runs replay exactly, so a refit runs again those that lack the sign measures, and `jetcarp jobs
  rescore --replay` does it for a whole search (`--gpu` on the GPU). The MCP `search_runs` tool
  can refit and rescore too, and `search_start` takes a model.
- **The explore search breeds JACA its own way.** Parents are chosen in proportion to quality, a
  fifth of the time at random, and for JACA only from runs whose signs both held. Strong JACA pairs
  are transplanted to tilings they have not run on (with their counts scaled when the neighbour
  counts differ), and pairs whose weaker sign is losing are tuned to help it, with 30% of JACA's
  candidates still fresh random draws. `jetcarp search explore --parents-from JOB` starts a search
  from another job's reviewed JACA runs (`--reviewed`, default keep and maybe), transplanting and
  tuning them first. See [Search methods](../research/search-methods.md#explore).
- **Every measure out.** `jetcarp jobs runs` takes `--measures all` (or a list), `--format
  csv|json`, `--since ID` for the runs after an id, `--limit 0` for every run, and `--out FILE`.
  The MCP `search_runs` tool writes a CSV file of every run with `csv` and limit 0.

- **A GPU setting shared by the app, the host, the command line and the MCP server**: `gpu.json`
  in the settings folder, with `gpu` (`auto`, `on`, `off`) and an optional `gpu_adapter`
  (`integrated`, `discrete`, `software`, or part of a name). `JETCARP_GPU` overrides it with the same
  words, so `JETCARP_GPU=UHD` uses an Intel UHD rather than the RTX beside it. Software adapters are
  used only when asked for, and Windows' WARP never.
- **A GPU self-test.** Before simulating on an adapter, JETCARP steps one rule of every family the
  GPU runs on two lattices and an aperiodic patch, and a small explore batch, and compares every hash
  with the processor's (about 0.1 seconds). An adapter whose results differ is not used. The result
  is kept in `gpu-checks.json` in the settings folder for that adapter, driver and version.
- **`jetcarp doctor` says which adapter each purpose uses and why**: the GPU setting and where it
  came from, and for displays and computing the adapter, how it was chosen, its limits, the
  self-test's result and whether the simulation uses it; `doctor --json` has it as `gpu_policy`.
- **`examples/bench.rs` in `jetcarp-gpu` times from 4,000 cells up**, with a `--paced` table of
  one generation a frame and the processor time each path takes, from which the automatic
  thresholds come.

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
