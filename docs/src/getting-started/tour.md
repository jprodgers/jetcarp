# A tour of the app

When the setup window closes ([First run and setup](first-run.md)), the app shows its *Quick Start*
screen: everything it can open, running. This page goes through that screen, then a world and the
panes and menus around it, and says where each tool window is. Each part links to the guide that
covers it in full.

## The Quick Start screen

<img src="../images/getting-started/tour-quick-start.webp" alt="The Quick Start screen: a sidebar of categories, sources, tilings and rule families with counts; a search box, Sort, Shuffle, a card size slider and Live; and a grid of running cards, each with its title, rule, category and tiling" width="700">

Each card runs its entry live, with its title, its rule, and its category and tiling below. A tag
in its corner says where it came from: *Built-in* (the examples that come with JETCARP), *Packs*
(from a pack you imported, such as the starter pack) or *My finds* (your own promotions). Entries of
an *atlas*, the result of a Quick Start build, have no tag.

**The sidebar** filters the cards. *Categories* sorts them by what they do (*Gliders & ships*,
*Waves & spirals*, *Oscillators*, …), *Source* by where they came from, *Tilings* by the kind of grid
(*Squares*, *Archimedean*, *k-uniform*, *Aperiodic*, …) and *Rule families* by the kind of rule
(*Life-like*, *Generations*, *JACA*, …). The number beside each is how many cards it would show.
Click one to filter by it and again to stop; filters of different kinds combine.

**Along the top:**

- **The search box** finds text in an entry's name, rule, tiling, family, category or seed:
  `penrose`, `B3/S23`, `k3-011`.
- **Sort** orders the cards: *Featured* (a shuffled selection of the best of every category, new at
  each start), *Score*, *Rule* or *Tiling*. **Shuffle** draws another *Featured* selection.
