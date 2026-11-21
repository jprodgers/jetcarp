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
- **Searches survive interruption.** Every kind of job but the exploratory search, stopped and
  resumed (on the same runner or another), must end with exactly the results of one that ran through;
  the exploratory search continues from its saved archive instead
  ([what is not guaranteed yet](../research/reproducibility.md#what-is-not-guaranteed-yet)).
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
- **The benchmark's answers.** A short run of the benchmark behind the
  [JETCARP Hardware Score](../guides/this-computer.md) (each workload once) must give its stored
  answers on the processor, and on the GPU where there is one (Mesa's software renderer in CI), and
  a device with a wrong answer must be left out of the score. CI checks the answers, never the
  speed: no test depends on how fast the machine is.
- **The limits.** The per-machine limits are worked out for machines of 4 to 128 GB, with the
  person's own values, the two linked memory values and the ceilings; a world over the limit is
  refused before anything is built, naming the limit and where it came from. The memory each kind
  of world takes is measured by counting every allocation while one is built and stepped, and the
  table the limits use must never be below it. A grid of exactly the largest number of cells the
  code can number (4,294,967,295) is built and its last cells' neighbours checked, without making
  any field.
- **Fit to this computer.** On made-up machines from one with room for a cell per pixel to one
  where nothing fits, the binary search must find what trying every size finds, in five sizes at
  most; a good first guess must settle it in two; a size measured either side of a limit from one
  fit to the next must keep the same recommendation; a rule series must be one scene; and the
  limits must be shared by pixel area. The host must time a frame by its work, not its wait to
  present, and a fit by itself must sweep only the screens whose monitor changed. End to end, with
  no host and with a headless one: a fit of three scenes measures one in at most six sizes, a series
  of 41 rules is one scene and the host shows its seventh rule at the size found, `--pick` measures
  nothing while the last fit is current, and two screens are fitted at once with their shares
  saved. The cards' row must open at the chosen card and scroll sideways with shift and the wheel
  and with a sideways swipe, the pane above it staying where it was.
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
  menus and close dialogs and tool windows; the grid's cell cursor must draw, erase, select and
  move a pattern. In every window, every control must have a role, a name that says something and
  a value that reads, and the status line must be a live region. Each runs in a process of its
  own with scratch folders (`crates/jetcarp-app/src/app/keyboard_tests.rs`); the cell cursor's
  next cell is checked on thirteen tilings (`cell_cursor.rs`), and an ignored test reads the tree
  again with the starter pack's cards, patterns and ant runs in the windows
  (`screen_reader_tree_with_the_starter_pack`). What a screen reader says aloud is checked by hand
  (see [What this testing does not cover](#what-this-testing-does-not-cover)).
- **The book.** The command-line and MCP reference pages and the keyboard page's table of keys are
  generated from the program and checked against it, and the copy of the book built into the
  program must match the book.

About 50 more tests are ignored by default and run only when asked (`-- --ignored`): most draw the
book's pictures of the app, and the rest time the program, run very large worlds, need a desktop
session, or read the app's accessibility tree with the starter pack set up. (The eleven halves of
the keyboard tests that run in a process of their own are ignored too: the suite starts them
itself, with their scratch folders, and they do nothing when started any other way.)

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
  failure, and checkerboards reversing.
- **The stripe check** is tested on stripes of every orientation from 4 to 128 pixels a pair,
  checkerboards and rows of dots, which it must find; and on noise, random cells of 2 to 16 pixels,
  faint cell outlines, low contrast and bright stripes, which it must not.
- **The guard's decisions** are tested end to end on a simulated screen: what it draws, softened as
  the guard says, read back a few frames late. A field flipping every frame is softened and then
  slowed and never shows more than three flashes a second, at any frame rate; flashing that slowing
  cannot stop is paused; small flashing and slow fades are left alone; switched off, it does
  nothing.
- **The GPU and the processor agree**, on any driver (CI runs this on Mesa's llvmpipe as well as
  the test machines' GPUs): the GPU's tile maps and half-resolution image match the processor's
  to the rounding of the sums, and its softened frames byte for byte. The GPU decodes pixels with
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
  striped scenes (still stripes, a reversing checkerboard of squares smaller than a tile),
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
- **The outside check** compares JETCARP's verdicts with those of EA's
  [IRIS](https://github.com/electronicarts/IRIS) on recordings of what each output path drew
  ([Comparing with IRIS](#comparing-with-iris)). For flashes it misses nothing IRIS flags, and with
  the guard on IRIS passes every recording for flashing. For patterns, the guard for someone at
  risk misses nothing IRIS's pattern check flags, and with it on IRIS passes every recording; the
  usual guard counts regular stripes only, and its disagreements with IRIS are listed. IRIS, like
  the guard, is a tool, not a certificate.

### Comparing with IRIS

JETCARP neither ships nor builds IRIS; this is how the comparison was run, and what it found.

1. **IRIS** at commit `d96978ac1107f3463b77f69a9c1b1ec5d45291a0` (14 January 2025), unchanged,
   built as its README says: CMake with vcpkg at IRIS's own baseline (`0affe8710a4a`, which brings
   OpenCV 4.8 with FFmpeg), its example program, release only. It was built on Fedora 45 without
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
| `desktop-off-tron-checkerboard` | pass | 0 / 0 | 0 | fail (stripes) | fail (stripes) | 0 / 0 |
| `desktop-off-tron-field` | fail (flash) | 60 / 0 | 0 | fail (flash) | fail (flash) | 8 / 0 |
| `desktop-off-tron-red` | fail (flash, red) | 37 / 37 | 0 | fail (flash, red) | fail (flash, red) | 8 / 8 |
| `desktop-on-brians-brain` | pass | 1 / 0 | 0 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-cyclic-waves` | fail (pattern) | 1 / 0 | 24 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-hodgepodge` | pass | 1 / 0 | 0 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-life-soup` | pass | 0 / 0 | 0 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-live-field-4.37px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-on-live-field-7.7px` | pass | 0 / 0 | 0 | pass | pass | 0 / 0 |
| `desktop-on-maze` | pass | 0 / 0 | 0 | pass | fail (stripes) | 1 / 0 |
| `desktop-on-seeds-soup` | fail (pattern) | 0 / 0 | 122 | pass | fail (stripes) | 2 / 0 |
| `desktop-on-stripes-still` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
| `desktop-on-tron-blob` | pass | 3 / 0 | 0 | pass | fail (stripes) | 3 / 0 |
| `desktop-on-tron-checkerboard` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
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
| `desktop-risk-tron-checkerboard` | pass | 1 / 0 | 0 | pass | pass | 1 / 0 |
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
guard on is a warning, a pass). One false alarm: the Seeds soup, whose flicker JETCARP counts by
its area rule, a quarter of a 10° field (about 0.7% of a 1080p screen), where IRIS takes the mean
of the whole frame. That difference is deliberate.

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
| One, 2560 × 1440 at 60 Hz | *Classics, hourly* (7 scenes) | 2.7 s, 2 sizes (3.1 s) | 1.5 s, 1 size (1.9 s), the same sizes | 0.6 s, none measured |
| Two at once: 2560 × 1440 at 60 Hz, and 1920 × 1080 at 120 Hz | *Classics, hourly*, and *Until it settles* (3 scenes) | 3.7 s, 5 sizes (4.3 s) | 2.6 to 3.8 s, 3 or 4 sizes (3.1 to 4.4 s) | 0.7 s, none measured |
| One, off screen (no host), 2560 × 1440 | *Classics, hourly* | 2.6 s, 2 sizes | | |

With two screens, the limits were shared by area as they should be: 64% and 36%, so 6.4% and 3.6%
of the processor, and 16% and 9% of the GPU. A fit repeated with nothing changed kept every
scene's size, on one screen and on two; on two, the second screen's first fit judged 2 px just over
its GPU share (7.5% against 7.2%) and the next one clearly within it (5.0%), so the first two fits
differed and every one after kept 2 px. The host's memory stayed between 244 and 260 MB over six
fits on one screen and between 245 and 286 MB on two.

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
  renderer (lavapipe), on macOS on the runner's virtual GPU, and on Windows they are skipped (JETCARP
  never uses Windows' software renderer, WARP). On Linux and macOS, and in the release workflow's
  tests, `JETCARP_REQUIRE_GPU=1` makes a GPU test that finds no adapter it can use (none, or one the
  GPU self-test turns down) fail, naming the adapter and why, rather than skip: there the GPU tests
  cannot pass without having run. On Linux the tests run once more with the default
  folders, failing if any test writes to them. On Windows each of the folder rules' link cases must
  make its junction or symbolic link and run; the log names each one.
- **The installers** are each given a release folder of the test's own: an intact archive installs;
  an archive with one byte changed, and one that `SHA256SUMS` does not list, are refused with
  nothing installed (the shell installer on Linux, the PowerShell one on Windows).
- **Build only** for macOS on Intel and Linux on ARM, which nobody tests by hand yet, and the whole
  workspace with the oldest Rust it says it supports (1.95), so that the stated minimum stays true.
- **Checks:** formatting (`cargo fmt --check`), the third-party notices are current, the workflows
  and packaging scripts pass `actionlint`, every crate packages and builds on its own (a crates.io
  dry run), and the generated reference pages match the program.
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
- **Golly:** the Linux job installs Ubuntu's Golly and runs the Golly test above.
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

- **Parity on every GPU.** 371 scenes (every built-in Quick Start scene, 364, and 7 sandpiles that
  drop grains) run for 300 generations on the CPU and on each adapter and graphics API the machine
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
  from 45 seconds to 12 minutes to size the desktop background for a machine, against Jimmie's target
  of 10 to 30 seconds, so 0.4.2 ships without it and a faster design is planned for 0.4.3 (#112,
  #113).
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
hands-on rows of each release candidate's plan (its part S, from 0.4.3), or by how the code is
built. The next audit checks this table against the code.

| # | Promise | Automated tests and CI checks | Hands-on (part S) |
|---|---|---|---|
| 1 | It makes no network connections; the only network sockets it opens are local, on `127.0.0.1` | The Dependencies workflow (`deny.toml`): no HTTP, TLS, DNS, WebSocket or peer-to-peer crate in the dependency tree that `deny.toml` does not allow, so networking comes only on purpose. `the_listener_is_on_loopback_only` (`jetcarp-mcp/tests/link.rs`). By construction: the workspace's one listening socket is bound to `127.0.0.1` (`Listener` in `jetcarp-mcp/src/link.rs`) | |
| 2 | Nothing in a pack is run; every hash and signature is checked before anything is stored | `jetcarp-pack/tests/hostile.rs`: oversized and deep documents, unknown versions and features, altered objects, names that would leave the store, too many parts, decompression and decoding bombs, records that do not hash to their ids, unsigned and badly signed versions, links out of an export or a store | S-P1 (the same tests on each machine), S-P2 (a pack signed by a key the machine has not seen) |
| 3 | A scene holds no code | By construction: a scene's `script` is a list of steps and the conditions between them, which is data (`jetcarp-core/src/script.rs`) | |
| 4 | Rhai scripts run only when you run them | By construction: a script runs from `jetcarp script`, the app's script window, the MCP server's `run_script` tool, or as a rotation's script while that rotation plays ([Rotations](../guides/rotations.md#a-rotations-script)); nothing else starts the engine | |
| 5 | Scripts write only inside the output folder, the folder given with `--out`, or (through MCP) the allowed folders | `writes_stay_inside_the_output_root` (`jetcarp-script/tests/script.rs`); `jetcarp-mcp/tests/folders.rs`, whose link cases run with symbolic links on every system and junctions on Windows | S-F1 to S-F10 |
| 6 | The app takes commands only while AI control is switched on | `nothing_listens_while_ai_control_is_off` (`jetcarp-app/src/app/ai_control.rs`) | S-L1 |
| 7 | Each listener makes a fresh 128-bit token from the system's random source; a wrong token is refused; the comparison takes the same time whichever character is wrong | `tokens_are_128_random_bits`, `each_listener_makes_its_own_token_and_records_it_privately`, `a_wrong_token_is_refused_and_the_right_one_works` (`jetcarp-mcp/tests/link.rs`). By construction: `constant_time_eq` (`jetcarp-mcp/src/link.rs`) compares every byte, with no early exit | S-L3, S-L4 (from a second user account) |
| 8 | The token file is readable by this user only (folder 0700, file 0600 on Linux and macOS), and is not left behind if it cannot be kept private | `a_loose_folder_and_file_are_made_private`, `each_listener_makes_its_own_token_and_records_it_privately` (Linux and macOS) | S-L2 (on Windows, the folder's permissions with `icacls`) |
| 9 | An AI client's tools read and write only in the output folder, the library's folder and the `--allow` folders | `files_stay_inside_the_folder_rules` (`jetcarp-mcp/tests/protocol.rs`), `jetcarp-mcp/tests/folders.rs`, `an_export_s_scratch_store_stays_inside_the_folder_rules` (`jetcarp-api/tests/reference.rs`) | S-F1 to S-F10 |
| 10 | An AI client cannot make the pack-signing key or record anyone's consent to publish their ratings | `tools_list_is_well_formed_and_from_the_operations` (`jetcarp-mcp/tests/protocol.rs`) checks that no tool does either | |
| 11 | What a script may use through MCP is limited | `run_script_with_limits_and_progress`, `a_grid_over_the_cell_limit_is_refused` (`jetcarp-mcp/tests/protocol.rs`) | S-S1 |
| 12 | The installers check the archive they download against `SHA256SUMS`; every release file has a provenance attestation | `packaging/test-install-sh.sh` (Linux) and `packaging/test-install-ps1.ps1` (Windows), in CI: an archive with one byte changed, and one `SHA256SUMS` does not list, are refused with nothing installed | S-I1 (the same with each system's installer), S-I2 (`gh attestation verify` on each file of the candidate), S-I3 and S-I4 (`jetcarp uninstall` with linked folders) |

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
