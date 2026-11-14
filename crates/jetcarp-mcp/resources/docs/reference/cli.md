# Command line

Everything the app does can also be done from a terminal with `jetcarp`. This page lists every
command and subcommand with its options, exactly as `jetcarp <command> --help` prints them, grouped
by what they are for. The guides explain when to use each one; this page is for looking up a flag.

A few things hold for every command:

- **`--json`** prints a command's result as JSON instead of text, for scripts and other tools
  (`jetcarp --json paths`, `jetcarp library list --json`). Progress and notes go to standard error,
  so standard output stays clean.
- **Folders** come from the environment when it sets them (`JETCARP_OUT`, `JETCARP_DATA_DIR`,
  `JETCARP_LIBRARY`, `JETCARP_CONFIG_DIR`); `jetcarp paths` shows the ones in use. See
  [Folders and environment variables](folders.md).
- **Jobs.** A search started from the command line is a job in the library like one started from
  the app, a script or an AI assistant: `jetcarp jobs` lists, follows, stops and resumes them all.
- The command line trusts its user: unlike the [MCP tools](mcp.md), it reads and writes wherever it
  is told and has no time or size limits beyond the engine's own.

The rest of this page is generated from the program's own help by `docs/tools/gen-cli.py`, so it
cannot drift from the program.

<!-- BEGIN GENERATED: docs/tools/gen-cli.py -->

## `jetcarp` itself

With no command, `jetcarp` opens the app; the app options below apply then. `--json` works with every command that reports a result, before or after the command's name.

```text
JETCARP: Jimmie's Euclidean Tilings and Cellular Automata Research Program.

With no subcommand, `jetcarp` opens the app (the app options below apply). The subcommands run JETCARP from the command line.

Usage: jetcarp [OPTIONS]
       jetcarp <COMMAND>

Commands:
  run         Load a scene, run it, and report generation, hash, and populations
  wallpaper   Desktop wallpapers: a scene or Quick Start entry at a monitor's size, a span of monitors, or one image per monitor cut so they line up
  palettes    List the palettes with their background colours
  verify      Check that a saved scene still replays to its recorded hash (or a PNG JETCARP wrote, which carries its scene)
  sheet       Contact sheet: one thumbnail per elementary rule, labelled with its number
  objects     Find still lifes, oscillators, and spaceships in a scene after running it
  library     Pattern library: list, show, curate, paste, promote to Quick Start; its ant runs and movers
  search      Start or carry on a search as a resumable catalog job: `hunt` (soup), `bounded`, `ants`, `census`, `mover`, `pile`, `collide`, `evolve`, `explore`; or `try` one configuration
  jobs        The catalog's jobs, whatever started them: `list`, `status`, `stop`, `resume`, `report`, `remove`; an explore job's `runs`, `sheet`, `scene`, `review`, `clips`, `rescore`
  hashlife    Jump a pattern (unbounded plane) or a wrap-edge world (power-of-two torus) far ahead with HashLife; export a region as a scene or PNG
  demo        Computing demos: a WireWorld adder and Life logic gates for any inputs
  pile        Large sandpile: N grains on one spot of an unbounded grid, built by doubling, with a cropped PNG, a preview, metadata, and a resumable pile file per step (the same as `jetcarp search pile`)
  pack        Packs: publish the library as a signed, content-addressed store (Parquet and JSON that any tool reads), export a version for a single download, summarise, check or import one: `key`, `rater`, `publish`, `export`, `info`, `check`, `import`
  media       Media for the catalog: per-step frames, a GIF or animated WebP, a poster, and a thumbnail for a pattern (alone), a Quick Start entry (from the seed), an ant run, or a scene; `--missing` renders everything missing or stale
  site        Website export: the public catalog as JSON and media with a manifest and a viewer page, incrementally
  tilings     List tiling families with their neighbour counts
  doctor      What JETCARP sees on this machine: version, folders, graphics adapters, desktop integration. Include it in bug reports
  uninstall   Undo what JETCARP set up outside its folders (the host and its still on the desktop, login entry, GNOME extension, screen saver), and optionally delete its settings and data. Shows the plan unless --yes
  paths       Print the settings, data, and output folders JETCARP uses (environment overrides included)
  setup       First-run setup: choose a profile (desktop-art, explorer, researcher, everything, blank-slate), and JETCARP imports its packs and builds Quick Start entries of its own, as jobs. Asks in a terminal; `--profile NAME --yes` asks nothing. `--status` shows where things stand
  isohedral   List Kaplan's isohedral types (IH01-IH93), or draw them all on a contact sheet
  rules       List named rules and notation
  script      Run a Rhai script: worlds, runs, images, parallel rule sweeps (see the book's "Scripting (Rhai) API" reference). `--examples` lists the example scripts built in, `--example NAME` runs one
  app         Control the running JETCARP app (AI control on, or `jetcarp --control`): `status`, `screensaver on|off [--profile NAME]`, `profiles`, `display ...`, `play`, `pause`, `speed N`, `next`, `previous`, `call <tool> <json>`
  host        The background host, which keeps scenes running with no app window: `start`, `stop`, `status`, `next`, `previous`, `pause`, `resume`, `rotation`, `caption`, `fps`, `login`
  mcp         MCP server on stdin/stdout so AI assistants (Claude Code, Claude Desktop, LM Studio) can drive JETCARP. Its tools read and write files only in JETCARP's output and library folders (and folders given with --allow)
  quickstart  Quick Start examples: list the built-in ones, check that each one works, or render a contact sheet; `publish` an explore job's best runs into the Quick Start as its atlas, or `build` some: a search of so many minutes, then its atlas published
  help        Print this message or the help of the given subcommand(s)

Options:
      --json
          Print the result as JSON instead of text (commands that report a result)

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

App options (with no subcommand):
      --screensaver
          Start in screensaver mode (Esc leaves it)

      --quit-on-exit
          With --screensaver: quit the app when the screensaver ends

      --scene <SCENE>
          Open this scene file (with --screensaver: show it)

      --quickstart <QUICKSTART>
          Open this Quick Start entry, e.g. `rule:B3/S23` (with --screensaver: show it)

      --profile <PROFILE>
          A screensaver profile by name (with --screensaver: show it; else load it into the window to try it)

      --rotation <ROTATION>
          A rotation by name (with --screensaver: run it; else choose it for the screensaver)

      --caption
          With --screensaver: show what is running (the rule and the grid); saved like the pane's checkbox

      --monitors <MONITORS>
          Show the grid on these monitors (indexes from the Display pane or `jetcarp app status`), one full-screen output each

      --span
          The outputs span one canvas (the default)

      --mirror
          The outputs mirror one view

      --fullscreen
          The main window full screen

      --cell-px <CELL_PX>
          Cell size in physical pixels (1 to 64) for Fill, the pixel-exact zoom, and the screensaver

      --keep-awake
          Keep the computer awake while the app is open

      --control
          Start the local control endpoint (AI control) for `jetcarp app` and `jetcarp mcp --app`
```

## Commands by what they are for

