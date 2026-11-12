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
  screen). **Clear** clears the search and every filter, and the count at the right says how many
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

The pictures show the Quick Start just after an Explorer setup with a one-minute build: 1,249
entries, 79 of them from the starter pack and 873 from the build's atlas. The other 297 are built
in: a built-in example whose rule an atlas entry also shows is left out, unless it is one of the
hand-made scenes or those picked for the starter set. [Building your own Quick
Start](../tutorials/building-quick-start.md) shows how to make more.

## A world

Click a card to open its entry. (Shift-click opens the small version its card runs, when there is
one.) The world opens running, with the grid on the right and the panes on the left:

<img src="../images/getting-started/tour-world.webp" alt="A world open in the app: a hexagonal Greenberg-Hastings spiral on an isohedral tiling fills the grid; the menu bar shows the scene's name, running, the generation, the hash and the frame rate; the Scene and CAs panes are on the left; the status bar is at the bottom" width="700">

- **Look around.** The scroll wheel zooms and dragging pans. (When the *Draw* pane's tool is *Draw*
  or *Erase*, the left button paints cells, and dragging with the right or middle button still
  pans.) *View → Fit* (F) shows the whole world again; + and − zoom.
- **Run and pause** with Space, or by clicking *▶ running* in the menu bar. *Run → Step* (S or →)
  goes one generation and pauses. *Run → Speed* sets the generations per second (1 to 240),
  *Max speed* runs as fast as the computer can, and *GPU simulation* steps on the graphics card,
  with exactly the same results. *Run → Jump* leaps far ahead with HashLife, for the rules it can
  run (see [HashLife and very long runs](../guides/hashlife.md)).
- **The menu bar's right end** shows the scene's name, whether it is running, the generation, the
  first digits of the [state hash](../guides/scenes.md#the-state-hash) (click it to copy all of it)
  and the frame rate. **The status bar** says whether the world is still changing, and counts its
  live cells and its tiles.
- **More room:** Tab hides the panes and brings them back, F11 fills the screen, and F12 starts
  screensaver mode (Esc leaves it).

*File → Quick Start* (Ctrl+H) goes back to the Quick Start screen, and *File → New empty world*
(Ctrl+N) starts a square grid with no automata on it, ready for your own
([Life on a new tiling](../tutorials/life-on-a-new-tiling.md) starts there).

## The panes

The panes are in two groups of tabs, upper and lower:

| Pane | What it holds | More |
| --- | --- | --- |
| *Scene* | The world's name, description, tiling, size and generation | [Scenes](../guides/scenes.md) |
| *Tiling* | The tiling: its family, size and edges, the catalog of 1,254 periodic tilings, *Edit tile…* and *Edit unit cell…* | [Tilings as neighbour graphs](../tilings/neighbour-graphs.md), [Drawing and editing tiles](../guides/drawing-tiles.md) |
| *Draw* | The mouse's tool (*Pan*, *Draw* or *Erase*), which automaton it paints *Into*, and the state it paints | [Several automata at once](../guides/several-automata.md#in-the-app) |
| *Objects* | *Find objects*: the still lifes, oscillators and spaceships in the world now, counted, and highlighted when you click a row | [Life on a new tiling](../tutorials/life-on-a-new-tiling.md) |
| *Display* | Where the grid shows (this window, full screen, or full-screen *Outputs* on chosen monitors) and the cell size in pixels | [Wallpapers](../guides/wallpapers.md#screensaver-mode-in-the-app) |
| *Screensaver* | Screensaver mode, its profiles and rotations, and the background host | [Wallpapers](../guides/wallpapers.md) |
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
| *File* | *Quick Start*, *New empty world*, *Open scene…* and *Save scene…*, the picture exports (*Export the world as PNG…*, *Export the view as PNG…*, *Export contact sheet…*, *Export wallpaper…*), *Quit* |
| *Edit* | *Reset* (R: clear and seed every automaton again), *Clear every CA*, *Copy state hash*, *Paste a library pattern…* |
| *View* | *Fit* and zoom, *Cell outlines*, the status bar and an auto-hiding menu bar, *Hide all panes*, *Pixel-exact zoom*, *Fill the canvas*, *Full screen*, *Show on outputs*, *Screensaver mode* and its settings, *Keep awake* |
| *Run* | *Run / Pause*, *Step*, *Jump*, *Speed*, *Max speed*, *GPU simulation* |
| *Tiling* | The *Tiling* pane, *Family*, *Edges*, *Tiling catalog*, *Tile editor…*, *Unit cell editor…* |
| *CA* | *Add from catalog*, *Add by notation…*, *Load rule table…*, the *CAs* and *Sequence* panes, *Palettes…* |
| *Seed* | *Seed every CA*, *Random cells in every CA*, each automaton's seed, *Reset*, the *Reseed* pane, *Reseed now*, *Auto reseed* |
| *Tools* | The tool windows below, *Sweep this world…*, the *Objects* pane |
| *Window* | Every pane and tool window, *Hide all panes*, *Reset layout*, *Displays* |
| *Help* | *Setup…* (the setup window again), *Keyboard shortcuts…* (F1), *About JETCARP…* |

*Help → Keyboard shortcuts…* lists every shortcut. Single-key shortcuts such as Space and R work
while no text box has the keyboard.

## The tool windows

Most tools open in windows of their own. Each floats over the world, and its ↗ button (or
*In its own window* under its name in the *Window* menu) moves it to a separate window that can go
on another monitor.

<img src="../images/getting-started/tour-tools-menu.webp" alt="The Tools menu: Scripts and sweeps, Sweep this world, Sweep results, Pattern library, Hunts, Catalog tools, Ants and turmites, Large sandpile, HashLife view, Objects and AI control" width="244">

| Window | Menu | What it is for |
| --- | --- | --- |
| *Pattern library* | *Tools* | Every pattern your searches found and your packs brought: browse, curate, paste into the world, promote to the Quick Start ([The pattern library](../guides/pattern-library.md)) |
| *Hunts* | *Tools* | Start, watch, stop and resume searches, with each job's progress and finds ([Searching](../guides/searching.md#in-the-app)) |
| *Ants and turmites* | *Tools* | Search for ant and turmite highways ([Langton's ant and turmites](../automata/turmites.md), [Searching](../guides/searching.md#the-other-strategies)) |
| *Large sandpile* | *Tools* | Settle millions of sandpile grains on an unbounded grid ([Large sandpiles](../guides/large-sandpiles.md)) |
| *HashLife view* | *Tools* | This world's pattern on the unbounded plane, jumped any distance ([HashLife](../guides/hashlife.md#in-the-app)) |
| *Palettes* | *CA* | Browse, make and apply palettes ([Palettes and colouring](../guides/palettes.md)) |
| *Scripts and sweeps* | *Tools* | Rhai scripts, and sweeps of many rules built from a form ([Scripting a rule sweep](../tutorials/scripting-a-sweep.md)) |
| *AI control* | *Tools* | Let an AI assistant act on this world, through `jetcarp mcp --app` ([Driving JETCARP from an AI assistant](../tutorials/mcp.md)) |
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