- **Cards** sets their size, and **Live** whether they run (up to 120 at once, only those on
  screen). When your system asks for reduced motion, *Live* starts off and the cards show stills
  ([Accessibility settings](../guides/accessibility-settings.md#reduced-motion)). **Clear** clears the search and every filter, and the count at the right says how many
  entries are shown.
- **Popular, recent, random** opens three rows: the entries you open most, those you opened last,
  and a random few.

The buttons above them open an *Empty world*, a scene file (*Open scene…*), and some tool windows
straight away: *Scripts and sweeps…*, *Sweep results…*, *Pattern library…* and *Hunts…*.

**A card's menu.** Right-click a card to rate it or hide it:

<img src="../images/getting-started/tour-card-menu.webp" alt="A card's menu: its title, Rate with five stars, and Hide from Quick Start" width="337">

A rating goes to the pattern or ant run the entry shows, if it has one, else to the entry, and is
kept in your library. *Hide from Quick Start* takes the entry out of the menu, and out of your packs
and website exports. `jetcarp library quickstart --hidden` lists hidden entries and `--unhide ID`
brings one back.

The pictures show the Quick Start just after an Explorer setup with a one-minute build: 1,292
entries, 79 of them from the starter pack and 842 from the build's atlas. The other 371 are built
in: a built-in example whose rule an atlas entry also shows is left out, unless it is one of the
hand-made scenes or those picked for the starter set. [Building your own Quick
Start](../tutorials/building-quick-start.md) shows how to make more.

## A world

Click a card to open its entry. (Shift-click opens the small version its card runs, when there is
one.) The world opens running, with the grid on the right and the panes on the left; when your
system asks for reduced motion it opens paused, with a note saying so, and Space runs it:

<img src="../images/getting-started/tour-world.webp" alt="A world open in the app: a hexagonal Greenberg-Hastings spiral on an isohedral tiling fills the grid; the menu bar shows the scene's name, running, the generation, the hash and the frame rate; the Scene and CAs panes are on the left; the status bar at the bottom carries the strobe guard's note that it slowed the scene because it flashed" width="700">

- **Look around.** The scroll wheel zooms and dragging pans. (When the *Draw* pane's tool is *Draw*
  or *Erase*, the left button paints cells, and dragging with the right or middle button still
  pans.) *View → Fit* (F) shows the whole world again; + and − zoom.
- **Run and pause** with Space, or by clicking *▶ running* in the menu bar. *Run → Step* (S or →)
  goes one generation and pauses. *Run → Speed* sets the generations per second (1 to 240),
  *Max speed* runs as fast as the computer can, and *GPU simulation* chooses where the world steps:
  *Follow the shared setting* (the default: the [GPU setting](../guides/gpu.md) all of JETCARP
  shares, which is automatic and puts worlds of 16,000 cells or more on the graphics card), *On*
  every world it can run, *Off* none. The results are exactly the same. *GPU adapter*, below it,
  chooses which graphics card JETCARP uses, on a computer with two (see
  [The GPU](../guides/gpu.md#the-setting)). *Run → Jump* leaps far ahead with HashLife, for
  the rules it can run (see [HashLife and very long runs](../guides/hashlife.md)).
- **The menu bar's right end** shows the scene's name, whether it is running, the generation, the
  first digits of the [state hash](../guides/scenes.md#the-state-hash) (click it to copy all of it)
  and the frame rate. **The status bar** says whether the world is still changing, and counts its
  live cells and its tiles; when the world is too big for the view to draw every cell at once, it
  says so (see [Bigger than the view can draw](../guides/this-computer.md#bigger-than-the-view-can-draw)).
- **The window** opens at the size it had when you quit, and on Windows, macOS and X11 in the same
  place, moved whole onto a monitor when part of it would be off screen (a monitor unplugged, a
  resolution lowered). Under Wayland the system chooses where windows go, so only the size comes
  back.
- **More room:** F9 hides the panes and brings them back, F11 (⌃⌘F on macOS) fills the screen, and
  F12 starts screensaver mode (Esc leaves it). On macOS, ⌘M minimises the window, and the world
  and [AI control](../tutorials/mcp.md) keep running while it is minimised or covered.
- **From the keyboard:** Tab moves from control to control, F6 from the menu bar to each group
  of panes, the grid, the status line and each floating window, and F10 opens the menus. On the
  grid, the arrow keys move a cell cursor that draws (Space), erases (Delete), selects
  (Shift+arrows) and moves what is selected (Ctrl+arrows). See
  [Using the app from the keyboard](../guides/keyboard.md).

*File → Quick Start* (Ctrl+Shift+H, ⇧⌘H on macOS) goes back to the Quick Start screen, and
*File → New empty world* (Ctrl+N) starts a square grid with no automata on it, ready for your own
([Life on a new tiling](../tutorials/life-on-a-new-tiling.md) starts there).

## The panes

The panes are in two groups of tabs, upper and lower:

| Pane | What it holds | More |
| --- | --- | --- |
| *Scene* | The world's name, description, tiling, size and generation | [Scenes](../guides/scenes.md) |
| *Tiling* | The tiling: its family, size and edges, the catalog of 1,254 periodic tilings, *Edit tile…* and *Edit unit cell…* | [Tilings as neighbour graphs](../tilings/neighbour-graphs.md), [Drawing and editing tiles](../guides/drawing-tiles.md) |
| *Draw* | The mouse's tool (*Pan*, *Draw* or *Erase*), and which automaton and state it and the keyboard's cell cursor paint (*Into*, *State*) | [Several automata at once](../guides/several-automata.md#in-the-app) |
| *Objects* | *Find objects*: the still lifes, oscillators and spaceships in the world now, counted, and highlighted when you click a row | [Life on a new tiling](../tutorials/life-on-a-new-tiling.md) |
| *Display* | Where the grid shows (this window, full screen, or full-screen *Outputs* on chosen monitors) and the cell size in pixels | [Wallpapers](../guides/wallpapers.md#screensaver-mode-in-the-app) |
| *Screensaver* | Screensaver mode, its profiles and rotations, and the system screen saver's rotation | [Wallpapers](../guides/wallpapers.md#screensaver-mode-in-the-app) |
| *Desktop background* | The background host: each screen's rotation and monitor, the lock screen, the still picture, how it runs, and *Fit to this computer* | [Wallpapers](../guides/wallpapers.md#the-desktop-background-pane) |
| *CAs* | Each automaton in the world: its rule, neighbours, field, steps per generation, colouring and palette, with *Seed*, *Random* and *Clear* | [Several automata at once](../guides/several-automata.md), [Palettes](../guides/palettes.md) |
| *Add* | Add an automaton from the catalog of named rules or by its notation, or load a rule table | [Rule notation](../reference/rule-notation.md), [Named rules](../automata/named-rules.md) |
| *Sequence* | A scripted sequence: which automata run, and when to move on | [Several automata at once](../guides/several-automata.md) |
| *Command log* | Every change since the world was built or reset, as the scene file keeps it | [Scenes](../guides/scenes.md) |
| *Reseed* | Reseeding now, and the automatic policies that keep a world from dying out | [Reseeding](../guides/reseeding.md) |

Drag a tab to move a pane to the other group, beside the grid, or into a split of its own. The
buttons at the right of each group float the pane in the window, pop it out to a window of its own
(on any monitor), or hide it. The *Window* menu lists every pane and tool window and where each is,
and *Window → Reset layout* puts everything back:

<img src="../images/getting-started/tour-window-menu.webp" alt="The Window menu: Panes, each with its place, such as Scene (Docked); Tool windows, each with its state, such as Scripts and sweeps (closed); Hide all panes, Reset layout and Displays" width="243">

## The menus

| Menu | What it has |
| --- | --- |
| *File* | *Quick Start*, *New empty world*, *Open scene…* and *Save scene…*, the picture exports (*Export the world as PNG…*, *Export the view as PNG…*, *Export animation (GIF or WebP)…*, *Export contact sheet…*, *Export wallpaper…*), *Quit* |
| *Edit* | *Reset* (R: clear and seed every automaton again), *Clear every CA*, *Copy state hash*, *Copy as RLE* (the pattern for Golly, LifeViewer and the forums, see [Copy as RLE](../guides/pattern-library.md#copy-as-rle)), *Paste a library pattern…* |
| *View* | *Fit* and zoom, *Cell outlines*, the status bar and an auto-hiding menu bar, *Hide all panes*, *Pixel-exact zoom*, *Fill the canvas*, *Full screen*, *Show on outputs*, *Screensaver mode* and its settings, *Desktop background settings…*, *Keep awake*, the *Display* pane |
| *Run* | *Run / Pause*, *Step*, *Jump*, *Speed*, *Max speed*, *GPU simulation*, *GPU adapter* |
| *Tiling* | The *Tiling* pane, *Family*, *Edges*, *Tiling catalog*, *Tile editor…*, *Unit cell editor…*, *Paste tile JSON…* |
| *CA* | *Add from catalog*, *Add by notation…*, *Load rule table…*, the *CAs* and *Sequence* panes, *Palettes…* |
| *Seed* | *Seed every CA*, *Random cells in every CA*, each automaton's seed, *Clear every CA*, *Reset*, the *Reseed* pane, *Reseed now*, *Auto reseed* |
| *Tools* | The tool windows below, *Sweep this world…*, the *Objects* pane |
| *Window* | Every pane and tool window, *Hide all panes*, *Reset layout*, *Displays* |
| *Settings* | *Safety…*: the [strobe guard](../guides/strobe-guard.md), what it is doing, and its switches; *Accessibility…*: reduced motion, the high-contrast theme, light or dark, and the palettes for scenes that name none ([Accessibility settings](../guides/accessibility-settings.md)) |
| *Help* | *Setup…* (the setup window again), *Keyboard shortcuts…* (F1), *About JETCARP…* |

*Help → Keyboard shortcuts…* lists every key: the menus' shortcuts, the keys that move about the
app, and the grid's cell cursor ([Using the app from the keyboard](../guides/keyboard.md) has the
same list). Single-key shortcuts such as R and F work except while you are typing in a text box;
Space, Enter and the arrows belong to whatever control has the keyboard.

## The tool windows

Most tools open in windows of their own. Each floats over the world, and its ↗ button (or
*In its own window* under its name in the *Window* menu) moves it to a separate window that can go
on another monitor. Esc closes the tool window whose control has the keyboard.

<img src="../images/getting-started/tour-tools-menu.webp" alt="The Tools menu: Scripts and sweeps, Sweep this world, Sweep results, Pattern library, Hunts, Catalog tools, Ants and turmites, Large sandpile, HashLife view, Objects, AI control and This computer" width="244">

| Window | Menu | What it is for |
| --- | --- | --- |
| *Pattern library* | *Tools* | Every pattern your searches found and your packs brought: browse, curate, paste into the world, promote to the Quick Start ([The pattern library](../guides/pattern-library.md)) |
| *Hunts* | *Tools* | Start, watch, stop and resume searches, with each job's progress and finds ([Searching](../guides/searching.md#in-the-app)) |
| *Ants and turmites* | *Tools* | Search for ant and turmite highways ([Langton's ant and turmites](../automata/turmites.md), [Searching](../guides/searching.md#the-other-strategies)) |
| *Large sandpile* | *Tools* | Settle millions of sandpile grains on an unbounded grid ([Large sandpiles](../guides/large-sandpiles.md)) |
| *HashLife view* | *Tools* | This world's pattern on the unbounded plane, jumped any distance ([HashLife](../guides/hashlife.md#in-the-app)) |
| *Palettes* | *CA* | Browse, make and apply palettes ([Palettes and colouring](../guides/palettes.md)) |
| *Accessibility* | *Settings* | Reduced motion, the high-contrast theme, light or dark, and the palettes for scenes that name none ([Accessibility settings](../guides/accessibility-settings.md)) |
| *Scripts and sweeps* | *Tools* | Rhai scripts, and sweeps of many rules built from a form ([Scripting a rule sweep](../tutorials/scripting-a-sweep.md)) |
| *AI control* | *Tools* | Let an AI assistant act on this world, through `jetcarp mcp --app`, or over HTTP for a client that connects to a URL ([Driving JETCARP from an AI assistant](../tutorials/mcp.md)) |
| *This computer* | *Tools* | The computer's hardware, its JETCARP Hardware Score, and the limits set from them ([This computer](../guides/this-computer.md)) |
| *Tile editor* | *Tiling* | Move a tile's corners and bend its edges ([Drawing and editing tiles](../guides/drawing-tiles.md#the-tile-editor)) |
| *Unit cell editor* | *Tiling* | Edit a periodic tiling's lattice and every tile of its unit cell ([Drawing and editing tiles](../guides/drawing-tiles.md#the-unit-cell-editor)) |

*Tools* also has *Sweep results…* (browse the folders sweeps wrote) and *Catalog tools…* (collide
patterns, the evolutionary glider search, and moving records between libraries), and *File* has
*Export wallpaper…* ([Pictures and animations](../guides/pictures.md#desktop-wallpapers)).

## Where next

- [Your first live wallpaper](../tutorials/first-wallpaper.md): a Quick Start entry in colours of
  your choosing, on your desktop.
- [Life on a new tiling](../tutorials/life-on-a-new-tiling.md): from an empty world to Life on a
  tiling of hexagons, squares and triangles, with the ships it makes.
- [Core ideas](core-ideas.md): tilings, cell fields, rules and scenes, and how they fit together.