- **Worlds and scenes**: [`run`](#jetcarp-run), [`verify`](#jetcarp-verify), [`objects`](#jetcarp-objects), [`hashlife`](#jetcarp-hashlife), [`demo`](#jetcarp-demo). See [Scenes](../guides/scenes.md), [HashLife and very long runs](../guides/hashlife.md).
- **Searches and jobs**: [`search`](#jetcarp-search), [`jobs`](#jetcarp-jobs), [`pile`](#jetcarp-pile). See [Searching for patterns](../guides/searching.md), [Large sandpiles](../guides/large-sandpiles.md), [Search runs](search-runs.md).
- **The library and Quick Start**: [`library`](#jetcarp-library), [`quickstart`](#jetcarp-quickstart). See [The pattern library](../guides/pattern-library.md), [Building your own Quick Start](../tutorials/building-quick-start.md).
- **Packs**: [`pack`](#jetcarp-pack). See [Packs: importing, exporting, checking](../guides/packs.md), [Pack format](pack-format.md).
- **Pictures, media and the website**: [`sheet`](#jetcarp-sheet), [`media`](#jetcarp-media), [`site`](#jetcarp-site), [`wallpaper`](#jetcarp-wallpaper). See [Pictures and animations](../guides/pictures.md), [Website export](../guides/website-export.md), [Wallpapers, screensavers and the background host](../guides/wallpapers.md).
- **Scripts**: [`script`](#jetcarp-script). See [Scripting (Rhai) API](scripting.md), [Scripting a rule sweep](../tutorials/scripting-a-sweep.md).
- **The app, the background host and AI assistants**: [`app`](#jetcarp-app), [`host`](#jetcarp-host), [`mcp`](#jetcarp-mcp). See [Wallpapers, screensavers and the background host](../guides/wallpapers.md), [Driving JETCARP from an AI assistant](../tutorials/mcp.md), [MCP tools](mcp.md).
- **Lists**: [`rules`](#jetcarp-rules), [`tilings`](#jetcarp-tilings), [`isohedral`](#jetcarp-isohedral), [`palettes`](#jetcarp-palettes). See [Named rule catalogue](../automata/named-rules.md), [Rule notation](rule-notation.md), [Tiling catalogue](../tilings/catalogue.md), [Palettes](palettes.md).
- **Setup and upkeep**: [`setup`](#jetcarp-setup), [`paths`](#jetcarp-paths), [`doctor`](#jetcarp-doctor), [`uninstall`](#jetcarp-uninstall). See [First run and setup](../getting-started/first-run.md), [Folders and environment variables](folders.md), [Uninstalling](../guides/uninstalling.md).

## Worlds and scenes

Run a scene headless, check that it replays, find its objects, jump far ahead with HashLife, and the computing demos. See [Scenes](../guides/scenes.md), [HashLife and very long runs](../guides/hashlife.md).

### `jetcarp run`

```text
Load a scene, run it, and report generation, hash, and populations

Usage: jetcarp run [OPTIONS] <SCENE>

Arguments:
  <SCENE>  

Options:
      --gens <GENS>              Generations to run after replaying the scene's own commands [default: 0]
      --json                     Print the result as JSON instead of text (commands that report a result)
      --png <PNG>                Write a PNG of the final state (scene, version, and hash are embedded as text chunks)
      --scale <SCALE>            Pixels per world unit (tile edges are about 1 unit). Default: fit into --max-side
      --max-side <MAX_SIDE>      [default: 1600]
      --save <SAVE>              Save the resulting scene (with its generation and hash recorded)
      --threads <THREADS>        Worker threads (default: all cores). The hash must not depend on this
      --expect <EXPECT>          Fail unless the final hash equals this
      --gpu                      Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                      Use only the CPU, whatever the GPU setting says
      --reseed <RESEED>          Install automatic reseed policies before running: one policy or a list, as JSON (the book's "Reseeding" guide), e.g. '{"mode":{"kind":"blobs"},"when":{"kind":"settled"},"cooldown":200}'
      --reseed-now <RESEED_NOW>  Reseed once before running, with a mode as JSON, e.g. '{"kind":"rain","drops":40}'
  -h, --help                     Print help
```

### `jetcarp verify`

```text
Check that a saved scene still replays to its recorded hash (or a PNG JETCARP wrote, which carries its scene)

Usage: jetcarp verify [OPTIONS] <SCENE>

Arguments:
  <SCENE>  

Options:
      --json  Print the result as JSON instead of text (commands that report a result)
  -h, --help  Print help
```

### `jetcarp objects`

```text
Find still lifes, oscillators, and spaceships in a scene after running it

Usage: jetcarp objects [OPTIONS] <SCENE>

Arguments:
  <SCENE>  

Options:
      --gens <GENS>              Generations to run after replaying the scene's commands [default: 0]
      --json                     Print the full report as JSON
      --list                     List every object, not just the counts
      --png <PNG>                PNG with each object's cells coloured by class
      --scale <SCALE>            
      --max-side <MAX_SIDE>      [default: 1600]
      --ca <CA>                  CA id to analyse (default: the first enabled B/S, Generations, WireWorld, or table CA)
      --max-period <MAX_PERIOD>  Longest period looked for [default: 64]
      --merge <MERGE>            Cells within this many neighbour steps start in one group [default: 2]
      --no-split                 Do not split merged groups into non-interacting parts
      --threads <THREADS>        
  -h, --help                     Print help
```

### `jetcarp hashlife`

```text
Jump a pattern (unbounded plane) or a wrap-edge world (power-of-two torus) far ahead with HashLife; export a region as a scene or PNG

Usage: jetcarp hashlife [OPTIONS] <INPUT>

Arguments:
  <INPUT>  A scene (.json), a pattern-library record (.json), or a pattern in RLE or plaintext

Options:
      --gens <GENS>              Generations to jump: 1000, 2^40, 1e6 [default: 0]
      --torus                    Jump the scene's wrap-edge world in place (its torus sides must be powers of two), exactly as stepping it would on states. HashLife does not track ages: every nonzero cell ends with age 1, so `--compare` checks states, not the full hash. Default: the unbounded plane (exported cells also start at age 1)
      --region <REGION>          Plane mode: the exported region `i0,j0,w,h` in unit cells (lattice coordinates), or `world` for the source scene's own frame. Default: the pattern's bounding box plus --margin
      --margin <MARGIN>          [default: 8]
      --boundary <BOUNDARY>      Edges of the exported world: dead, wrap, or live [default: dead]
      --rule <RULE>              Override the rule (RLE and plaintext default to the RLE header, else B3/S23)
      --png <PNG>                Write a PNG: the JETCARP renderer up to --max-cells, a density map above that
      --palette <PALETTE>        The PNG's palette (`jetcarp palettes` lists them): every CA's colours for the JETCARP renderer, the stops and background of the density map. Default: the scene's own palettes, and Ember for the density map
      --max-side <MAX_SIDE>      Largest side of the PNG in pixels [default: 1600]
      --scale <SCALE>            Pixels per world unit for the JETCARP renderer (default: fit --max-side)
      --max-cells <MAX_CELLS>    Regions with more cells than this are drawn as a density map [default: 16000000]
      --save <SAVE>              Save the result as a scene (plane mode: the region as a new world)
      --node-cap <NODE_CAP>      Live nodes before a garbage collection (each node is about 40 bytes with its table slot) [default: 48000000]
      --leaf-level <LEAF_LEVEL>  Largest node level stepped by the bit-parallel leaf (B/S and Generations rules without B0; at most 6, 0 = off). Results never depend on it [default: 6]
      --compare <COMPARE>        Also run the ordinary engine (`cpu` or `gpu`) and compare cell states (torus mode, or plane mode with `--region world`)
      --json                     
  -h, --help                     Print help
```

### `jetcarp demo`

```text
Computing demos: a WireWorld adder and Life logic gates for any inputs

Usage: jetcarp demo [OPTIONS] <KIND>

Arguments:
  <KIND>  wireworld-adder, life-not, life-and, life-or

Options:
      --a <A>          First input (adder: 0-3; gates: 0 or 1) [default: 1]
      --json           Print the result as JSON instead of text (commands that report a result)
      --b <B>          Second input (adder: 0-3; gates: 0 or 1; NOT ignores it) [default: 1]
      --table          Life gates: every input combination side by side (the Quick Start scenes)
      --save <SAVE>    Save the scene here (open it in the app or with `jetcarp run`)
      --png <PNG>      Run this many generations and write a PNG
      --gens <GENS>    
      --scale <SCALE>  Pixels per world unit for the PNG [default: 4]
  -h, --help           Print help
```

## Searches and jobs

Start or carry on a search as a resumable job, and follow, stop, resume, report on and read the runs of any job, whatever started it. See [Searching for patterns](../guides/searching.md), [Large sandpiles](../guides/large-sandpiles.md), [Search runs](search-runs.md).

### `jetcarp search`

```text
Start or carry on a search as a resumable catalog job: `hunt` (soup), `bounded`, `ants`, `census`, `mover`, `pile`, `collide`, `evolve`, `explore`; or `try` one configuration

Usage: jetcarp search [OPTIONS] <COMMAND>

Commands:
  hunt     Fast soup hunter: small soups on an unbounded plane, bit-parallel, until they settle; every object classed, new patterns verified and cataloged [alias: soup]
  bounded  Bounded soup search: soups on a finite grid of any tiling (aperiodic patches too); every verified still life, oscillator, and spaceship cataloged
  ants     Ant and turmite search: turn strings and turmite tables on unbounded periodic tilings, highways proved exactly, every run classed and cataloged
  census   Rule census: rules run from seeded soups, each run classed (CSV, JSON, contact sheet)
  mover    A mover on an aperiodic tiling, followed on a sliding window of supertiles as far as --rings or --gens allow, verified to a distance (`jetcarp library movers track`: fixed patch levels) [alias: movers]
  pile     Large sandpile: N grains on one spot of an unbounded grid, built by doubling, with a cropped PNG, a preview, metadata, and a resumable pile file per step
  collide  Collide pattern B into pattern A at every offset and phase in a range
  evolve   Evolutionary glider search (Edmond's method) on any tiling; verified movers go to the library
  explore  The broad search over tilings, rule families, rules, and seeds (MAP-Elites): runs until stopped or out of budget, every run kept in the job's own folder (`searches/<uuid>/` beside the library: Parquet segments of a million runs, and a working database). Not deterministic: a stopped search continues from its archive. `jetcarp jobs runs|sheet|scene|review|clips|rescore|seal` read it
  try      Run one configuration now on the CPU, measured as the explore search measures its runs: fate, measures, score, and a picture (not a job)
  help     Print this message or the help of the given subcommand(s)

Options:
      --json  Print the result as JSON instead of text (commands that report a result)
  -h, --help  Print help
```

#### `jetcarp search hunt`

```text
Fast soup hunter: small soups on an unbounded plane, bit-parallel, until they settle; every object classed, new patterns verified and cataloged

Usage: jetcarp search hunt [OPTIONS]

Options:
      --json
          Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>
          Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>
          A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>
          Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>
          Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>
          Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>
          Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>
          Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach
          Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet
          No progress on stderr
      --rules <RULES>
          Rules, separated by spaces or semicolons (B/S and Generations rules without B0; a `V` suffix means edge neighbours) [default: B3/S23]
      --rules-file <RULES_FILE>
          More rules, one per line
      --tilings <TILINGS>
          Tilings: ids such as hex or k2-017 (`catalog:k2-017` works too), or the sets all-periodic, archimedean, kuniform:<k>, isohedral [default: square]
      --nb <NB>
          Neighbourhood for every rule: touch or edge (default: the notation decides)
      --soups <SOUPS>
          Soups per rule and tiling [default: 100000]
      --forever
          Run until stopped (Ctrl-C, `jetcarp jobs stop JOB`, or the MCP tool job_stop)
      --size <SIZE>
          Soup size in unit cells, WxH [default: 16x16]
      --density <DENSITY>
          [default: 0.5]
      --shape <SHAPE>
          rect or disc [default: rect]
      --seed <SEED>
          [default: 1]
      --gens <GENS>
          Generations before a soup that has not settled is censused as it is [default: 20000]
      --max-period <MAX_PERIOD>
          Longest period looked for [default: 64]
      --pop-cap <POP_CAP>
          Live cells above which a soup counts as exploded (0: automatic) [default: 0]
      --margin <MARGIN>
          Empty margin around the soup before escaping ships are cut (0: automatic) [default: 0]
      --store-max-cells <STORE_MAX_CELLS>
          Storage policy: still lifes and oscillators with more cells than this are kept as unstored rows (no full record) until seen --store-min-seen times [default: 64]
      --store-max-volume <STORE_MAX_VOLUME>
          ... or with period x cells above this [default: 4000]
      --store-min-seen <STORE_MIN_SEEN>
          Copies in this hunt after which such an object is stored anyway (0: never) [default: 2]
      --store-all
          Store every new object as a full record (the policy before wave 4)
      --gpu
          Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu
          Use only the CPU, whatever the GPU setting says
      --max-rss-gb <MAX_RSS_GB>
          Stop (paused, resumable) if the process uses more memory than this many GB
  -h, --help
          Print help
```

#### `jetcarp search bounded`

```text
Bounded soup search: soups on a finite grid of any tiling (aperiodic patches too); every verified still life, oscillator, and spaceship cataloged

Usage: jetcarp search bounded [OPTIONS]

Options:
      --json                       Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>          Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>              A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>               Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>          Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>              Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>              Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>          Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach                     Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet                      No progress on stderr
      --rules <RULES>              Rules separated by spaces or semicolons ("B3/S23 B36/S23") [default: B3/S23]
      --rules-file <RULES_FILE>    More rules, one per line (# comments)
      --tilings <TILINGS>          Tilings: ids such as hex or k2-017 (`catalog:k2-017` works too), or the sets all-periodic, archimedean, kuniform:<k>, isohedral; comma separated [default: square]
      --size <SIZE>                WxH unit cells (64x64), about N cells (4000), or L<level> for aperiodic patches [default: 64x64]
      --boundary <BOUNDARY>        wrap or dead [default: wrap]
      --nb <NB>                    Neighbourhood for every rule: touch or edge (default: per rule; V suffix = edge)
      --soups <SOUPS>              [default: 16]
      --density <DENSITY>          [default: 0.37]
      --gens <GENS>                [default: 2000]
      --seed <SEED>                [default: 1]
      --max-period <MAX_PERIOD>    [default: 64]
      --soup-radius <SOUP_RADIUS>  Soup only a disc at the centre: radius as a fraction of half the shorter side (e.g. 0.15). With --boundary dead, escaping spaceships are caught at the edge and cataloged
      --json-out <JSON_OUT>        Write the full report as JSON
      --no-store                   Report only; do not add to the library
      --top <TOP>                  Rows per group in the printed report [default: 12]
      --max-rss-gb <MAX_RSS_GB>    Stop (paused, resumable) if the process uses more memory than this many GB
  -h, --help                       Print help
```

#### `jetcarp search ants`

```text
Ant and turmite search: turn strings and turmite tables on unbounded periodic tilings, highways proved exactly, every run classed and cataloged

Usage: jetcarp search ants [OPTIONS]

Options:
      --json                     Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>            A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>             Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>        Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>            Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>            Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>        Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach                   Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet                    No progress on stderr
      --tilings <TILINGS>        Tilings: family ids, catalog ids, vertex configurations, or sets (archimedean, kuniform:2, isohedral, all-periodic) [default: square]
      --strings <STRINGS>        Turn string lengths, `2-6` or `4`
      --tables <TABLES>          Turmite tables STATESxCOLORS, e.g. `2x2`
      --alphabet <ALPHABET>      Turn letters to search (default: the tiling's: L R N U on squares, L R N U L2 R2 on hexagons, L R N on triangles)
      --all-max <ALL_MAX>        Tables: enumerate every table up to this many, else sample --sample of them [default: 1000000]
      --sample <SAMPLE>          [default: 10000]
      --rules <RULES>            Explicit rules, separated by spaces or semicolons (`RL`, `LLRR`, `RL2N`, `1R1,1L0/1R1,0N0`)
      --soups <SOUPS>            Soups per rule (default: an empty field)
      --size <SIZE>              Soup size in unit cells, WxH [default: 8x8]
      --density <DENSITY>        [default: 0.5]
      --seed <SEED>              [default: 1]
      --steps <STEPS>            Steps before a run is classed as it is [default: 1000000]
      --max-period <MAX_PERIOD>  Longest highway or orbit period looked for [default: 20000]
      --no-symmetry              Skip the mirror-symmetry test
  -h, --help                     Print help
```

#### `jetcarp search census`

```text
Rule census: rules run from seeded soups, each run classed (CSV, JSON, contact sheet)

Usage: jetcarp search census [OPTIONS]

Options:
      --json                       Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>          Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>              A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>               Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>          Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>              Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>              Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>          Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach                     Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet                      No progress on stderr
      --tilings <TILINGS>          The tiling: an id such as k2-017 (see `jetcarp tilings`), or a scene or tile JSON file, whose tiling replaces --size and --boundary. A census runs on one tiling [default: square]
      --size <SIZE>                WxH unit cells (64x64), about N cells (4000), or L<level> for aperiodic tilings (L4) [default: 64x64]
      --boundary <BOUNDARY>        wrap, dead, or live [default: wrap]
      --nb <NB>                    Neighbourhood for every rule: touch or edge (default: per rule; a V suffix means edge)
      --rules <RULES>              Rules separated by spaces or semicolons, e.g. "B3/S23 B36/S23 B2/S/C3"
      --rules-file <RULES_FILE>    File with one rule per line (# comments)
      --all                        Every B/S rule whose digits come from --birth and --survive
      --sample <SAMPLE>            A deterministic random sample of N distinct B/S rules over --birth and --survive
      --sample-seed <SAMPLE_SEED>  [default: 1]
      --birth <BIRTH>              Birth digits for --all/--sample, like 1-8 or 1,2,3,6 ("max" = the tiling's largest degree) [default: 1-max]
      --survive <SURVIVE>          Survival digits for --all/--sample [default: 0-max]
      --states <STATES>            Generations rules with this many states (for --all/--sample)
      --soups <SOUPS>              Soups per rule (soup s uses RNG state seed + s) [default: 8]
      --density <DENSITY>          [default: 0.37]
      --gens <GENS>                Generation limit per run [default: 2000]
      --seed <SEED>                [default: 1]
      --explode <EXPLODE>          Stop a run and call it exploded once its population exceeds this fraction of the cells [default: 0.5]
      --csv <CSV>                  Write per-rule summaries as CSV
      --json-out <JSON_OUT>        Write the full report (spec, seeds, every run) as JSON
      --sheet <SHEET>              Labelled contact sheet of each rule's first soup at the end of its run
      --sheet-cols <SHEET_COLS>    [default: 8]
      --thumb <THUMB>              Thumbnail size in pixels for --sheet [default: 160]
      --rerun <RERUN>              Rerun the census recorded in a JSON report (its spec replaces the tiling and rule flags)
  -h, --help                       Print help
```

#### `jetcarp search mover`

```text
A mover on an aperiodic tiling, followed on a sliding window of supertiles as far as --rings or --gens allow, verified to a distance (`jetcarp library movers track`: fixed patch levels)

Usage: jetcarp search mover [OPTIONS]

Options:
      --json                         Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>            Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>                A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>                 Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>            Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>                Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>                Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>            Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach                       Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet                        No progress on stderr
      --rule <RULE>                  Rule notation (SpectreGlider, "Priority 4: ...", B/S) [default: SpectreGlider]
      --tiling <TILING>              Aperiodic tiling: spectre, hat, penrose-kd, penrose-rhomb [default: spectre]
      --level <LEVEL>                Patch level the cells are given on [default: 4]
      --nb <NB>                      [default: touch]
      --cells <CELLS>                Starting cells as CELL:STATE pairs ("1203:1,1204:3")
      --states <STATES>              Instead of --cells: put these states (comma separated, "1,3") on the tile --at and its edge neighbours in order; with no --at, try pairs around the patch's deepest tile until one travels
      --at <AT>                      The tile for --states
      --gens <GENS>                  Generation limit (default 20000; with --rings, 100 per ring more)
      --max-pop <MAX_POP>            Stop as grown above this many live cells [default: 2000]
      --store                        Add the mover to the library when verified
      --json-out <JSON_OUT>          Write the tracker's full report as JSON
      --rings <RINGS>                Stop, verified, at this many edge rings
      --margin <MARGIN>              World units of tiles kept around the live cells [default: 24]
      --fixed-levels <FIXED_LEVELS>  Spectre: follow the fixed patches' Delta chain up to this level [default: 8]
  -h, --help                         Print help
```

#### `jetcarp search pile`

```text
Large sandpile: N grains on one spot of an unbounded grid, built by doubling, with a cropped PNG, a preview, metadata, and a resumable pile file per step

Usage: jetcarp search pile [OPTIONS]

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>      A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>       Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>  Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>      Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>      Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>  Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach             Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet              No progress on stderr
      --tiling <TILING>    Tiling id: square, hex, triangle, snubsq, cairo, k3-012, ih41, spectre, hat, ... [default: square]
      --grains <GRAINS>    Grains: 1000000, 1e7, 2^30 (up to 2^64 − 1) [default: 100000]
      --nb <NB>            edge (von Neumann on squares) or touch (Moore) [default: edge]
      --centre <CENTRE>    cell, vertex, or edge (grains split among the cells meeting at the point) [default: cell]
      --level <LEVEL>      Aperiodic tilings: patch level (default: the smallest that should hold the pile, up to 8)
      --out <OUT>          Parent folder for the run folder (default `<Pictures>/JETCARP/piles`)
      --px <PX>            Pixels across a cell in pile.png (default 2; below 1.5, each pixel averages the cells centred in it). With --resume on a finished pile, the picture options export it again
      --margin <MARGIN>    Empty cells around the pile in the image (default 4)
      --palette <PALETTE>  Palette for the heights, with its background and stage (`jetcarp palettes` lists them). Default: Ember, or the folder's own when resuming one
      --reverse            Run the palette the other way (height 1 at its last stop)
      --preview <PREVIEW>  Longest side of preview.png (default 1024; 0 = none)
      --no-png             Skip the full-size pile.png
      --gpu                Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                Use only the CPU, whatever the GPU setting says
  -h, --help               Print help
```

#### `jetcarp search collide`

```text
Collide pattern B into pattern A at every offset and phase in a range

Usage: jetcarp search collide [OPTIONS] [A] [B]

Arguments:
  [A]  Pattern A (stays at the origin)
  [B]  Pattern B (placed at each offset)

Options:
      --json                           Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>              Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>                  A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>                   Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>              Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>                  Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>                  Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>              Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach                         Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet                          No progress on stderr
      --offsets <OFFSETS>              Offset range for both axes, like -6..6 (overridden by --dx/--dy) [default: -6..6]
      --dx <DX>                        
      --dy <DY>                        
      --phases <PHASES>                B's phases: all, or a list like 0,1 [default: all]
      --orientation-a <ORIENTATION_A>  [default: 0]
      --orientation-b <ORIENTATION_B>  B's orientation (index into the tiling's symmetries; `jetcarp library show` lists the direction each one moves a spaceship) [default: 0]
      --gens <GENS>                    [default: 1024]
      --margin <MARGIN>                [default: 40]
      --rule <RULE>                    Rule to run (both patterns must work in it; default: A's rule if B works in it, else B's)
      --csv <CSV>                      
      --json-out <JSON_OUT>            
      --sheet <SHEET>                  Labelled contact sheet of every run's final state, objects coloured by class
      --thumb <THUMB>                  [default: 96]
      --cols <COLS>                    [default: 13]
      --no-store                       Do not catalog the products
  -h, --help                           Print help
```

#### `jetcarp search evolve`

```text
Evolutionary glider search (Edmond's method) on any tiling; verified movers go to the library

Usage: jetcarp search evolve [OPTIONS]

Options:
      --json                     Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>            A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>             Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>        Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>            Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>            Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>        Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach                   Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet                    No progress on stderr
      --tiling <TILING>          Tiling: an id such as spectre, hat, penrose-kd, penrose-rhomb, square, hex, trihex or k2-017 (k-uniform and Archimedean tilings; `catalog:k2-017` works too) [default: spectre]
      --nb <NB>                  Neighbourhood: touch (the paper's vertex adjacency) or edge [default: touch]
      --states <STATES>          States per cell (2-8) [default: 4]
      --pop <POP>                Population size [default: 256]
      --gens <GENS>              GA generations [default: 800]
      --seed <SEED>              RNG seed (the run is a pure function of it) [default: 1]
      --radius <RADIUS>          Arena radius in edge rings [default: 48]
      --horizon <HORIZON>        Generations per seed run [default: 2500]
      --cap <CAP>                Population cap during the GA (explosive rules score 0) [default: 64]
      --champions <CHAMPIONS>    Distinct top genomes verified at the end [default: 16]
      --verify <VERIFY>          Verification radii as multiples of --radius [default: 1,2,4]
      --stop-on-glider           Stop at the first genome that passes verification
      --out <OUT>                Output root (default: $JETCARP_OUT, else <Pictures>/JETCARP); the run gets its own folder
      --no-store                 Do not add verified movers to the library
      --track-gens <TRACK_GENS>  Generations the mover tracker may run on aperiodic patches [default: 20000]
  -h, --help                     Print help
```

#### `jetcarp search explore`

```text
The broad search over tilings, rule families, rules, and seeds (MAP-Elites): runs until stopped or out of budget, every run kept in the job's own folder (`searches/<uuid>/` beside the library: Parquet segments of a million runs, and a working database). Not deterministic: a stopped search continues from its archive. `jetcarp jobs runs|sheet|scene|review|clips|rescore|seal` read it

Usage: jetcarp search explore [OPTIONS]

Options:
      --json                           Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>              Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>                  A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>                   Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>              Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>                  Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>                  Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>              Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach                         Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet                          No progress on stderr
      --hours <HOURS>                  Hours to run (the same as --minutes 60 x HOURS)
      --tilings <TILINGS>              Only these tilings: ids, comma separated (square,hex,trihex,ih28,k2-001; default: every tiling the search knows, 1,359 today; the progress line counts them)
      --families <FAMILIES>            Only these rule families, comma separated (life_like, generations, ltl, jaca, cyclic, rps, greenberg_hastings, hodgepodge, priority, wireworld, turmite, layered, wolfram, hex_wolfram, hex_tri, margolus, lenia, sandpile, forest_fire, table, von_neumann, spectre_glider; default: all)
      --screen-cells <SCREEN_CELLS>    About this many cells in a screening run's grid [default: 16000]
      --screen-gens <SCREEN_GENS>      [default: 1024]
      --confirm-cells <CONFIRM_CELLS>  Cells of a confirmation run (the best runs again, on a bigger grid) [default: 64000]
      --confirm-gens <CONFIRM_GENS>    [default: 4096]
      --confirm-seeds <CONFIRM_SEEDS>  Seeds an elite is confirmed with [default: 3]
      --batch <BATCH>                  Most worlds in a GPU batch [default: 1024]
      --seed <SEED>                    [default: 0]
      --gpu                            Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                            Use only the CPU, whatever the GPU setting says
      --max-rss-gb <MAX_RSS_GB>        Stop (paused, resumable) if the process uses more memory than this many GB
      --score <SCORE>                  Score runs with this fitted score model (`jetcarp jobs refit --out` writes one) instead of the built-in score; the job keeps a copy, so resuming it keeps the model
      --parents-from <PARENTS_FROM>    Take parents from this explore job: the JACA rules of its reviewed runs (or its best runs), transplanted to other tilings and tuned for balance early in this search
      --reviewed <REVIEWED>            With --parents-from: the verdicts whose runs to take (default keep,maybe)
  -h, --help                           Print help
```

#### `jetcarp search try`

```text
Run one configuration now on the CPU, measured as the explore search measures its runs: fate, measures, score, and a picture (not a job)

Usage: jetcarp search try [OPTIONS] --rule <RULE>

Options:
      --json             Print the result as JSON instead of text (commands that report a result)
      --tiling <TILING>  Tiling id (square, hex, trihex, spectre, k2-001, ih28, ...) [default: square]
      --rule <RULE>      Rule notation
      --seed <SEED>      Starting pattern: a seed shorthand ("random:0.3", "pattern:blobs:0.4:by_region") or JSON (default: the rule's own)
      --nb <NB>          touch or edge (default: touch, or what the notation says)
      --cells <CELLS>    About this many cells [default: 16000]
      --gens <GENS>      [default: 1024]
      --rng <RNG>        Random seed [default: 1]
      --px <PX>          Picture size in pixels [default: 420]
      --png <PNG>        Write a picture of the run's end here
      --save <SAVE>      Save the scene that replays the run here
  -h, --help             Print help
```

### `jetcarp jobs`

```text
The catalog's jobs, whatever started them: `list`, `status`, `stop`, `resume`, `report`, `remove`; an explore job's `runs`, `sheet`, `scene`, `review`, `clips`, `rescore`

Usage: jetcarp jobs [OPTIONS] <COMMAND>

Commands:
  list     The jobs, newest first: id, kind, status, started, progress
  status   One job's progress (no JOB: the list), live when it runs in this process
  wait     Wait until a job stops running, in any process. The exit status says how it ended: 0 done, 3 paused (stopped, out of budget, or past its memory cap), 4 failed, 5 still running at --timeout, 6 gone (marked running, but its process crashed or was killed)
  stop     Ask a running job (in any process) to stop at its next checkpoint; it stays resumable
  resume   Carry on a stopped, paused, or cut-off job from its last checkpoint
  report   A Markdown report of what a job (or every job with a label prefix) searched, for how long, and what it found
  remove   Delete a job that is not running, with its sightings (and with --delete-results, the patterns only it found, its ant runs, and its working folder: an explore search's runs)
  runs     An explore job's stored runs, best first (JOB default: the newest explore job)
  sheet    A contact sheet of an explore job's runs at several moments, labelled
  scene    An explore job's run as a scene that replays it (printed, or saved with --save)
  review   Record a verdict on an explore job's run (keep, maybe, drop), or with --from the ratings that the clip-rating page saved (`jobs clips --page`)
  clips    Animated clips of a stratified sample of an explore job's runs (or chosen runs), with a manifest, for rating by eye
  refit    Fit a score model to the keep, maybe and drop reviews of explore jobs' runs (JOB... default: the newest explore job), and report how well it ranks runs held out of the fit
  rescore  Recompute every stored score of a stopped explore job from its stored measures (after the score changes, or by a fitted model), and rebuild its archive. Sealed segments are not rewritten: their new scores go in a sidecar per segment (`scores-<n>/` in the search folder)
  seal     Seal a stopped explore job's newest runs (those not yet in a Parquet segment), so its folder holds every run in Parquet; a search seals every million runs by itself
  help     Print this message or the help of the given subcommand(s)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp jobs list`

```text
The jobs, newest first: id, kind, status, started, progress

Usage: jetcarp jobs list [OPTIONS]

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --kind <KIND>        Only jobs of this kind: hunt, search, ants, census, mover, pile, collide, evolve, explore
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --status <STATUS>    Only jobs with this status: running, paused, done, failed
      --label <LABEL>      Only jobs with this label
      --prefix <PREFIX>    Only jobs whose label starts with this
      --limit <LIMIT>      At most this many (newest first)
  -h, --help               Print help
```

#### `jetcarp jobs status`

```text
One job's progress (no JOB: the list), live when it runs in this process

Usage: jetcarp jobs status [OPTIONS] [JOB]

Arguments:
  [JOB]  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --wait <WAIT>        Wait up to this many seconds for a running job to stop running
      --finds <FINDS>      Show this many of its newest and rarest finds
      --kind <KIND>        Only jobs of this kind: hunt, search, ants, census, mover, pile, collide, evolve, explore
      --status <STATUS>    Only jobs with this status: running, paused, done, failed
      --label <LABEL>      Only jobs with this label
      --prefix <PREFIX>    Only jobs whose label starts with this
      --limit <LIMIT>      At most this many (newest first)
  -h, --help               Print help
```

#### `jetcarp jobs wait`

```text
Wait until a job stops running, in any process. The exit status says how it ended: 0 done, 3 paused (stopped, out of budget, or past its memory cap), 4 failed, 5 still running at --timeout, 6 gone (marked running, but its process crashed or was killed)

Usage: jetcarp jobs wait [OPTIONS] <JOB>

Arguments:
  <JOB>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --timeout <TIMEOUT>  Give up after this many seconds
      --quiet              No progress on stderr
  -h, --help               Print help
```

#### `jetcarp jobs stop`

```text
Ask a running job (in any process) to stop at its next checkpoint; it stays resumable

Usage: jetcarp jobs stop [OPTIONS] <JOB>

Arguments:
  <JOB>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp jobs resume`

```text
Carry on a stopped, paused, or cut-off job from its last checkpoint

Usage: jetcarp jobs resume [OPTIONS] <JOB>

Arguments:
  <JOB>  

Options:
      --json                 Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>    Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --threads <THREADS>    Worker threads (default: all cores; an explore search leaves two free)
      --gpu                  Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                  Use only the CPU, whatever the GPU setting says
      --minutes <MINUTES>    Stop at the next checkpoint after this many minutes
      --until <UNTIL>        Stop at the next checkpoint after this Unix time (seconds since 1970)
      --limit <LIMIT>        Stop after this many units in this session
      --detach               Run in a process of its own and return at once
      --json-out <JSON_OUT>  When it ends: the full report as JSON
      --csv <CSV>            When it ends: a census's or a collision's table as CSV
      --sheet <SHEET>        When it ends: a census's or a collision's contact sheet
      --quiet                No progress on stderr
  -h, --help                 Print help
```

#### `jetcarp jobs report`

```text
A Markdown report of what a job (or every job with a label prefix) searched, for how long, and what it found

Usage: jetcarp jobs report [OPTIONS] [JOB]

Arguments:
  [JOB]  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --prefix <PREFIX>    Every job whose label starts with this, instead of one job
      --md <MD>            Write the Markdown here instead of printing it
      --top <TOP>          Rules or finds listed per row [default: 5]
  -h, --help               Print help
```

#### `jetcarp jobs remove`

```text
Delete a job that is not running, with its sightings (and with --delete-results, the patterns only it found, its ant runs, and its working folder: an explore search's runs)

Usage: jetcarp jobs remove [OPTIONS] <JOB>

Arguments:
  <JOB>  

Options:
      --delete-results     
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp jobs runs`

```text
An explore job's stored runs, best first (JOB default: the newest explore job)

Usage: jetcarp jobs runs [OPTIONS] [JOB]

Arguments:
  [JOB]  

Options:
      --family <FAMILY>          Only this rule family
      --json                     Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --tiling <TILING>          Only this tiling key
      --fate <FATE>              died, froze, cycle, or running
      --min-score <MIN_SCORE>    
      --min-movers <MIN_MOVERS>  
      --stage <STAGE>            1 screened (default), 2 confirmation runs
      --reviewed <REVIEWED>      Only reviewed runs: any, keep, maybe, or drop
      --ids <IDS>                These runs (comma separated) instead of the filters
      --order <ORDER>            score (default), newest, or id (the default with --since)
      --limit <LIMIT>            Most runs (0: every run) [default: 40]
      --measures <MEASURES>      Also show these measures: all, or names, comma separated (settle, density_late, activity_late, smooth, sign_balance, sign_sorted, sign_trade, sign_lost, ...)
      --format <FORMAT>          text (default), csv, or json [default: text]
      --since <SINCE>            Only runs with a larger id than this, in id order (to export a search in parts)
      --out <OUT>                Write the runs here as CSV instead of printing them
  -h, --help                     Print help
```

#### `jetcarp jobs sheet`

```text
A contact sheet of an explore job's runs at several moments, labelled

Usage: jetcarp jobs sheet [OPTIONS] --png <PNG> [JOB]

Arguments:
  [JOB]  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --png <PNG>          Where to write the PNG
      --ids <IDS>          Runs to show, comma separated (`jetcarp jobs runs` finds them)
      --px <PX>            Pixels per thumbnail [default: 220]
      --cols <COLS>        Thumbnails per row [default: 9]
      --fracs <FRACS>      Fractions of each run to show, comma separated [default: 0.125,0.5,1]
      --palette <PALETTE>  A palette; several (comma separated) compare them side by side, one row per run
      --max <MAX>          Most runs on the sheet [default: 36]
  -h, --help               Print help
```

#### `jetcarp jobs scene`

```text
An explore job's run as a scene that replays it (printed, or saved with --save)

Usage: jetcarp jobs scene [OPTIONS] <JOB> <RUN>

Arguments:
  <JOB>  
  <RUN>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --save <SAVE>        
  -h, --help               Print help
```

#### `jetcarp jobs review`

```text
Record a verdict on an explore job's run (keep, maybe, drop), or with --from the ratings that the clip-rating page saved (`jobs clips --page`)

Usage: jetcarp jobs review [OPTIONS] <JOB> [RUN] [VERDICT]

Arguments:
  <JOB>      The explore job
  [RUN]      The run reviewed (its id in the job; `jetcarp jobs runs` lists them)
  [VERDICT]  keep, maybe, or drop

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --note <NOTE>        What was seen, in a sentence
      --rater <RATER>      Who judged; with --from, instead of the name the ratings file gives (default: the file's name, else the publisher's name when `jetcarp pack key --name` set one, else user)
      --from <FROM>        Instead of one run: the ratings file the clip-rating page saved (its verdicts become reviews, its stars stars), or a folder of rating files (`<run id>.json`); ratings already recorded from it are skipped
  -h, --help               Print help
```

#### `jetcarp jobs clips`

```text
Animated clips of a stratified sample of an explore job's runs (or chosen runs), with a manifest, for rating by eye

Usage: jetcarp jobs clips [OPTIONS] --out <OUT> [JOB]

Arguments:
  [JOB]  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --out <OUT>          The folder for the clips and manifest.json
      --sample <SAMPLE>    [default: 96]
      --seed <SEED>        [default: 1]
      --px <PX>            [default: 150]
      --frames <FRAMES>    [default: 28]
      --ids <IDS>          Render these runs instead of a sample: a file of one `id[ bucket]` per line
      --page               Also write index.html, a page for rating the clips by eye: Keep, Maybe, Drop and a star per clip, with the keyboard too. It works offline; its Save ratings button downloads a file for `jetcarp jobs review JOB --from FILE`
  -h, --help               Print help
```

#### `jetcarp jobs refit`

```text
Fit a score model to the keep, maybe and drop reviews of explore jobs' runs (JOB... default: the newest explore job), and report how well it ranks runs held out of the fit

Usage: jetcarp jobs refit [OPTIONS] [JOBS]...

Arguments:
  [JOBS]...  The explore jobs whose runs' reviews to learn (default: the newest explore job)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --rater <RATER>      Only this rater's reviews (default: every rater's)
      --family <FAMILY>    Fit to this family's runs only; the model then scores only that family
      --out <OUT>          Write the model here (JSON), for `search explore --score` and `jobs rescore --score`
      --no-replay          Do not replay reviewed runs that lack measures (JACA runs stored before sign measures); they count at the mean
      --gpu                Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                Use only the CPU, whatever the GPU setting says
  -h, --help               Print help
```

#### `jetcarp jobs rescore`

```text
Recompute every stored score of a stopped explore job from its stored measures (after the score changes, or by a fitted model), and rebuild its archive. Sealed segments are not rewritten: their new scores go in a sidecar per segment (`scores-<n>/` in the search folder)

Usage: jetcarp jobs rescore [OPTIONS] [JOB]

Arguments:
  [JOB]  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --score <SCORE>      Score with this fitted model (`jetcarp jobs refit --out` writes one); the job keeps it, so the search goes on with it. `builtin` goes back to the built-in score
      --replay             First replay the runs that lack measures the score reads (JACA runs stored before sign measures), caching what they measure. Slow for a large search
      --gpu                Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                Use only the CPU, whatever the GPU setting says
  -h, --help               Print help
```

#### `jetcarp jobs seal`

```text
Seal a stopped explore job's newest runs (those not yet in a Parquet segment), so its folder holds every run in Parquet; a search seals every million runs by itself

Usage: jetcarp jobs seal [OPTIONS] [JOB]

Arguments:
  [JOB]  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

### `jetcarp pile`

```text
Large sandpile: N grains on one spot of an unbounded grid, built by doubling, with a cropped PNG, a preview, metadata, and a resumable pile file per step (the same as `jetcarp search pile`)

Usage: jetcarp pile [OPTIONS]

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --label <LABEL>      A name to find the job by (`jetcarp jobs list --label`)
      --resume <JOB>       Carry on this job instead of starting one (its spec is kept; the other options are ignored). Piles: a job id or the pile's run folder
      --minutes <MINUTES>  Stop at the next checkpoint after this many minutes; the job stays resumable
      --until <UNTIL>      Stop at the next checkpoint after this Unix time (seconds since 1970): the same end however often the job is resumed
      --limit <LIMIT>      Stop after this many units in this session (soups per rule and tiling, runs, rules, generations); the job stays resumable
      --threads <THREADS>  Worker threads (default: all cores; an explore search leaves two free). Results do not depend on this
      --detach             Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it)
      --quiet              No progress on stderr
      --tiling <TILING>    Tiling id: square, hex, triangle, snubsq, cairo, k3-012, ih41, spectre, hat, ... [default: square]
      --grains <GRAINS>    Grains: 1000000, 1e7, 2^30 (up to 2^64 − 1) [default: 100000]
      --nb <NB>            edge (von Neumann on squares) or touch (Moore) [default: edge]
      --centre <CENTRE>    cell, vertex, or edge (grains split among the cells meeting at the point) [default: cell]
      --level <LEVEL>      Aperiodic tilings: patch level (default: the smallest that should hold the pile, up to 8)
      --out <OUT>          Parent folder for the run folder (default `<Pictures>/JETCARP/piles`)
      --px <PX>            Pixels across a cell in pile.png (default 2; below 1.5, each pixel averages the cells centred in it). With --resume on a finished pile, the picture options export it again
      --margin <MARGIN>    Empty cells around the pile in the image (default 4)
      --palette <PALETTE>  Palette for the heights, with its background and stage (`jetcarp palettes` lists them). Default: Ember, or the folder's own when resuming one
      --reverse            Run the palette the other way (height 1 at its last stop)
      --preview <PREVIEW>  Longest side of preview.png (default 1024; 0 = none)
      --no-png             Skip the full-size pile.png
      --gpu                Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                Use only the CPU, whatever the GPU setting says
  -h, --help               Print help
```

## The library and Quick Start

List, show, curate and promote what the searches found; the Quick Start's built-in examples and an explore search's atlas. See [The pattern library](../guides/pattern-library.md), [Building your own Quick Start](../tutorials/building-quick-start.md).

### `jetcarp library`

```text
Pattern library: list, show, curate, paste, promote to Quick Start; its ant runs and movers

Usage: jetcarp library [OPTIONS] <COMMAND>

Commands:
  list         List patterns (highest interest score first), optionally filtered
  show         Show one pattern: its record, and a text drawing on square and hexagon grids
  paste        Make a scene from several library patterns placed on one grid
  soup         Rebuild the exact soup a hunt find came from, as a scene that replays to the find (run again with the hunt's engine to check it); --best N gives the N soups of its hunt that made the most copies of it instead
  promote      Add a Quick Start entry ("My finds"): a library pattern (ID, which is marked promoted), a run of an explore search (--search-run), or a scene file (--scene)
  promote-run  Promote one run of a sweep folder to the Quick Start ("My finds"): its tiling, CAs, and start, so it replays as in the sweep. The run is also marked a favourite in the folder
  render       Draw a pattern's phases (PNG), or print them as text
  update       Change a pattern's name, tags, notes, rating, or curation state
  remove       Delete a pattern (with its rules, sightings, and Quick Start entry)
  quickstart   List the Quick Start entries (promotions, packs' and the atlas's; --source for one source, the built-ins too), or hide, show or remove one
  ants         The catalog's ant and turmite runs: `list`, `show`, `census`, `update`, `remove`, `scene`, `promote`, and `run` (one rule by itself)
  movers       Movers on aperiodic tilings: `track` (over growing patch levels), `verify` (a library mover again), `list`
  info         The catalog's file, schema version, and row counts
  verify       Verify stored patterns again with this build (e.g. after a geometry change): every pattern off squares and hexagons, or all with --all, plus --sample square and hexagon ones; --drop removes those that fail
  reverify     Check again the records flagged after a change of geometry or of a rule's meaning (their geometry pin, or a pattern's rule-semantics version, is not this engine's), or every record with --all: those that pass get this engine's pin and version and a provenance row (and a new id if their geometry changed, the old id kept as an alias); those that fail stay flagged and are listed, or are deleted with --drop. Runs as a job, resumable with --job
  compounds    Find the stored compounds: patterns that are several independent objects, such as two gliders flying side by side or a pulsar with blinkers beside it (each part runs alone as an object of the same period and velocity, and the pattern is exactly the union of the parts' runs). Lists them and writes a report (one JSON object per compound, with its parts and the whole record); nothing changes without --apply. With --apply each compound's parts are stored (credited with its seen count and sightings, and given its history) and the compound is removed, or kept tagged `compound` when someone curated it or a part does not run in all its rules. Runs as a job, resumable with --job
  prune        Delete the still lifes and oscillators a hunt would no longer store (the hunt storage policy: more than --max-cells cells or period x cells above --max-volume, unless a job saw them --min-seen times; curated patterns are kept). Their sightings become unstored rows. Then compresses old pattern bodies and VACUUMs
  help         Print this message or the help of the given subcommand(s)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library list`

```text
List patterns (highest interest score first), optionally filtered

Usage: jetcarp library list [OPTIONS]

Options:
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --rule <RULE>              Patterns verified in a rule whose notation contains this
      --works-in <WORKS_IN>      Patterns that run in this rule (inside their rule range, or verified there)
      --order <ORDER>            score (default), seen, rare, newest, updated, cells, period, rating, id
      --offset <OFFSET>          
      --tiling <TILING>          Family id (hex), catalog id (k2-001), or part of the tiling name
      --class <CLASS>            still_life, oscillator, spaceship
      --min-period <MIN_PERIOD>  
      --max-period <MAX_PERIOD>  
      --min-score <MIN_SCORE>    
      --state <STATE>            new, reviewed, promoted, rejected
      --tag <TAG>                
      --text <TEXT>              Part of the name or label
      --by <BY>                  search, script, llm, user
      --limit <LIMIT>            
      --json                     Print the records as a JSON array
      --ids                      Print only the ids
  -h, --help                     Print help
```

#### `jetcarp library show`

```text
Show one pattern: its record, and a text drawing on square and hexagon grids

Usage: jetcarp library show [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --json               Print the record as JSON
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --phases <PHASES>    Draw this many phases (square and hex only) [default: 1]
  -h, --help               Print help
```

#### `jetcarp library paste`

```text
Make a scene from several library patterns placed on one grid

Usage: jetcarp library paste [OPTIONS] --out <OUT> <PLACEMENTS>...

Arguments:
  <PLACEMENTS>...  Placements `ID@X,Y[,oK][,pK]` (orientation K, phase K), or `ID` alone for the centre. X is a tile column and Y a row, as in scripts' `w.get(x, y)`, while --size counts unit cells: a unit cell of k tiles spans k columns, so on k2-017 (12 tiles a unit cell) x = 60 is unit cell 5, near the left edge. `ID@uX,Y` gives X in unit cells. The output prints the grid's width and height in tiles

Options:
      --json                 Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>    Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --out <OUT>            Scene file to write
      --size <SIZE>          Grid size in unit cells (not tiles), or L<level> on aperiodic tilings [default: 120x80]
      --boundary <BOUNDARY>  [default: wrap]
      --name <NAME>          
      --png <PNG>            Also write a PNG of the scene after this many generations
      --gens <GENS>          [default: 0]
  -h, --help                 Print help
```

#### `jetcarp library soup`

```text
Rebuild the exact soup a hunt find came from, as a scene that replays to the find (run again with the hunt's engine to check it); --best N gives the N soups of its hunt that made the most copies of it instead

Usage: jetcarp library soup [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --best <BEST>        The N soups of the find's hunt that made the most copies of it (then the most spaceships). Needs the hunt's seed: the record's (finds from 0.4.2 on) or its job's
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --soups <SOUPS>      With --best: how many of the hunt's soups to run again, from soup 0 (default: as many as the hunt ran, at most 10,000)
      --save <SAVE>        Write the scene here; with --best, the best soup's, and the others beside it as NAME-2.json, NAME-3.json, ...
  -h, --help               Print help
```

#### `jetcarp library promote`

```text
Add a Quick Start entry ("My finds"): a library pattern (ID, which is marked promoted), a run of an explore search (--search-run), or a scene file (--scene)

Usage: jetcarp library promote [OPTIONS] [ID]

Arguments:
  [ID]  

Options:
      --json                       Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>          Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --search-run <SEARCH_RUN>    A run of an explore search, by its id: on the grid it was confirmed on, its card on the grid it was found on, as the atlas shows its entries
      --job <JOB>                  With --search-run: the explore job (default: the newest)
      --scene <SCENE>              A scene file
      --title <TITLE>              
      --description <DESCRIPTION>  
  -h, --help                       Print help
```

#### `jetcarp library promote-run`

```text
Promote one run of a sweep folder to the Quick Start ("My finds"): its tiling, CAs, and start, so it replays as in the sweep. The run is also marked a favourite in the folder

Usage: jetcarp library promote-run [OPTIONS] <FOLDER> <RUN>

Arguments:
  <FOLDER>  The sweep folder (with manifest.json and run.json)
  <RUN>     The run: rule text ("HexT3 sum 588"), manifest index ("#12"), or series number ("n=588")

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --title <TITLE>      Quick Start title (default: the rule)
      --note <NOTE>        A note shown before the description (e.g. why it is interesting)
  -h, --help               Print help
```

#### `jetcarp library render`

```text
Draw a pattern's phases (PNG), or print them as text

Usage: jetcarp library render [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --json                     Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --png <PNG>                
      --px <PX>                  Panel size in pixels [default: 160]
      --max-phases <MAX_PHASES>  [default: 16]
      --ascii                    Print text drawings of the phases (square and hex grids)
      --palette <PALETTE>        The PNG's palette (`jetcarp palettes` lists them; default the rule's own colours)
  -h, --help                     Print help
```

#### `jetcarp library update`

```text
Change a pattern's name, tags, notes, rating, or curation state

Usage: jetcarp library update [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --library <LIBRARY>              Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --name <NAME>                    
      --tags <TAGS>                    Replace the tags (comma separated)
      --add-tag <ADD_TAG>              
      --remove-tag <REMOVE_TAG>        
      --notes <NOTES>                  
      --append-notes <APPEND_NOTES>    
      --rating <RATING>                1-5 (0 clears)
      --state <STATE>                  new, reviewed, promoted, rejected
      --discovered-by <DISCOVERED_BY>  search, script, llm, user
      --json                           Print the updated record as JSON
  -h, --help                           Print help
```

#### `jetcarp library remove`

```text
Delete a pattern (with its rules, sightings, and Quick Start entry)

Usage: jetcarp library remove [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library quickstart`

```text
List the Quick Start entries (promotions, packs' and the atlas's; --source for one source, the built-ins too), or hide, show or remove one

Usage: jetcarp library quickstart [OPTIONS]

Options:
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --source <SOURCE>    Only entries of this source: builtin, pack, atlas, promotion, or all (default: all but builtin)
      --hidden             Only hidden entries
      --shown              Only entries shown in the menu
      --hide <ID>          Hide an entry (by handle or id) from the menu, packs and the website
      --unhide <ID>        Show a hidden entry again
      --remove <REMOVE>    Delete an entry
      --json               
  -h, --help               Print help
```

#### `jetcarp library ants`

```text
The catalog's ant and turmite runs: `list`, `show`, `census`, `update`, `remove`, `scene`, `promote`, and `run` (one rule by itself)

Usage: jetcarp library ants [OPTIONS] <COMMAND>

Commands:
  run      Run one rule (no catalog unless --store)
  list     Stored runs, best first, with filters
  show     One stored run
  census   Outcome counts per tiling (of one job, or every run)
  update   Name, rate, or annotate a stored run
  remove   Delete a stored run (with its media rows and ratings)
  scene    Write the build-up scene (the run from step 0) to a file the app opens
  promote  Add the build-up scene to the Quick Start ("My finds")
  help     Print this message or the help of the given subcommand(s)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library ants run`

```text
Run one rule (no catalog unless --store)

Usage: jetcarp library ants run [OPTIONS] <RULE>

Arguments:
  <RULE>  

Options:
      --json                     Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --tiling <TILING>          [default: square]
      --steps <STEPS>            [default: 1000000]
      --max-period <MAX_PERIOD>  [default: 20000]
      --soup <SOUP>              Start from a soup WxH:density:seed (e.g. 8x8:0.5:1) instead of an empty field
      --store                    Store the result in the catalog
  -h, --help                     Print help
```

#### `jetcarp library ants list`

```text
Stored runs, best first, with filters

Usage: jetcarp library ants list [OPTIONS]

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --outcome <OUTCOME>  highway, periodic, symmetric, bounded, chaotic
      --tiling <TILING>    
      --rule <RULE>        
      --job <JOB>          
      --sort <SORT>        score, onset, period, speed, rule [default: score]
      --limit <LIMIT>      [default: 50]
  -h, --help               Print help
```

#### `jetcarp library ants show`

```text
One stored run

Usage: jetcarp library ants show [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library ants census`

```text
Outcome counts per tiling (of one job, or every run)

Usage: jetcarp library ants census [OPTIONS]

Options:
      --job <JOB>          
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library ants update`

```text
Name, rate, or annotate a stored run

Usage: jetcarp library ants update [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --name <NAME>        
      --notes <NOTES>      
      --rating <RATING>    1 to 5
      --state <STATE>      new, kept, promoted, rejected, ... (free text)
  -h, --help               Print help
```

#### `jetcarp library ants remove`

```text
Delete a stored run (with its media rows and ratings)

Usage: jetcarp library ants remove [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library ants scene`

```text
Write the build-up scene (the run from step 0) to a file the app opens

Usage: jetcarp library ants scene [OPTIONS] --out <OUT> <ID>

Arguments:
  <ID>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --out <OUT>          
      --palette <PALETTE>  The scene's palette (default Prism; `jetcarp palettes` lists them)
  -h, --help               Print help
```

#### `jetcarp library ants promote`

```text
Add the build-up scene to the Quick Start ("My finds")

Usage: jetcarp library ants promote [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --palette <PALETTE>  The scene's palette (default Prism)
  -h, --help               Print help
```

#### `jetcarp library movers`

```text
Movers on aperiodic tilings: `track` (over growing patch levels), `verify` (a library mover again), `list`

Usage: jetcarp library movers [OPTIONS] <COMMAND>

Commands:
  track   Track a pattern over growing patch levels and report the distance it was verified to
  verify  Track a library mover again from its stored site and update its record
  list    List the movers in the library (farthest verified first)
  help    Print this message or the help of the given subcommand(s)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library movers track`

```text
Track a pattern over growing patch levels and report the distance it was verified to

Usage: jetcarp library movers track [OPTIONS]

Options:
      --json                   Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>      Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --rule <RULE>            Rule notation (SpectreGlider, "Priority 4: ...", B/S) [default: SpectreGlider]
      --tiling <TILING>        Aperiodic tiling: spectre, hat, penrose-kd, penrose-rhomb [default: spectre]
      --level <LEVEL>          Patch level the cells are given on (the tracker continues on every larger level) [default: 4]
      --max-level <MAX_LEVEL>  Largest level to use (default: 7 on the Spectre, where 8 is on request; the largest elsewhere)
      --nb <NB>                [default: touch]
      --cells <CELLS>          Starting cells as CELL:STATE pairs ("1203:1,1204:3")
      --states <STATES>        Instead of --cells: put these states (comma separated, "1,3") on the tile --at and its edge neighbours in order; with no --at, try pairs around the patch's deepest tile until one is verified
      --at <AT>                The tile for --states
      --gens <GENS>            Generation limit over all levels (default 20000)
      --max-pop <MAX_POP>      Stop as grown above this many live cells [default: 2000]
      --store                  Add the mover to the library when verified
      --json-out <JSON_OUT>    Write the full report as JSON
  -h, --help                   Print help
```

#### `jetcarp library movers verify`

```text
Track a library mover again from its stored site and update its record

Usage: jetcarp library movers verify [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --gens <GENS>        [default: 20000]
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library movers list`

```text
List the movers in the library (farthest verified first)

Usage: jetcarp library movers list [OPTIONS]

Options:
      --json               
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library info`

```text
The catalog's file, schema version, and row counts

Usage: jetcarp library info [OPTIONS]

Options:
      --json               
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp library verify`

```text
Verify stored patterns again with this build (e.g. after a geometry change): every pattern off squares and hexagons, or all with --all, plus --sample square and hexagon ones; --drop removes those that fail

Usage: jetcarp library verify [OPTIONS]

Options:
      --all                
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --sample <SAMPLE>    Square and hexagon patterns checked too (the first by id) [default: 200]
      --periods <PERIODS>  Periods run (default: the record's own default)
      --drop               
  -h, --help               Print help
```

#### `jetcarp library reverify`

```text
Check again the records flagged after a change of geometry or of a rule's meaning (their geometry pin, or a pattern's rule-semantics version, is not this engine's), or every record with --all: those that pass get this engine's pin and version and a provenance row (and a new id if their geometry changed, the old id kept as an alias); those that fail stay flagged and are listed, or are deleted with --drop. Runs as a job, resumable with --job

Usage: jetcarp library reverify [OPTIONS]

Options:
      --all                
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --drop               
      --periods <PERIODS>  Periods each pattern is run (default: its own default)
      --job <JOB>          Resume this re-verification job
      --threads <THREADS>  Worker threads (default: all cores) [default: 0]
      --minutes <MINUTES>  Stop after this many minutes (resume with --job)
  -h, --help               Print help
```

#### `jetcarp library compounds`

```text
Find the stored compounds: patterns that are several independent objects, such as two gliders flying side by side or a pulsar with blinkers beside it (each part runs alone as an object of the same period and velocity, and the pattern is exactly the union of the parts' runs). Lists them and writes a report (one JSON object per compound, with its parts and the whole record); nothing changes without --apply. With --apply each compound's parts are stored (credited with its seen count and sightings, and given its history) and the compound is removed, or kept tagged `compound` when someone curated it or a part does not run in all its rules. Runs as a job, resumable with --job

Usage: jetcarp library compounds [OPTIONS]

Options:
      --apply              Change the library (default: a dry run that only lists)
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --report <REPORT>    The report file (default: reports/compounds-<job>.jsonl beside the library)
      --job <JOB>          Resume this compounds job
      --threads <THREADS>  Worker threads (default: all cores) [default: 0]
      --minutes <MINUTES>  Stop after this many minutes (resume with --job)
  -h, --help               Print help
```

#### `jetcarp library prune`

```text
Delete the still lifes and oscillators a hunt would no longer store (the hunt storage policy: more than --max-cells cells or period x cells above --max-volume, unless a job saw them --min-seen times; curated patterns are kept). Their sightings become unstored rows. Then compresses old pattern bodies and VACUUMs

Usage: jetcarp library prune [OPTIONS]

Options:
      --json                     Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --max-cells <MAX_CELLS>    [default: 64]
      --max-volume <MAX_VOLUME>  [default: 4000]
      --min-seen <MIN_SEEN>      [default: 2]
      --backup <BACKUP>          Copy the catalog here first (a consistent copy, made by SQLite)
      --no-vacuum                Skip the VACUUM (the file keeps its size until one runs)
  -h, --help                     Print help
```

### `jetcarp quickstart`

```text
Quick Start examples: list the built-in ones, check that each one works, or render a contact sheet; `publish` an explore job's best runs into the Quick Start as its atlas, or `build` some: a search of so many minutes, then its atlas published

Usage: jetcarp quickstart [OPTIONS]
       jetcarp quickstart <COMMAND>

Commands:
  publish  Publish an explore job's confirmed best runs into the library's Quick Start as its atlas (JOB default: the newest explore job); the job's earlier atlas entries are replaced
  build    Build Quick Start entries of your own: an explore search of MINUTES in all (on the GPU when there is one), then its best runs published into the Quick Start, as one job. Ctrl-C stops it; `jetcarp jobs resume JOB` carries it on to its end
  help     Print this message or the help of the given subcommand(s)

Options:
      --json             Print the result as JSON instead of text (commands that report a result)
      --check            Run each example (at preview size) and report whether it is alive and changing
      --survey           Try a range of starting patterns for each example and report how each does
      --survey-tilings   For each tiling family, report the first candidate rule that stays lively (used to choose the tiling showcases)
      --gens <GENS>      Generations for --check and --sheet [default: 300]
      --sheet <SHEET>    Write a numbered contact sheet of every example (numbers match the list)
      --filter <FILTER>  Only entries whose name, rule, family, or tiling contains this text
      --thumb <THUMB>    [default: 160]
      --cols <COLS>      [default: 10]
  -h, --help             Print help
```

#### `jetcarp quickstart publish`

```text
Publish an explore job's confirmed best runs into the library's Quick Start as its atlas (JOB default: the newest explore job); the job's earlier atlas entries are replaced

Usage: jetcarp quickstart publish [OPTIONS] [JOB]

Arguments:
  [JOB]  

Options:
      --json                       Print the result as JSON instead of text (commands that report a result)
      --per-niche <PER_NICHE>      Entries per category, family, and tiling [default: 12]
      --min-score <MIN_SCORE>      Lowest confirmed score (contrast picks are exempt) [default: 1.25]
      --max-entries <MAX_ENTRIES>  [default: 12000]
      --library <LIBRARY>          Catalog: a .sqlite file, or a folder holding library.sqlite
  -h, --help                       Print help
```

#### `jetcarp quickstart build`

```text
Build Quick Start entries of your own: an explore search of MINUTES in all (on the GPU when there is one), then its best runs published into the Quick Start, as one job. Ctrl-C stops it; `jetcarp jobs resume JOB` carries it on to its end

Usage: jetcarp quickstart build [OPTIONS]

Options:
      --json                 Print the result as JSON instead of text (commands that report a result)
      --minutes <MINUTES>    Minutes to search before publishing [default: 10]
      --tilings <TILINGS>    Only these tilings (ids such as hex or k2-017, separated by commas or spaces; default: every one)
      --families <FAMILIES>  Only these rule families (default: every one)
      --no-gpu               Run on the CPU only
      --threads <THREADS>    Worker threads (default: every core but two, a third of them for the GPU's batches)
      --detach               Run in a process of its own and return at once
      --library <LIBRARY>    Catalog: a .sqlite file, or a folder holding library.sqlite
      --quiet                No progress on standard error
  -h, --help                 Print help
```

## Packs

Publish the library as a signed pack, export a version for a single download, and check, summarise or import one. See [Packs: importing, exporting, checking](../guides/packs.md), [Pack format](pack-format.md).

### `jetcarp pack`

```text
Packs: publish the library as a signed, content-addressed store (Parquet and JSON that any tool reads), export a version for a single download, summarise, check or import one: `key`, `rater`, `publish`, `export`, `info`, `check`, `import`

Usage: jetcarp pack [OPTIONS] <COMMAND>

Commands:
  key      Make or show the publisher's Ed25519 key (kept in the settings folder), and who the publisher is (named only if given)
  rater    Record a rater's consent to permanent public release of their ratings (or withdraw it)
  publish  Publish the library (or a selection) as a new signed version of a feed in a store
  export   Lay one version out with readable names for a single download (from a store, or the library published first with the publish flags)
  info     A version's summary: title, publisher and signature, tables (rows, parts, bytes), searches (nothing is imported, and the data files are not read)
  check    Check a store or export: format, signatures, every hash and id (nothing is imported)
  import   Import a store or export into the library: every hash, id and signature checked, records merged by id, a sample verified again
  help     Print this message or the help of the given subcommand(s)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
  -h, --help               Print help
```

#### `jetcarp pack key`

```text
Make or show the publisher's Ed25519 key (kept in the settings folder), and who the publisher is (named only if given)

Usage: jetcarp pack key [OPTIONS]

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --name <NAME>        The publisher's name, as packs show it (and the default rater of `jobs review`); "" for none
      --orcid <ORCID>      The publisher's ORCID iD (0000-0002-1825-0097); "" for none
      --github <GITHUB>    The publisher's GitHub account name; "" for none
      --key <KEY>          Use this OpenSSH Ed25519 private key file instead
  -h, --help               Print help
```

#### `jetcarp pack rater`

```text
Record a rater's consent to permanent public release of their ratings (or withdraw it)

Usage: jetcarp pack rater [OPTIONS] <NAME>

Arguments:
  <NAME>  The rater's name as the library's ratings write it

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --withdraw           Withdraw consent: later packs leave their ratings out
      --show-name          Packs may show the name (else only a pseudonymous id)
      --orcid <ORCID>      [default: ""]
      --notes              Their notes may be published too
  -h, --help               Print help
```

#### `jetcarp pack publish`

```text
Publish the library (or a selection) as a new signed version of a feed in a store

Usage: jetcarp pack publish [OPTIONS] --to <TO>

Options:
      --json                           Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>              Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --to <TO>                        The store's folder
      --feed <FEED>                    The feed: letters, digits and - (jetcarp-starter)
      --title <TITLE>                  [default: ""]
      --description <DESCRIPTION>      [default: ""]
      --version-label <VERSION_LABEL>  Free text for people (2026.11) [default: ""]
      --licence <LICENCE>              An SPDX licence id (default CC0-1.0)
      --created <CREATED>              The version's time (RFC 3339 UTC, default now), for reproducible packs
      --id <IDS>                       Only these records (ids, prefixes or names; repeat)
      --query <QUERY>                  Only the patterns a library query finds, as JSON ('{"class":"spaceship"}')
      --search <SEARCHES>              Only these explore searches (uuids; repeat). Default: every search, or none with --id or --query
      --no-searches                    Leave out explore searches (their runs, scores and settings)
      --no-media                       Leave out media (posters, thumbnails, animations)
      --no-ratings                     Leave out ratings (otherwise only consented raters' go in)
      --no-ants                        Leave out ant and turmite runs
      --no-quickstart                  Leave out Quick Start entries
      --retract <RETRACT>              Withdraw a record: pattern:<id>=reason (repeat)
      --merge <MERGE>                  Merge another version: a store's folder, or <folder>@sha256:<manifest> (repeat)
      --views                          Write views.sql (DuckDB views over the store)
      --key <KEY>                      The signing key (an OpenSSH Ed25519 private key; default the settings folder's)
  -h, --help                           Print help
```

#### `jetcarp pack export`

```text
Lay one version out with readable names for a single download (from a store, or the library published first with the publish flags)

Usage: jetcarp pack export [OPTIONS] --to <TO>

Options:
      --json                           Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>              Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --to <TO>                        The export's folder (conventionally <feed>-<version label>)
      --from <FROM>                    The store to export a version of (default: publish the library first)
      --manifest <MANIFEST>            The version (default: the newest of the store's feed)
      --feed <FEED>                    The feed: letters, digits and - (jetcarp-starter)
      --title <TITLE>                  [default: ""]
      --description <DESCRIPTION>      [default: ""]
      --version-label <VERSION_LABEL>  Free text for people (2026.11) [default: ""]
      --licence <LICENCE>              An SPDX licence id (default CC0-1.0)
      --created <CREATED>              The version's time (RFC 3339 UTC, default now), for reproducible packs
      --id <IDS>                       Only these records (ids, prefixes or names; repeat)
      --query <QUERY>                  Only the patterns a library query finds, as JSON ('{"class":"spaceship"}')
      --search <SEARCHES>              Only these explore searches (uuids; repeat). Default: every search, or none with --id or --query
      --no-searches                    Leave out explore searches (their runs, scores and settings)
      --no-media                       Leave out media (posters, thumbnails, animations)
      --no-ratings                     Leave out ratings (otherwise only consented raters' go in)
      --no-ants                        Leave out ant and turmite runs
      --no-quickstart                  Leave out Quick Start entries
      --retract <RETRACT>              Withdraw a record: pattern:<id>=reason (repeat)
      --merge <MERGE>                  Merge another version: a store's folder, or <folder>@sha256:<manifest> (repeat)
      --views                          Write views.sql (DuckDB views over the store)
      --key <KEY>                      The signing key (an OpenSSH Ed25519 private key; default the settings folder's)
  -h, --help                           Print help
```

#### `jetcarp pack info`

```text
A version's summary: title, publisher and signature, tables (rows, parts, bytes), searches (nothing is imported, and the data files are not read)

Usage: jetcarp pack info [OPTIONS] <PATH>

Arguments:
  <PATH>  A store's or export's folder

Options:
      --feed <FEED>          The feed, when the store has several
      --json                 Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>    Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --manifest <MANIFEST>  A version other than the feed's newest (sha256:...)
  -h, --help                 Print help
```

#### `jetcarp pack check`

```text
Check a store or export: format, signatures, every hash and id (nothing is imported)

Usage: jetcarp pack check [OPTIONS] <PATH>

Arguments:
  <PATH>  A store's or export's folder

Options:
      --feed <FEED>          The feed, when the store has several
      --json                 Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>    Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --manifest <MANIFEST>  A version other than the feed's newest (sha256:...)
      --allow-unsigned       Accept a version its publisher did not sign
  -h, --help                 Print help
```

#### `jetcarp pack import`

```text
Import a store or export into the library: every hash, id and signature checked, records merged by id, a sample verified again

Usage: jetcarp pack import [OPTIONS] <PATH>

Arguments:
  <PATH>  A store's or export's folder

Options:
      --feed <FEED>
          The feed, when the store has several
      --json
          Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>
          Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite)
      --manifest <MANIFEST>
          A version other than the feed's newest (sha256:...)
      --allow-unsigned
          Accept a version its publisher did not sign
      --strict
          Refuse two different rows for one record id
      --verify <VERIFY>
          sample, full or none [default: sample]
      --sample <SAMPLE>
          How many new patterns the sample verifies again [default: 3000]
      --seed <SEED>
          The sample's seed (default: from the manifest hash, so the same pack samples the same patterns)
      --verify-seconds <VERIFY_SECONDS>
          Most seconds verification may take [default: 600]
  -h, --help
          Print help
```

## Pictures, media and the website

Contact sheets, animations and posters of library records, desktop wallpapers, and the website export. See [Pictures and animations](../guides/pictures.md), [Website export](../guides/website-export.md), [Wallpapers, screensavers and the background host](../guides/wallpapers.md).

### `jetcarp sheet`

```text
Contact sheet: one thumbnail per elementary rule, labelled with its number

Usage: jetcarp sheet [OPTIONS] --png <PNG>

Options:
      --json                       Print the result as JSON instead of text (commands that report a result)
      --kind <KIND>                w (squares, 0-255), hexw (binary hexagon, 0-15), hexw3 (ternary hexagon, 0-19682), hext (flat-top hexagons, three parents, 0-255), hext3 (0 to 3^27 - 1), or hext3sum (totalistic ternary three-parent rules, 0-2186) [default: w]
      --from <FROM>                [default: 0]
      --count <COUNT>              [default: 64]
      --cols <COLS>                [default: 8]
      --width <WIDTH>              Cells across each thumbnail [default: 81]
      --thumb <THUMB>              [default: 180]
      --random-row <RANDOM_ROW>    Start from a random top row with this density instead of one cell
      --palette <PALETTE>          A palette (`jetcarp palettes` lists them; names are case-sensitive) or hex colours like `#103050-#4fc3ff` [default: Ember]
      --png <PNG>                  
      --folder <FOLDER>            A sweep folder: lay out its pictures instead of running rules (with --runs)
      --runs <RUNS>                With --folder: the runs, comma separated: rule text ("HexT3 sum 588"), manifest index ("#12" or "12"), or series number ("n=588")
      --labels <LABELS>            With --folder: a label per run, comma separated (default: "#index"); the rule is printed under it
      --label-scale <LABEL_SCALE>  Largest label scale (the 3x5 font at this many pixels per dot) [default: 2]
  -h, --help                       Print help
```

### `jetcarp media`

```text
Media for the catalog: per-step frames, a GIF or animated WebP, a poster, and a thumbnail for a pattern (alone), a Quick Start entry (from the seed), an ant run, or a scene; `--missing` renders everything missing or stale

Usage: jetcarp media [OPTIONS] <COMMAND>

Commands:
  render  Render frames, an animation, a poster, and a thumbnail (skipped when up to date)
  list    List a target's stored media files, with their paths (content-addressed: media/ab/<sha256>.<ext>)
  export  Copy a target's stored media into a folder under readable names (frames/0000.png, anim.gif or anim.webp, poster.png, thumb.png)
  gc      Delete the media files no record names any more, and leftover temporary files
  help    Print this message or the help of the given subcommand(s)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite). Media files go under its folder, in media/ab/<sha256>.<ext>
  -h, --help               Print help
```

#### `jetcarp media render`

```text
Render frames, an animation, a poster, and a thumbnail (skipped when up to date)

Usage: jetcarp media render [OPTIONS]

Options:
      --library <LIBRARY>        Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite). Media files go under its folder, in media/ab/<sha256>.<ext>
      --pattern <PATTERN>        A catalog pattern (id, unique prefix, or name)
      --quickstart <QUICKSTART>  A Quick Start entry: a built-in one by id or number (`jetcarp quickstart` lists them), written like --scene, or one of the catalog's (`find:...`, `jetcarp library quickstart`)
      --ant <ANT>                An ant run by id (`jetcarp library ants list`): a highway's moving loop alone, the build-up from the seed
      --scene <SCENE>            Any scene file (from the seed); written to --out, not the library
      --missing                  Every pattern and shown Quick Start entry whose media are missing or stale
      --class <CLASS>            With --missing: only these classes (comma-separated: spaceship,oscillator,gun,...)
      --only-patterns            With --missing: patterns only, or Quick Start entries only
      --only-quickstart          
      --limit <LIMIT>            With --missing: at most this many targets [default: 0]
      --mode <MODE>              auto (patterns alone, everything else from the seed), alone, or seed [default: auto]
      --format <FORMAT>          auto (GIF up to 320 x 320 pixels and 256 colours, else WebP), gif, or webp [default: auto]
      --periods <PERIODS>        Alone: full periods to show [default: 1]
      --gens <GENS>              From the seed: generations (default 200; row rules one per row)
      --every <EVERY>            From the seed: a frame every this many generations [default: 1]
      --no-frames                Skip the per-step PNG frames
      --force                    Render even when up to date
      --palette <PALETTE>        Colour the media with this palette (its background, stage, and stops) instead of the house style: a name (`jetcarp palettes` lists them) or hex colours like `#103050-#4fc3ff`. The palette is part of the render key, so media stored in another palette render again
      --out <OUT>                --scene or a built-in --quickstart entry: the folder for its files (default `<output folder>/media/<scene file's name, or the entry's id>`)
      --threads <THREADS>        Worker threads (default: all cores)
      --json                     Print the result as JSON
  -h, --help                     Print help
```

#### `jetcarp media list`

```text
List a target's stored media files, with their paths (content-addressed: media/ab/<sha256>.<ext>)

Usage: jetcarp media list [OPTIONS] <KIND> <ID>

Arguments:
  <KIND>  `pattern`, `quickstart`, or `ant`
  <ID>    

Options:
      --json               
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite). Media files go under its folder, in media/ab/<sha256>.<ext>
  -h, --help               Print help
```

#### `jetcarp media export`

```text
Copy a target's stored media into a folder under readable names (frames/0000.png, anim.gif or anim.webp, poster.png, thumb.png)

Usage: jetcarp media export [OPTIONS] --to <TO> <KIND> <ID>

Arguments:
  <KIND>  `pattern`, `quickstart`, or `ant`
  <ID>    

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite). Media files go under its folder, in media/ab/<sha256>.<ext>
      --to <TO>            The folder to copy them into
      --mode <MODE>        alone or seed (default: alone if it has media alone, else seed)
  -h, --help               Print help
```

#### `jetcarp media gc`

```text
Delete the media files no record names any more, and leftover temporary files

Usage: jetcarp media gc [OPTIONS]

Options:
      --dry-run            Only list what would be deleted
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else <data>/library.sqlite). Media files go under its folder, in media/ab/<sha256>.<ext>
      --min-age <MIN_AGE>  Leave files younger than this many seconds (a render in another process may not have recorded them yet) [default: 600]
  -h, --help               Print help
```

### `jetcarp site`

```text
Website export: the public catalog as JSON and media with a manifest and a viewer page, incrementally

Usage: jetcarp site [OPTIONS] <COMMAND>

Commands:
  export  Write the public part of the catalog (JSON and media, with manifest.json, and a viewer page, index.html) to a folder; only new or changed files are written, and items no longer public are deleted
  help    Print this message or the help of the given subcommand(s)

Options:
      --json               Print the result as JSON instead of text (commands that report a result)
      --library <LIBRARY>  Catalog (as `jetcarp library --library`)
  -h, --help               Print help
```

#### `jetcarp site export`

```text
Write the public part of the catalog (JSON and media, with manifest.json, and a viewer page, index.html) to a folder; only new or changed files are written, and items no longer public are deleted

Usage: jetcarp site export [OPTIONS] --to <TO>

Options:
      --library <LIBRARY>
          Catalog (as `jetcarp library --library`)
      --to <TO>
          The export folder
      --state <STATE>
          Curation states exported (comma-separated; default promoted,reviewed; never rejected) [default: promoted,reviewed]
      --min-score <MIN_SCORE>
          Only patterns with at least this interest score
      --min-rating <MIN_RATING>
          Only patterns rated at least this (1-5)
      --min-osc-period <MIN_OSC_PERIOD>
          Leave out oscillators with a shorter period than this
      --class <CLASS>
          Only these classes (comma-separated)
      --quickstart <QUICKSTART>
          Quick Start sources exported, their entries that are not hidden (comma-separated: promotion, pack, atlas, builtin; default promotion,pack; "none" for none) [default: promotion,pack]
      --ants <ANTS>
          Ant and turmite curation states exported (comma-separated; default promoted,reviewed; "none" for none) [default: promoted,reviewed]
      --ant-outcome <ANT_OUTCOME>
          Only ants with these outcomes (comma-separated: highway, symmetric, ...; default all)
      --frames
          Copy the per-step PNG frames too
      --no-render
          Do not render missing media first
      --palette <PALETTE>
          Render missing media in this palette (a name or hex colours) instead of the house style
      --no-page
          Do not write the viewer page (index.html, a grid of every item over manifest.json)
      --json
          
  -h, --help
          Print help
```

### `jetcarp wallpaper`

```text
Desktop wallpapers: a scene or Quick Start entry at a monitor's size, a span of monitors, or one image per monitor cut so they line up

Usage: jetcarp wallpaper [OPTIONS] <SOURCE>

Arguments:
  <SOURCE>  A scene file, or a Quick Start id (`jetcarp quickstart` lists the built-in ones; `find:...` for "My finds")

Options:
      --json                 Print the result as JSON instead of text (commands that report a result)
      --size <SIZE>          WIDTHxHEIGHT or a preset (HD, QHD, 4K, 5K, 8K, UWQHD, WUXGA, WQXGA, Phone). Ignored with --monitors (the span's size is used) [default: 3840x2160]
      --fit <FIT>            cover (fill and crop, no stage showing), contain (whole grid, letterboxed), or repeat (a wrap-edge grid tiled at --cell-px) [default: cover]
      --center <CENTER>      Look at this world point (`X,Y`) at --scale pixels per world unit instead of fitting
      --scale <SCALE>        Pixels per world unit, with --center
      --fresh                Build a fresh grid of the scene's tiling sized to fill the image at --cell-px (the scene's CAs and seeds, without its edits). An aperiodic tiling gets the substitution level nearest that
      --cell-px <CELL_PX>    Tile size in pixels for --fresh and --fit repeat [default: 4]
      --gens <GENS>          Generations to run before rendering [default: 0]
      --gpu                  Use the GPU where the rules allow it, whatever the GPU setting says (results are identical). Default: as the GPU setting says (automatic: the GPU once the work is large enough to gain)
      --cpu                  Use only the CPU, whatever the GPU setting says
      --palette <PALETTE>    Colour every CA with this palette (`jetcarp palettes` lists them), or with hex colours like `#4fc3ff` or `#103050-#4fc3ff`
      --monitors <MONITORS>  Monitors as WIDTHxHEIGHT+X+Y, comma separated (xrandr's layout), for a span image
      --split                With --monitors: write one image per monitor instead of the whole span
      --outline <OUTLINE>    Draw tile outlines when tiles are at least this many pixels across (0 = never) [default: 0]
  -o, --out <OUT>            Output file (.png) or folder. Default: <Pictures>/JETCARP/wallpapers/
  -h, --help                 Print help
```

## Scripts

Run a Rhai script, or one of the examples built in. See [Scripting (Rhai) API](scripting.md), [Scripting a rule sweep](../tutorials/scripting-a-sweep.md).

### `jetcarp script`

```text
Run a Rhai script: worlds, runs, images, parallel rule sweeps (see the book's "Scripting (Rhai) API" reference). `--examples` lists the example scripts built in, `--example NAME` runs one

Usage: jetcarp script [OPTIONS] [FILE]

Arguments:
  [FILE]  The script (.rhai). Omit with --functions, --examples or --example

Options:
      --example <NAME>             Run a built-in example script instead of a file (`hexw-all`, `wolfram-all`, ...; the `.rhai` may be left off)
      --examples                   List the built-in example scripts and exit
      --set <KEY=VALUE>            Script parameter, seen as `params.key` (repeatable): --set from=0 --set label=test
      --screen <SCREEN>            Screen size that `size: "screen"` fills and images default to [default: 1920x1080]
      --cell-px <CELL_PX>          Tile size in pixels for screen-sized worlds (the app's Fill the view tile size) [default: 8]
      --threads <THREADS>          Worker threads (default: all cores). Results do not depend on it
      --out <OUT>                  Output root instead of $JETCARP_OUT or <Pictures>/JETCARP
      --check                      Only check the script for syntax errors
      --max-seconds <MAX_SECONDS>  Stop the script after this many seconds
      --json                       Print the summary as JSON on stdout at the end
      --functions                  Print the function reference and exit
  -h, --help                       Print help

Example scripts are built in, as in the app's Examples menu: --examples lists them and --example NAME runs one (with --check, checks it; it writes into the output folder like any script).
```

## The app, the background host and AI assistants

Control the running app and the background host, and serve the MCP tools to an AI assistant. See [Wallpapers, screensavers and the background host](../guides/wallpapers.md), [Driving JETCARP from an AI assistant](../tutorials/mcp.md), [MCP tools](mcp.md).

### `jetcarp app`

```text
Control the running JETCARP app (AI control on, or `jetcarp --control`): `status`, `screensaver on|off [--profile NAME]`, `profiles`, `display ...`, `play`, `pause`, `speed N`, `next`, `previous`, `call <tool> <json>`

Usage: jetcarp app [OPTIONS] <COMMAND>

Commands:
  status       Where the grid shows, the monitors, the screensaver, keep-awake
  screensaver  Screensaver mode on or off (with --profile: show that profile)
  profiles     The screensaver profiles, with the one showing marked
  display      Displays: mode, full screen, outputs, span or mirror, cell size, chrome, keep-awake
  play         Run generations
  pause        Pause
  speed        Generations per second (0: as fast as possible)
  next         The next item of the screensaver's rotation
  previous     The previous item of the screensaver's rotation
  call         Call any MCP tool on the app's world: `jetcarp app call world_status` or `jetcarp app call world_step '{"generations": 10}'`
  help         Print this message or the help of the given subcommand(s)

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app status`

```text
Where the grid shows, the monitors, the screensaver, keep-awake

Usage: jetcarp app status [OPTIONS]

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app screensaver`

```text
Screensaver mode on or off (with --profile: show that profile)

Usage: jetcarp app screensaver [OPTIONS] <STATE>

Arguments:
  <STATE>  [possible values: on, off]

Options:
      --profile <PROFILE>  Show this screensaver profile (`jetcarp app profiles` lists them)
      --port <PORT>        App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>      App token (default: the recorded one)
      --json               Print the app's reply as JSON
  -h, --help               Print help
```

#### `jetcarp app profiles`

```text
The screensaver profiles, with the one showing marked

Usage: jetcarp app profiles [OPTIONS]

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app display`

```text
Displays: mode, full screen, outputs, span or mirror, cell size, chrome, keep-awake

Usage: jetcarp app display [OPTIONS]

Options:
      --mode <MODE>
          window, fullscreen, or outputs

      --fullscreen <FULLSCREEN>
          The main window full screen (on) or windowed (off)
          
          [possible values: on, off]

      --fullscreen-monitor <FULLSCREEN_MONITOR>
          Monitor for full screen (index from `jetcarp app status`)

      --monitors <MONITORS>
          Output monitors by index, comma separated (sets mode outputs)

      --windows <WINDOWS>
          Outputs as plain windows: WxH@X,Y separated by ';' (trying a span on one monitor)

      --span
          One canvas across the outputs

      --mirror
          The same view on each output

      --units <UNITS>
          Canvas units: pixels or logical

      --hide-preview <HIDE_PREVIEW>
          Draw no grid in the main window while outputs are on
          
          [possible values: on, off]

      --cell-px <CELL_PX>
          Cell size in physical pixels (1 to 64)

      --port <PORT>
          App port (default: the one the app recorded when AI control was switched on)

      --refill
          Rebuild the tiling to fill the canvas at the cell size

      --pixel-zoom
          Zoom so a cell edge is the cell size in pixels

      --token <TOKEN>
          App token (default: the recorded one)

      --json
          Print the app's reply as JSON

      --keep-awake <KEEP_AWAKE>
          [possible values: on, off]

      --menu-autohide <MENU_AUTOHIDE>
          [possible values: on, off]

      --panes-hidden <PANES_HIDDEN>
          Hide (on) or show (off) every pane
          
          [possible values: on, off]

      --status-bar <STATUS_BAR>
          [possible values: on, off]

      --gpu <GPU>
          The app's own GPU choice: follow (JETCARP's shared setting, automatic unless changed), on (every world the rules allow) or off (saved)

          Possible values:
          - follow: JETCARP's shared GPU setting
          - on
          - off

      --identify
          Show each monitor's index on it for three seconds

      --saver <SAVER>
          Screensaver settings as JSON (see the app_display tool's `saver`), e.g. '{"source": "rotation", "rotation": "Wolfram series"}'

  -h, --help
          Print help (see a summary with '-h')
```

#### `jetcarp app play`

```text
Run generations

Usage: jetcarp app play [OPTIONS]

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app pause`

```text
Pause

Usage: jetcarp app pause [OPTIONS]

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app speed`

```text
Generations per second (0: as fast as possible)

Usage: jetcarp app speed [OPTIONS] <GENS_PER_SEC>

Arguments:
  <GENS_PER_SEC>  

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app next`

```text
The next item of the screensaver's rotation

Usage: jetcarp app next [OPTIONS]

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app previous`

```text
The previous item of the screensaver's rotation

Usage: jetcarp app previous [OPTIONS]

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

#### `jetcarp app call`

```text
Call any MCP tool on the app's world: `jetcarp app call world_status` or `jetcarp app call world_step '{"generations": 10}'`

Usage: jetcarp app call [OPTIONS] <TOOL> [ARGS]

Arguments:
  <TOOL>  
  [ARGS]  Arguments as a JSON object (default `{}`)

Options:
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
      --json           Print the app's reply as JSON
  -h, --help           Print help
```

### `jetcarp host`

```text
The background host, which keeps scenes running with no app window: `start`, `stop`, `status`, `next`, `previous`, `pause`, `resume`, `rotation`, `caption`, `fps`, `login`

Usage: jetcarp host [OPTIONS] <COMMAND>

Commands:
  serve      Run the host in this process (what `start`, login and the GNOME menu run)
  start      Start the host (it keeps running after this command ends)
  stop       Quit the host
  status     What the host is showing
  next       The next item, on every screen or one
  previous   The previous item, on every screen or one
  pause      Freeze generations and the rotation
  resume     Carry on
  rotation   Choose the rotation for a screen (saved)
  caption    Show or hide what is running, and how (saved)
  fps        Frames per second, at most (saved)
  still      Leave a still of the scene as the desktop picture when the host stops, or while the live wallpaper can't be shown (saved; off by default). Turning it or the wallpaper off puts your own picture back
  battery    Pause while the computer runs on battery (saved; on by default)
  busy       Pause while the whole processor stays at least this busy, in percent, as during research runs (saved; default 80, 0 never)
  login      Start the host at login, or stop doing that
  wallpaper  Show screens as the desktop background (saved). On GNOME this needs the extension
  lock       What a wallpaper does on the GNOME lock screen (saved)
  gnome      The GNOME Shell extension behind wallpapers and the top-bar menu
  saver      The screen saver (Windows and macOS)
  tune       Fit the desktop background to this computer: run each scene of the wallpaper's rotation at its screen's size and keep, for each, the most cells that stay within the processor and GPU limits at a smooth frame rate (saved). The rules and colours stay as they are. A running host pauses while it runs, then shows the sizes found
  scene      The scene a screen shows now, its grid's size included, as a scene file that replays it
  help       Print this message or the help of the given subcommand(s)

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host serve`

```text
Run the host in this process (what `start`, login and the GNOME menu run)

Usage: jetcarp host serve [OPTIONS]

Options:
      --headless             Run with no window or tray (scripts, and automated tests)
      --canvas <CANVAS>      Canvas size with --headless, as WxH [default: 1280x720]
      --rotation <ROTATION>  Run this rotation on the first screen, for this run only
      --seconds <SECONDS>    Quit after this many seconds
      --saver                Run as a screen saver: full screen on every monitor until there is input
      --login                Started at login (what the login entry runs): show only the screens placed on the desktop, and open no windows. With none, the host waits in the tray or menu bar
      --json                 Print the host's reply as JSON
  -h, --help                 Print help
```

#### `jetcarp host start`

```text
Start the host (it keeps running after this command ends)

Usage: jetcarp host start [OPTIONS]

Options:
      --rotation <ROTATION>  Run this rotation, for this run only
      --headless             No window or tray
      --json                 Print the host's reply as JSON
  -h, --help                 Print help
```

#### `jetcarp host stop`

```text
Quit the host

Usage: jetcarp host stop [OPTIONS]

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host status`

```text
What the host is showing

Usage: jetcarp host status [OPTIONS]

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host next`

```text
The next item, on every screen or one

Usage: jetcarp host next [OPTIONS]

Options:
      --screen <SCREEN>  
      --json             Print the host's reply as JSON
  -h, --help             Print help
```

#### `jetcarp host previous`

```text
The previous item, on every screen or one

Usage: jetcarp host previous [OPTIONS]

Options:
      --screen <SCREEN>  
      --json             Print the host's reply as JSON
  -h, --help             Print help
```

#### `jetcarp host pause`

```text
Freeze generations and the rotation

Usage: jetcarp host pause [OPTIONS]

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host resume`

```text
Carry on

Usage: jetcarp host resume [OPTIONS]

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host rotation`

```text
Choose the rotation for a screen (saved)

Usage: jetcarp host rotation [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --screen <SCREEN>  
      --json             Print the host's reply as JSON
  -h, --help             Print help
```

#### `jetcarp host caption`

```text
Show or hide what is running, and how (saved)

Usage: jetcarp host caption [OPTIONS] <STATE>

Arguments:
  <STATE>  [possible values: on, off]

Options:
      --corner <CORNER>          [possible values: bottom-left, bottom-right, top-left, top-right]
      --size <SIZE>              Height of the first line, in points
      --generation <GENERATION>  Include the generation count [possible values: on, off]
      --seconds <SECONDS>        Seconds to stay up after each new item (0: always)
      --json                     Print the host's reply as JSON
  -h, --help                     Print help
```

#### `jetcarp host fps`

```text
Frames per second, at most (saved)

Usage: jetcarp host fps [OPTIONS] <FPS>

Arguments:
  <FPS>  

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host still`

```text
Leave a still of the scene as the desktop picture when the host stops, or while the live wallpaper can't be shown (saved; off by default). Turning it or the wallpaper off puts your own picture back

Usage: jetcarp host still [OPTIONS] <STATE>

Arguments:
  <STATE>  [possible values: on, off]

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host battery`

```text
Pause while the computer runs on battery (saved; on by default)

Usage: jetcarp host battery [OPTIONS] <STATE>

Arguments:
  <STATE>  [possible values: on, off]

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host busy`

```text
Pause while the whole processor stays at least this busy, in percent, as during research runs (saved; default 80, 0 never)

Usage: jetcarp host busy [OPTIONS] <PERCENT>

Arguments:
  <PERCENT>  

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host login`

```text
Start the host at login, or stop doing that

Usage: jetcarp host login [OPTIONS] <STATE>

Arguments:
  <STATE>  [possible values: on, off]

Options:
      --json  Print the host's reply as JSON
  -h, --help  Print help
```

#### `jetcarp host wallpaper`

```text
Show screens as the desktop background (saved). On GNOME this needs the extension

Usage: jetcarp host wallpaper [OPTIONS] <STATE>

Arguments:
  <STATE>  [possible values: on, off]

Options:
      --screen <SCREEN>      Only this screen. A screen one past the last is added
      --monitor <MONITOR>    Put that screen on this monitor (see `jetcarp host status`)
      --rotation <ROTATION>  The rotation for that screen
      --json                 Print the host's reply as JSON
  -h, --help                 Print help
```

#### `jetcarp host lock`

```text
What a wallpaper does on the GNOME lock screen (saved)

Usage: jetcarp host lock [OPTIONS] <MODE>

Arguments:
  <MODE>  [possible values: off, live, screensaver]

Options:
      --awake <AWAKE>  In screensaver mode, let the screen sleep after this many minutes (0: never)
      --json           Print the host's reply as JSON
  -h, --help           Print help
```

#### `jetcarp host gnome`

```text
The GNOME Shell extension behind wallpapers and the top-bar menu

Usage: jetcarp host gnome [OPTIONS] <ACTION>

Arguments:
  <ACTION>
          Possible values:
          - install: Copy the extension into place and switch it on (takes effect at the next login)
          - remove:  Switch the extension off and delete it
          - status:  Say whether the extension is running

Options:
      --json
          Print the host's reply as JSON

  -h, --help
          Print help (see a summary with '-h')
```

#### `jetcarp host saver`

```text
The screen saver (Windows and macOS)

Usage: jetcarp host saver [OPTIONS] <ACTION>

Arguments:
  <ACTION>
          Possible values:
          - install: Windows: copy JETCARP to a `.scr` file and open Screen Saver Settings with it selected. macOS: build `JETCARP.saver` with the rotation inside it and open System Settings
          - remove:  Delete it (Windows: and stop Windows from running it; close Screen Saver Settings first)
          - status:  Say whether it is installed (Windows: and selected)

Options:
      --rotation <ROTATION>
          The rotation it runs (saved). Default: the first screen's

      --json
          Print the host's reply as JSON

  -h, --help
          Print help (see a summary with '-h')
```

#### `jetcarp host tune`

```text
Fit the desktop background to this computer: run each scene of the wallpaper's rotation at its screen's size and keep, for each, the most cells that stay within the processor and GPU limits at a smooth frame rate (saved). The rules and colours stay as they are. A running host pauses while it runs, then shows the sizes found

Usage: jetcarp host tune [OPTIONS]

Options:
      --cpu-limit <PERCENT>            JETCARP's processor time, in percent of the whole machine, every core counted (saved; default 10)
      --gpu-limit <PERCENT>            JETCARP's share of the GPU's time, in percent (saved; default 25)
      --fps <FPS>                      Frames per second it must keep (saved; default: the monitor's refresh rate, or 60, at most the host's frame rate)
      --screen <SCREEN>                Only this screen (counted from 0). Default: every screen on the desktop
      --pick <WHICH>                   Which size each scene is left at: recommended (the finest within the limits; the default), finest (the finest shown), own (the scene's own), or a cell size in pixels. Without it, a size chosen before is kept while it is still within the limits
      --crops <DIR>                    Save each size's picture here as a PNG (they are kept beside the host's settings too)
      --canvas <CANVAS>                The screen's size in pixels, as WxH, when no host is running to say (default: the size it was tuned for before, else its monitor's)
      --trial-seconds <TRIAL_SECONDS>  Seconds each size shows for (the last three quarters are measured) [default: 2]
      --dry-run                        Measure and report, but save nothing
      --stop                           Stop the sweep a running host is making (the scenes already done stay saved)
      --json                           Print the host's reply as JSON
  -h, --help                           Print help
```

#### `jetcarp host scene`

```text
The scene a screen shows now, its grid's size included, as a scene file that replays it

Usage: jetcarp host scene [OPTIONS]

Options:
      --screen <SCREEN>  
      --save <SAVE>      Write it to this file (default: print it)
      --json             Print the host's reply as JSON
  -h, --help             Print help
```

### `jetcarp mcp`

```text
MCP server on stdin/stdout so AI assistants (Claude Code, Claude Desktop, LM Studio) can drive JETCARP. Its tools read and write files only in JETCARP's output and library folders (and folders given with --allow)

Usage: jetcarp mcp [OPTIONS]

Options:
      --app            Act on the world in the running JETCARP app (switch on AI control there first)
      --json           Print the result as JSON instead of text (commands that report a result)
      --allow <DIR>    Let the tools read and write in this folder too (repeatable)
      --port <PORT>    App port (default: the one the app recorded when AI control was switched on)
      --token <TOKEN>  App token (default: the recorded one)
  -h, --help           Print help
```

## Lists

What there is to choose from. See [Named rule catalogue](../automata/named-rules.md), [Rule notation](rule-notation.md), [Tiling catalogue](../tilings/catalogue.md), [Palettes](palettes.md).

### `jetcarp rules`

```text
List named rules and notation

Usage: jetcarp rules [OPTIONS]

Options:
      --json  Print the result as JSON instead of text (commands that report a result)
  -h, --help  Print help
```

### `jetcarp tilings`

```text
List tiling families with their neighbour counts

Usage: jetcarp tilings [OPTIONS]

Options:
      --json  Print the result as JSON instead of text (commands that report a result)
  -h, --help  Print help
```

### `jetcarp isohedral`

```text
List Kaplan's isohedral types (IH01-IH93), or draw them all on a contact sheet

Usage: jetcarp isohedral [OPTIONS]

Options:
      --json           Print the result as JSON instead of text (commands that report a result)
      --png <PNG>      Write a contact sheet of all 81 types here instead of listing them
      --straight       Keep edges straight (default: random edge shapes within each type's constraints)
      --cols <COLS>    [default: 9]
      --thumb <THUMB>  [default: 200]
      --seed <SEED>    [default: 7]
  -h, --help           Print help
```

### `jetcarp palettes`

```text
List the palettes with their background colours

Usage: jetcarp palettes [OPTIONS]

Options:
      --json  Print the result as JSON instead of text (commands that report a result)
  -h, --help  Print help
```

## Setup and upkeep

Set JETCARP up for what you want it for, where it keeps its files, what it sees on this machine, and how to take it off again. See [First run and setup](../getting-started/first-run.md), [Folders and environment variables](folders.md), [Uninstalling](../guides/uninstalling.md).

### `jetcarp setup`

```text
First-run setup: choose a profile (desktop-art, explorer, researcher, everything, blank-slate), and JETCARP imports its packs and builds Quick Start entries of its own, as jobs. Asks in a terminal; `--profile NAME --yes` asks nothing. `--status` shows where things stand

Usage: jetcarp setup [OPTIONS]

Options:
      --json                    Print the result as JSON instead of text (commands that report a result)
      --profile <PROFILE>       The profile: desktop-art, explorer, researcher, everything, or blank-slate. Asked for in a terminal when left out
      --data-dir <DIR>          Keep JETCARP's data (the library, its media and search folders) in this folder from now on, for big libraries; recorded in paths.json in the settings folder. It must be empty, or hold a JETCARP data folder already: nothing is moved
      --quickstart-minutes <N>  Minutes of the Quick Start build: a search whose best runs are added to the Quick Start (default: the profile's, 10, 30 or 60; 0: none)
      --packs <DIR>             Import the packs in this folder too: a pack store, an export, or a folder holding them (repeatable)
  -y, --yes                     Ask nothing: do what the profile does (the desktop offers are only printed)
      --status                  Only show where things stand: whether setup has run, the folders, the packs that came with JETCARP, the GPU, and what each profile takes here
      --no-gpu                  The Quick Start build runs on the CPU even when there is a GPU
      --threads <THREADS>       Worker threads (default: all cores, and all but two for the Quick Start build)
      --quiet                   No progress on standard error
  -h, --help                    Print help
```

### `jetcarp paths`

```text
Print the settings, data, and output folders JETCARP uses (environment overrides included)

Usage: jetcarp paths [OPTIONS]

Options:
      --json  Print the result as JSON instead of text (commands that report a result)
  -h, --help  Print help
```

### `jetcarp doctor`

```text
What JETCARP sees on this machine: version, folders, graphics adapters, desktop integration. Include it in bug reports

Usage: jetcarp doctor [OPTIONS]

Options:
      --json  Print the result as JSON instead of text (commands that report a result)
  -h, --help  Print help
```

### `jetcarp uninstall`

```text
Undo what JETCARP set up outside its folders (the host and its still on the desktop, login entry, GNOME extension, screen saver), and optionally delete its settings and data. Shows the plan unless --yes

Usage: jetcarp uninstall [OPTIONS]

Options:
      --json      Print the result as JSON instead of text (commands that report a result)
      --yes       Do it. Without this, only show what would be done
      --settings  Also delete JETCARP's settings folder (except a picture the desktop still shows)
      --data      Also delete JETCARP's data folder (the pattern library, search databases, packs). This cannot be undone
  -h, --help      Print help
```

<!-- END GENERATED: docs/tools/gen-cli.py -->
