# Using the app from the keyboard

Everything in the app can be reached from the keyboard: the menus, every pane and tool window,
and the grid itself, where a cell cursor draws, erases, selects and moves patterns. Screen readers
are told what each control is, what it holds, and what changes (see
[With a screen reader](#with-a-screen-reader)). This page says how the keyboard moves about the
app and lists every key. The same list is in *Help → Keyboard shortcuts…* (F1), and
[`jetcarp shortcuts`](../reference/cli.md#jetcarp-shortcuts) prints it in a terminal.

## Moving about

**Tab** goes to the next control and **Shift+Tab** to the one before, in the order the window
shows them: the menu bar, the panes (each group's tabs, then the pane showing), the grid, the
status line, then each floating window. Whatever has the keyboard has a ring round it, in every
theme: two points of blue in the standard dark and light themes, three of gold (or navy, in its
light form) in the [high-contrast theme](accessibility-settings.md#high-contrast). A control that
was out of sight, further down a long pane or a list of cards, is scrolled into view as the
keyboard reaches it. The few things only the mouse uses, such as a window's title bar and the
lines between docked panes, are passed over.

**F6** goes to the next part of the window, to its first control: the menu bar, each group of
docked panes, the grid, the status line, and each floating window in turn. **Shift+F6** goes
back. It is the quickest way from the grid to a pane and back again.

**F10** opens the *File* menu. **→** and **←** move along the menu bar, opening each menu in turn,
**↓** goes into a menu and **↑** and **↓** move through it, **→** opens a submenu and **←** comes
back out, **Enter** chooses, and **Esc** closes the menu. A menu that holds a text box or a slider
(*Run → Jump*, *Run → Speed*) takes typing and the arrows like any other control. With the menu
bar auto-hidden (*View → Auto-hide menu bar*, Ctrl+F10 or ⌘F10 on macOS), F10 shows it while a
menu is open.

**Space** or **Enter** presses the button, ticks the box, chooses the item, or opens the list that
has the keyboard; in an open list, the arrows move and Enter chooses. On a slider the arrows
change its value; a number box takes typing once it has the keyboard.

**Esc** leaves a text box, closes a menu or a list, closes *Export animation* and the two
confirmations, and closes the tool window whose control has the keyboard (Esc in one of its text
boxes only leaves the box), the setup window among them. **Enter** confirms: *Export animation*
opens with *Export…* holding the keyboard, each page of the setup window with its main button
(*Next*, *Start*, *Close*), and each confirmation with its safe choice (*Keep it on*, *Save the
softened version*). The photosensitive seizure warning waits for an answer: neither Esc nor Enter
answers it for you.

**The single-key shortcuts** (R, F, S, + and −) act whatever has the keyboard, except while you
are typing in a text box; the function keys (F1, F6, F9 to F12) act even then. Space, Enter, Tab
and the arrows belong to the control that has the keyboard, so while one has it, Space presses it
rather than running the world: press Esc first, and Space runs and pauses again.

### Panes and tool windows

A pane's tab takes the keyboard like a button: Space or Enter shows its pane. The buttons at the
right of each group of tabs hide the pane, pop it out to a window of its own, or float it. Every
pane and tool window is in the *Window* menu: under each pane's name, choose where it goes
(*Docked*, *Floating*, *Own window* or *Hidden*); under each tool window's, *Open* and *In its own
window*. *Window → Reset layout* puts everything back. **F9** hides all the panes and brings them
back.

A pane or tool window popped out to its own window is a separate window of the system: move to it
as to any other window (Alt+Tab on Windows and Linux, ⌘\` on macOS). F6 moves within one window.

### The Quick Start

Tab goes through the buttons along the top, the sidebar's filters (each a toggle that says how
many cards it would show, and whether it filters now), the search box and the cards. Each card is
a button named by its title, rule, category and tiling. Enter opens it, and **Shift+F10** opens its
menu, to rate it or hide it (↓ goes into the menu).

## The grid

The grid takes the keyboard like any other control: Tab to it, or F6. While it has it, a **cell
cursor** shows, a ring round one cell in the focus colour on a dark edge, so it shows on any
palette; with the keyboard elsewhere, the grid looks as it always does.

<img src="../images/guides/keyboard-grid-cursor.webp" alt="The grid with the keyboard on it: Life on a square grid, a glider of five orange cells drawn from the keyboard, beside it a selection of nine cells tinted and outlined in pale blue, and the cell cursor at the selection's corner in a heavier pale blue ring on a dark edge" width="481">

- **The arrow keys** move the cursor to the next cell that way. On any tiling, the next cell is
  the touching cell whose centre lies most nearly that way, keeping to the row or column the
  cursor is moving along: a run of → follows a row of triangles one triangle at a time, and a run
  of ↑ follows a column of hexagons, zigzagging about it. The view moves to keep the cursor in
  sight. At the edge of a tiling with dead edges the cursor stays, and the status line says so; on
  wrapped edges it goes round.
- **Space** draws the cursor's cell, into the automaton and in the state the *Draw* pane chooses
  (its *Into* and *State*, shown whatever the mouse's tool; the keyboard draws with them in *Pan*
  too). **Delete** or **Backspace** erases it.
- **Shift and the arrows** select: the cells between where you began and the cursor (every cell
  whose centre lies in the rectangle their two centres span). Space then fills the selection, and
  Delete clears it.
- **Ctrl and the arrows** (⌘ on macOS) move the selected pattern, or the cursor's cell, one step,
  with the selection. On a periodic tiling the step is the tiling's own shortest translation that
  way, so the pattern keeps its shape: one cell on squares, one pair of triangles on triangles, and
  on hexagons, up and down a column in turn left and right. An aperiodic tiling has no such moves:
  there each cell goes to the cell under its moved centre, and the pattern can change shape. Every
  automaton's cells move together; a sandpile's grains stay where they are.
- **A library pattern** waiting to be pasted (*Pattern library → Paste*) is pasted at the cursor
  with Space.
- **Esc** ends the selection; pressed again, it leaves the grid. The single-key shortcuts (Space to
  run and pause, R, S and the rest) then act again.

## Every key

The menus have 19 shortcuts, each shown beside its item in the menu. Letters and other keys with
no Ctrl or ⌘ act whatever has the keyboard, except in a text box (Space and the arrows: except on
a control); the function keys act everywhere.

<!-- GENERATED by docs/tools/gen-shortcuts.py: do not edit by hand -->

### Moving about

| Windows and Linux | macOS | What it does |
| --- | --- | --- |
| <kbd>Tab</kbd>, <kbd>Shift+Tab</kbd> | <kbd>Tab</kbd>, <kbd>⇧Tab</kbd> | The next control, the one before |
| <kbd>F6</kbd>, <kbd>Shift+F6</kbd> | <kbd>F6</kbd>, <kbd>⇧F6</kbd> | The next part of the window: the menu bar, each group of panes, the grid, the status line, each floating window; the part before |
| <kbd>F10</kbd> | <kbd>F10</kbd> | The menu bar: opens File. ← and → move along it, ↓ goes into a menu, ← comes back out, Enter chooses, Esc closes |
| <kbd>Space</kbd>, <kbd>Enter</kbd> | <kbd>Space</kbd>, <kbd>Return</kbd> | Press the button, tick the box, choose the item, open the list that has the keyboard |
| <kbd>Arrow keys</kbd> | <kbd>Arrow keys</kbd> | Between controls; in a menu or list, between its items; on a slider, its value |
| <kbd>Esc</kbd> | <kbd>Esc</kbd> | Leave a text box; close a menu, a list, a dialog, or the tool window that has the keyboard |
| <kbd>Shift+F10</kbd> | <kbd>⇧F10</kbd> | A Quick Start card's menu: rate it, hide it |
| <kbd>Alt (held)</kbd> | <kbd>⌥ (held)</kbd> | Show the auto-hidden menu bar |

### The menus' shortcuts

| Windows and Linux | macOS | What it does |
| --- | --- | --- |
| <kbd>Ctrl+Shift+H</kbd> or <kbd>Ctrl+H</kbd> | <kbd>⇧⌘H</kbd> or <kbd>⌘H</kbd> | File → Quick Start |
| <kbd>Ctrl+N</kbd> | <kbd>⌘N</kbd> | File → New empty world |
| <kbd>Ctrl+O</kbd> | <kbd>⌘O</kbd> | File → Open scene |
| <kbd>Ctrl+S</kbd> | <kbd>⌘S</kbd> | File → Save scene |
| <kbd>Ctrl+E</kbd> | <kbd>⌘E</kbd> | File → Export the world as PNG |
| <kbd>Ctrl+Q</kbd> | <kbd>⌘Q</kbd> | File → Quit |
| <kbd>R</kbd> | <kbd>R</kbd> | Edit → Reset |
| <kbd>Esc</kbd> | <kbd>Esc</kbd> | Edit → Stop pasting |
| <kbd>F</kbd> | <kbd>F</kbd> | View → Fit |
| <kbd>+</kbd> or <kbd>=</kbd> | <kbd>+</kbd> or <kbd>=</kbd> | View → Zoom in |
| <kbd>−</kbd> | <kbd>−</kbd> | View → Zoom out |
| <kbd>Ctrl+F10</kbd> | <kbd>⌘F10</kbd> | View → Auto-hide menu bar |
| <kbd>F9</kbd> | <kbd>F9</kbd> | View → Hide all panes |
| <kbd>Space</kbd> | <kbd>Space</kbd> | Run → Run / Pause |
| <kbd>S</kbd> or <kbd>→</kbd> | <kbd>S</kbd> or <kbd>→</kbd> | Run → Step |
| <kbd>F11</kbd> | <kbd>⌃⌘F</kbd> or <kbd>F11</kbd> | View → Full screen |
| <kbd>F12</kbd> | <kbd>F12</kbd> | View → Screensaver mode |
| <kbd>Ctrl+1</kbd> | <kbd>⌘1</kbd> | View → Pixel-exact zoom |
| <kbd>F1</kbd> | <kbd>F1</kbd> | Help → Keyboard shortcuts |
| – | <kbd>⌘M</kbd> | Minimise the window |

### On the grid, while it has the keyboard

| Windows and Linux | macOS | What it does |
| --- | --- | --- |
| <kbd>Arrow keys</kbd> | <kbd>Arrow keys</kbd> | Move the cell cursor to the next cell that way |
| <kbd>Shift+arrow keys</kbd> | <kbd>⇧ and arrow keys</kbd> | Select: the cells between where you began and the cursor |
| <kbd>Space</kbd> | <kbd>Space</kbd> | Draw the cursor's cell, or every selected cell, in the Draw pane's state; with a library pattern waiting, paste it there |
| <kbd>Delete</kbd>, <kbd>Backspace</kbd> | <kbd>⌦</kbd>, <kbd>⌫</kbd> | Erase the cursor's cell or the selection |
| <kbd>Ctrl+arrow keys</kbd> | <kbd>⌘ and arrow keys</kbd> | Move the selected pattern (or the cursor's cell) one step that way |
| <kbd>Esc</kbd> | <kbd>Esc</kbd> | End the selection; again, leave the grid, so Space runs and pauses |

<!-- END GENERATED -->

The mouse: dragging with the right or middle button pans, whatever the *Draw* pane's tool is, and
the scroll wheel zooms about the pointer.

## With a screen reader

The app gives screen readers a description of everything it shows through
[AccessKit](https://accesskit.dev): UI Automation on Windows (Narrator, NVDA, JAWS), the
accessibility API on macOS (VoiceOver), and AT-SPI on Linux (Orca). The description is made only
while a screen reader asks for it.

- **Every control has a role and a name:** a button, a check box, a list, a slider, a text box or
  a tab, named by its own words or by the label beside it ("Into" for the list of automata in the
  *Draw* pane). Buttons drawn as symbols (×, ↗, ◱, ◀, ▶) are named by what they do ("Hide the CAs
  pane", "Previous item"). Values read: a check box's tick, a slider's or a number box's number, a
  list's choice, a text box's text.
- **The grid** is a grid named after the world. While it has the keyboard, the cursor's cell is
  what the screen reader is on, named by its number, its column and row on a lattice tiling, and
  each automaton's state there: "Cell 1234, column 5, row 7: alive". Moving the cursor reads the
  new cell, and a selection says how many cells it holds.
- **The status line is read out as it changes,** while you do something else: a scene the strobe
  guard paused or slowed, a job that finished, failed or paused (from the *Hunts* window too), a
  scene that could not be opened, and the app's other notes. An error is read at once; anything
  else when the screen reader has finished what it is saying. It is read with the status bar
  hidden too.
- **The pictures** (the Quick Start's cards, the Hunts window's finds, an ant run's animation, the
  editors' canvases) are named by what they show.

## What the keyboard and screen readers can't do

Some things are limits of egui, the toolkit the app is drawn with, or of AccessKit; others are
the app's, and are listed so they are known.

- **Dragging.** Rearranging docked panes by dragging their tabs, resizing them, the shapes in the
  [tile editor and unit cell editor](drawing-tiles.md), the outputs' places in the *Display* pane,
  and panning the view by dragging all need a pointer. From the keyboard: the *Window* menu places
  panes; *View → Fit* (F), + and − look around; the cell cursor moves the view to follow it.
- **The menus are the app's own.** egui draws its menus inside the window, so they are not in the
  system's menu bar on macOS, and VoiceOver's command for the menu bar (Control-Option-M) does not
  reach them; F10 does. Alt (Option on macOS) does not open them: held, it shows an auto-hidden
  menu bar.
- **Scrolling from the keyboard.** egui's scrolled areas don't scroll with Page Up and Page Down;
  Tab and the arrows scroll each control into view as the keyboard reaches it.
- **One cell at a time.** A screen reader is told of the cursor's cell, not of every cell: a world
  can have seventeen million. The status line counts the live cells; the *Objects* pane lists what
  the world holds.
- **Windows of their own.** A popped-out window is reached with the system's own keys, not F6.
- **Text boxes that take Tab.** In the script editor and the tile JSON box, Tab types a tab: press
  Esc to leave the box, then Tab moves on.

The checks behind this page drive the whole app with key presses in tests and read its
accessibility tree, as a screen reader would: every control in every window has a role and a name
and its value reads, Tab reaches every control, F6 and F10 go where this page says, and the cell
cursor draws, erases, selects and moves patterns (see [How JETCARP is tested](../about/testing.md)).
What a screen reader says aloud is checked by hand, with Narrator and NVDA on Windows, VoiceOver
on macOS and Orca on GNOME. So far Orca has been, with 0.4.3-rc.1: it named each control by what
it does, symbol buttons too, gave the cards, swatches and presets their roles and names, read the
grid cursor's cell, and read out the status line as it changed. The others have not been yet (see
[Where JETCARP falls short](../about/accessibility.md#where-jetcarp-falls-short)).

## Settings

Reduced motion, the high-contrast theme, light or dark, and the palettes chosen for colour-vision
deficiency are in [Settings → Accessibility](accessibility-settings.md).
