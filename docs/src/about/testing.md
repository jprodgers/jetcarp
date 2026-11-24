# How JETCARP is tested

This page sets out how JETCARP is tested before a release: the automated tests, the
continuous-integration runs, the release candidates tested by hand on real hardware, how each
problem found is handled, and who does what. It ends with a record of one release cycle and with
what this testing does not cover. [How JETCARP was made](how-made.md) says how the program was
written, and [Reproducibility](../research/reproducibility.md) describes the tests that pin its
behaviour in detail.

## Automated tests

JETCARP has over a thousand automated tests, beside the code (`#[cfg(test)]`) and in each crate's
`tests/` folder. `cargo test --workspace --release` runs them (see
[Contributing](contributing.md#building-and-testing)). They cover:

- **Pinned behaviour.** The cell polygons and neighbour lists of every tiling, the state of every
  built-in Quick Start entry after 48 generations, and one fixed scene for each rule family. A change
  that alters what an old scene replays to fails these tests
  ([Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics)).
- **The CPU and the GPU agree.** Wherever a world can run on the GPU, the tests require the same
  state hash, generation, status and command log as the CPU, over every family the GPU runs, every
  kind of edge, and runs from 1 to 180 generations; hunts with and without the GPU must leave the same
  catalogue. A GPU that cannot run something must fall back to the CPU, never run it differently.
  The explore search's batch kernel must give every run the processor would, cell for cell: for
  isotropic rules, 17 rules for each of eight square grids (wrapped, dead and live edges, down to
  8 by 8) and both neighbourhoods, random over all 102 letters and near Life, two-state, B0 and
  Generations, each run as written canonically and as read back from a shuffled notation (counts
  and letters in any order, a count's letters listed or left out with `-`), every cell of every
  generation up to 40 on random soups, and in batches beside B/S, JACA blasts and Larger than Life
  (`isotropic_rules_match_cell_for_cell`, `jetcarp-gpu/tests/batch.rs`). With one letter of every
  rule read wrong (birth on `4e`) it fails at the first generation, and the GPU self-test, whose
  explore batches run isotropic rules too, refuses the adapter.
- **Searches survive interruption.** Every kind of job but the exploratory search, stopped and
  resumed (on the same runner or another), must end with exactly the results of one that ran through;
  the exploratory search continues from its saved archive instead
  ([what is not guaranteed yet](../research/reproducibility.md#what-is-not-guaranteed-yet)).
- **Explore searches keep to what they were told, and older ones to what they did.** A search told
  which tilings, groups of tilings and rule families to search, or to leave out, must draw only
  from them, on the processor and beside a GPU, through the search itself, the operations, the
  command line, MCP, scripts and the app's form (whose checkboxes a test reaches with Tab and
  ticks with Space, reading their names from the screen-reader tree). Isotropic rules must come
  only when named. The tiling list, catalogue rules and first candidates of an older search, and
  of one that chooses nothing, are pinned as 0.4.2's code made them (`an_old_spec_replays_as_before`,
  `jetcarp-search/src/explore/job.rs`): with isotropic rules in the default schedule, an older
  search's tiling list cut to its families' grids, or one more random draw in a GPU round, it
  fails. With a GPU, a search that names isotropic rules must run every one of them, screened and
  confirming, in the GPU's batches, and the processor none: a stand-in GPU counts them
  (`isotropic_rules_run_on_the_gpu_when_there_is_one`), and on a real GPU an explore job's
  isotropic runs must all be the GPU's and match the processor's own loop run for run
  (`jetcarp-search/tests/gpu_explore.rs`); before they went to the GPU, 116 of 116 were the
  processor's. On the processor alone such a search must draw as it did when only the processor
  ran them, pinned as that code made them; with isotropic rules moved to the end of the
  processor's list it fails.
- **Exact verification.** Every object a hunt keeps is run again alone and compared cell by cell.
- **Golly agrees.** [Golly](https://golly.sourceforge.io/)'s command-line program, `bgolly`, reads
  the RLE JETCARP writes as the same cells, and writes it back as JETCARP reads it. It runs the
  pattern to the same generations as JETCARP, cell for cell. The test uses soups of nine rules on
  the square grid (Moore and von Neumann) and the hexagon grid, placed on rows of both parities,
  where JETCARP's offset rows and Golly's sheared grid differ: 62 generations up to 100
  (`golly_agrees_on_jetcarp_s_rle`, `jetcarp-api/tests/golly.rs`, ignored where Golly isn't
  installed). It fails if the hexagon layout is shifted the wrong way, at the first generation. A
  second test does the same for every find in a library that Golly can hold, over four of each
  one's periods (`golly_agrees_on_a_library_s_finds`, given a library with
  `JETCARP_GOLLY_LIBRARY`). On 21 November 2026, a library of fresh hunts on squares, hexagons and
  the von Neumann neighbourhood held 3,314 finds: 2,024 oscillators, 1,282 still lifes and 8
  spaceships. Every one pasted back as the same cells and ran to the same generation in Golly 5.0.
  A third test runs isotropic rules in Hensel's notation the same way, in both programs: 73 rules
  (66 of them random, two-state, B0 and Generations, written in shuffled forms, and two B/S rules
  with B0), two soups each, every generation up to 100, cell by cell and state by state, with Golly
  naming every rule as JETCARP does (`golly_agrees_on_isotropic_rules`). Its cases are chosen, and
  checked, so that any one letter read as another of its count would show at the first generation:
  with the arrangements of `3r` and `3y` swapped it fails in 120 of its 146 soups, with `4j` and
  `4r` swapped in 88. A fourth runs Larger than Life in both programs: ten rules on the square
  grid's box, diamond and cross, among them Bosco's Rule, one with four states, and five two-state
  rules with birth on 0 (two whose background flashes, three whose background is live for good from
  the first step), two soups each, every generation up to 40 (20 for Bosco's Rule), after Golly has
  read each two-state soup copied with *Copy as RLE* as the same rule and cells
  (`golly_agrees_on_larger_than_life`). With rules with birth on 0 run as written, as JETCARP ran
  them before 0.4.3, all ten of their soups differ from Golly at the first generation. It needs
  Golly 4.0 or later, the first to read Larger than Life's `R2,C2,S3-6,B3-4` notation and to
  emulate birth on 0. Ubuntu packages only Golly 3.3, so CI's Linux job builds `bgolly` 4.0 (the
  oldest Golly that reads everything JETCARP writes) and 5.0 (the newest) from their source, each
  download checked against its SHA-256, and runs every one of these tests with each.
- **The benchmark's answers.** A short run of the benchmark behind the
  [JETCARP Hardware Score](../guides/this-computer.md) (each workload once) must give its stored
  answers on the processor, and on the GPU where there is one (Mesa's software renderer in CI), and
  a device with a wrong answer must be left out of the score. CI checks the answers, never the
  speed: no test depends on how fast the machine is.
- **The load before measuring.** The GPU's load is read from a different place on each system, so
  each reader's arithmetic is tested on every system from fixtures (`jetcarp-core`'s
  `machine::gpu_load`): a list of Windows' GPU engine counters from a machine with an NVIDIA and an
  Intel GPU (each engine's processes summed, video and copy engines and the measuring program's
  own left out), two IOAccelerator `PerformanceStatistics` dictionaries as `ioreg` prints them (an
  Apple GPU's `Device Utilization %`, an older driver's `GPU Activity(%)`), `fdinfo` of amdgpu,
  i915 and xe clients a second apart (engine time in nanoseconds, and xe's cycles), and NVML
  stand-ins (three GPUs, one of which can't say). A live reading must give a figure between 0 and
  1 and say where from, or say why there is none; on Linux with an AMD GPU it must give one.
  Whether a busy GPU is noticed is checked by hand in the sittings on each system, since CI's
  runners have no GPU to keep busy.
- **The limits.** The per-machine limits are worked out for machines of 4 to 128 GB, with the
  person's own values, the two linked memory values and the ceilings; a world over the limit is
  refused before anything is built, naming the limit and where it came from, and never made
  smaller (#300): in the whole app with no window, 100,000 × 100,000 squares typed into the
  *Tiling* pane's *Width × height* fields, on a machine whose largest world is lowered to 1 GB, must
  keep the sides typed, get the over-limit question and build nothing, as must the same size from a
  tile's JSON; 20,000 × 1,000 squares must be built at that size, with the status line saying the
  view cannot draw every cell, until it is zoomed in. The command line, `world.new` and
  `set_tiling` must refuse 100,000 × 100,000 with the reason, an AI's sides, cells and level saying
  they are its share. The memory each kind
  of world takes is measured by counting every allocation while one is built and stepped, and the
  table the limits use must never be below it. A grid of exactly the largest number of cells the
  code can number (4,294,967,295) is built and its last cells' neighbours checked, without making
  any field.
- **Fit to this computer.** On made-up machines from one with room for a cell per pixel to one
  where nothing fits, the binary search must find what trying every size finds, in five sizes at
  most; a good first guess must settle it in two; a size measured either side of a limit from one
  fit to the next must keep the same recommendation; a rule series must be one scene; and the
  limits must be shared by pixel area. The other scenes must be held to twice the measured scene's
  cells, and say so, unless a size is chosen or picked, and a fit's first round must warm up half a
  second longer than the rest. The host must time a frame by its work, not its wait to present, and
  a fit by itself must sweep only the screens whose monitor changed. End to end, with no host and
  with a headless one: a fit of three scenes measures one in at most six sizes, a rotation mixing
  scenes on the GPU and on the processor has one measured on each, a series of 41 rules is one scene
  and the host shows its seventh rule at the size found, `--pick` measures nothing while the last
  fit is current, and two screens are fitted at once with their shares saved. The cards' row must
  open at the chosen card and scroll sideways with shift and the wheel and with a sideways swipe,
  the pane above it staying where it was. The MCP tool `host_control` must stay short, with its
  text in the help topic `host`.
- **The front ends.** The command line's JSON output, the operations the front ends share, the MCP
  server's protocol, its folder rules (paths outside its folders, symbolic links, read-only folders;
  on Windows directory junctions and symbolic links too) and its limits, that nothing listens for AI
  control while it is switched off, the scripting API, packs (signed, unsigned, tampered and truncated), and the app,
  which a test harness drives offscreen, clicking widgets by their text.
- **The keyboard and screen readers.** Tests drive the whole app, with no window and no GPU, by
  key presses, and read the accessibility tree it gives screen readers through AccessKit. Tab must
  reach every control of the main window, of each tool window, of the setup window and of the
  Quick Start, scroll each into view and draw the focus ring round it, in the standard and the
  high-contrast theme; F6 must visit the menu bar, each group of panes, the grid, the status line
  and each floating window, and Shift+F6 go back; F10, the arrows, Enter and Esc must work the
  menus and close dialogs and tool windows; every tool window and dialog, and a floating pane, must
  close with Esc, with Ctrl+W and through its *Close* button, the last control Tab reaches in it,
  every tool window opened from its menu item must close with Esc at once, and Esc in a docked
  pane must take the keyboard back to the grid; in the setup window's *What it searches*, at three
  window sizes, ↓ and ↑, Tab and Shift+Tab must go through every choice and back, an arrow must
  never take the keyboard out of the window, and Space must tick the choice with the keyboard; in
  the *Hunts* window's explore form, at the same sizes, ↓ and ↑ must go through every choice and
  back; *Tools → Start an explore search…*, chosen with F10, the arrows and Enter, must open the
  explore form with the keyboard on its first box; the grid's cell cursor must draw, erase, select
  and move a pattern. In every window, every control must have a role, a name that says something
  and a value that reads, and the status line must be a live region. Each runs in a process of its
  own with scratch folders (`crates/jetcarp-app/src/app/keyboard_tests.rs`); the cell cursor's
  next cell is checked on thirteen tilings (`cell_cursor.rs`), and an ignored test reads the tree
  again with the starter pack's cards, patterns and ant runs in the windows
  (`screen_reader_tree_with_the_starter_pack`). What a screen reader says aloud is checked by hand
  (see [What this testing does not cover](#what-this-testing-does-not-cover)). Every pane, menu and
  tool window must give screen readers the same roles and names in Atkinson Hyperlegible as in
  egui's original fonts, where a font's widths and line heights move controls and their labels
  about (`screen_reader_names_are_the_same_in_every_font`).
- **The fonts.** The app must start in Atkinson Hyperlegible Next and Mono, with Next Bold for
  headings, the files byte for byte those the fonts' README names by SHA-256, and egui's fonts
  behind them in egui's order; egui's original choice must give exactly the fonts the app had
  before. Every non-ASCII character in the app's and the display crate's strings, read from their
  source, must be drawn in each family wherever egui's own fonts drew it, and Greek and accented
  Latin must be drawn; the five macOS key symbols, which no built-in font has, are listed, and CJK
  is drawn by none of them. With a stand-in installed font that has the key symbols, an arrow and
  two CJK characters (a font file the test builds, as no machine's fonts can be counted on), every
  one of those characters must be drawn in each family, in Atkinson and in egui's original fonts:
  the key symbols and CJK from the installed font, the arrow still from egui's; and the installed
  fonts' character maps must be read on a thread of their own, starting it taking under 50 ms, and
  put in place when read. The index of installed fonts by character must give the upright regular
  face in the smallest file, and leave out LastResort, hidden fonts and files that aren't fonts
  (`crates/jetcarp-display/src/fonts.rs`); an ignored test times it on a machine's own fonts
  (`time_the_fallback_scan`: 0.03 to 0.09 ms to start the thread, 9 to 94 ms for the reading on the
  test machine's 299 font files). Installed fonts must be found by
  family in nested folders of a scratch font folder, the upright regular face of each, with a file
  that isn't a font, one without basic Latin and one of another type left out; a font chosen from
  them must be in place at once, saved in `app.json` and put back by the next start; and a file
  that isn't a font, one without basic Latin, and one that has gone must each be refused with a
  sentence that names the file and no folder, the setting going back to Atkinson
  (`crates/jetcarp-app/src/fonts.rs`, `app/accessibility.rs`). The caption on surfaces with no egui
  must draw in Atkinson Hyperlegible Next, with Ubuntu Light for the Greek it lacks, and follow
  the app's font setting in `app.json`: a font file chosen there from the next check without a
  restart and at the next start, egui's original, then Atkinson again; and with the chosen file
  removed, Atkinson, with a note that names the file and no folder
  (`jetcarp-display/src/caption.rs`). Installing the macOS screen saver, in a scratch folder on any
  system, must record the font chosen and copy its file whole into the bundle's Resources (a
  collection with its face), and copy nothing for Atkinson or egui's original; a chosen font that
  is gone or isn't a font must give Atkinson and a warning that names the file and no folder; a
  bundle of format 1, from before, must still load, in Atkinson, and a newer format be refused;
  and the saver's loader must draw in the font copied in, and in Atkinson with a note when the
  copy is broken or gone (`jetcarp-display/src/macsaver.rs`). The caption's last resort must draw
  a character only an installed font has (the stand-in's CJK and key symbols) in it, behind
  Atkinson and behind egui's original, on the desktop background too, while what the fonts in place
  have stays theirs; the installed fonts must be read on a thread of their own, the caption drawn
  meanwhile with the reading held back (a box for such a character), starting it taking under
  50 ms, and the font used once read (`caption.rs`). Installing the macOS saver with a rotation
  named in Chinese must copy in the stand-in installed font that has those characters, and not
  one the caption needs nothing from, and the saver must draw them from its bundle alone; a
  rotation its fonts draw whole copies none (`macsaver.rs`). The clip-rating page and the website export's page must
  carry Atkinson Hyperlegible Next (and the rating page Mono) inside them, the files those the
  fonts' README names by SHA-256, and fetch nothing from elsewhere
  (`jetcarp-core/src/web_fonts.rs`, `jetcarp-api/tests/explore.rs`, `jetcarp-store/tests/website.rs`);
  so must `THIRD_PARTY_LICENSES.html`, Next's regular and bold and Mono's regular, which
  `packaging/notices_union.py` puts in when the page is generated (it refuses a file whose SHA-256
  isn't the README's), and CI's check that the page is current regenerates it so.
- **The macOS screen saver kept up to date** (#287), in scratch folders on Linux and macOS (what
  needs no Mac): each thing the bundle bakes (the rotation chosen, a rotation or scene saved, the
  caption, its font, the strobe guard, the seizure warning's answer, the saver's code) must mark it
  out of date; the rebuild must write a fresh folder and put it in the old one's place, a file of
  the old bundle held open reading as it was and nothing left beside the bundle; a rebuild that
  fails (its signing, here) must leave the old bundle as it was, keep the reason for `jetcarp
  doctor`, and not be tried again until the next change; and a change to the guard or the answer
  must reach the bundle without installing again, while a change that bakes the same leaves it
  alone (`jetcarp-display/tests/saver_update.rs`).
- **The launchers and the icon.** `jetcarp launcher install` runs in a scratch home on each
  system, cut off from the desktop session: on Linux the menu entry must pass the Desktop Entry
  spec's rules (and `desktop-file-validate`, where it is installed, with nothing to say), name the
  program by its quoted path, and come with the icon at all eight sizes, byte for byte; on macOS
  *JETCARP.app* must hold its `Info.plist`, its launcher naming the program and the `.icns`, and
  pass `codesign --verify`; on Windows the Start menu's `JETCARP.lnk` must be a shell link, with
  the copy of `jetcarpw.exe` it starts and a record naming the program. `jetcarp doctor` must say
  where it is. Then `jetcarp uninstall` must list it and take every file out (on Windows,
  `jetcarp launcher remove`, since `jetcarp uninstall` would reach the real registry), and a menu
  entry, app or shortcut of the same name that JETCARP did not make must be neither replaced nor
  removed (`crates/jetcarp/tests/launcher.rs`). On Windows a winget upgrade is played out for
  real: JETCARP in a versioned folder makes the entry, the folder is replaced by the next
  version's on the `PATH`, and the entry's launcher must still start JETCARP, whose first run
  mends the record. How the launcher finds `jetcarp.exe` (beside itself, the record, the screen
  saver's host command, the `PATH`, a stale record falling through), the record's mending (only
  when it names a program that has gone, or is missing: never in place of another program that is
  still there), and a removal that takes only JETCARP's two files are checked on every system with
  scratch folders
  (`crates/jetcarp/tests/jetcarpw_find.rs`). The bundle and the menu entry are also written into
  scratch folders on every system, and Homebrew's link is found from a version's own folder
  (`jetcarp-display/src/launcher.rs`). Setup's summary must tick the box for every profile when
  JETCARP isn't among the apps, and add it once (`setup_ui.rs`), and `jetcarp setup --yes` must add
  it unless `--no-launcher` (`crates/jetcarp/tests/setup.rs`). The Windows resource file the build
  script writes must hold the `.ico` file's seven images as icon resources and list them in icon
  group 1 (`crates/jetcarp/tests/version_res.rs`), and the app's windows' icon must decode to the
  256 px picture.
- **Uninstalling.** In scratch folders on every system, `jetcarp uninstall --data` and
  `--settings` must delete only folders that hold nothing but JETCARP's own, refusing any other
  before doing anything (`crates/jetcarp-api/tests/uninstall_folders.rs`), both of them with the
  data folder inside the settings folder, as on macOS (`uninstall_nested.rs`), and a second run
  must say the folders are already gone (`uninstall_again.rs`). The runtime
  folder must be emptied of the locks, port and token files (one of them half-written) and memory
  records that programs left, keeping and naming those a running app or host still uses, then
  removed; a file JETCARP did not put there must be left alone, and in Windows' layout, where it
  is the data folder, the library must stay unless `--data` deletes the whole folder
  (`uninstall_runtime.rs`, `uninstall_leftovers.rs`). The whole app, headless and with AI control
  on, must end on SIGTERM as closing its window does, removing its endpoint file
  (`crates/jetcarp-app/src/quit_signal.rs`, Linux and macOS). The background host, headless, and
  `jetcarp mcp --http`, each in scratch folders, must stop on SIGTERM as their own stop does: a
  clean exit, their endpoint file and memory record gone, and the HTTP server's hunt paused at its
  checkpoint; a second SIGTERM, while either waits for a caller to hang up, must end it at once
  with 143. A `job_status` waiting up to two minutes for a hunt must not hold up the HTTP server's
  stop: the call must be told the server is stopping, and the server must end within 3 seconds,
  with 0, its files gone. A call that ignores its cancel (`use_app` reaching for an "app" that never
  answers) must be left after the 2-second grace, the server still ending within 3 seconds with
  its files gone. `jetcarp mcp` over stdio must stop as stdin closing stops it, its waiting call
  told and its hunt paused; a foreground `jetcarp search` must stop at its checkpoint, paused, and
  `--resume` must carry it on from there; `jetcarp script` must stop at its next step and end with
  143; each with its memory record gone (`crates/jetcarp/tests/quit_signal.rs`). How a cut-short
  answer reads and when the grace ends are checked in `crates/jetcarp-mcp/src/stopping.rs`. The
  handling they all share must run a program's own stop on the first signal, end it on the
  second, and leave a signal ignored at start (SIGHUP under `nohup`) ignored
  (`crates/jetcarp-core/src/quit_signal.rs`). Windows' console events are handed to a test
  program as Windows hands them, on a thread of their own, on every system, and when the handler
  returns the program ends at once, as Windows then ends it: a closed console must run the
  program's stop and its handler must not return until the program has ended itself, with its own
  status; a program still stopping must see the handler return after Windows' own wait (5 seconds
  unless changed); Ctrl-C must stop it and return at once, and a second end it with 130. On
  Windows, the hidden window that hears the session end must answer `WM_QUERYENDSESSION` with yes
  and `WM_ENDSESSION` only once the program has ended, and `WM_CLOSE` (what `taskkill` without
  `/F` sends) must stop it, a second ending it with 143 (the same file). A closed console must
  stop a running `jetcarp search` at its checkpoint, run as `main` runs it, its job paused and
  resumable and its memory record gone (`crates/jetcarp/src/stop_signal.rs`). On Windows, a
  sign-out, sent as Windows sends it to the real program's hidden window, must stop a search at
  its checkpoint and the headless host as `jetcarp host stop` does, and the real `taskkill`
  without `/F` must stop a search at its checkpoint (`crates/jetcarp/tests/session_end.rs`); the
  whole app, headless, must end at a sign-out as closing its window does
  (`crates/jetcarp-app/src/quit_signal.rs`). A headless host leaves no still picture, so the
  still a host showing the desktop leaves as it stops is checked by hand, at a logout and on
  Windows at a sign-out and a shutdown.
- **The book.** The command-line and MCP reference pages and the keyboard page's table of keys are
  generated from the program and checked against it, and the copy of the book built into the
  program must match the book.

About 50 more tests are ignored by default and run only when asked (`-- --ignored`): most draw the
book's pictures of the app, and the rest time the program, run very large worlds, need a desktop
session, or read the app's accessibility tree with the starter pack set up. (The twelve halves of
the keyboard tests that run in a process of their own are ignored too, and so are those of the
signal tests: the suite starts them itself, with their scratch folders, and they do nothing when
started any other way.)

Tests never write to your own settings, data or pictures: they run in scratch folders, and CI fails
if a test touches the default ones.

## The strobe guard

The [strobe guard](../guides/strobe-guard.md) is tested without anyone watching flashing scenes.
Nobody with photosensitivity is asked to test it: the flashing scenes are checked by numbers and
recordings, never by eye, and the hands-on sittings watch the guard's status, in a small window,
with the guard already on.

- **The detector** (`jetcarp_core::strobe`) is tested on synthetic frame sequences at 30, 60, 120
  and 144 frames a second: just under and just over three flashes a second, a luminance step of 0.1
  and just under, the darker state at 0.8, a flashing square just under and just over a quarter of
  the 10° field, a strip larger than that area that never fills a quarter of a field, a view smaller
  than the field, red flashes apart from general ones, the sliding one-second window, the extended
  failure, and checkerboards reversing. **Fine patterns** (#269,
  `crates/jetcarp-core/tests/strobe_checkerboard.rs`): a checkerboard of squares from 3 to 32
  pixels (from just over 0.1° to twice a tile, either side of the tile's 16) at five offsets from
  the tiles' grid, reversing 4, 6 and 10 times a second over 6% of half a 1080p screen (over twice a
  quarter of the 10° field, far under a quarter of the screen), must read as many transitions as it
  reverses, a warning at 4 and 6 and a failure at 10; grey squares 0.114 apart in luminance flash
  and squares under 0.09 apart do not, nor light squares whose darker state is 0.8 or more; and
  squares of 1 and 2
  pixels, under 0.1°, are exempt at every offset. These tests were written first and failed before
  the fix (the tiles' means alone counted none of them, nor 16- to 32-pixel squares half a tile off
  the grid). Ordinary scenes must read exactly as they did without the fine patterns: Life soups at
  2 to 16 pixels a cell, a soup's ash (still lifes and blinkers) at 4 and 30 generations a second,
  and fields of blocks, gliders and blinkers. **Red flashes by either definition** (#270,
  `crates/jetcarp-core/tests/strobe_red.rs`): a pair of colours only WCAG 2.0's red measure counts
  (two reds of one chromaticity, 135 apart in the measure), one only WCAG 2.2's counts (a dark red
  and a dark green, 0.33 apart in chromaticity and 14.5 in the measure), and one both count (red
  and a grey of its brightness), none of them a general flash, each fail at ten flashes a second,
  and the guard keeps each to at most three red transitions a second on screen; WCAG 2.2's red
  flashes keep the area rule, a square just under a quarter of the 10° field passing and one just
  over failing. These were written first too, and the one only 2.2 counts failed before the fix.
  **Change in part of a tile** (#275, `crates/jetcarp-core/tests/strobe_period2.rs`): fields of
  period-2 oscillators whose two phases are not each other's negative, stepped and drawn by
  JETCARP, are measured against a reference that applies WCAG 2.x's general and red flash to each
  pixel, with WCAG's own field (341 by 256 pixels at 1024 by 768) scaled to the 10° the guard
  assumes. The densest lattices of blinkers, toads and clocks, and the densest period-2 agars a
  search of every pattern on tori of up to 24 cells found (half to three quarters of the area
  flipping), at 3 to 16 pixels a cell in the default palette and the strongest dark, light and red
  ones, must be counted where the reference counts them, and through the guard, in a window and
  over the whole screen, show at most 4 transitions in any second; sparser fields (a blinker every
  5 cells, beacons) and ordinary scenes (a Life soup, still lifes, gliders, Life's ash) must not be
  veiled where the reference counts no flash. These were written first, and failed: the guard
  passed every dense field, and 8 transitions a second reached the screen. The GPU's reading of
  such a field counts it too, and its guard keeps it under the limit
  (`the_gpu_measure_counts_a_dense_period_2_field`,
  `the_gpu_guard_keeps_a_dense_period_2_field_under_the_limit`). The study's full table is an
  ignored test (`study_table`). **Restless change** (#289, the same file): the settled soups of
  Walled Cities, 34 Life and Long Life, and a Seeds soup, whose cells keep changing without coming
  back, must be counted where the reference counts them, strictly and concurrently, and through
  the guard show at most 6 transitions in any second from the start, before the scene is slowed
  as after; Brian's Brain, Lava and a Life soup, which the reference does not count as flashing
  together, must not be veiled. These were written first, and failed: 8 transitions a second
  reached the screen for up to 77 frames. On the GPU, a Walled Cities soup's guard must show at
  most 4 (`the_gpu_guard_keeps_restless_ash_under_the_limit`). The study's table of 15 rules at 1
  to 16 pixels a cell, and the reference on recordings for IRIS, are ignored tests
  (`restless_table`, `the_reference_on_recordings`).
- **The stripe check** is tested on stripes of every orientation from 4 to 128 pixels a pair,
  checkerboards and rows of dots, which it must find; and on noise, random cells of 2 to 16 pixels,
  faint cell outlines, low contrast and bright stripes, which it must not.
- **The guard's decisions** are tested end to end on a simulated screen: what it draws, softened as
  the guard says, read back a few frames late. A field flipping every frame is softened and then
  slowed and never shows more than three flashes a second, at any frame rate; flashing that slowing
  cannot stop is paused; small flashing and slow fades are left alone; switched off, it does
  nothing. A small reversing checkerboard (squares of 4 to 12 pixels, 7 to 30 generations a second)
  is veiled and slowed, and what is shown, measured pixel by pixel as WCAG defines a flash (no
  tiles), makes at most 4 transitions in any second, where unguarded it makes 7 or 8; on the GPU
  (`the_gpu_guard_keeps_a_small_reversing_checkerboard_under_the_limit`), at most 3.
- **The GPU and the processor agree**, on any driver (CI runs this on Mesa's llvmpipe as well as
  the test machines' GPUs): the GPU's tile maps and half-resolution image match the processor's
  to the rounding of the sums, and its softened frames byte for byte; so do the tiles' mean colours,
  as drawn and as shown, and each of the three red pairs above fails on the GPU's measure as on the
  processor's. The tiles' fine patterns
  match bit for bit, but for a window whose mean is exactly the threshold away from its tile's in
  exact arithmetic (a frame of a few colours can make one), which the test recomputes and allows.
  The GPU decodes pixels with
  the processor's own tables, not the texture unit's sRGB decoding, which differs by driver, and
  makes the red measure's saturation test in whole numbers: the measure steps from 0 to over 100
  there, so a pixel decoded a little differently could move a tile's mean by a whole step (it did,
  on llvmpipe).
- **The corpus** (`docs/scenes/strobe/`): scenes made to flash (a Margolus Tron field, which flips
  every generation, in grey and in a saturated red-orange; Tron from a blob, whose rectilinear
  growth is regular stripes as well; a Seeds soup; Hodgepodge as Quick Start shows it, the seizure
  warning's example, which fails at 60 generations a second, is a warning at 30 and passes at 15),
  the Tron field just under and just over the rate (six and seven generations a second, and six
  for long enough to be the extended failure) and the area (shown 112 and 128 pixels across),
  striped scenes (still stripes, and a checkerboard of 8-pixel squares reversing 15 times a
  second, which since #269 fails for flashing as well),
  ordinary scenes (a Life soup, Brian's Brain, a maze, Greenberg-Hastings waves) and still fields
  with thin lines over them (white live cells at a fractional size, where seams would show, and
  with cell outlines at 8 and 16 pixels a cell; Paper's light ground with scattered cells and
  outlines). Each is drawn frame by frame on the processor, at a fixed speed, at 320 by 240 and 60
  frames a second, and JETCARP's verdict on each is pinned
  (`crates/jetcarp-core/tests/strobe_corpus.rs`), its stripes by both rules: the usual one and
  the at-risk one, which counts similar parts as well. The app's view of the outlined fields,
  drawn on the GPU, is pinned too (`outlines_in_the_app_s_view_are_pinned`). Each is also run
  through the guard as a screen shows it, readings two frames late, with each rule in turn, and
  nothing it shows may fail. The corpus found two things the first version got wrong: a Life soup
  of 4-pixel cells counted as flashing (tiles flickering independently), so a transition now counts
  only where neighbouring tiles change together; and stripes took about a second to veil, longer
  than the half second they are allowed.
- **Every output path, offscreen**: the shared painter behind the desktop background and the screen
  savers, both ways it can draw, and the app's whole window (ignored test, on the GPU), with the
  flipping field: what they show, read back and measured on the processor, never fails. An exported
  animation of it is caught, and its softened version, read back from the file, passes.
- **The outside check** compares JETCARP's verdicts with those of EA's IRIS
  ([Electronic Arts 2023](../research/bibliography.md#ea2023iris)) on recordings of what each
  output path drew ([Comparing with IRIS](#comparing-with-iris)). For flashes it misses nothing
  IRIS flags, and with the guard on IRIS passes every recording for flashing. For patterns, the
  guard for someone at risk misses nothing IRIS's pattern check flags, and with it on IRIS passes
  every recording; the usual guard counts regular stripes only, and its disagreements with IRIS are
  listed. IRIS, like the guard, is a tool, not a certificate.

### Comparing with IRIS

JETCARP neither ships nor builds IRIS; this is how the comparison was run, and what it found.

1. **IRIS** ([Electronic Arts 2023](../research/bibliography.md#ea2023iris)) at commit
   `d96978ac1107f3463b77f69a9c1b1ec5d45291a0` (14 January 2025), unchanged, built as its README
   says: CMake with vcpkg at IRIS's own baseline (`0affe8710a4a`, which brings OpenCV 4.8 with
   FFmpeg), its example program, release only. It was built on Fedora 45 without
   administrator rights: the build tools vcpkg does not fetch on Linux (m4, bison, autoconf,
   automake, libtool, gperf and NASM, a few pure-Perl modules Fedora packages apart, and Jinja2)
   were built or fetched into the build's own folder. The host's compiler and headers are newer
   than IRIS's dependencies: C was built as gnu17, three new warnings that ports building with
   `-Werror` trip were kept as warnings, and hdf5 was pointed at vcpkg's zlib (the system's zlib
   configuration names a static library that is not installed). None of that touches what IRIS
   computes.
2. **Settings**: its example program, `IrisApp -v <recording> -p 1 -j 1`, with its default
   `config/appsettings.json`: a transition is a change of 0.1 in the frame's mean luminance (the
   darker state below 0.8) or of 20 in its red measure, counted when at least a quarter of the
   frame's pixels changed; more than 6 in a second fails, 4 to 6 for 4 seconds of 5 is the
   extended failure. IRIS's defaults leave its pattern check off; `-p 1` turns it on, for the
   stripe check's comparison: at least 6 similar parts over a quarter of the frame, with light
   parts of at least 0.25, for half a second. IRIS's own test images were checked first:
   `20stripes.png` fails and `shapes.png` passes, as its tests expect.
3. **Recordings**, made headlessly (nothing shown on a screen), 320 by 240:
   - *The desktop background and the screen savers* (the shared painter, on the GPU), every
     corpus scene for 6 seconds at 60 frames a second, with the guard off (`desktop-off`, what the
     scene draws), on (`desktop-on`, what the guard shows) and on for someone at risk
     (`desktop-risk`, with its stricter stripe rule): `JETCARP_STROBE_RECORD=DIR cargo test
     --release -p jetcarp-display --test strobe_painter record_the_corpus_for_iris -- --ignored`,
     which also writes `DIR/verdicts.json`, JETCARP's verdict on each recording by both stripe
     rules, from the processor's meters on the frames recorded. The live field is also drawn at
     fractional sizes (4.37 and 7.7 pixels a cell), where seams between cells would show.
   - *The app's view with cell outlines* (`view-off`, `view-on`, `view-risk`): the same test draws
     the live field and the Paper field through the tile renderer with outlines, as the app's view
     draws them (faint from 7 pixels a cell, full from 16, and faint over a large even bright
     field), through the GPU guard.
   - *Pictures drawn on the processor with outlines* (`still`), as desktop pictures and exported
     animations draw them from 7 pixels a cell: `jetcarp wallpaper docs/scenes/strobe/<scene>.json
     --size 320x240 --fit repeat --cell-px 8 --outline 7`, held for 6 seconds. JETCARP's verdicts
     on these are the corpus's, pinned for the same scenes.
   - *The same pictures exported for someone at risk* (`export-risk`): `cargo test --release -p
     jetcarp-core --test strobe_corpus record_at_risk_exports_for_iris -- --ignored` saves each, 60
     frames at 10 a second, as WebP through the at-risk guard and reads the file back.
   - Each folder of PNGs became a lossless video at the rate it was drawn, `ffmpeg -framerate 60
     -i %05d.png -c:v ffv1 -level 3 -pix_fmt bgr0 out.mkv` (10 for the exports; RGB, no chroma
     subsampling, a constant rate: IRIS takes the rate from the file), checked frame for frame
     against the PNGs.
   - Still to do, on the test machines: *the screens themselves*, with the guard on and nobody
     watching a flashing scene: Windows, `ffmpeg -f gdigrab -framerate 60 -i desktop -c:v ffv1
     desktop.mkv`; macOS (the screen saver), `ffmpeg -f avfoundation -framerate 60
     -capture_cursor 0 -i "<screen>:none" -c:v ffv1 saver.mkv`; GNOME (the extension's desktop
     background), GNOME Shell's own screencast in a headless session, or `ffmpeg -f kmsgrab`.
4. **Scoring**: a *miss* is a recording of what was drawn (guard off, or a picture) that IRIS fails
   and JETCARP passes; a *false alarm* one JETCARP fails and IRIS passes. With the guard on, and
   with the at-risk guard on, every recording should pass IRIS.

**Results.** Transitions are the most in any one second (JETCARP stops counting at 8, past a
failure); IRIS's pattern column is the frames it failed for a pattern. *JETCARP, at-risk rule* is
the verdict by the stricter stripe rule for someone who answered the warning at risk.

| Recording | IRIS | IRIS transitions (light / red) | IRIS pattern frames failed | JETCARP | JETCARP, at-risk rule | JETCARP transitions (light / red) |
|---|---|---|---|---|---|---|
| `desktop-off-brians-brain` | pass | 1 / 0 | 0 | pass | pass | 3 / 0 |
| `desktop-off-cyclic-waves` | pass | 1 / 0 | 0 | pass | fail (stripes) | 2 / 0 |
| `desktop-off-hodgepodge` | fail (flash) | 12 / 0 | 0 | fail (flash) | fail (flash, stripes) | 8 / 0 |
| `desktop-off-life-soup` | pass | 0 / 0 | 0 | pass, warning | fail (stripes) | 4 / 0 |
| `desktop-off-live-field-4.37px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-off-live-field-7.7px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-off-maze` | pass | 0 / 0 | 0 | pass | fail (stripes) | 1 / 0 |
| `desktop-off-seeds-soup` | fail (pattern) | 0 / 0 | 297 | fail (flash) | fail (flash, stripes) | 8 / 0 |
| `desktop-off-stripes-still` | pass | 1 / 0 | 0 | fail (stripes) | fail (stripes) | 1 / 0 |
| `desktop-off-tron-blob` | fail (flash, pattern) | 31 / 0 | 53 | fail (flash, stripes) | fail (flash, stripes) | 8 / 0 |
| `desktop-off-tron-checkerboard` | pass | 0 / 0 | 0 | fail (flash, stripes) | fail (flash, stripes) | 8 / 0 |
| `desktop-off-tron-field` | fail (flash) | 60 / 0 | 0 | fail (flash) | fail (flash) | 8 / 0 |
| `desktop-off-tron-red` | fail (flash, red) | 37 / 37 | 0 | fail (flash, red) | fail (flash, red) | 8 / 8 |
| `desktop-on-brians-brain` | pass | 1 / 0 | 0 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-cyclic-waves` | fail (pattern) | 1 / 0 | 24 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-hodgepodge` | pass | 2 / 0 | 0 | pass | fail (stripes) | 3 / 0 |
| `desktop-on-life-soup` | pass | 0 / 0 | 0 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-live-field-4.37px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-on-live-field-7.7px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-on-maze` | pass | 0 / 0 | 0 | pass | fail (stripes) | 1 / 0 |
| `desktop-on-seeds-soup` | fail (pattern) | 0 / 0 | 122 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-stripes-still` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
| `desktop-on-tron-blob` | pass | 3 / 0 | 0 | pass | fail (stripes) | 3 / 0 |
| `desktop-on-tron-checkerboard` | pass | 1 / 0 | 0 | pass | pass | 3 / 0 |
| `desktop-on-tron-field` | pass | 3 / 0 | 0 | pass | pass | 3 / 0 |
| `desktop-on-tron-red` | pass | 2 / 3 | 0 | pass | pass | 3 / 3 |
| `desktop-risk-brians-brain` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
| `desktop-risk-cyclic-waves` | pass | 1 / 0 | 0 | pass | pass | 2 / 0 |
| `desktop-risk-hodgepodge` | pass | 2 / 0 | 0 | pass | pass | 2 / 0 |
| `desktop-risk-life-soup` | pass | 0 / 0 | 0 | pass | pass | 1 / 0 |
| `desktop-risk-live-field-4.37px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-risk-live-field-7.7px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-risk-maze` | pass | 0 / 0 | 0 | pass | pass | 1 / 0 |
| `desktop-risk-seeds-soup` | pass | 0 / 0 | 0 | pass | pass | 1 / 0 |
| `desktop-risk-stripes-still` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
| `desktop-risk-tron-blob` | pass, warning | 5 / 0 | 0 | pass, warning | pass, warning | 4 / 0 |
| `desktop-risk-tron-checkerboard` | pass | 1 / 0 | 0 | pass | pass | 3 / 0 |
| `desktop-risk-tron-field` | pass | 3 / 0 | 0 | pass | pass | 3 / 0 |
| `desktop-risk-tron-red` | pass | 2 / 3 | 0 | pass | pass | 3 / 3 |
| `view-off-live-field-outlined-12px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-off-live-field-outlined-16px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-off-live-field-outlined-8px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-off-paper-field-outlined-16px` | fail (pattern) | 0 / 0 | 331 | pass | fail (stripes) | 0 / 0 |
| `view-off-paper-field-outlined-8px` | pass | 0 / 0 | 0 | pass | fail (stripes) | 0 / 0 |
| `view-on-live-field-outlined-12px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-on-live-field-outlined-16px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-on-live-field-outlined-8px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-on-paper-field-outlined-16px` | fail (pattern) | 0 / 0 | 331 | pass | fail (stripes) | 0 / 0 |
| `view-on-paper-field-outlined-8px` | pass | 0 / 0 | 0 | pass | fail (stripes) | 0 / 0 |
| `view-risk-live-field-outlined-12px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-risk-live-field-outlined-16px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-risk-live-field-outlined-8px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `view-risk-paper-field-outlined-16px` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
| `view-risk-paper-field-outlined-8px` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
| `still-live-field-outlined-16px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `still-live-field-outlined-8px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `still-paper-field-outlined-16px` | fail (pattern) | 0 / 0 | 331 | pass | fail (stripes) | 0 / 0 |
| `still-paper-field-outlined-8px` | fail (pattern) | 0 / 0 | 331 | pass | fail (stripes) | 0 / 0 |
| `export-risk-live-field-16px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `export-risk-live-field-8px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `export-risk-paper-field-16px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `export-risk-paper-field-8px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |

**Flashes**: no misses. IRIS fails four recordings for flashing (the Tron field, in grey and in
red, Tron from a blob, and Hodgepodge at 60 generations a second), and JETCARP fails all four.
With either guard on, IRIS passes every recording for flashing (Tron from a blob with the at-risk
guard on is a warning, a pass). Two false alarms: the Seeds soup, whose flicker JETCARP counts by
its area rule, a quarter of a 10° field (about 0.7% of a 1080p screen), where IRIS takes the mean
of the whole frame; and since #269 the reversing checkerboard, whose frame mean never changes
though every pixel flashes, which WCAG counts (its 8-pixel squares are over 0.1°) and JETCARP
counts by its tiles' fine patterns. Both differences are deliberate. The comparison was run again
for #269 with the fine patterns in: IRIS's verdicts did not change on any recording, and every
recording with the guard on is as before, to a byte's rounding (a pixel's value may move by one,
as the GPU tests allow), but two: the checkerboard, now veiled where it reverses as well as where
its stripes are; and Hodgepodge with the usual guard, whose tiles' fine patterns reverse when the
whole field swings, which now count where its tiles' means do not move, so the guard paces it
differently. It was run once more for #270, with red flashes counted by WCAG 2.2's chromaticity as
well as WCAG 2.0's measure: every recording came out the same, byte for byte, and every verdict
with it. The red flashes the corpus has (the Tron field in red-orange, against black, which has no
chromaticity) are WCAG 2.0's, which IRIS counts too; by either definition JETCARP is never more
lenient than IRIS. It was run again for #289, through the guard that veils restless change: IRIS's
verdict on every recording was as before, in every column, and the Seeds soup with the usual guard
is now veiled, where pixel by pixel it flashed together. On dense restless ash (Walled Cities, 34
Life, Long Life, Seeds and Replicator soups at 2 to 16 pixels a cell), which WCAG counted pixel by
pixel fails, IRIS counts no flash at all, as drawn or through either guard, since the frame's mean
hardly moves: a difference like the checkerboard's, and JETCARP follows WCAG.

**Patterns, for someone at risk**: no misses, and with the at-risk guard on IRIS passes all 22
recordings, the Seeds soup, the Greenberg-Hastings waves and the Paper field among them, for
flashes and for patterns. For someone who answered the warning at risk, whose guard can never be
turned off, the guard also counts what IRIS's pattern check counts: similar parts, regular or not
([the at-risk guard](../guides/strobe-guard.md#stricter-for-someone-at-risk)). Its false alarms
on these recordings are six: the still stripes and the reversing checkerboard (which IRIS passes
at this size), and ordinary scenes it counts as similar parts more often than IRIS does (the Life
soup, the maze, the waves with the guard off, the Paper field at 8 pixels a cell in the app's
view). It is the stricter of the two, by choice.

**Patterns, for everyone else**: the usual rule counts regular stripes only, so an ordinary soup is
not dimmed. Its two false alarms are the still stripes and the checkerboard; IRIS fails three
recordings of what was drawn that it passes, none of them stripes: the Paper field's scattered
dark cells on a light ground, drawn on the processor at 8 and 16 pixels a cell and in the app's
view at 16. IRIS's check counts similar shapes without asking whether they repeat regularly. With
the usual guard on, IRIS fails the same kind of scene: the Seeds soup and the waves (which the
guard slows, so their frames hold still for longer) and the Paper field at 16. Those are listed,
not followed: the stricter rule is for someone at risk.

**Seams**: there are none. A live field drawn at fractional sizes is one colour on the GPU (the
tile renderer's triangles share exact edges; `a_live_field_at_a_fractional_size_draws_no_seams`
checks it at 4.37 to 12.4 pixels a cell) and on the processor; both pass, and IRIS agrees. The
lines first seen over a live field in the app's view were its cell outlines.

**Outlines over a live field**, once a false alarm (a strong grid, which the stripe check counted
as rows of repeated squares and the guard dimmed), are now drawn faint where they would form
strong stripes ([the strobe guard](../guides/strobe-guard.md#what-it-measures)): the outlined live
field passes by both rules, on the GPU at 8, 12 and 16 pixels a cell and on the processor at 8 and
16, and IRIS passes it too.

**Found on the way**: with the guard on, the outlined field at 16 pixels a cell failed JETCARP's
own check of what was shown. The stripe check's windows were laid from the top left, so at the
coarser scales a band along the right and bottom edges (up to a window wide) was never looked at,
and never veiled; on a large pattern the band alone could be over a quarter of the screen.
Windows now also sit against the far edges (`stripes_against_the_far_edges_are_found_too`).

## The test machines' scores

Each release's sittings measure the three test machines with `jetcarp score --report`, and the
reports go to the public test records. The scores are of score v1 (see
[This computer](../guides/this-computer.md#the-benchmark-and-the-score)): the geometric mean of five
Life soup rates, in soups a second.

| Machine | Score | Measured |
| --- | --- | --- |
| A desktop with 128 GB of memory (96 GB for the system, 32 GB set aside for its integrated GPU), 16 cores | 24,997 (its GPU about 10 times its processor on the soups) | during development, 21 November 2026 |
| A Windows laptop | at the 0.4.3 sittings | |
| An Apple silicon laptop | at the 0.4.3 sittings | |

The desktop is measured at two or three firmware splits of its memory between the system and the
GPU, since its limits follow the split.

## How long Fit to this computer takes

A fit must take about 10 seconds, 30 at most, on one monitor or several
([#113](https://github.com/jprodgers/jetcarp/issues/113)). In 0.4.2's release candidates, which
showed every scene at every size for about 2 seconds each, a fit took 45 seconds to 12 minutes. The
times below are the host's own (`seconds` in the fit's last line), with the time of the whole
`jetcarp host tune` command, its start and its following of the host included, in brackets. The
default limits (processor 10%, GPU 25%) and one second a size, on the desktop of the table above
(an AMD Radeon 8060S, Linux), during development on 21 November 2026, in a private headless GNOME
Shell with virtual monitors, so nothing showed on a real screen:

| Screens | Rotations | First fit | The same fit again | `--pick` |
| --- | --- | --- | --- | --- |
| One, 2560 × 1440 at 60 Hz | *Classics, hourly* (7 scenes, all on the GPU) | 3.2 s, 2 sizes (3.5 s) | 2.0 s, 1 size (2.3 s), the same sizes | none measured (0.6 to 0.7 s) |
| Two at once: 2560 × 1440 at 60 Hz, and 1920 × 1080 at 120 Hz | *Classics, hourly*, and *Until it settles* (3 scenes) | 5.4 s, 6 sizes (5.9 s) | 3.1 s, 3 sizes (3.5 s), the same sizes | none measured (0.6 s) |
| One, 2560 × 1440 at 60 Hz | Life and B36/S23 on the GPU, Wolfram's rule 30 on the processor: one scene measured on each | 9.6 s, 8 sizes, 4 for each (10.0 s) | 2.8 s, 2 sizes (3.0 to 3.1 s), the same sizes | none measured (0.6 to 0.7 s) |
| Two at once, as above | that rotation, and *Until it settles* | 9.4 s, 12 sizes (10.0 s) | 3.8 to 5.0 s, 5 or 6 sizes (4.3 to 5.5 s) | none measured (0.6 to 0.7 s) |
| One, off screen (no host), 2560 × 1440 | *Classics, hourly* | 3.1 s, 2 sizes | 2.0 s, 1 size | |
| One, off screen (no host), 2560 × 1440 | the rotation mixing devices | 8.7 s, 8 sizes | 2.6 s, 2 sizes | |

A fit's first size warms up half a second longer than the rest (so a fit just after the host
started measures as later ones do), which is the half second each fit gained over the first
measurements, made without it: 2.7 s and 1.5 s on one screen, 3.7 s and 2.6 to 3.8 s on two. A
rotation mixing devices has two scenes to measure on its screen, one after the other, so its first
fit takes about twice as long; with no earlier fit or score to guess from, each search here started
at 6 px and went to 1 or 2 px, four sizes each.

With two screens, the limits were shared by area as they should be: 64% and 36%, so 6.4% and 3.6%
of the processor, and 16% and 9% of the GPU. A fit repeated with nothing changed kept every
scene's size, on one screen and on two, the first fit included: the second screen's 2 px, which a
first fit without the longer warm-up had once judged just over its GPU share, was within it on the
first fit and *at the limit* (kept) on every fit after. Beside the second screen, the rotation
mixing devices kept W30's 2 px and the second screen's 2 px on every fit, but Life went from 2 px
on the first fit to 1 px after: 1 px measured 6.2% of the processor against its 5.1% line the first
time, then 3.5%, then 5.3% and 5.4% (*at the limit*, so kept). The host's memory stayed between 208 and
226 MB over six fits on one screen and between 263 and 312 MB on two.

These were measured with nothing else busy on the machine (a load average under 3 at the start of
each run). With other work compiling at the same time, the frame times went up to 25 ms and more,
and the same fits tried up to six sizes and settled on coarser ones.

The same measurements are wanted on the Windows laptop and the Mac at the 0.4.3 sittings, with one
monitor and with two, including the Mac's two monitors of different refresh rates
([#106](https://github.com/jprodgers/jetcarp/issues/106)), its GPU share now from the Mac's own
count of JETCARP's GPU time ([#93](https://github.com/jprodgers/jetcarp/issues/93)), and its memory
over two-screen fits ([#111](https://github.com/jprodgers/jetcarp/issues/111)).

## Continuous integration

GitHub Actions runs on every push to `main` and every pull request. A release is prepared on a
branch with a pull request open against `main`, so every push to it is tested; a tag runs the
release workflow. The workflows name each action they use by its commit, with its version beside it,
so a tag that moves cannot change what runs. Every job has a time limit, about twice its usual length or more, so a
job that hangs is stopped rather than left running for hours.

- **Build and test** the whole workspace on Linux (x86_64), Windows (x86_64) and macOS (Apple
  silicon). The runners have no GPU of their own, so on Linux the GPU tests run on Mesa's software
  renderer (lavapipe), on macOS on the runner's virtual GPU, and on Windows on WARP, Windows'
  software renderer, in its newer build 1.0.22. The WARP that comes with Windows crashes compiling
  JETCARP's kernels, so JETCARP never uses it, and no user gets WARP from JETCARP
  ([#321](https://github.com/jprodgers/jetcarp/issues/321)). The Windows job downloads
  Microsoft's `Microsoft.Direct3D.WARP` 1.0.22 from NuGet, checks it against a pinned SHA-256, and
  puts its `d3d10warp.dll` beside every program the tests run, which load it before the system's;
  `JETCARP_ALLOW_WARP=1` lets the tests choose it. The soup hunt's GPU tests skip there, saying
  why: that WARP takes minutes to compile each of the hunt's kernels and gets Generations rules
  wrong, so the hunter never runs on WARP. It is a tool of CI's: nothing a release ships is built
  there, and no release contains it. On all three systems, and in the release workflow's
  tests, `JETCARP_REQUIRE_GPU=1` makes a GPU test that finds no adapter it can use (none, or one
  the GPU self-test turns down) fail, naming the adapter and why, rather than skip: there the GPU
  tests cannot pass without having run. On Linux the tests run once more with the default
  folders, failing if any test writes to them. On Windows each of the folder rules' link cases
  must make its junction or symbolic link and run; the log names each one.
- **A debug build** of the whole workspace runs every test again on Linux, GPU tests on lavapipe:
  release builds check neither arithmetic for overflow nor debug assertions, and a bug in the
  animated WebP encoder hid from them that way until 0.4.3.
- **The installers** are each given a release folder of the test's own: an intact archive installs;
  an archive with one byte changed, and one that `SHA256SUMS` does not list, are refused with
  nothing installed (the shell installer on Linux, the PowerShell one on Windows). The intact install
  must have the program add JETCARP to the system's apps, `--no-launcher` must leave that out (the
  shell installer), and uninstalling must take the launcher out with the program: on Windows the
  real program makes and removes the Start menu entry in the test's own `APPDATA`.
- **Build only** for macOS on Intel and Linux on ARM, which nobody tests by hand yet, and the whole
  workspace with the oldest Rust it says it supports (1.95), so that the stated minimum stays true.
- **Checks:** formatting (`cargo fmt --check`), the third-party notices are current, the workflows
  and packaging scripts pass `actionlint`, every crate packages and builds on its own (a crates.io
  dry run), the generated reference pages match the program, and every file of the app icon matches
  its master SVG (`packaging/gen-icons.py --check`: each PNG rendered again by librsvg, whose pixels
  may differ only by another version's antialiasing, and the `.ico`, the `.icns` and the copies the
  program embeds byte for byte).
- **The dependencies,** with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (`deny.toml`),
  on every push to `main`, every pull request and once a week, since an advisory can be published
  without anything in JETCARP changing. The check fails on a known vulnerability or unsoundness in
  any dependency (the [RustSec](https://rustsec.org/) database), on any HTTP, TLS, DNS, WebSocket
  or peer-to-peer client or server crate that `deny.toml` does not allow, so that no networking
  comes in unplanned while [`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)
  says JETCARP makes no network connections, and on any crate from somewhere other than crates.io. A notice that a crate
  is no longer maintained is shown as a warning, and never fails it.
- **The book** builds, its internal links all resolve, and its scripting reference matches the
  program.
- **Golly:** the Linux job builds Golly's `bgolly` 4.0 and 5.0 from their source (Ubuntu packages
  only Golly 3.3, which can't read every rule JETCARP writes) and runs every Golly test above with
  each, but the one that needs a library of finds.
- **A release** is built only from a tag whose version agrees with the program and the changelog,
  and only after the tests pass.

## Release candidates

Since 0.4.2, a release goes through release candidates first: `0.4.2-rc.1`, `rc.2`, and so on.
Each is a public pre-release, built by the same workflow as a release, which the package managers
(Homebrew, Scoop, PyPI, crates.io) do not pick up. Each candidate is tested on three machines, with GPUs from
four vendors (as of 0.4.2):

| | Framework Desktop (the main machine) | Acer Nitro V15 laptop | MacBook Pro |
|---|---|---|---|
| Processor | AMD Ryzen AI Max+ 395, 16 cores, 32 threads | Intel Core i5-13420H, 8 cores, 12 threads | Apple M2 Max, 12 cores |
| Memory | 128 GB unified, split by hand: 96 GB for the system and 32 GB for the GPU (changeable with a reboot) | 15.7 GB | 64 GB unified (shared with the GPU) |
| Graphics | AMD Radeon 8060S (integrated), Vulkan (Mesa 26.2.5, RADV); Mesa's llvmpipe, a software renderer | NVIDIA GeForce RTX 3050 6GB Laptop GPU (driver 546.18) and Intel UHD Graphics (driver 31.0.101.4146), each on Vulkan and DirectX 12; an Optimus laptop, the UHD driving the screen | Apple M2 Max, 38-core GPU, Metal |
| System | Fedora Linux 45, Linux 7.3, GNOME Shell 51.1 on Wayland | Windows 11 Home 26H2 (build 26300) | macOS 27.2.1 |
| Screen | AOC Q3277, 2560 × 1440 at 60 Hz (moved to a laptop for the two-monitor sitting) | 1920 × 1080, built in | the built-in Retina panel |

The test plan for a candidate is written down before it is tested: a document listing each step,
what to run, and what counts as a pass. It is carried out in three parts.

**Phase 1, unattended**, on each machine:

- **Parity on every GPU.** 378 scenes (every built-in Quick Start scene, 371 since 0.4.3's
  isotropic entries, 364 for 0.4.2, and 7 sandpiles that drop grains) run for 300 generations on the CPU and on each adapter and graphics API the machine
  has: seven in all, from the Radeon and llvmpipe to the RTX and the UHD on both Vulkan and
  DirectX 12, to the M2 Max on Metal. Every final hash must match the one recorded on the Framework's CPU. Then 40 scenes chosen
  across every family and kind of tiling run for 5,000 generations each, so that slow drift shows.
- **The whole test suite on real GPUs**, built from a clean clone of the candidate's tag, on each
  machine's own adapters rather than CI's software ones.
- **A long search.** A four-hour exploratory search, killed hard after an hour, resumed, then packed
  and imported into a fresh library. Its memory is logged every few minutes, the killed run must be
  reported as stopped at once, the resume must keep the search's original time budget, and the
  imported search must be the same as the original.
- **A deterministic hunt** that must give the same numbers on every machine (in 0.4.2: 1,783
  distinct objects among 760,892, on all three).
- **The hunt against Catagolue** (from 0.4.3). `docs/tools/check-catagolue.py` runs a fixed hunt
  of 150,000 soups of Life and holds the shares of its twelve commonest objects against a copy of
  the top of Catagolue's census kept in the repository. It fails when one lies more than 4 standard
  errors from Catagolue's share, or the sum of their squares passes 32.9: an outside check of how
  the hunt runs soups, cuts out ships and separates and names objects, which the project's own
  tests cannot give (see [Soup hunt](../research/search-methods.md#soup-hunt)). JETCARP 0.4.2
  fails it.
- **An overnight run** of the live desktop background, eight hours, for leaks and stalls, when a
  machine can be left idle that long.

**Phase 2, by hand**, in sittings of an hour or two at each machine:

- **Searching:** an exploratory search on the GPU, replays, the CPU confirming the GPU's finds,
  rating clips on the rating page in two browsers, fitting a score to the ratings, packs.
- **The app:** every menu item, pane and tool window, popped out and back, AI control from an MCP
  client, light and dark, the scripting examples, exports, and big worlds on each GPU.
- **The desktop background:** rotations, *Fit to this computer* on one monitor and two, pauses for full-screen windows,
  battery and busy processors, sleep, lock and login, and the background host killed and restarted.
- **MCP and robustness:** every tool, bad input of every kind (124 malformed calls in one sitting),
  paths outside the allowed folders, read-only folders, damaged and tampered packs, two programs
  using one library, and the largest worlds the limits allow.
- **A newcomer's first hour and the upgrade from the previous release**, each in a separate user
  account, and **two monitors**.

**R, a check of every fix.** Each candidate's plan lists every problem fixed since the last one,
with a step that shows the fix working, run on the machines it applies to.

**A pre-check before the tag** (since `rc.4`): the suite and both parity sets run on every machine's
adapters from the release branch before the candidate is tagged, with the scripted checks of what
the candidate changed, so a candidate starts from a branch already known to pass.

**A narrow candidate** is tested to fit what it changes. `rc.5` carried one fix that couldn't wait
(#131: on macOS, the app's window stopped drawing after ⌘M) and seven small ones, none of them in the
background host or the simulation. So the Acer's and the Mac's overnight runs on `rc.4` counted for it
(the Framework's ran on `rc.5`). Its sittings ran on
`rc.4` or `rc.5`, and every machine ran parity, the suite and each fix's check again on `rc.5`.

**What each candidate actually got.** The lists above are what a candidate's plan draws on, not what
every candidate had on every machine. In the 0.4.2 cycle the eight-hour overnight run happened four
times (the Mac on a branch build, the Acer and the Mac on `rc.4`, the Framework on `rc.5`), none on
`rc.1` to `rc.3`. The four-hour search ran on `rc.3` on all three machines (`rc.2`'s plan asked for
two hours). No candidate had every sitting on every machine: the sittings were spread across the
candidates. In `rc.4`'s check of every fix, 12 of the 22 rows were for all three machines and 9 for
one. Each candidate's plan, with every machine's results, is in
[jetcarp-testing](https://github.com/jprodgers/jetcarp-testing).

The unattended steps, and the commands within the sittings, are run by a Claude Code session on each
machine, working from the written plan. It records every step with its evidence (commands, outputs,
timings, screenshots where they matter) and asks Jimmie, at the machine, whenever a step needs eyes
or hands: whether a picture looks right, whether a page is clumsy, whether a shortcut does what it
says. Results are committed as they come in, and a main session reads them, files the problems and
fixes them. The plans, logs and results are published in
[jetcarp-testing](https://github.com/jprodgers/jetcarp-testing).

## How a problem is handled

- **Every problem becomes a GitHub issue**, however small, saying where it was found (the
  candidate, the machine, the step) with the evidence. The issues for 0.4.2 are on the
  [tracker](https://github.com/jprodgers/jetcarp/issues).
- **Triage.** A fix that fits goes into the next candidate; anything that would change stored data,
  or is too large to test properly in time, goes to the next version. GitHub milestones track which.
  A feature that misses what it is for is held back: in `rc.3`'s testing, Fit to this computer took
  from 45 seconds to 12 minutes to size the desktop background for a machine, against a target of
  10 to 30 seconds, so 0.4.2 shipped without it; 0.4.3 brings it back, redesigned to take a few
  seconds (#112, #113; see [How long Fit to this computer takes](#how-long-fit-to-this-computer-takes)).
- **Fixes come with tests.** Where a test can be written, it is. Some fixes can only be checked on a
  particular system (how macOS treats a hidden window, for example); those are checked by hand
  there, in the next candidate's R. In the 0.4.2 cycle, 74 of the 102 closed issues have a test
  change in a commit that cites them; 14 fixes have neither a test nor a stated reason, and that a
  test failed without its fix is written down for five. From 0.4.3, each issue is closed with a note
  naming its fix, its test and that the test failed before the fix, or why there is no test.
- **The book is updated with each change**, and every candidate gets a full audit of the book before
  its tag: every page that mentions something changed, the app's pictures, and the changelog.
- **A candidate is tagged** only on a commit whose CI has passed on all three systems, and only on
  Jimmie's go. Since 0.4.3 the release workflow holds to the first half itself: it waits for CI on
  the tagged commit and stops unless it passed. And `main` takes changes only through pull requests
  whose CI checks have passed. In the 0.4.2 cycle, `rc.1` was tagged before its CI run had finished,
  and that run failed (#15); `rc.3` and `rc.5` were tagged while their commit's run was still going,
  and both passed.
- **Security problems** are reported privately, as
  [SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md) says. In the 0.4.2 cycle
  what stayed private was the problem: no public issue, its test records withheld and its advisory
  drafted privately until the release. The fixes went onto the public release branch, so their
  commits were public before 0.4.2 came out, while 0.4.1 was the current release. From 0.4.3, a fix
  for a problem not yet made public is made in its advisory's private fork on GitHub and merged when
  the release is made.

A step that turns out to have been set up wrong is run again, and the result recorded as the second
run's, with a note. A step that fails in a way that may be chance (a hang in one run of thirty) is
repeated until the evidence either way is clear, and the issue says how strong that evidence is.

## Who does what

- **Jimmie** decided what needed checking and how thoroughly: every operating system and GPU vendor,
  the upgrade path, a newcomer's first hour, full passes rather than quick ones, an issue for every
  problem. Jimmie approved each test plan, ran the hands-on sittings on each machine, judged the
  results by eye, and decided which problems held a release back. Jimmie wrote no test code.
- **Claude**, in Claude Code, drafted the plans' detailed steps from Jimmie's requests, wrote all of
  the test code and the test scripts, ran the unattended parts, read the results, filed the issues
  and wrote the fixes.

Many problems were found only because a person was using the program: a rotation that could not be
saved because its Save button was out of sight, blinkers that looked frozen on the desktop, a round of
clips that was "lots of static", a menu that closed before a number could be typed into it, an app
that stopped when its window was hidden. Others were found only by the scripted checks: a memory
reading on macOS that counted memory the system could take back, a resumed search that ran past its
budget, a pack that silently lost its runs. Neither would have been enough alone.

## The 0.4.2 cycle in numbers

A record of one cycle, from the first issue to the release, after five candidates (November 2026):

| Stage | Issues filed | Found by |
|---|---|---|
| Development, before the first candidate | 12 (#2–#13) | reading, using and porting the code, an audit of the GPU code, a long search, everyday use |
| `rc.1`'s testing | 32 (#15–#46) | the Acer (23), the Framework (3), CI (3), the book audit (3) |
| `rc.2`'s testing | 31 (#47–#78, less #70), and one security problem, handled privately | the three machines, a full pass |
| `rc.3`'s testing | 37 (#80–#116) | the three machines (30: 4 in Phase 1, 25 in the Mac's sittings, 1 in the Acer's), writing the book (1), a user's report and one issue split from it (#114, #115), Jimmie's decisions (4: the seizure warning #96 and its roadmap item #97; holding Fit back, #112 and #113) |
| `rc.4`'s pre-check, on the branch | 6 (#117–#122) | the Acer (3: #117, #121, #122), the book audit (#118), a Dependabot alert (#119), CI and the Acer (#120) |
| `rc.4`'s testing | 15 (#123–#137), and a second security problem, handled privately | the Mac (8), the Framework (4), the Acer (2), looking into #119 (#123) |
| `rc.5`'s testing | 7 (#138–#144) | the Framework (3: the suite run with the GPU taken away, #138; a newcomer's first hour, #139, #140), the Mac (2), the Acer (2) |

Of the 141 issues filed in the cycle (#2–#144), 101 were fixed or done for 0.4.2 (each checked on the
next candidate), 37 were moved to 0.4.3 (the strobe guard, #97, first meant for later, among them),
one was answered in part (#114, below), and two were closed with no change: a Dependabot alert for a
crate that no build compiles (#119), and a rule applied only when its box loses focus, which is by
design (#132). One came from outside: a user's report on the MCP server (#114), answered with a fix in
0.4.2 and an issue for 0.4.3 (#115); the same user's report on 0.4.1 (#1) came before the cycle. A
question on the ConwayLife forum became #70, filed by the maintainer. Every issue says where it was
found, and the [changelog](changelog.md) lists the fixes to problems that 0.4.1 had, with their
issues.

## What guards each security promise

[`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md) says what JETCARP does
and does not do. Each of its promises is held up by automated tests, by a check in CI, by the
hands-on rows of each release candidate's plan (its security rows, from 0.4.3), or by how the code is
built. The next audit checks this table against the code.

| # | Promise | Automated tests and CI checks | Hands-on (the plan's security rows) |
|---|---|---|---|
| 1 | It makes no network connections; the only network sockets it opens are local, on `127.0.0.1` | The Dependencies workflow (`deny.toml`): no HTTP, TLS, DNS, WebSocket or peer-to-peer crate, and no HTTP parser, in the dependency tree that `deny.toml` does not allow, so networking comes only on purpose; the one allowed, `httparse`, which reads the MCP server's HTTP requests, is allowed for `jetcarp-mcp` only. `the_listener_is_on_loopback_only` (`jetcarp-mcp/tests/link.rs`) and `the_endpoint_is_on_loopback_only` (`jetcarp-mcp/tests/http.rs`): no other address of the machine reaches them, and on Linux the HTTP server's one listening socket is `127.0.0.1`. By construction: the workspace's two listening sockets, the link's and the MCP server's HTTP endpoint's, are bound to `127.0.0.1` (`Listener` in `jetcarp-mcp/src/link.rs` and in `jetcarp-mcp/src/http.rs`), and the only connections it makes are to them | |
| 2 | Nothing in a pack is run; every hash and signature is checked before anything is stored | `jetcarp-pack/tests/hostile.rs`: oversized and deep documents, unknown versions and features, altered objects, names that would leave the store, too many parts, decompression and decoding bombs, records that do not hash to their ids, unsigned and badly signed versions, links out of an export or a store | S-P1 (the same tests on each machine), S-P2 (a pack signed by a key the machine has not seen) |
| 3 | A scene holds no code | By construction: a scene's `script` is a list of steps and the conditions between them, which is data (`jetcarp-core/src/script.rs`) | |
| 4 | Rhai scripts run only when you run them | By construction: a script runs from `jetcarp script`, the app's script window, the MCP server's `run_script` tool, or as a rotation's script while that rotation plays ([Rotations](../guides/rotations.md#a-rotations-script)); nothing else starts the engine | |
| 5 | Scripts write only inside the output folder, the folder given with `--out`, or (through MCP) the allowed folders | `writes_stay_inside_the_output_root` (`jetcarp-script/tests/script.rs`); `jetcarp-mcp/tests/folders.rs`, whose link cases run with symbolic links on every system and junctions on Windows | S-F1 to S-F10 |
| 6 | The app takes commands only while AI control is switched on | `nothing_listens_while_ai_control_is_off` (`jetcarp-app/src/app/ai_control.rs`) | S-L1 |
| 7 | Each listener makes a fresh 128-bit token from the system's random source; a wrong token is refused; the comparison takes the same time whichever character is wrong | `tokens_are_128_random_bits`, `each_listener_makes_its_own_token_and_records_it_privately`, `a_wrong_token_is_refused_and_the_right_one_works` (`jetcarp-mcp/tests/link.rs`); for the HTTP endpoint, which makes its token the same way, `the_token_is_required` (`jetcarp-mcp/tests/http.rs`) and `http_listens_only_when_asked_and_closes_with_ai_control` (a new token each time it is ticked). By construction: `constant_time_eq` (`jetcarp-mcp/src/link.rs`) and `same_token` (`jetcarp-mcp/src/http.rs`) compare every byte, with no early exit | S-L3, S-L4 (from a second user account) |
| 8 | The token file is readable by this user only (folder 0700, file 0600 on Linux and macOS), and is not left behind if it cannot be kept private | `a_loose_folder_and_file_are_made_private`, `each_listener_makes_its_own_token_and_records_it_privately` (Linux and macOS) | S-L2 (on Windows, the folder's permissions with `icacls`) |
| 9 | An AI client's tools read and write only in the output folder, the library's folder and the `--allow` folders | `files_stay_inside_the_folder_rules` (`jetcarp-mcp/tests/protocol.rs`), `jetcarp-mcp/tests/folders.rs`, `an_export_s_scratch_store_stays_inside_the_folder_rules` (`jetcarp-api/tests/reference.rs`) | S-F1 to S-F10 |
| 10 | An AI client cannot make the pack-signing key or record anyone's consent to publish their ratings | `tools_list_is_well_formed_and_from_the_operations` (`jetcarp-mcp/tests/protocol.rs`) checks that no tool does either | |
| 11 | What a script may use through MCP is limited | `run_script_with_limits_and_progress`, `a_grid_over_the_cell_limit_is_refused` (`jetcarp-mcp/tests/protocol.rs`) | S-S1 |
| 12 | The installers check the archive they download against `SHA256SUMS`; every release file has a provenance attestation | `packaging/test-install-sh.sh` (Linux) and `packaging/test-install-ps1.ps1` (Windows), in CI: an archive with one byte changed, and one `SHA256SUMS` does not list, are refused with nothing installed | S-I1 (the same with each system's installer), S-I2 (`gh attestation verify` on each file of the candidate), S-I3 and S-I4 (`jetcarp uninstall` with linked folders) |
| 13 | The MCP server's HTTP endpoint listens only when started; every request needs its token; requests addressed to other hosts or from origins not allowed (DNS rebinding) are refused; request sizes, times and connections are limited; an HTTP client has the stdio server's tools and folders. A kept token is kept only when asked for, in a private file in the settings folder that survives a fresh runtime folder, comes back only from that file while it is private, and is deleted when keeping is turned off or a new token is made, which refuses the old one; keeping it is warned of; only exact origins can be allowed, and only they get CORS answers | `jetcarp-mcp/tests/http.rs`: `nothing_listens_unless_asked` (on Linux, the stdio server has no listening socket), `the_token_is_required` (missing, wrong, shortened, lengthened and non-bearer tokens; the token file 0600), `other_origins_and_hosts_are_refused`, `oversized_requests_are_refused`, `a_chunked_body_over_the_limit_is_refused`, `slow_clients_time_out`, `connections_are_limited`, `stopping_closes_the_open_connections`, `sessions_behave_as_specified`, `a_session_over_http` (the stdio tool list); the kept token: `a_kept_token_survives_a_fresh_runtime_folder`, `keeping_off_or_a_new_token_deletes_the_kept_token`, `a_kept_token_in_a_loose_file_is_not_taken`, `the_kept_token_survives_a_restart_of_the_server` (the warning on standard error, the token never); allowed origins: `an_allowed_origin_connects_preflight_included`, `unlisted_origins_are_still_refused`, `wildcards_and_other_patterns_are_refused`, `no_cors_headers_when_the_list_is_empty`, and `only_exact_origins_can_be_allowed` (`jetcarp-mcp/src/http.rs`); `http_listens_only_when_asked_and_closes_with_ai_control`, `http_keeps_its_token_when_asked_and_lets_listed_pages_in` (the warning under the box; `jetcarp-app/src/app/ai_control.rs`), `only_folders_of_jetcarps_own_are_deleted` (`jetcarp-api/tests/uninstall_folders.rs`: the kept tokens count as JETCARP's), `the_http_choices_round_trip_and_default_to_off` (`jetcarp-app/src/app/settings.rs`); `mcp_over_http_says_where_it_listens`, `mcp_over_http_keeps_its_token_and_allows_exact_origins` (`jetcarp/tests/cli_json.rs`). By construction: `same_token` (`jetcarp-mcp/src/http.rs`) compares every byte, with no early exit; each HTTP session is a `stdio::Client`, as a stdio client is; the keeping and the origins are off unless chosen | |

The crates that take in other people's data (`jetcarp-mcp`, `jetcarp-pack`, `jetcarp-script`,
`jetcarp-store` and `jetcarp-gpu`) forbid unsafe code (`#![forbid(unsafe_code)]`), so the compiler
keeps them free of it.

## What this testing does not cover

- **Hardware and systems.** Three machines: no Linux on NVIDIA or Intel graphics, no Windows on AMD,
  no Mac with an Intel processor, and on Linux only GNOME on Wayland has been tried for the desktop
  background. CI's runners have no real GPU. A driver that miscomputes on hardware we have not tried
  could still give a different answer; `jetcarp verify` checks any run on the CPU.
- **One person at the keyboard.** The hands-on sittings are Jimmie's: one person's eyes and habits,
  on one set of machines.
- **What a screen reader says.** The tests read the tree a screen reader is given, not what
  Narrator, NVDA, VoiceOver or Orca make of it: how each reads a role, a live region or a grid's
  active cell is checked by hand, on each system.
- **What the desktop does with a launcher.** The tests check the files `jetcarp launcher install`
  writes, not that GNOME's app grid, Launchpad, Spotlight, the Dock or the Start menu show JETCARP
  with its icon, nor that Windows shows the icon inside the program: those are checked by hand, on
  each system.
- **The same author for the code and the tests.** The plans' detailed steps and all the test code
  were written by the AI that wrote the program, so they can share its blind spots. The checks on
  that are the hands-on sittings, the checks that compare one way of computing with another (each
  GPU with the CPU, each machine with the others), and reports from outside. None of these is an
  independent implementation: a mistake made the same way on the CPU and the GPU would pass.
- **The AI's reading of results** can be wrong. Its test sessions have misread or mis-set-up steps:
  one ran a read-only-folder check against a folder it had itself left writable, and the step had to
  be rerun. Results are kept with their evidence so such mistakes can be found.
- **Withheld records.** The test plans, logs and results are public, in
  [jetcarp-testing](https://github.com/jprodgers/jetcarp-testing): a copy of the repository the test
  sessions worked in, less a few personal details. The records of a security problem not yet fixed
  in a release are left out until that release (the two problems of the 0.4.2 cycle are in it now).
  Its `WITHHELD.md` lists what is left out.
- **Expert review.** The check by people who know the field best, the ConwayLife community, is still
  to come.
